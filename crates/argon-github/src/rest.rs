use reqwest::header::{ACCEPT, USER_AGENT};
use reqwest::{Method, RequestBuilder, Url};
use serde::de::DeserializeOwned;

use crate::client::GithubClient;
use crate::error::{GithubError, Result};

const API_VERSION: &str = "2022-11-28";

pub(crate) fn repo_path(repo: &str) -> Result<&str> {
    let valid_part = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    match repo.split_once('/') {
        Some((owner, name)) if valid_part(owner) && valid_part(name) => Ok(repo),
        _ => Err(GithubError::Decode(format!(
            "invalid repository name: {repo}"
        ))),
    }
}

impl GithubClient {
    pub(crate) fn rest_url_for(&self, path: &str, params: &[(&str, &str)]) -> Result<Url> {
        let mut url = Url::parse(&format!("{}{path}", self.rest_url))
            .map_err(|error| GithubError::Decode(error.to_string()))?;
        if !params.is_empty() {
            url.query_pairs_mut().extend_pairs(params);
        }
        Ok(url)
    }

    pub(crate) fn rest_request(&self, method: Method, url: Url) -> RequestBuilder {
        self.http
            .request(method, url)
            .bearer_auth(&self.token)
            .header(USER_AGENT, concat!("argon/", env!("CARGO_PKG_VERSION")))
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
    }

    pub(crate) async fn rest_get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<T> {
        let url = self.rest_url_for(path, params)?;
        let response = self.rest_request(Method::GET, url).send().await?;
        let body = self.check_status(response)?.bytes().await?;
        Ok(serde_json::from_slice(&body)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_path_accepts_owner_and_name() {
        assert_eq!(
            repo_path("orbita-health/atlas").unwrap(),
            "orbita-health/atlas"
        );
        assert_eq!(repo_path("a_b/c.d-e").unwrap(), "a_b/c.d-e");
    }

    #[test]
    fn repo_path_rejects_traversal_and_malformed_names() {
        for repo in [
            "", "atlas", "a/b/c", "../x", "a/..", "a/b?x=1", "a /b", "/b", "a/",
        ] {
            assert!(repo_path(repo).is_err(), "{repo} should be rejected");
        }
    }
}
