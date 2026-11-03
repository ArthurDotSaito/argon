mod checkout;
mod error;
mod invocation;
mod layout;
mod mirror;

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use argon_core::Paths;

use checkout::Slot;
pub use error::{Result, WorktreeError};
use invocation::{FETCH_TIMEOUT, Invocation, WORKTREE_TIMEOUT};
use layout::{
    RepoRef, mirror_dir, mirror_from_gitfile, pr_ref, session_prefix, validate_host,
    worktree_parent,
};

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
        let repos_root = canonical_root(&self.repos_dir).await?;
        let worktrees_root = canonical_root(&self.worktrees_dir).await?;
        let mirror = mirror_dir(&repos_root, host, &repo);

        let lock = self.lock_for(&mirror);
        let _guard = lock.lock().await;

        mirror::ensure(host, &repo, &mirror).await?;
        let reference = pr_ref(number);
        Invocation::git_authenticated(&mirror, host)
            .args(["fetch", "origin"])
            .arg(format!("+pull/{number}/head:{reference}"))
            .run(FETCH_TIMEOUT)
            .await?;
        prune(&mirror).await?;

        let parent = worktree_parent(&worktrees_root, host, &repo);
        let slot = checkout::pick_slot(&parent, number, &prefix, session).await?;
        let target = slot.path().to_path_buf();
        self.clear_slot(&mirror, slot).await?;
        add_worktree(&mirror, host, &target, &reference).await?;
        checkout::claim(&target, session).await?;
        Ok(target)
    }

    pub async fn remove(&self, path: &Path) -> Result<()> {
        if !tokio::fs::try_exists(path).await? {
            return Ok(());
        }
        let worktree = canonical_within(&self.worktrees_dir, path)?;
        let gitfile = tokio::fs::read_to_string(worktree.join(".git"))
            .await
            .map_err(|_| WorktreeError::InvalidWorktree(worktree.clone()))?;
        let mirror = mirror_from_gitfile(&worktree, &gitfile)
            .ok_or_else(|| WorktreeError::InvalidWorktree(worktree.clone()))?;
        let mirror = match canonical_within(&self.repos_dir, &mirror) {
            Ok(mirror) => mirror,
            Err(WorktreeError::Io(error)) if error.kind() == ErrorKind::NotFound => {
                tokio::fs::remove_dir_all(&worktree).await?;
                return Ok(());
            }
            Err(error) => return Err(error),
        };

        let lock = self.lock_for(&mirror);
        let _guard = lock.lock().await;
        unregister(&mirror, &worktree).await
    }

    pub async fn head_oid(&self, path: &Path) -> Result<String> {
        Invocation::git(path)
            .args(["rev-parse", "HEAD"])
            .run(WORKTREE_TIMEOUT)
            .await
    }

    async fn clear_slot(&self, mirror: &Path, slot: Slot) -> Result<()> {
        match slot {
            Slot::Free(_) => Ok(()),
            Slot::Unregistered(path) => {
                canonical_within(&self.worktrees_dir, &path)?;
                tokio::fs::remove_dir_all(&path).await?;
                prune(mirror).await
            }
            Slot::Reentry(path) => {
                let path = canonical_within(&self.worktrees_dir, &path)?;
                if unregister(mirror, &path).await.is_err() {
                    tokio::fs::remove_dir_all(&path).await?;
                    prune(mirror).await?;
                }
                Ok(())
            }
        }
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

async fn canonical_root(root: &Path) -> Result<PathBuf> {
    tokio::fs::create_dir_all(root).await?;
    Ok(dunce::canonicalize(root)?)
}

fn canonical_within(root: &Path, path: &Path) -> Result<PathBuf> {
    let outside = || WorktreeError::OutsideRoot(path.to_path_buf());
    let root = dunce::canonicalize(root).map_err(|_| outside())?;
    let canonical = dunce::canonicalize(path)?;
    if canonical.starts_with(&root) && canonical != root {
        Ok(canonical)
    } else {
        Err(outside())
    }
}

async fn prune(mirror: &Path) -> Result<()> {
    Invocation::git(mirror)
        .args(["worktree", "prune"])
        .run(WORKTREE_TIMEOUT)
        .await?;
    Ok(())
}

async fn unregister(mirror: &Path, worktree: &Path) -> Result<()> {
    Invocation::git(mirror)
        .args(["worktree", "remove", "--force"])
        .arg(worktree)
        .run(WORKTREE_TIMEOUT)
        .await?;
    prune(mirror).await
}

async fn add_worktree(mirror: &Path, host: &str, target: &Path, reference: &str) -> Result<()> {
    let attempt = || {
        Invocation::git_authenticated(mirror, host)
            .args(["worktree", "add", "--detach"])
            .arg(target)
            .arg(reference)
            .run(WORKTREE_TIMEOUT)
    };
    match attempt().await {
        Err(WorktreeError::Git { stderr, .. }) if stderr.contains("already registered") => {
            prune(mirror).await?;
            attempt().await.map(drop)
        }
        other => other.map(drop),
    }
}
