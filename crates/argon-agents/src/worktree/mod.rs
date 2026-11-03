mod error;
mod layout;
mod process;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use argon_core::Paths;

pub use error::{Result, WorktreeError};
use layout::{
    RepoRef, mirror_dir, mirror_from_gitfile, pr_ref, session_prefix, validate_host, worktree_dir,
};
use process::{CLONE_TIMEOUT, FETCH_TIMEOUT, Invocation, WORKTREE_TIMEOUT};

type RepoLock = Arc<tokio::sync::Mutex<()>>;

#[derive(Debug, Clone)]
pub struct WorktreeManager {
    repos_dir: PathBuf,
    worktrees_dir: PathBuf,
    locks: Arc<Mutex<HashMap<String, RepoLock>>>,
}

impl WorktreeManager {
    pub fn new(paths: &Paths) -> Self {
        Self {
            repos_dir: paths.repos_dir(),
            worktrees_dir: paths.worktrees_dir(),
            locks: Arc::default(),
        }
    }

    pub async fn prepare(
        &self,
        host: &str,
        repo: &str,
        number: u64,
        session: &str,
    ) -> Result<PathBuf> {
        let host = validate_host(host)?;
        let repo = RepoRef::parse(repo)?;
        let prefix = session_prefix(session)?;
        let mirror = mirror_dir(&self.repos_dir, host, &repo);
        let target = worktree_dir(&self.worktrees_dir, host, &repo, number, &prefix);

        let lock = self.lock_for(&mirror);
        let _guard = lock.lock().await;

        self.ensure_mirror(host, &repo, &mirror).await?;
        let reference = pr_ref(number);
        Invocation::git(&mirror)
            .args(["fetch", "origin"])
            .arg(format!("+pull/{number}/head:{reference}"))
            .run(FETCH_TIMEOUT)
            .await?;
        if tokio::fs::try_exists(&target).await? {
            self.unregister(&mirror, &target).await?;
        }
        Invocation::git(&mirror)
            .args(["worktree", "add", "--detach"])
            .arg(&target)
            .arg(&reference)
            .run(WORKTREE_TIMEOUT)
            .await?;
        Ok(target)
    }

    pub async fn remove(&self, path: &Path) -> Result<()> {
        if !tokio::fs::try_exists(path).await? {
            return Ok(());
        }
        if !path.starts_with(&self.worktrees_dir) {
            return Err(WorktreeError::InvalidWorktree(path.to_path_buf()));
        }
        let gitfile = tokio::fs::read_to_string(path.join(".git"))
            .await
            .map_err(|_| WorktreeError::InvalidWorktree(path.to_path_buf()))?;
        let mirror = mirror_from_gitfile(path, &gitfile)
            .ok_or_else(|| WorktreeError::InvalidWorktree(path.to_path_buf()))?;

        let lock = self.lock_for(&mirror);
        let _guard = lock.lock().await;
        self.unregister(&mirror, path).await
    }

    pub async fn head_oid(&self, path: &Path) -> Result<String> {
        Invocation::git(path)
            .args(["rev-parse", "HEAD"])
            .run(WORKTREE_TIMEOUT)
            .await
    }

    async fn ensure_mirror(&self, host: &str, repo: &RepoRef, mirror: &Path) -> Result<()> {
        if tokio::fs::try_exists(mirror.join(".git")).await? {
            return Ok(());
        }
        if tokio::fs::try_exists(mirror).await? {
            tokio::fs::remove_dir_all(mirror).await?;
        }
        if let Some(parent) = mirror.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let cloned = Invocation::gh(host)
            .args(["repo", "clone"])
            .arg(repo.slug())
            .arg(mirror)
            .args(["--", "--filter=blob:none", "--no-checkout"])
            .run(CLONE_TIMEOUT)
            .await;
        if let Err(error) = cloned {
            let _ = tokio::fs::remove_dir_all(mirror).await;
            return Err(error);
        }
        Invocation::git(mirror)
            .args(["config", "core.longpaths", "true"])
            .run(WORKTREE_TIMEOUT)
            .await?;
        Ok(())
    }

    async fn unregister(&self, mirror: &Path, worktree: &Path) -> Result<()> {
        Invocation::git(mirror)
            .args(["worktree", "remove", "--force"])
            .arg(worktree)
            .run(WORKTREE_TIMEOUT)
            .await?;
        Invocation::git(mirror)
            .args(["worktree", "prune"])
            .run(WORKTREE_TIMEOUT)
            .await?;
        Ok(())
    }

    fn lock_for(&self, mirror: &Path) -> RepoLock {
        let key = mirror
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
            .collect::<Vec<_>>()
            .join("/");
        let mut locks = self.locks.lock().unwrap_or_else(PoisonError::into_inner);
        locks.entry(key).or_default().clone()
    }
}
