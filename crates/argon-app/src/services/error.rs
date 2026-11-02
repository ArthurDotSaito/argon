use argon_github::GithubError;

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Github(#[from] GithubError),
    #[error("background task panicked")]
    Panicked,
    #[error("background services are not running")]
    Stopped,
}
