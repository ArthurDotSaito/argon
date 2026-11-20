use std::path::{Path, PathBuf};

use crate::{ReviewCfg, is_repo_full_name};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelatedEntry {
    Repo(String),
    Local(PathBuf),
}

impl RelatedEntry {
    pub fn parse(text: &str) -> Option<RelatedEntry> {
        let text = text.trim();
        if Path::new(text).is_absolute() {
            Some(Self::Local(PathBuf::from(text)))
        } else if is_repo_full_name(text) {
            Some(Self::Repo(text.to_owned()))
        } else {
            None
        }
    }
}

pub fn related_entries(cfg: &ReviewCfg, host: &str, repo: &str) -> Vec<RelatedEntry> {
    cfg.related_repos(host, repo)
        .iter()
        .filter_map(|text| RelatedEntry::parse(text))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalFolderError {
    Missing,
    NotADirectory,
    DriveRoot,
    UserHome,
}

pub fn user_home() -> Option<PathBuf> {
    dirs::home_dir()
}

pub fn local_folder_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn comparable(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.trim_end_matches('/');
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.to_owned()
    }
}

fn resolved_key(path: &Path) -> String {
    comparable(&std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
}

pub fn same_local_folder(left: &Path, right: &Path) -> bool {
    resolved_key(left) == resolved_key(right)
}

pub fn checked_local_folder(path: &Path, home: Option<&Path>) -> Result<PathBuf, LocalFolderError> {
    if !path.is_absolute() {
        return Err(LocalFolderError::Missing);
    }
    let resolved = std::fs::canonicalize(path).map_err(|_| LocalFolderError::Missing)?;
    if !resolved.is_dir() {
        return Err(LocalFolderError::NotADirectory);
    }
    if resolved.parent().is_none() {
        return Err(LocalFolderError::DriveRoot);
    }
    if home.is_some_and(|home| resolved_key(home) == comparable(&resolved)) {
        return Err(LocalFolderError::UserHome);
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let path =
                std::env::temp_dir().join(format!("argon-related-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn absolute(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    fn cfg(items: &[String]) -> ReviewCfg {
        ReviewCfg {
            repo_related: BTreeMap::from([("github.com/acme/api".to_owned(), items.to_vec())]),
            ..ReviewCfg::default()
        }
    }

    #[test]
    fn entries_classify_repos_and_absolute_folders_and_skip_the_rest() {
        let folder = absolute("pacientes-api");
        let items = vec![
            "acme/web".to_owned(),
            format!("  {}  ", folder.display()),
            "relative/dir/x".to_owned(),
            "acme".to_owned(),
            String::new(),
            "https://github.com/acme/x".to_owned(),
        ];

        let entries = related_entries(&cfg(&items), "github.com", "acme/api");

        assert_eq!(
            entries,
            [
                RelatedEntry::Repo("acme/web".into()),
                RelatedEntry::Local(folder),
            ]
        );
        assert!(related_entries(&cfg(&items), "github.com", "acme/other").is_empty());
    }

    #[test]
    fn folder_name_is_the_last_component() {
        assert_eq!(
            local_folder_name(&absolute("pacientes-api")),
            "pacientes-api"
        );
    }

    #[test]
    fn an_existing_folder_is_accepted_and_resolved() {
        let dir = TempDir::new("ok");

        let resolved = checked_local_folder(&dir.0, None).unwrap();

        assert!(resolved.is_dir());
        assert!(same_local_folder(&resolved, &dir.0));
    }

    #[test]
    fn missing_relative_and_file_paths_are_refused() {
        let dir = TempDir::new("refused");
        let file = dir.0.join("notes.txt");
        std::fs::write(&file, "x").unwrap();

        assert_eq!(
            checked_local_folder(&dir.0.join("gone"), None),
            Err(LocalFolderError::Missing)
        );
        assert_eq!(
            checked_local_folder(Path::new("relative"), None),
            Err(LocalFolderError::Missing)
        );
        assert_eq!(
            checked_local_folder(&file, None),
            Err(LocalFolderError::NotADirectory)
        );
    }

    #[test]
    fn the_drive_root_and_the_user_home_are_refused() {
        let dir = TempDir::new("home");
        let root = dir.0.ancestors().last().unwrap().to_path_buf();

        assert_eq!(
            checked_local_folder(&root, None),
            Err(LocalFolderError::DriveRoot)
        );
        assert_eq!(
            checked_local_folder(&dir.0, Some(&dir.0)),
            Err(LocalFolderError::UserHome)
        );
        assert!(checked_local_folder(&dir.0, Some(&dir.0.join("other"))).is_ok());
    }

    #[test]
    fn the_same_folder_matches_through_a_parent_segment() {
        let dir = TempDir::new("same");
        let child = dir.0.join("child");
        std::fs::create_dir_all(&child).unwrap();

        assert!(same_local_folder(&child.join(".."), &dir.0));
        assert!(!same_local_folder(&child, &dir.0));
    }
}
