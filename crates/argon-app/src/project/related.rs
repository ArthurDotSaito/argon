use std::collections::BTreeMap;
use std::path::Path;

use argon_core::{
    LocalFolderError, MAX_RELATED_REPOS, RelatedEntry, checked_local_folder, is_repo_full_name,
    repo_key, same_local_folder,
};

use super::labels;

const URL_SCHEMES: [&str; 2] = ["https://", "http://"];
const WWW_PREFIX: &str = "www.";
const GIT_SUFFIX: &str = ".git";
const URL_TAIL_STARTS: [char; 2] = ['?', '#'];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelatedError {
    Empty,
    InvalidFormat,
    OtherHost,
    Duplicate,
    OwnRepo,
    LimitReached,
    FolderMissing,
    NotAFolder,
    DriveRoot,
    UserHome,
}

impl RelatedError {
    pub fn message(self) -> String {
        match self {
            Self::Empty => labels::ERROR_EMPTY.to_owned(),
            Self::InvalidFormat => labels::ERROR_FORMAT.to_owned(),
            Self::OtherHost => labels::ERROR_OTHER_HOST.to_owned(),
            Self::Duplicate => labels::ERROR_DUPLICATE.to_owned(),
            Self::OwnRepo => labels::ERROR_OWN_REPO.to_owned(),
            Self::LimitReached => labels::error_limit(),
            Self::FolderMissing => labels::ERROR_FOLDER_MISSING.to_owned(),
            Self::NotAFolder => labels::ERROR_NOT_A_FOLDER.to_owned(),
            Self::DriveRoot => labels::ERROR_DRIVE_ROOT.to_owned(),
            Self::UserHome => labels::ERROR_USER_HOME.to_owned(),
        }
    }
}

impl From<LocalFolderError> for RelatedError {
    fn from(error: LocalFolderError) -> Self {
        match error {
            LocalFolderError::Missing => Self::FolderMissing,
            LocalFolderError::NotADirectory => Self::NotAFolder,
            LocalFolderError::DriveRoot => Self::DriveRoot,
            LocalFolderError::UserHome => Self::UserHome,
        }
    }
}

fn strip_prefix_ignoring_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

fn strip_suffix_ignoring_case<'a>(text: &'a str, suffix: &str) -> &'a str {
    let Some(cut) = text.len().checked_sub(suffix.len()) else {
        return text;
    };
    match text.get(cut..) {
        Some(tail) if tail.eq_ignore_ascii_case(suffix) => &text[..cut],
        _ => text,
    }
}

fn repo_from_url(rest: &str, host: &str) -> Result<String, RelatedError> {
    let rest = rest.split(URL_TAIL_STARTS).next().unwrap_or_default();
    let (url_host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let url_host = strip_prefix_ignoring_case(url_host, WWW_PREFIX).unwrap_or(url_host);
    if url_host.is_empty() {
        return Err(RelatedError::InvalidFormat);
    }
    if !url_host.eq_ignore_ascii_case(host) {
        return Err(RelatedError::OtherHost);
    }
    let mut parts = path.split('/').filter(|part| !part.is_empty());
    let (Some(owner), Some(name)) = (parts.next(), parts.next()) else {
        return Err(RelatedError::InvalidFormat);
    };
    Ok(format!(
        "{owner}/{}",
        strip_suffix_ignoring_case(name, GIT_SUFFIX)
    ))
}

pub fn normalize_repo(input: &str, host: &str) -> Result<String, RelatedError> {
    let text = input.trim();
    if text.is_empty() {
        return Err(RelatedError::Empty);
    }
    let repo = match URL_SCHEMES
        .iter()
        .find_map(|scheme| strip_prefix_ignoring_case(text, scheme))
    {
        Some(rest) => repo_from_url(rest, host)?,
        None => text.to_owned(),
    };
    if is_repo_full_name(&repo) {
        Ok(repo)
    } else {
        Err(RelatedError::InvalidFormat)
    }
}

fn is_same_repo(existing: &str, repo: &str) -> bool {
    matches!(RelatedEntry::parse(existing), Some(RelatedEntry::Repo(name)) if name.eq_ignore_ascii_case(repo))
}

fn is_same_folder(existing: &str, path: &Path) -> bool {
    matches!(RelatedEntry::parse(existing), Some(RelatedEntry::Local(folder)) if same_local_folder(&folder, path))
}

fn appended(
    current: &[String],
    entry: String,
    duplicate: impl Fn(&str) -> bool,
) -> Result<Vec<String>, RelatedError> {
    if current.iter().any(|existing| duplicate(existing)) {
        return Err(RelatedError::Duplicate);
    }
    if current.len() >= MAX_RELATED_REPOS {
        return Err(RelatedError::LimitReached);
    }
    let mut next = current.to_vec();
    next.push(entry);
    Ok(next)
}

pub fn add_related(
    current: &[String],
    own_repo: &str,
    host: &str,
    input: &str,
) -> Result<Vec<String>, RelatedError> {
    let repo = normalize_repo(input, host)?;
    if repo.eq_ignore_ascii_case(own_repo) {
        return Err(RelatedError::OwnRepo);
    }
    appended(current, repo.clone(), |existing| {
        is_same_repo(existing, &repo)
    })
}

pub fn add_local_related(
    current: &[String],
    path: &Path,
    home: Option<&Path>,
) -> Result<Vec<String>, RelatedError> {
    checked_local_folder(path, home)?;
    appended(current, path.to_string_lossy().into_owned(), |existing| {
        is_same_folder(existing, path)
    })
}

pub fn remove_related(current: &[String], repo: &str) -> Vec<String> {
    current
        .iter()
        .filter(|existing| existing.as_str() != repo)
        .cloned()
        .collect()
}

pub fn store_related(map: &mut BTreeMap<String, Vec<String>>, key: String, related: Vec<String>) {
    if related.is_empty() {
        map.remove(&key);
    } else {
        map.insert(key, related);
    }
}

pub fn forget_related(map: &mut BTreeMap<String, Vec<String>>, host: &str, repo: &str) -> bool {
    map.remove(&repo_key(host, repo)).is_some()
}

#[cfg(test)]
mod tests;
