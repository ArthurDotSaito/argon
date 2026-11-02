use std::fmt;
use std::time::Duration;

use reqwest::header::USER_AGENT;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::error::Result;
use crate::wire::{Envelope, ViewerData};

const GITHUB_COM: &str = "github.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const VIEWER_QUERY: &str = include_str!("../queries/viewer.graphql");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgInfo {
    pub login: String,
    pub can_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    pub login: String,
    pub orgs: Vec<OrgInfo>,
}

pub struct GithubClient {
    pub(crate) host: String,
    token: String,
    http: reqwest::Client,
    graphql_url: String,
    rest_url: String,
}

impl fmt::Debug for GithubClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GithubClient")
            .field("host", &self.host)
            .finish_non_exhaustive()
    }
}

impl GithubClient {
    pub fn new(host: &str, token: String) -> Self {
        let (graphql_url, rest_url) = endpoints(host);
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self {
            host: host.to_string(),
            token,
            http,
            graphql_url,
            rest_url,
        }
    }

    pub fn rest_url(&self) -> &str {
        &self.rest_url
    }

    pub async fn viewer(&self) -> Result<Viewer> {
        let data: ViewerData = self.graphql(VIEWER_QUERY, json!({})).await?;
        Ok(Viewer {
            login: data.viewer.login,
            orgs: data
                .viewer
                .organizations
                .into_items()
                .into_iter()
                .map(|org| OrgInfo {
                    login: org.login,
                    can_admin: org.viewer_can_administer,
                })
                .collect(),
        })
    }

    pub(crate) async fn graphql<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: Value,
    ) -> Result<T> {
        let response = self
            .http
            .post(&self.graphql_url)
            .bearer_auth(&self.token)
            .header(USER_AGENT, concat!("argon/", env!("CARGO_PKG_VERSION")))
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await?
            .error_for_status()?;
        let body = response.bytes().await?;
        let envelope: Envelope<T> = serde_json::from_slice(&body)?;
        envelope.into_data()
    }
}

fn endpoints(host: &str) -> (String, String) {
    if host == GITHUB_COM {
        (
            "https://api.github.com/graphql".to_string(),
            "https://api.github.com".to_string(),
        )
    } else {
        (
            format!("https://{host}/api/graphql"),
            format!("https://{host}/api/v3"),
        )
    }
}
