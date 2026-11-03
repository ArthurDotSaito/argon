use std::path::{Path, PathBuf};

use super::error::{Result, WorktreeError};

const SESSION_PREFIX_LEN: usize = 12;
const GIT_SUFFIX: &str = ".git";
const GITDIR_KEY: &str = "gitdir:";
const TMP_MARKER: &str = ".tmp-";
const RESERVED_NAMES: [&str; 4] = ["con", "prn", "aux", "nul"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RepoRef {
    pub owner: String,
    pub name: String,
}

impl RepoRef {
    pub fn parse(input: &str) -> Result<RepoRef> {
        let invalid = || WorktreeError::InvalidRepo(input.to_string());
        let (owner, name) = input.trim().split_once('/').ok_or_else(invalid)?;
        let name = name.strip_suffix(GIT_SUFFIX).unwrap_or(name);
        if !is_safe_component(owner, true) || !is_safe_component(name, true) {
            return Err(invalid());
        }
        Ok(RepoRef {
            owner: owner.to_string(),
            name: name.to_string(),
        })
    }

    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

pub(super) fn validate_host(host: &str) -> Result<&str> {
    if is_safe_component(host, false) {
        Ok(host)
    } else {
        Err(WorktreeError::InvalidRepo(host.to_string()))
    }
}

pub(super) fn session_prefix(session: &str) -> Result<String> {
    let prefix: String = session.chars().take(SESSION_PREFIX_LEN).collect();
    let valid = !prefix.is_empty()
        && prefix
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if valid {
        Ok(prefix)
    } else {
        Err(WorktreeError::InvalidRepo(session.to_string()))
    }
}

pub(super) fn mirror_dir(repos_root: &Path, host: &str, repo: &RepoRef) -> PathBuf {
    repos_root.join(host).join(&repo.owner).join(&repo.name)
}

pub(super) fn worktree_parent(worktrees_root: &Path, host: &str, repo: &RepoRef) -> PathBuf {
    worktrees_root.join(host).join(&repo.owner).join(&repo.name)
}

pub(super) fn worktree_dir_name(number: u64, session_prefix: &str, suffix: Option<&str>) -> String {
    match suffix {
        Some(suffix) => format!("pr-{number}-{session_prefix}-{suffix}"),
        None => format!("pr-{number}-{session_prefix}"),
    }
}

pub(super) fn pr_ref(number: u64) -> String {
    format!("refs/argon/pr-{number}")
}

pub(super) fn tmp_clone_dir(mirror: &Path, pid: u32) -> Option<PathBuf> {
    let name = mirror.file_name()?.to_str()?;
    Some(mirror.with_file_name(format!("{name}{TMP_MARKER}{pid}")))
}

pub(super) fn is_tmp_clone_name(entry: &str, repo_name: &str) -> bool {
    entry
        .strip_prefix(repo_name)
        .and_then(|rest| rest.strip_prefix(TMP_MARKER))
        .is_some_and(|pid| !pid.is_empty() && pid.chars().all(|ch| ch.is_ascii_digit()))
}

pub(super) fn admin_dir_from_gitfile(worktree: &Path, contents: &str) -> Option<PathBuf> {
    let raw = contents
        .lines()
        .find_map(|line| line.trim().strip_prefix(GITDIR_KEY))?
        .trim();
    if raw.is_empty() {
        return None;
    }
    Some(worktree.join(raw))
}

pub(super) fn mirror_from_gitfile(worktree: &Path, contents: &str) -> Option<PathBuf> {
    let admin_dir = admin_dir_from_gitfile(worktree, contents)?;
    let worktrees_dir = admin_dir.parent()?;
    if worktrees_dir.file_name()? != "worktrees" {
        return None;
    }
    let git_dir = worktrees_dir.parent()?;
    if git_dir.file_name()? != GIT_SUFFIX {
        return None;
    }
    Some(git_dir.parent()?.to_path_buf())
}

fn is_safe_component(value: &str, allow_underscore: bool) -> bool {
    let allowed = |ch: char| {
        ch.is_ascii_alphanumeric() || ch == '-' || ch == '.' || (allow_underscore && ch == '_')
    };
    !value.is_empty()
        && value.chars().all(allowed)
        && !value.starts_with('-')
        && !value.ends_with('.')
        && !value.chars().all(|ch| ch == '.')
        && !is_reserved_name(value)
}

fn is_reserved_name(value: &str) -> bool {
    let stem = value
        .split('.')
        .next()
        .unwrap_or(value)
        .to_ascii_lowercase();
    if RESERVED_NAMES.contains(&stem.as_str()) {
        return true;
    }
    ["com", "lpt"].iter().any(|device| {
        stem.strip_prefix(device)
            .is_some_and(|digit| matches!(digit.as_bytes(), [b'1'..=b'9']))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> RepoRef {
        RepoRef::parse("nuria-tech/nuria-auth").unwrap()
    }

    #[test]
    fn parses_owner_and_name() {
        assert_eq!(
            repo(),
            RepoRef {
                owner: "nuria-tech".into(),
                name: "nuria-auth".into()
            }
        );
        assert_eq!(repo().slug(), "nuria-tech/nuria-auth");
    }

    #[test]
    fn strips_git_suffix_and_whitespace() {
        let parsed = RepoRef::parse("  acme/widgets.git ").unwrap();
        assert_eq!(parsed.slug(), "acme/widgets");
    }

    #[test]
    fn keeps_dots_inside_names() {
        assert_eq!(RepoRef::parse("acme/site.io").unwrap().name, "site.io");
        assert_eq!(RepoRef::parse("acme/.github").unwrap().name, ".github");
    }

    #[test]
    fn rejects_malformed_repos() {
        for input in [
            "",
            "acme",
            "acme/",
            "/widgets",
            "acme/widgets/extra",
            "../widgets",
            "acme/..",
            "acme/.",
            "ac me/widgets",
            "acme\\widgets",
            "acme/wid:gets",
            "acme/.git",
            "acme/widgets.",
            "-acme/widgets",
            "acme/-widgets",
            "acme/...",
        ] {
            assert!(RepoRef::parse(input).is_err(), "{input} should be rejected");
        }
    }

    #[test]
    fn rejects_windows_reserved_names() {
        for input in [
            "acme/con",
            "acme/CON",
            "acme/nul.txt",
            "acme/Aux",
            "acme/prn",
            "acme/com1",
            "acme/COM9.log",
            "acme/lpt3",
            "lpt1/widgets",
        ] {
            assert!(RepoRef::parse(input).is_err(), "{input} should be rejected");
        }
        for input in ["acme/com0", "acme/com10", "acme/console", "acme/lpt"] {
            assert!(RepoRef::parse(input).is_ok(), "{input} should be accepted");
        }
    }

    #[test]
    fn validates_hosts() {
        assert_eq!(validate_host("github.com").unwrap(), "github.com");
        assert_eq!(
            validate_host("ghe.acme-corp.io").unwrap(),
            "ghe.acme-corp.io"
        );
        for host in ["", "..", "a/b", "host:8080", "a b", "a_b", "-host", "nul"] {
            assert!(validate_host(host).is_err(), "{host} should be rejected");
        }
    }

    #[test]
    fn session_prefix_takes_first_twelve_chars() {
        assert_eq!(
            session_prefix("0190a3b2-7c11-7e2f-9a10-123456789abc").unwrap(),
            "0190a3b2-7c1"
        );
        assert_eq!(session_prefix("abc").unwrap(), "abc");
    }

    #[test]
    fn session_prefix_rejects_unsafe_ids() {
        for session in ["", "../../x", "a/b", "ab cd"] {
            assert!(
                session_prefix(session).is_err(),
                "{session} should be rejected"
            );
        }
    }

    #[test]
    fn builds_mirror_dir() {
        let repos = Path::new("data").join("repos");
        assert_eq!(
            mirror_dir(&repos, "github.com", &repo()),
            repos
                .join("github.com")
                .join("nuria-tech")
                .join("nuria-auth")
        );
    }

    #[test]
    fn builds_worktree_paths() {
        let root = Path::new("data").join("worktrees");
        let parent = worktree_parent(&root, "github.com", &repo());
        assert_eq!(
            parent,
            root.join("github.com")
                .join("nuria-tech")
                .join("nuria-auth")
        );
        assert_eq!(
            worktree_dir_name(42, "0190a3b2-7c1", None),
            "pr-42-0190a3b2-7c1"
        );
        assert_eq!(
            worktree_dir_name(42, "0190a3b2-7c1", Some("a1f0")),
            "pr-42-0190a3b2-7c1-a1f0"
        );
    }

    #[test]
    fn builds_pr_ref() {
        assert_eq!(pr_ref(42), "refs/argon/pr-42");
    }

    #[test]
    fn builds_tmp_clone_dir_next_to_mirror() {
        let mirror = Path::new("repos")
            .join("github.com")
            .join("acme")
            .join("widgets");
        assert_eq!(
            tmp_clone_dir(&mirror, 4242),
            Some(mirror.with_file_name("widgets.tmp-4242"))
        );
    }

    #[test]
    fn recognizes_only_tmp_clones_of_the_same_repo() {
        assert!(is_tmp_clone_name("widgets.tmp-4242", "widgets"));
        assert!(!is_tmp_clone_name("widgets", "widgets"));
        assert!(!is_tmp_clone_name("widgets.tmp-", "widgets"));
        assert!(!is_tmp_clone_name("widgets.tmp-abc", "widgets"));
        assert!(!is_tmp_clone_name("gadgets.tmp-1", "widgets"));
        assert!(!is_tmp_clone_name("widgets2.tmp-1", "widgets"));
    }

    #[test]
    fn finds_admin_dir_in_gitfile() {
        let wt = Path::new("wt");
        assert_eq!(
            admin_dir_from_gitfile(wt, "gitdir: C:/m/.git/worktrees/pr-1\n"),
            Some(PathBuf::from("C:/m/.git/worktrees/pr-1"))
        );
        assert_eq!(admin_dir_from_gitfile(wt, "gitdir:  "), None);
        assert_eq!(admin_dir_from_gitfile(wt, "nothing"), None);
    }

    #[test]
    fn finds_mirror_from_absolute_gitfile() {
        let wt = Path::new("wt");
        let contents = "gitdir: C:/data/repos/github.com/acme/widgets/.git/worktrees/pr-1-abcd\n";
        assert_eq!(
            mirror_from_gitfile(wt, contents),
            Some(PathBuf::from("C:/data/repos/github.com/acme/widgets"))
        );
    }

    #[test]
    fn finds_mirror_from_relative_gitfile() {
        let wt = Path::new("data").join("worktrees").join("pr-1-abcd");
        let contents = "gitdir: ../../repos/widgets/.git/worktrees/pr-1-abcd";
        assert_eq!(
            mirror_from_gitfile(&wt, contents),
            Some(wt.join("../../repos/widgets"))
        );
    }

    #[test]
    fn ignores_gitfiles_that_are_not_linked_worktrees() {
        let wt = Path::new("wt");
        for contents in [
            "",
            "gitdir:",
            "ref: refs/heads/main",
            "gitdir: C:/data/widgets/.git",
            "gitdir: C:/data/widgets/.git/modules/sub",
            "gitdir: C:/data/widgets/other/worktrees/pr-1",
        ] {
            assert_eq!(mirror_from_gitfile(wt, contents), None, "{contents}");
        }
    }
}
