mod checkout;
mod error;
mod invocation;
mod layout;
mod mirror;
mod scan;

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use argon_core::Paths;

use checkout::Slot;
pub use error::{Result, WorktreeError};
use invocation::{FETCH_TIMEOUT, Invocation, WORKTREE_TIMEOUT};
pub(crate) use layout::is_related_dir_name;
use layout::{
    DEFAULT_REF, RepoRef, WorktreeKind, mirror_dir, mirror_from_gitfile, pr_ref, session_prefix,
    validate_host, worktree_dir_name, worktree_parent,
};
pub use scan::{FoundWorktree, orphan_worktrees};

type RepoLock = Arc<tokio::sync::Mutex<()>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedWorktree {
    pub path: PathBuf,
    pub base_sha: String,
    pub branch: String,
}

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
        self.prepare_kind(WorktreeKind::PullRequest, host, repo, Some(number), session)
            .await
            .map(|(path, _)| path)
    }

    pub async fn prepare_default(
        &self,
        host: &str,
        repo: &str,
        number: u64,
        session: &str,
    ) -> Result<PreparedWorktree> {
        self.prepare_default_checkout(WorktreeKind::Issue, host, repo, Some(number), session)
            .await
    }

    pub async fn prepare_related(
        &self,
        host: &str,
        repo: &str,
        session: &str,
    ) -> Result<PreparedWorktree> {
        self.prepare_default_checkout(WorktreeKind::Related, host, repo, None, session)
            .await
    }

    async fn prepare_default_checkout(
        &self,
        kind: WorktreeKind,
        host: &str,
        repo: &str,
        number: Option<u64>,
        session: &str,
    ) -> Result<PreparedWorktree> {
        let (path, branch) = self.prepare_kind(kind, host, repo, number, session).await?;
        match self.head_oid(&path).await {
            Ok(base_sha) => Ok(PreparedWorktree {
                path,
                base_sha,
                branch: branch.unwrap_or_default(),
            }),
            Err(error) => {
                let _ = self.discard(&path).await;
                Err(error)
            }
        }
    }

    async fn prepare_kind(
        &self,
        kind: WorktreeKind,
        host: &str,
        repo: &str,
        number: Option<u64>,
        session: &str,
    ) -> Result<(PathBuf, Option<String>)> {
        let host = validate_host(host)?;
        let repo = RepoRef::parse(repo)?;
        let prefix = session_prefix(session)?;
        let repos_root = canonical_root(&self.repos_dir).await?;
        let worktrees_root = canonical_root(&self.worktrees_dir).await?;
        let mirror = mirror_dir(&repos_root, host, &repo);

        let lock = self.lock_for(&mirror);
        let _guard = lock.lock().await;

        mirror::ensure(host, &repo, &mirror).await?;
        let (reference, branch) = match (kind, number) {
            (WorktreeKind::PullRequest, Some(number)) => {
                let reference = pr_ref(number);
                Invocation::git_authenticated(&mirror, host)
                    .args(pr_fetch_args(number, &reference))
                    .run(FETCH_TIMEOUT)
                    .await?;
                (reference, None)
            }
            _ => {
                let branch = mirror::default_branch(host, &mirror).await?;
                Invocation::git_authenticated(&mirror, host)
                    .args(default_fetch_args(&branch))
                    .run(FETCH_TIMEOUT)
                    .await?;
                (DEFAULT_REF.to_owned(), Some(branch))
            }
        };
        prune(&mirror).await?;

        let parent = worktree_parent(&worktrees_root, host, &repo);
        let slot = checkout::pick_slot(&parent, kind, number, &prefix, session).await?;
        let target = slot.path().to_path_buf();
        self.clear_slot(&mirror, slot).await?;
        add_worktree(&mirror, host, &target, &reference).await?;
        checkout::claim(&target, session).await?;
        Ok((target, branch))
    }

    pub fn planned_path(
        &self,
        host: &str,
        repo: &str,
        number: u64,
        session: &str,
    ) -> Result<PathBuf> {
        self.planned_slot_path(WorktreeKind::PullRequest, host, repo, Some(number), session)
    }

    pub fn planned_default_path(
        &self,
        host: &str,
        repo: &str,
        number: u64,
        session: &str,
    ) -> Result<PathBuf> {
        self.planned_slot_path(WorktreeKind::Issue, host, repo, Some(number), session)
    }

    pub fn planned_related_path(&self, host: &str, repo: &str, session: &str) -> Result<PathBuf> {
        self.planned_slot_path(WorktreeKind::Related, host, repo, None, session)
    }

    fn planned_slot_path(
        &self,
        kind: WorktreeKind,
        host: &str,
        repo: &str,
        number: Option<u64>,
        session: &str,
    ) -> Result<PathBuf> {
        let host = validate_host(host)?;
        let repo = RepoRef::parse(repo)?;
        let prefix = session_prefix(session)?;
        Ok(worktree_parent(&self.worktrees_dir, host, &repo)
            .join(worktree_dir_name(kind, number, &prefix, None)))
    }

    pub async fn discard(&self, path: &Path) -> Result<()> {
        match self.remove(path).await {
            Err(WorktreeError::InvalidWorktree(_)) => {
                let partial = canonical_within(&self.worktrees_dir, path)?;
                tokio::fs::remove_dir_all(&partial).await?;
                Ok(())
            }
            other => other,
        }
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

fn pr_fetch_args(number: u64, reference: &str) -> [String; 4] {
    [
        "fetch".to_owned(),
        "--no-prune".to_owned(),
        "origin".to_owned(),
        format!("+pull/{number}/head:{reference}"),
    ]
}

fn default_fetch_args(branch: &str) -> [String; 4] {
    [
        "fetch".to_owned(),
        "--no-prune".to_owned(),
        "origin".to_owned(),
        format!("+refs/heads/{branch}:{DEFAULT_REF}"),
    ]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> WorktreeManager {
        WorktreeManager::new(&Paths {
            config_dir: PathBuf::from("cfg"),
            data_dir: PathBuf::from("data"),
        })
    }

    #[test]
    fn planned_path_matches_the_first_slot_name() {
        let path = manager()
            .planned_path(
                "github.com",
                "nuria-tech/nuria-auth",
                34,
                "0190a3b27c117e2f9a10",
            )
            .unwrap();
        assert_eq!(
            path,
            Path::new("data/worktrees/github.com/nuria-tech/nuria-auth/pr-34-0190a3b27c11")
        );
    }

    #[test]
    fn planned_default_path_uses_the_issue_slot_name() {
        let path = manager()
            .planned_default_path(
                "github.com",
                "nuria-tech/nuria-auth",
                12,
                "0190a3b27c117e2f9a10",
            )
            .unwrap();
        assert_eq!(
            path,
            Path::new("data/worktrees/github.com/nuria-tech/nuria-auth/issue-12-0190a3b27c11")
        );
        assert!(
            manager()
                .planned_default_path("github.com", "../x", 1, "abc")
                .is_err()
        );
    }

    #[test]
    fn planned_related_path_has_no_number_and_sits_beside_the_other_slots() {
        let manager = manager();
        let session = "0190a3b27c117e2f9a10";
        let path = manager
            .planned_related_path("github.com", "nuria-tech/nuria-auth", session)
            .unwrap();
        assert_eq!(
            path,
            Path::new("data/worktrees/github.com/nuria-tech/nuria-auth/rel-0190a3b27c11")
        );
        let pr = manager
            .planned_path("github.com", "nuria-tech/nuria-auth", 1, session)
            .unwrap();
        assert_eq!(path.parent(), pr.parent());
    }

    #[test]
    fn planned_related_path_rejects_unsafe_input() {
        let manager = manager();
        assert!(
            manager
                .planned_related_path("github.com", "../x", "abc")
                .is_err()
        );
        assert!(
            manager
                .planned_related_path("github.com", "a", "abc")
                .is_err()
        );
        assert!(manager.planned_related_path("a/b", "o/r", "abc").is_err());
        assert!(manager.planned_related_path("", "o/r", "abc").is_err());
        assert!(
            manager
                .planned_related_path("github.com", "o/r", "../x")
                .is_err()
        );
    }

    #[tokio::test]
    async fn prepare_related_rejects_invalid_repo_and_host_before_touching_disk() {
        let manager = manager();
        assert!(matches!(
            manager.prepare_related("github.com", "../x", "abc").await,
            Err(WorktreeError::InvalidRepo(_))
        ));
        assert!(matches!(
            manager.prepare_related("a/b", "o/r", "abc").await,
            Err(WorktreeError::InvalidRepo(_))
        ));
    }

    #[test]
    fn default_fetch_targets_the_argon_default_ref_without_pruning() {
        assert_eq!(
            default_fetch_args("main"),
            [
                "fetch",
                "--no-prune",
                "origin",
                "+refs/heads/main:refs/argon/default"
            ]
        );
    }

    #[test]
    fn pr_fetch_never_prunes_the_review_refs() {
        assert_eq!(
            pr_fetch_args(14, "refs/argon/pr-14"),
            [
                "fetch",
                "--no-prune",
                "origin",
                "+pull/14/head:refs/argon/pr-14"
            ]
        );
    }

    #[test]
    fn planned_path_rejects_unsafe_input() {
        assert!(
            manager()
                .planned_path("github.com", "../x", 1, "abc")
                .is_err()
        );
        assert!(
            manager()
                .planned_path("github.com", "a/b", 1, "../x")
                .is_err()
        );
    }

    #[tokio::test]
    #[ignore = "needs network, git and gh; set ARGON_WORKTREE_TEST_DIR to a scratch directory"]
    async fn prepares_and_removes_review_and_issue_worktrees_with_real_git() {
        let dir = PathBuf::from(
            std::env::var("ARGON_WORKTREE_TEST_DIR").expect("ARGON_WORKTREE_TEST_DIR is not set"),
        );
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dunce::canonicalize(dir).unwrap();
        let manager = WorktreeManager::new(&Paths {
            config_dir: dir.join("config"),
            data_dir: dir.join("data"),
        });
        let (host, repo, session) = ("github.com", "octocat/Hello-World", "issue0-session-real");

        let planned = manager
            .planned_default_path(host, repo, 1, session)
            .unwrap();
        let prepared = manager
            .prepare_default(host, repo, 1, session)
            .await
            .unwrap();
        assert_eq!(prepared.path, planned);
        assert!(prepared.path.join("README").exists());
        assert_eq!(prepared.base_sha.len(), 40);
        let default_tip = Invocation::git(&dir.join("data/repos/github.com/octocat/Hello-World"))
            .args(["rev-parse", DEFAULT_REF])
            .run(WORKTREE_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(prepared.base_sha, default_tip);
        println!(
            "issue worktree: {} @ {}",
            prepared.path.display(),
            prepared.base_sha
        );

        let again = manager
            .prepare_default(host, repo, 1, session)
            .await
            .unwrap();
        assert_eq!(again, prepared);

        let review = manager.prepare(host, repo, 6, session).await.unwrap();
        let review_name = review.file_name().unwrap().to_string_lossy().into_owned();
        assert!(review_name.starts_with("pr-6-"), "{review_name}");
        assert_eq!(
            manager.head_oid(&review).await.unwrap(),
            "762941318ee16e59dabbacb1b4049eec22f0d303"
        );

        let found: Vec<PathBuf> = manager.scan().into_iter().map(|slot| slot.path).collect();
        assert!(found.contains(&prepared.path) && found.contains(&review));
        let orphans = orphan_worktrees(
            &manager.scan(),
            std::slice::from_ref(&review),
            std::time::SystemTime::now() + std::time::Duration::from_secs(60),
        );
        assert_eq!(orphans, std::slice::from_ref(&prepared.path));

        manager.discard(&orphans[0]).await.unwrap();
        manager.remove(&review).await.unwrap();
        assert!(!prepared.path.exists() && !review.exists());
    }
}
