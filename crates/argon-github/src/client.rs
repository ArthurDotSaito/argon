use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use argon_core::DEFAULT_HOST;
use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, USER_AGENT};
use reqwest::{Response, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::error::{GithubError, Result};
use crate::wire::{Envelope, ViewerData};

pub(crate) const USER_AGENT_VALUE: &str = concat!("argon/", env!("CARGO_PKG_VERSION"));
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const FALLBACK_RESET: chrono::Duration = chrono::Duration::seconds(60);
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
    pub(crate) token: String,
    pub(crate) http: reqwest::Client,
    graphql_url: String,
    pub(crate) rest_url: String,
    dropped_nodes: AtomicUsize,
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
            dropped_nodes: AtomicUsize::new(0),
        }
    }

    pub fn rest_url(&self) -> &str {
        &self.rest_url
    }

    pub fn dropped_nodes(&self) -> usize {
        self.dropped_nodes.load(Ordering::Relaxed)
    }

    pub async fn viewer(&self) -> Result<Viewer> {
        let data: ViewerData = self.graphql("viewer", VIEWER_QUERY, json!({})).await?;
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

    pub async fn viewer_id(&self) -> Result<String> {
        let data: ViewerData = self.graphql("viewer_id", VIEWER_QUERY, json!({})).await?;
        Ok(data.viewer.id)
    }

    pub(crate) fn check_status(&self, response: Response) -> Result<Response> {
        let status = response.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(GithubError::AuthMissing {
                host: self.host.clone(),
            });
        }
        if status == StatusCode::TOO_MANY_REQUESTS
            || (status == StatusCode::FORBIDDEN && rate_limit_exhausted(response.headers()))
        {
            return Err(GithubError::RateLimited {
                reset_at: rate_limit_reset(response.headers()),
            });
        }
        Ok(response
            .error_for_status()
            .map_err(reqwest::Error::without_url)?)
    }

    pub(crate) async fn graphql<T: DeserializeOwned>(
        &self,
        operation: &'static str,
        query: &str,
        variables: Value,
    ) -> Result<T> {
        let started = Instant::now();
        let response = self
            .http
            .post(&self.graphql_url)
            .bearer_auth(&self.token)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await?;
        tracing::debug!(
            host = %self.host,
            operation,
            status = response.status().as_u16(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "graphql request"
        );
        let reset_at = rate_limit_reset(response.headers());
        let body = self.check_status(response)?.bytes().await?;
        let envelope: Envelope<T> = serde_json::from_slice(&body)?;
        let parsed = envelope.into_data(reset_at)?;
        if parsed.dropped_nodes > 0 {
            self.dropped_nodes
                .fetch_add(parsed.dropped_nodes, Ordering::Relaxed);
            tracing::warn!(
                host = %self.host,
                operation,
                dropped = parsed.dropped_nodes,
                kinds = ?parsed.dropped_kinds,
                "dropped nodes with graphql errors"
            );
        }
        if !parsed.partial_paths.is_empty() {
            tracing::warn!(
                host = %self.host,
                operation,
                paths = ?parsed.partial_paths,
                "graphql returned partial data, missing fields left empty"
            );
        }
        Ok(parsed.data)
    }
}

fn rate_limit_exhausted(headers: &HeaderMap) -> bool {
    headers
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim() == "0")
}

fn rate_limit_reset(headers: &HeaderMap) -> DateTime<Utc> {
    headers
        .get("x-ratelimit-reset")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<i64>().ok())
        .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
        .unwrap_or_else(|| Utc::now() + FALLBACK_RESET)
}

fn endpoints(host: &str) -> (String, String) {
    if host == DEFAULT_HOST {
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
