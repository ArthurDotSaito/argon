use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::error::Result;
use super::invocation::{CLONE_TIMEOUT, FETCH_TIMEOUT, Invocation, WORKTREE_TIMEOUT};
use super::layout::{RepoRef, is_tmp_clone_name, parse_default_branch, tmp_clone_dir};

const STALE_LOCK_AGE: Duration = Duration::from_secs(10 * 60);
const RENAME_ATTEMPTS: u32 = 5;
const RENAME_RETRY_DELAY: Duration = Duration::from_millis(300);

pub(super) async fn ensure(host: &str, repo: &RepoRef, mirror: &Path) -> Result<()> {
    remove_orphan_clones(mirror).await;
    if tokio::fs::try_exists(mirror.join(".git")).await? {
        remove_stale_locks(mirror).await;
        return Ok(());
    }
    if tokio::fs::try_exists(mirror).await? {
        tokio::fs::remove_dir_all(mirror).await?;
    }
    if let Some(parent) = mirror.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let Some(staging) = tmp_clone_dir(mirror, std::process::id()) else {
        return Err(super::error::WorktreeError::InvalidRepo(repo.slug()));
    };
    let published = clone_and_publish(host, repo, &staging, mirror).await;
    if published.is_err() {
        let _ = tokio::fs::remove_dir_all(&staging).await;
    }
    published
}

pub(super) async fn default_branch(host: &str, mirror: &Path) -> Result<String> {
    let output = Invocation::git_authenticated(mirror, host)
        .args(["ls-remote", "--symref", "origin", "HEAD"])
        .run(FETCH_TIMEOUT)
        .await?;
    parse_default_branch(&output).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "the repository has no default branch to branch from",
        )
        .into()
    })
}

async fn clone_and_publish(
    host: &str,
    repo: &RepoRef,
    staging: &Path,
    mirror: &Path,
) -> Result<()> {
    Invocation::gh(host)
        .args(["repo", "clone"])
        .arg(repo.slug())
        .arg(staging)
        .args(["--", "--filter=blob:none", "--no-checkout"])
        .run(CLONE_TIMEOUT)
        .await?;
    Invocation::git(staging)
        .args(["config", "core.longpaths", "true"])
        .run(WORKTREE_TIMEOUT)
        .await?;
    rename_with_retry(staging, mirror).await
}

async fn rename_with_retry(from: &Path, to: &Path) -> Result<()> {
    let mut attempt = 1;
    loop {
        match tokio::fs::rename(from, to).await {
            Ok(()) => return Ok(()),
            Err(error) if attempt >= RENAME_ATTEMPTS => return Err(error.into()),
            Err(_) => {
                attempt += 1;
                tokio::time::sleep(RENAME_RETRY_DELAY).await;
            }
        }
    }
}

async fn remove_orphan_clones(mirror: &Path) {
    let (Some(parent), Some(name)) = (
        mirror.parent(),
        mirror.file_name().and_then(|name| name.to_str()),
    ) else {
        return;
    };
    let Ok(mut entries) = tokio::fs::read_dir(parent).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let is_orphan = entry
            .file_name()
            .to_str()
            .is_some_and(|entry_name| is_tmp_clone_name(entry_name, name));
        if is_orphan {
            tracing::warn!(path = %entry.path().display(), "removing orphan clone directory");
            let _ = tokio::fs::remove_dir_all(entry.path()).await;
        }
    }
}

async fn remove_stale_locks(mirror: &Path) {
    let git_dir = mirror.join(".git");
    let now = SystemTime::now();
    for dir in [git_dir.join("refs").join("argon"), git_dir] {
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let is_lock = entry.file_name().to_string_lossy().ends_with(".lock");
            let modified = entry.metadata().await.and_then(|meta| meta.modified());
            let stale = is_lock && modified.is_ok_and(|time| is_stale(time, now));
            if stale && tokio::fs::remove_file(entry.path()).await.is_ok() {
                tracing::warn!(path = %entry.path().display(), "removed stale git lock file");
            }
        }
    }
}

fn is_stale(modified: SystemTime, now: SystemTime) -> bool {
    now.duration_since(modified)
        .is_ok_and(|age| age >= STALE_LOCK_AGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_older_than_ten_minutes_are_stale() {
        let now = SystemTime::now();
        assert!(is_stale(now - Duration::from_secs(11 * 60), now));
        assert!(is_stale(now - STALE_LOCK_AGE, now));
        assert!(!is_stale(now - Duration::from_secs(9 * 60), now));
        assert!(!is_stale(now + Duration::from_secs(60), now));
    }
}
