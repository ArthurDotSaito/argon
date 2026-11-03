use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::client::GithubClient;
use crate::error::{GithubError, Result};
use crate::rest::repo_path;

const SETUP_ASSET_SUFFIX: &str = "_x64-setup.exe";
const SHA256_PREFIX: &str = "sha256:";
const SHA256_HEX_LEN: usize = 64;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const PARTIAL_SUFFIX: &str = ".part";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: semver::Version,
    pub tag: String,
    pub html_url: String,
    pub asset: Option<ReleaseAsset>,
}

#[derive(Deserialize)]
struct WireRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<WireAsset>,
}

#[derive(Deserialize)]
struct WireAsset {
    id: u64,
    name: String,
    size: u64,
    digest: Option<String>,
}

fn sha256_of(digest: Option<String>) -> Option<String> {
    let digest = digest?;
    let hex = digest
        .strip_prefix(SHA256_PREFIX)
        .map(str::to_ascii_lowercase)
        .filter(|hex| hex.len() == SHA256_HEX_LEN && hex.bytes().all(|b| b.is_ascii_hexdigit()));
    if hex.is_none() {
        tracing::warn!(%digest, "release asset digest is not a sha256 hex string");
    }
    hex
}

fn into_asset(asset: WireAsset) -> ReleaseAsset {
    ReleaseAsset {
        id: asset.id,
        name: asset.name,
        size: asset.size,
        sha256: sha256_of(asset.digest),
    }
}

fn into_release(release: WireRelease) -> Result<Release> {
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    let Ok(version) = semver::Version::parse(version) else {
        tracing::warn!(tag = %release.tag_name, "release tag is not a valid semver version");
        return Err(GithubError::InvalidReleaseTag(release.tag_name));
    };
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name.ends_with(SETUP_ASSET_SUFFIX))
        .map(into_asset);
    Ok(Release {
        version,
        tag: release.tag_name,
        html_url: release.html_url,
        asset,
    })
}

fn release_from_body(body: &[u8]) -> Result<Release> {
    let release: WireRelease = serde_json::from_slice(body)?;
    into_release(release)
}

fn partial_path(dest: &Path) -> PathBuf {
    let mut path = OsString::from(dest);
    path.push(PARTIAL_SUFFIX);
    PathBuf::from(path)
}

fn octet_stream_accept() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/octet-stream"));
    headers
}

impl GithubClient {
    pub async fn latest_release(&self, repo: &str) -> Result<Option<Release>> {
        let repo = repo_path(repo)?;
        let url = self.rest_url_for(&format!("/repos/{repo}/releases/latest"), &[])?;
        let response = self
            .rest_request(Method::GET, url)
            .send()
            .await
            .map_err(reqwest::Error::without_url)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let body = self
            .check_status(response)?
            .bytes()
            .await
            .map_err(reqwest::Error::without_url)?;
        release_from_body(&body).map(Some)
    }

    pub async fn download_asset(
        &self,
        repo: &str,
        asset: &ReleaseAsset,
        dest: &Path,
        progress: impl Fn(u64) + Send,
    ) -> Result<()> {
        let repo = repo_path(repo)?;
        let url = self.rest_url_for(&format!("/repos/{repo}/releases/assets/{}", asset.id), &[])?;
        let response = self
            .rest_request(Method::GET, url)
            .headers(octet_stream_accept())
            .timeout(DOWNLOAD_TIMEOUT)
            .send()
            .await
            .map_err(reqwest::Error::without_url)?;
        let response = self.check_status(response)?;
        let partial = partial_path(dest);
        let finished = match stream_to_file(response, asset, &partial, progress).await {
            Ok(()) => tokio::fs::rename(&partial, dest).await.map_err(Into::into),
            Err(error) => Err(error),
        };
        if finished.is_err() {
            let _ = tokio::fs::remove_file(&partial).await;
        }
        finished
    }
}

