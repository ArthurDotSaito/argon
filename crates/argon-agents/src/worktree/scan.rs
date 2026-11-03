use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::WorktreeManager;

const SLOT_DEPTH: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundWorktree {
    pub path: PathBuf,
    pub modified: SystemTime,
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<FoundWorktree>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() {
            continue;
        }
        let path = entry.path();
        if depth + 1 == SLOT_DEPTH {
            let modified = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            out.push(FoundWorktree { path, modified });
        } else {
            walk(&path, depth + 1, out);
        }
    }
}

fn key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

pub fn orphan_worktrees(
    found: &[FoundWorktree],
    known: &[PathBuf],
    older_than: SystemTime,
) -> Vec<PathBuf> {
    let known: Vec<String> = known.iter().map(|path| key(path)).collect();
    found
        .iter()
        .filter(|worktree| worktree.modified < older_than)
        .filter(|worktree| !known.contains(&key(&worktree.path)))
        .map(|worktree| worktree.path.clone())
        .collect()
}

impl WorktreeManager {
    pub fn scan(&self) -> Vec<FoundWorktree> {
        let mut found = Vec::new();
        walk(&self.worktrees_dir, 0, &mut found);
        found
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn found(path: &str, age: u64, now: SystemTime) -> FoundWorktree {
        FoundWorktree {
            path: PathBuf::from(path),
            modified: now - Duration::from_secs(age),
        }
    }

    #[test]
    fn orphans_are_old_worktrees_without_a_session_row() {
        let now = SystemTime::now();
        let tree = [
            found(r"C:\wt\github.com\o\r\pr-1-aaaa", 600, now),
            found(r"C:\wt\github.com\o\r\pr-2-bbbb", 600, now),
            found(r"C:\wt\github.com\o\r\pr-3-cccc", 1, now),
        ];
        let known = [PathBuf::from("c:/WT/github.com/o/r/pr-1-aaaa/")];
        let orphans = orphan_worktrees(&tree, &known, now - Duration::from_secs(60));
        assert_eq!(orphans, [PathBuf::from(r"C:\wt\github.com\o\r\pr-2-bbbb")]);
    }

    #[test]
    fn scan_lists_only_slot_directories() {
        let root = std::env::temp_dir().join(format!("argon-scan-{}", std::process::id()));
        let slot = root
            .join("github.com")
            .join("o")
            .join("r")
            .join("pr-1-aaaa");
        std::fs::create_dir_all(slot.join("src")).unwrap();
        std::fs::write(root.join("github.com").join("stray.txt"), "x").unwrap();
        let mut found = Vec::new();
        walk(&root, 0, &mut found);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, slot);
    }
}
