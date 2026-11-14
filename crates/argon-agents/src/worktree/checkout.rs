use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use super::error::{Result, WorktreeError};
use super::layout::{WorktreeKind, admin_dir_from_gitfile, worktree_dir_name};

const SESSION_MARKER: &str = "argon-session";
const MAX_SLOT_ATTEMPTS: usize = 16;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Slot {
    Free(PathBuf),
    Unregistered(PathBuf),
    Reentry(PathBuf),
}

impl Slot {
    pub fn path(&self) -> &Path {
        match self {
            Slot::Free(path) | Slot::Unregistered(path) | Slot::Reentry(path) => path,
        }
    }
}

pub(super) async fn pick_slot(
    parent: &Path,
    kind: WorktreeKind,
    number: u64,
    session_prefix: &str,
    session: &str,
) -> Result<Slot> {
    for attempt in 0..MAX_SLOT_ATTEMPTS {
        let suffix = (attempt > 0).then(random_suffix);
        let path = parent.join(worktree_dir_name(
            kind,
            number,
            session_prefix,
            suffix.as_deref(),
        ));
        if !tokio::fs::try_exists(&path).await? {
            return Ok(Slot::Free(path));
        }
        let Some(admin_dir) = registered_admin_dir(&path).await else {
            return Ok(Slot::Unregistered(path));
        };
        if read_marker(&admin_dir).await.as_deref() == Some(session) {
            return Ok(Slot::Reentry(path));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free worktree directory name",
    )
    .into())
}

pub(super) async fn claim(worktree: &Path, session: &str) -> Result<()> {
    let admin_dir = registered_admin_dir(worktree)
        .await
        .ok_or_else(|| WorktreeError::InvalidWorktree(worktree.to_path_buf()))?;
    tokio::fs::write(admin_dir.join(SESSION_MARKER), session).await?;
    Ok(())
}

async fn registered_admin_dir(worktree: &Path) -> Option<PathBuf> {
    let gitfile = tokio::fs::read_to_string(worktree.join(".git"))
        .await
        .ok()?;
    let admin_dir = admin_dir_from_gitfile(worktree, &gitfile)?;
    tokio::fs::metadata(&admin_dir)
        .await
        .ok()?
        .is_dir()
        .then_some(admin_dir)
}

async fn read_marker(admin_dir: &Path) -> Option<String> {
    tokio::fs::read_to_string(admin_dir.join(SESSION_MARKER))
        .await
        .ok()
}

fn random_suffix() -> String {
    let value = RandomState::new().build_hasher().finish();
    format!("{:04x}", value & 0xffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_suffix_is_four_hex_chars() {
        for _ in 0..20 {
            let suffix = random_suffix();
            assert_eq!(suffix.len(), 4);
            assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()));
        }
    }

    #[tokio::test]
    async fn free_slot_when_directory_is_missing() {
        let parent = tempdir("free");
        let slot = pick_slot(
            &parent,
            WorktreeKind::PullRequest,
            7,
            "abcdef123456",
            "abcdef123456-full",
        )
        .await
        .unwrap();
        assert_eq!(slot, Slot::Free(parent.join("pr-7-abcdef123456")));
        cleanup(&parent);
    }

    #[tokio::test]
    async fn issue_slot_uses_the_issue_prefix() {
        let parent = tempdir("issue");
        let slot = pick_slot(
            &parent,
            WorktreeKind::Issue,
            7,
            "abcdef123456",
            "abcdef123456-full",
        )
        .await
        .unwrap();
        assert_eq!(slot, Slot::Free(parent.join("issue-7-abcdef123456")));
        cleanup(&parent);
    }

    #[tokio::test]
    async fn unregistered_directory_is_flagged_for_cleanup() {
        let parent = tempdir("stale");
        std::fs::create_dir_all(parent.join("pr-7-abcdef123456")).unwrap();
        let slot = pick_slot(
            &parent,
            WorktreeKind::PullRequest,
            7,
            "abcdef123456",
            "abcdef123456-full",
        )
        .await
        .unwrap();
        assert_eq!(slot, Slot::Unregistered(parent.join("pr-7-abcdef123456")));
        cleanup(&parent);
    }

    #[tokio::test]
    async fn same_session_reenters_and_other_session_gets_a_new_name() {
        let parent = tempdir("owner");
        let worktree = parent.join("pr-7-abcdef123456");
        let admin = parent.join("admin");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", admin.display()),
        )
        .unwrap();
        claim(&worktree, "abcdef123456-first").await.unwrap();

        let same = pick_slot(
            &parent,
            WorktreeKind::PullRequest,
            7,
            "abcdef123456",
            "abcdef123456-first",
        )
        .await
        .unwrap();
        assert_eq!(same, Slot::Reentry(worktree.clone()));

        let other = pick_slot(
            &parent,
            WorktreeKind::PullRequest,
            7,
            "abcdef123456",
            "abcdef123456-second",
        )
        .await
        .unwrap();
        assert!(matches!(other, Slot::Free(_)));
        assert_ne!(other.path(), worktree);
        let name = other.path().file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("pr-7-abcdef123456-"), "{name}");
        cleanup(&parent);
    }

    fn tempdir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "argon-checkout-{label}-{}-{}",
            std::process::id(),
            random_suffix()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }
}
