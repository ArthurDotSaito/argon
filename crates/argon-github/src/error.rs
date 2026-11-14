use chrono::{DateTime, Utc};

#[derive(Debug, thiserror::Error)]
pub enum GithubError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("graphql errors: {}", .0.join("; "))]
    GraphQl(Vec<String>),
    #[error("{0} ran but did not return its result")]
    Unconfirmed(String),
    #[error("no gh authentication for host {host}")]
    AuthMissing { host: String },
    #[error("rate limited until {reset_at}")]
    RateLimited { reset_at: DateTime<Utc> },
    #[error("release tag is not a valid version: {0}")]
    InvalidReleaseTag(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<serde_json::Error> for GithubError {
    fn from(error: serde_json::Error) -> Self {
        Self::Decode(error.to_string())
    }
}

impl From<serde_yaml_ng::Error> for GithubError {
    fn from(error: serde_yaml_ng::Error) -> Self {
        Self::Decode(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, GithubError>;
