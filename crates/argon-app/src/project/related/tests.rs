use std::path::PathBuf;

use super::*;

const HOST: &str = "github.com";

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn add(current: &[String], input: &str) -> Result<Vec<String>, RelatedError> {
    add_related(current, "acme/api", HOST, input)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let path =
            std::env::temp_dir().join(format!("argon-app-related-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn folder(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn text(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
fn adds_a_valid_repo_after_trimming() {
    let next = add(&list(&["acme/web"]), "  acme/worker \n").unwrap();
    assert_eq!(next, list(&["acme/web", "acme/worker"]));
}

#[test]
fn accepts_dots_underscores_and_hyphens() {
    let next = add(&[], "acme-co/my_repo.v2").unwrap();
    assert_eq!(next, list(&["acme-co/my_repo.v2"]));
}

#[test]
fn every_github_url_form_is_stored_as_owner_slash_name() {
    for input in [
        "https://github.com/acme/web",
        "http://github.com/acme/web",
        "https://www.github.com/acme/web",
        "https://github.com/acme/web.git",
        "https://github.com/acme/web/",
        "https://github.com/acme/web.git/",
        "https://github.com/acme/web/tree/main",
        "https://github.com/acme/web/tree/feat/x/src",
        "https://github.com/acme/web/pull/3",
        "https://github.com/acme/web/pull/3/files",
        "https://github.com/acme/web/blob/main/src/lib.rs",
        "https://github.com/acme/web?tab=readme",
        "https://github.com/acme/web#readme",
        "  https://github.com/acme/web  ",
    ] {
        assert_eq!(add(&[], input), Ok(list(&["acme/web"])), "{input}");
    }
}

#[test]
fn urls_are_read_ignoring_the_case_of_scheme_host_and_suffix() {
    assert_eq!(
        add(&[], "HTTPS://WWW.GitHub.COM/Acme/Web.GIT"),
        Ok(list(&["Acme/Web"]))
    );
    assert_eq!(
        add(&list(&["acme/web"]), "https://github.com/ACME/WEB"),
        Err(RelatedError::Duplicate)
    );
    assert_eq!(
        add(&[], "https://github.com/Acme/API/pull/9"),
        Err(RelatedError::OwnRepo)
    );
}

#[test]
fn urls_of_another_host_are_refused() {
    for input in [
        "https://gitlab.com/acme/web",
        "https://ghe.acme.io/acme/web",
        "http://github.com.evil.io/acme/web",
        "https://github.com:8443/acme/web",
    ] {
        assert_eq!(add(&[], input), Err(RelatedError::OtherHost), "{input}");
    }
    assert_eq!(
        add_related(
            &[],
            "acme/api",
            "ghe.acme.io",
            "https://ghe.acme.io/acme/web"
        ),
        Ok(list(&["acme/web"]))
    );
    assert_eq!(
        add_related(
            &[],
            "acme/api",
            "ghe.acme.io",
            "https://github.com/acme/web"
        ),
        Err(RelatedError::OtherHost)
    );
}

#[test]
fn incomplete_or_broken_urls_are_invalid() {
    for input in [
        "https://",
        "https:///acme/web",
        "https://github.com",
        "https://github.com/",
        "https://github.com/acme",
        "https://github.com/acme/",
        "https://github.com/acme/..",
        "https://github.com/acme/we b",
        "https://github.com/acme/wéb",
        "ftp://github.com/acme/web",
        "github.com/acme/web",
    ] {
        assert_eq!(add(&[], input), Err(RelatedError::InvalidFormat), "{input}");
    }
}

#[test]
fn refuses_empty_input() {
    assert_eq!(add(&[], ""), Err(RelatedError::Empty));
    assert_eq!(add(&[], "   "), Err(RelatedError::Empty));
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
    ] {
        assert_eq!(add(&[], input), Err(RelatedError::InvalidFormat), "{input}");
    }
}

#[test]
fn refuses_duplicates_ignoring_case() {
    assert_eq!(
        add(&list(&["acme/Web"]), "ACME/web"),
        Err(RelatedError::Duplicate)
    );
}

#[test]
fn refuses_the_own_repo_ignoring_case() {
    assert_eq!(add(&[], "Acme/API"), Err(RelatedError::OwnRepo));
}

#[test]
fn refuses_the_ninth_item() {
    let full: Vec<String> = (0..MAX_RELATED_REPOS)
        .map(|ix| format!("acme/repo-{ix}"))
        .collect();
    assert_eq!(add(&full, "acme/extra"), Err(RelatedError::LimitReached));
    let seven = &full[..MAX_RELATED_REPOS - 1];
    assert_eq!(add(seven, "acme/extra").unwrap().len(), MAX_RELATED_REPOS);
}

#[test]
fn a_local_folder_is_stored_as_its_path_after_the_repos() {
    let dir = TempDir::new("valid");
    let folder = dir.folder("pacientes-api");

    let next = add_local_related(&list(&["acme/web"]), &folder, None).unwrap();

    assert_eq!(next, vec!["acme/web".to_owned(), text(&folder)]);
}

#[test]
fn a_missing_path_or_a_file_is_refused() {
    let dir = TempDir::new("missing");
    let file = dir.0.join("notes.txt");
    std::fs::write(&file, "x").unwrap();

    assert_eq!(
        add_local_related(&[], &dir.0.join("gone"), None),
        Err(RelatedError::FolderMissing)
    );
    assert_eq!(
        add_local_related(&[], &file, None),
        Err(RelatedError::NotAFolder)
    );
}

#[test]
fn the_drive_root_and_the_user_folder_are_refused() {
    let dir = TempDir::new("roots");
    let root = dir.0.ancestors().last().unwrap().to_path_buf();
    let home = dir.folder("me");

    assert_eq!(
        add_local_related(&[], &root, None),
        Err(RelatedError::DriveRoot)
    );
    assert_eq!(
        add_local_related(&[], &home, Some(&home)),
        Err(RelatedError::UserHome)
    );
    let project = dir.folder("me/project");
    assert!(add_local_related(&[], &project, Some(&home)).is_ok());
}

#[test]
fn the_same_folder_twice_is_a_duplicate() {
    let dir = TempDir::new("duplicate");
    let folder = dir.folder("fila");
    let current = vec![text(&folder)];

    assert_eq!(
        add_local_related(&current, &folder.join("..").join("fila"), None),
        Err(RelatedError::Duplicate)
    );
    if cfg!(windows) {
        let shouted = PathBuf::from(text(&folder).to_uppercase());
        assert_eq!(
            add_local_related(&current, &shouted, None),
            Err(RelatedError::Duplicate)
        );
    }
    let other = dir.folder("outra");
    assert_eq!(add_local_related(&current, &other, None).unwrap().len(), 2);
}

#[test]
fn a_local_folder_counts_toward_the_limit() {
    let dir = TempDir::new("limit");
    let folder = dir.folder("ultimo");
    let mut full: Vec<String> = (0..MAX_RELATED_REPOS - 1)
        .map(|ix| format!("acme/repo-{ix}"))
        .collect();
    full.push(text(&dir.folder("local")));

    assert_eq!(
        add_local_related(&full, &folder, None),
        Err(RelatedError::LimitReached)
    );
    assert_eq!(add(&full, "acme/extra"), Err(RelatedError::LimitReached));
}

#[test]
fn every_error_has_a_message() {
    for error in [
        RelatedError::Empty,
        RelatedError::InvalidFormat,
        RelatedError::OtherHost,
        RelatedError::Duplicate,
        RelatedError::OwnRepo,
        RelatedError::LimitReached,
        RelatedError::FolderMissing,
        RelatedError::NotAFolder,
        RelatedError::DriveRoot,
        RelatedError::UserHome,
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

#[test]
fn forgetting_a_project_drops_only_its_entry() {
    let mut map = BTreeMap::new();
    store_related(&mut map, "github.com/acme/api".into(), list(&["acme/web"]));
    store_related(&mut map, "github.com/acme/web".into(), list(&["acme/api"]));
    store_related(&mut map, "ghe.local/acme/api".into(), list(&["acme/db"]));

    assert!(forget_related(&mut map, "github.com", "acme/api"));

    assert_eq!(map.len(), 2);
    assert!(map.contains_key("github.com/acme/web"));
    assert!(map.contains_key("ghe.local/acme/api"));
    assert!(!forget_related(&mut map, "github.com", "acme/api"));
}

#[test]
fn forgetting_the_last_entry_leaves_the_review_config_without_the_map() {
    let mut review = argon_core::ReviewCfg::default();
    store_related(
        &mut review.repo_related,
        "github.com/acme/api".into(),
        list(&["acme/web"]),
    );
    assert!(
        serde_json::to_string(&review)
            .unwrap()
            .contains("repo_related")
    );

    forget_related(&mut review.repo_related, "github.com", "acme/api");

    assert!(review.repo_related.is_empty());
    assert!(
        !serde_json::to_string(&review)
            .unwrap()
            .contains("repo_related")
    );
}
