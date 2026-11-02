use crate::actions::{PER_PAGE, collect_pages_capped, is_sha};
use crate::client::GithubClient;
use crate::error::{GithubError, Result};
use crate::files_wire::{CommitResponse, WireFile};
use crate::rest::repo_path;

const PR_FILES_MAX_PAGES: u32 = 30;
const COMMIT_FILES_MAX_PAGES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileStatus {
    Added,
    Modified,
    Removed,
    Renamed,
    Copied,
    Changed,
    Unchanged,
}

impl FileStatus {
    fn from_wire(status: Option<&str>) -> Self {
        match status {
            Some("added") => Self::Added,
            Some("modified") => Self::Modified,
            Some("removed") => Self::Removed,
            Some("renamed") => Self::Renamed,
            Some("copied") => Self::Copied,
            Some("unchanged") => Self::Unchanged,
            _ => Self::Changed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrFile {
    pub path: String,
    pub previous_path: Option<String>,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
    pub patch: Option<String>,
    pub sha: Option<String>,
}

impl From<WireFile> for PrFile {
    fn from(file: WireFile) -> Self {
        Self {
            status: FileStatus::from_wire(file.status.as_deref()),
            path: file.filename,
            previous_path: file.previous_filename,
            additions: file.additions,
            deletions: file.deletions,
            patch: file.patch,
            sha: file.sha,
        }
    }
}

fn pr_files_path(repo: &str, number: u64) -> Result<String> {
    Ok(format!("/repos/{}/pulls/{number}/files", repo_path(repo)?))
}

fn commit_path(repo: &str, sha: &str) -> Result<String> {
    if !is_sha(sha) {
        return Err(GithubError::Decode(format!("invalid commit sha: {sha}")));
    }
    Ok(format!("/repos/{}/commits/{sha}", repo_path(repo)?))
}

impl GithubClient {
    pub async fn pr_files(&self, repo: &str, number: u64) -> Result<Vec<PrFile>> {
        let path = pr_files_path(repo, number)?;
        let path = path.as_str();
        let files = collect_pages_capped(
            "pull request files",
            PR_FILES_MAX_PAGES,
            |page| async move {
                let page = page.to_string();
                let files: Vec<WireFile> = self
                    .rest_get(
                        path,
                        &[("per_page", &PER_PAGE.to_string()), ("page", &page)],
                    )
                    .await?;
                Ok((files, None))
            },
        )
        .await?;
        Ok(files.into_iter().map(PrFile::from).collect())
    }

    pub async fn commit_files(&self, repo: &str, sha: &str) -> Result<Vec<PrFile>> {
        let path = commit_path(repo, sha)?;
        let path = path.as_str();
        let files =
            collect_pages_capped("commit files", COMMIT_FILES_MAX_PAGES, |page| async move {
                let page = page.to_string();
                let response: CommitResponse = self
                    .rest_get(
                        path,
                        &[("per_page", &PER_PAGE.to_string()), ("page", &page)],
                    )
                    .await?;
                Ok((response.files, None))
            })
            .await?;
        Ok(files.into_iter().map(PrFile::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_server::{json_response, serve};

    const PR_FILES: &str = include_str!("../tests/fixtures/pr_files.json");
    const COMMIT: &str = include_str!("../tests/fixtures/commit.json");

    fn pr_files() -> Vec<PrFile> {
        let files: Vec<WireFile> = serde_json::from_str(PR_FILES).unwrap();
        files.into_iter().map(PrFile::from).collect()
    }

    fn client_for(port: u16) -> GithubClient {
        let mut client = GithubClient::new("github.com", "test-token-not-real".to_string());
        client.rest_url = format!("http://127.0.0.1:{port}");
        client
    }

    fn full_page_body() -> String {
        let file = |index: usize| {
            format!(
                r#"{{"filename":"src/file-{index}.ts","status":"modified","additions":1,"deletions":1,"patch":"@@ -1 +1 @@\n-a\n+b"}}"#
            )
        };
        let files: Vec<String> = (0..PER_PAGE).map(file).collect();
        format!("[{}]", files.join(","))
    }

    #[test]
    fn every_rest_status_maps_and_unknown_becomes_changed() {
        let files = pr_files();
        let statuses: Vec<(&str, FileStatus)> = files
            .iter()
            .map(|file| (file.path.as_str(), file.status))
            .collect();
        assert_eq!(
            statuses,
            vec![
                ("src/reschedule/retry-budget.ts", FileStatus::Added),
                ("src/reschedule/slot-finder.ts", FileStatus::Modified),
                ("src/legacy/old.ts", FileStatus::Removed),
                ("src/core/scheduler.ts", FileStatus::Renamed),
                ("src/core/clock.ts", FileStatus::Renamed),
                ("src/core/template.ts", FileStatus::Copied),
                ("scripts/run.sh", FileStatus::Changed),
                ("README.md", FileStatus::Unchanged),
                ("assets/logo.png", FileStatus::Modified),
                ("vendor/odd.bin", FileStatus::Changed),
            ]
        );
    }

    #[test]
    fn missing_status_is_changed() {
        assert_eq!(FileStatus::from_wire(None), FileStatus::Changed);
    }

    #[test]
    fn added_file_carries_counts_patch_and_blob_sha() {
        let file = &pr_files()[0];
        assert_eq!(file.previous_path, None);
        assert_eq!((file.additions, file.deletions), (12, 0));
        assert_eq!(
            file.patch.as_deref(),
            Some("@@ -0,0 +1,12 @@\n+export const RETRY_BUDGET = 3;\n")
        );
        assert_eq!(
            file.sha.as_deref(),
            Some("b1c2d3e4f5a6978800112233445566778899aabb")
        );
    }

    #[test]
    fn renames_keep_the_previous_path_with_or_without_a_patch() {
        let files = pr_files();
        assert_eq!(files[3].previous_path.as_deref(), Some("src/scheduler.ts"));
        assert!(files[3].patch.is_some());
        assert_eq!(files[4].previous_path.as_deref(), Some("src/clock.ts"));
        assert_eq!(files[4].patch, None);
    }

    #[test]
    fn binary_file_has_no_patch() {
        let files = pr_files();
        let binary = files.iter().find(|file| file.path == "assets/logo.png");
        assert_eq!(binary.unwrap().patch, None);
    }

    #[test]
    fn commit_response_reads_the_files_field() {
        let response: CommitResponse = serde_json::from_str(COMMIT).unwrap();
        let files: Vec<PrFile> = response.files.into_iter().map(PrFile::from).collect();
        let summary: Vec<(&str, FileStatus, u32, u32)> = files
            .iter()
            .map(|file| {
                (
                    file.path.as_str(),
                    file.status,
                    file.additions,
                    file.deletions,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("src/reschedule/retry-budget.ts", FileStatus::Added, 4, 0),
                ("src/reschedule/slot-finder.ts", FileStatus::Modified, 3, 2),
                ("src/core/clock.ts", FileStatus::Renamed, 0, 0),
            ]
        );
        assert_eq!(files[2].previous_path.as_deref(), Some("src/clock.ts"));
    }

    #[test]
    fn commit_response_without_files_is_empty() {
        for json in [r#"{ "sha": "abc" }"#, r#"{ "files": null }"#] {
            let response: CommitResponse = serde_json::from_str(json).unwrap();
            assert!(response.files.is_empty());
        }
    }

    #[test]
    fn paths_target_the_right_endpoints_and_reject_bad_input() {
        assert_eq!(
            pr_files_path("orbita-health/atlas", 412).unwrap(),
            "/repos/orbita-health/atlas/pulls/412/files"
        );
        assert_eq!(
            commit_path("orbita-health/atlas", "3fa91c0de4").unwrap(),
            "/repos/orbita-health/atlas/commits/3fa91c0de4"
        );
        assert!(pr_files_path("../atlas", 1).is_err());
        assert!(commit_path("orbita-health/atlas", "abc/../x").is_err());
        assert!(commit_path("orbita-health/atlas", "").is_err());
    }

    #[tokio::test]
    async fn pr_files_follows_pages_in_api_order() {
        let port = serve(|request_line| {
            if request_line.contains("/pulls/7/files?per_page=100&page=1") {
                json_response("200 OK", &full_page_body())
            } else if request_line.contains("/pulls/7/files?per_page=100&page=2") {
                json_response("200 OK", PR_FILES)
            } else {
                json_response("500 Internal Server Error", "{}")
            }
        })
        .await;

        let files = client_for(port)
            .pr_files("orbita-health/atlas", 7)
            .await
            .unwrap();

        assert_eq!(files.len(), PER_PAGE + 10);
        assert_eq!(files[0].path, "src/file-0.ts");
        assert_eq!(files[PER_PAGE].path, "src/reschedule/retry-budget.ts");
    }

    #[tokio::test]
    async fn pr_files_stops_at_the_page_cap() {
        let port = serve(|_| json_response("200 OK", &full_page_body())).await;

        let files = client_for(port)
            .pr_files("orbita-health/atlas", 7)
            .await
            .unwrap();

        assert_eq!(files.len(), PER_PAGE * PR_FILES_MAX_PAGES as usize);
    }

    #[tokio::test]
    async fn commit_files_pages_are_capped_at_three() {
        let body = format!(r#"{{"files":{}}}"#, full_page_body());
        let port = serve(move |request_line| {
            assert!(request_line.contains("/commits/3fa91c0de4?per_page=100&page="));
            json_response("200 OK", &body)
        })
        .await;

        let files = client_for(port)
            .commit_files("orbita-health/atlas", "3fa91c0de4")
            .await
            .unwrap();

        assert_eq!(files.len(), PER_PAGE * COMMIT_FILES_MAX_PAGES as usize);
    }

    #[tokio::test]
    async fn commit_files_stops_on_a_short_page() {
        let port = serve(|_| json_response("200 OK", COMMIT)).await;

        let files = client_for(port)
            .commit_files("orbita-health/atlas", "3fa91c0de4")
            .await
            .unwrap();

        assert_eq!(files.len(), 3);
    }
}
