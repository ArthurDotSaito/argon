use std::path::PathBuf;
use std::process::ExitStatus;

#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("`{cmd}` failed ({status}): {stderr}")]
    Git {
        cmd: String,
        status: ExitStatus,
        stderr: String,
    },
    #[error("`{cmd}` timed out")]
    Timeout { cmd: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("invalid repository `{0}`")]
    InvalidRepo(String),
    #[error("not a managed worktree: {}", .0.display())]
    InvalidWorktree(PathBuf),
    #[error("path is outside the managed directories: {}", .0.display())]
    OutsideRoot(PathBuf),
}

pub type Result<T> = std::result::Result<T, WorktreeError>;
