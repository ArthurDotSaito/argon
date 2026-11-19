use std::collections::BTreeMap;

use argon_core::{MAX_RELATED_REPOS, is_repo_full_name};

use super::labels;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelatedError {
    Empty,
    InvalidFormat,
    Duplicate,
    OwnRepo,
    LimitReached,
}

impl RelatedError {
    pub fn message(self) -> String {
        match self {
            Self::Empty => labels::ERROR_EMPTY.to_owned(),
            Self::InvalidFormat => labels::ERROR_FORMAT.to_owned(),
            Self::Duplicate => labels::ERROR_DUPLICATE.to_owned(),
            Self::OwnRepo => labels::ERROR_OWN_REPO.to_owned(),
            Self::LimitReached => labels::error_limit(),
        }
    }
}

pub fn add_related(
    current: &[String],
    own_repo: &str,
    input: &str,
) -> Result<Vec<String>, RelatedError> {
    let repo = input.trim();
    if repo.is_empty() {
        return Err(RelatedError::Empty);
    }
    if !is_repo_full_name(repo) {
        return Err(RelatedError::InvalidFormat);
    }
    if repo.eq_ignore_ascii_case(own_repo) {
        return Err(RelatedError::OwnRepo);
    }
    if current
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(repo))
    {
        return Err(RelatedError::Duplicate);
    }
    if current.len() >= MAX_RELATED_REPOS {
        return Err(RelatedError::LimitReached);
    }
    let mut next = current.to_vec();
    next.push(repo.to_owned());
    Ok(next)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn adds_a_valid_repo_after_trimming() {
        let next = add_related(&list(&["acme/web"]), "acme/api", "  acme/worker \n").unwrap();
        assert_eq!(next, list(&["acme/web", "acme/worker"]));
    }

    #[test]
    fn accepts_dots_underscores_and_hyphens() {
        let next = add_related(&[], "acme/api", "acme-co/my_repo.v2").unwrap();
        assert_eq!(next, list(&["acme-co/my_repo.v2"]));
    }

    #[test]
    fn refuses_empty_input() {
        assert_eq!(add_related(&[], "acme/api", ""), Err(RelatedError::Empty));
        assert_eq!(
            add_related(&[], "acme/api", "   "),
            Err(RelatedError::Empty)
        );
    }

    #[test]
    fn refuses_invalid_formats() {
        for input in [
            "acme",
            "acme/",
            "/web",
            "acme/web/extra",
            "acme/we b",
            "acme/../web",
            "./web",
            "acme/..",
            "acme/wéb",
            "https://github.com/acme/web",
        ] {
            assert_eq!(
                add_related(&[], "acme/api", input),
                Err(RelatedError::InvalidFormat),
                "{input}"
            );
        }
    }

    #[test]
    fn refuses_duplicates_ignoring_case() {
        assert_eq!(
            add_related(&list(&["acme/Web"]), "acme/api", "ACME/web"),
            Err(RelatedError::Duplicate)
        );
    }

    #[test]
    fn refuses_the_own_repo_ignoring_case() {
        assert_eq!(
            add_related(&[], "acme/api", "Acme/API"),
            Err(RelatedError::OwnRepo)
        );
    }

    #[test]
    fn refuses_the_ninth_item() {
        let full: Vec<String> = (0..MAX_RELATED_REPOS)
            .map(|ix| format!("acme/repo-{ix}"))
            .collect();
        assert_eq!(
            add_related(&full, "acme/api", "acme/extra"),
            Err(RelatedError::LimitReached)
        );
        let seven = &full[..MAX_RELATED_REPOS - 1];
        assert_eq!(
            add_related(seven, "acme/api", "acme/extra").unwrap().len(),
            MAX_RELATED_REPOS
        );
    }

    #[test]
    fn every_error_has_a_message() {
        for error in [
            RelatedError::Empty,
            RelatedError::InvalidFormat,
            RelatedError::Duplicate,
            RelatedError::OwnRepo,
            RelatedError::LimitReached,
        ] {
            assert!(!error.message().is_empty());
        }
    }

    #[test]
    fn removal_keeps_the_order_of_the_rest() {
        let next = remove_related(&list(&["a/1", "a/2", "a/3"]), "a/2");
        assert_eq!(next, list(&["a/1", "a/3"]));
    }

    #[test]
    fn storing_an_empty_list_drops_the_key() {
        let mut map = BTreeMap::new();
        store_related(&mut map, "github.com/acme/api".into(), list(&["acme/web"]));
        assert_eq!(map.len(), 1);
        store_related(&mut map, "github.com/acme/api".into(), Vec::new());
        assert!(map.is_empty());
    }
}