async fn stream_to_file(
    mut response: reqwest::Response,
    asset: &ReleaseAsset,
    path: &Path,
    progress: impl Fn(u64),
) -> Result<()> {
    let mut file = File::create(path).await?;
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(reqwest::Error::without_url)?
    {
        file.write_all(&chunk).await?;
        hasher.update(&chunk);
        received += chunk.len() as u64;
        progress(received);
    }
    file.flush().await?;
    drop(file);
    if received != asset.size {
        return Err(GithubError::Decode(format!(
            "downloaded {received} bytes of {}, expected {}",
            asset.name, asset.size
        )));
    }
    let actual = format!("{:x}", hasher.finalize());
    match &asset.sha256 {
        Some(expected) if *expected != actual => Err(GithubError::Decode(format!(
            "sha256 mismatch for {}: expected {expected}, got {actual}",
            asset.name
        ))),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use tokio::net::TcpListener;

    use super::*;
    use crate::test_server::{json_response, read_request, serve};

    const LATEST: &str = include_str!("../tests/fixtures/release_latest.json");
    const TOKEN: &str = "test-token-not-real";
    const REPO: &str = "ArthurDotSaito/argon";
    const INSTALLER: &str = "argon-installer-bytes";

    fn client_for(port: u16) -> GithubClient {
        let mut client = GithubClient::new("github.com", TOKEN.to_string());
        client.rest_url = format!("http://127.0.0.1:{port}");
        client
    }

    fn sha256_hex(content: &str) -> String {
        format!("{:x}", Sha256::digest(content.as_bytes()))
    }

    fn installer_asset(sha256: Option<String>) -> ReleaseAsset {
        ReleaseAsset {
            id: 412000008,
            name: "argon_0.1.10_x64-setup.exe".to_string(),
            size: INSTALLER.len() as u64,
            sha256,
        }
    }

    fn release_with_tag(tag: &str) -> String {
        format!(r#"{{"tag_name":"{tag}","html_url":"https://github.com/x/y","body":null}}"#)
    }

    fn octet_stream_response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("argon-releases-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn latest_fixture_maps_the_release_and_picks_the_setup_asset() {
        let release = release_from_body(LATEST.as_bytes()).unwrap();
        assert_eq!(
            release,
            Release {
                version: semver::Version::new(0, 1, 10),
                tag: "v0.1.10".to_string(),
                html_url: "https://github.com/ArthurDotSaito/argon/releases/tag/v0.1.10"
                    .to_string(),
                asset: Some(ReleaseAsset {
                    id: 412000008,
                    name: "argon_0.1.10_x64-setup.exe".to_string(),
                    size: 48213504,
                    sha256: Some(
                        "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
                            .to_string()
                    ),
                }),
            }
        );
    }

    #[test]
    fn version_ordering_is_numeric_not_lexical() {
        let newer = release_from_body(release_with_tag("v0.1.10").as_bytes()).unwrap();
        let older = release_from_body(release_with_tag("v0.1.9").as_bytes()).unwrap();
        assert!(newer.version > older.version);
    }

    #[test]
    fn tag_without_the_v_prefix_is_accepted() {
        let release = release_from_body(release_with_tag("1.2.3").as_bytes()).unwrap();
        assert_eq!(release.version, semver::Version::new(1, 2, 3));
        assert_eq!(release.tag, "1.2.3");
    }

    #[test]
    fn invalid_tag_is_reported_with_the_offending_tag() {
        for tag in ["nightly", "v1.2", "vv1.2.3", ""] {
            let error = release_from_body(release_with_tag(tag).as_bytes()).unwrap_err();
            assert!(
                matches!(&error, GithubError::InvalidReleaseTag(found) if found == tag),
                "{tag:?}: {error}"
            );
        }
    }

    #[test]
    fn release_without_assets_has_no_asset() {
        let release = release_from_body(release_with_tag("v0.2.0").as_bytes()).unwrap();
        assert_eq!(release.asset, None);
    }

    #[test]
    fn valid_sha256_digests_are_kept_lowercased() {
        let valid = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        assert_eq!(
            sha256_of(Some(format!("sha256:{valid}"))),
            Some(valid.to_string())
        );
        assert_eq!(
            sha256_of(Some(format!("sha256:{}", valid.to_uppercase()))),
            Some(valid.to_string())
        );
    }

    #[test]
    fn missing_or_malformed_digests_are_dropped() {
        let valid = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        for digest in [
            None,
            Some("sha1:abc".to_string()),
            Some("sha256:abc123".to_string()),
            Some(format!("sha256:{}zz", &valid[..62])),
            Some(valid.to_string()),
        ] {
            assert_eq!(sha256_of(digest.clone()), None, "{digest:?}");
        }
    }

    #[test]
    fn setup_asset_without_digest_has_no_sha256() {
        let body = r#"{"tag_name":"v0.3.0","html_url":"u","assets":[
            {"id":1,"name":"argon_0.3.0_x64-setup.exe","size":10,"digest":null}]}"#;
        let release = release_from_body(body.as_bytes()).unwrap();
        assert_eq!(release.asset.unwrap().sha256, None);
    }

    #[tokio::test]
    async fn latest_release_fetches_and_parses_the_latest_endpoint() {
        let port = serve(|request_line| {
            if request_line.starts_with("GET /repos/ArthurDotSaito/argon/releases/latest ") {
                json_response("200 OK", LATEST)
            } else {
                json_response("500 Internal Server Error", "{}")
            }
        })
        .await;

        let release = client_for(port)
            .latest_release(REPO)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(release.tag, "v0.1.10");
        assert_eq!(release.asset.unwrap().id, 412000008);
    }

    #[tokio::test]
    async fn latest_release_maps_404_to_none() {
        let port = serve(|_| json_response("404 Not Found", r#"{"message":"Not Found"}"#)).await;

        let release = client_for(port).latest_release(REPO).await.unwrap();

        assert_eq!(release, None);
    }

    #[tokio::test]
    async fn latest_release_reports_auth_failures_as_errors() {
        let port =
            serve(|_| json_response("401 Unauthorized", r#"{"message":"Bad credentials"}"#)).await;
        let result = client_for(port).latest_release(REPO).await;
        assert!(matches!(result, Err(GithubError::AuthMissing { .. })));

        let port = serve(|_| json_response("403 Forbidden", r#"{"message":"Forbidden"}"#)).await;
        let error = client_for(port).latest_release(REPO).await.unwrap_err();
        assert!(error.to_string().contains("403"), "{error}");
    }

    #[tokio::test]
    async fn latest_release_rejects_malformed_repository_names() {
        let port = serve(|_| json_response("200 OK", LATEST)).await;
        assert!(client_for(port).latest_release("not-a-repo").await.is_err());
    }

    #[tokio::test]
    async fn download_writes_the_body_and_reports_progress() {
        let port = serve(|_| octet_stream_response(INSTALLER)).await;
        let dir = scratch_dir("ok");
        let dest = dir.join("argon-setup.exe");
        let last_progress = Arc::new(AtomicU64::new(0));
        let calls = Arc::new(AtomicU64::new(0));
        let asset = installer_asset(Some(sha256_hex(INSTALLER)));

        let (seen, count) = (last_progress.clone(), calls.clone());
        client_for(port)
            .download_asset(REPO, &asset, &dest, move |bytes| {
                seen.store(bytes, Ordering::SeqCst);
                count.fetch_add(1, Ordering::SeqCst);
            })
            .await
            .unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), INSTALLER);
        assert!(calls.load(Ordering::SeqCst) >= 1);
        assert_eq!(last_progress.load(Ordering::SeqCst), INSTALLER.len() as u64);
        assert!(!partial_path(&dest).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn download_without_a_known_digest_still_succeeds() {
        let port = serve(|_| octet_stream_response(INSTALLER)).await;
        let dir = scratch_dir("nodigest");
        let dest = dir.join("argon-setup.exe");

        client_for(port)
            .download_asset(REPO, &installer_asset(None), &dest, |_| {})
            .await
            .unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), INSTALLER);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn digest_mismatch_fails_and_leaves_no_file() {
        let port = serve(|_| octet_stream_response(INSTALLER)).await;
        let dir = scratch_dir("digest");
        let dest = dir.join("argon-setup.exe");
        let asset = installer_asset(Some(sha256_hex("something else")));

        let error = client_for(port)
            .download_asset(REPO, &asset, &dest, |_| {})
            .await
            .unwrap_err();

        assert!(error.to_string().contains("sha256 mismatch"), "{error}");
        assert!(!dest.exists());
        assert!(!partial_path(&dest).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn size_mismatch_fails_and_leaves_no_file() {
        let port = serve(|_| octet_stream_response(INSTALLER)).await;
        let dir = scratch_dir("size");
        let dest = dir.join("argon-setup.exe");
        let mut asset = installer_asset(None);
        asset.size += 1;

        let error = client_for(port)
            .download_asset(REPO, &asset, &dest, |_| {})
            .await
            .unwrap_err();

        assert!(error.to_string().contains("expected"), "{error}");
        assert!(!dest.exists());
        assert!(!partial_path(&dest).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn failed_download_keeps_a_previously_downloaded_file() {
        let port = serve(|_| octet_stream_response("corrupt")).await;
        let dir = scratch_dir("keep");
        let dest = dir.join("argon-setup.exe");
        std::fs::write(&dest, "previous").unwrap();

        let result = client_for(port)
            .download_asset(REPO, &installer_asset(None), &dest, |_| {})
            .await;

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "previous");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn http_failure_surfaces_as_error_without_creating_files() {
        let port = serve(|_| json_response("404 Not Found", "{}")).await;
        let dir = scratch_dir("http");
        let dest = dir.join("argon-setup.exe");

        let result = client_for(port)
            .download_asset(REPO, &installer_asset(None), &dest, |_| {})
            .await;

        assert!(result.is_err());
        assert!(!dest.exists());
        assert!(!partial_path(&dest).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn download_requests_the_asset_as_octet_stream_with_a_single_accept() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_request(&mut socket).await;
            socket
                .write_all(octet_stream_response(INSTALLER).as_bytes())
                .await
                .unwrap();
            socket.shutdown().await.unwrap();
            request
        });
        let dir = scratch_dir("headers");
        let dest = dir.join("argon-setup.exe");

        client_for(port)
            .download_asset(REPO, &installer_asset(None), &dest, |_| {})
            .await
            .unwrap();

        let request = server.await.unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /repos/arthurdotsaito/argon/releases/assets/412000008 "));
        let accepts: Vec<&str> = request
            .lines()
            .filter(|line| line.starts_with("accept:"))
            .collect();
        assert_eq!(accepts, vec!["accept: application/octet-stream"]);
        assert!(request.contains("authorization: bearer "));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_path_appends_the_suffix_to_the_whole_file_name() {
        assert_eq!(
            partial_path(Path::new("dir/argon-setup.exe")),
            PathBuf::from("dir/argon-setup.exe.part")
        );
    }
}
