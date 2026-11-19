mod architecture;
pub(crate) mod labels;
mod project_view;
mod related;
mod side_panel;

use std::collections::BTreeSet;

use argon_core::{RepoKey, ReviewCfg, is_repo_full_name};
use gpui_kit::{App, Window};

use crate::center::CenterItem;
use crate::workspace::Workspace;

pub use project_view::ProjectView;

pub fn project_list(saved: &[RepoKey], review: &ReviewCfg) -> Vec<RepoKey> {
    let configured = review
        .repo_related
        .iter()
        .filter(|(_, related)| !related.is_empty())
        .filter_map(|(key, _)| parse_repo_key(key));
    let mut keys: Vec<RepoKey> = saved
        .iter()
        .cloned()
        .chain(configured)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    keys.sort_by_cached_key(|key| {
        (
            labels::repo_name(&key.repo).to_lowercase(),
            key.repo.clone(),
            key.host.clone(),
        )
    });
    keys
}

pub fn parse_repo_key(key: &str) -> Option<RepoKey> {
    let (host, repo) = key.split_once('/')?;
    (!host.is_empty() && is_repo_full_name(repo)).then(|| RepoKey::new(host, repo))
}

pub fn repo_url(key: &RepoKey) -> String {
    format!("https://{}/{}", key.host, key.repo)
}

pub fn open(key: RepoKey, window: &mut Window, cx: &mut App) {
    if let Some(workspace) = Workspace::global(cx) {
        _ = workspace.update(cx, |workspace, cx| {
            workspace.open_item(CenterItem::Project(key), false, window, cx)
        });
    }
}

pub fn open_architecture(key: RepoKey, window: &mut Window, cx: &mut App) {
    if let Some(workspace) = Workspace::global(cx) {
        _ = workspace.update(cx, |workspace, cx| {
            workspace.open_project_architecture(key, window, cx)
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn key(host: &str, repo: &str) -> RepoKey {
        RepoKey::new(host, repo)
    }

    fn review(related: &[(&str, &[&str])]) -> ReviewCfg {
        let repo_related = related
            .iter()
            .map(|(key, items)| {
                (
                    (*key).to_owned(),
                    items.iter().map(|item| (*item).to_owned()).collect(),
                )
            })
            .collect::<BTreeMap<_, Vec<String>>>();
        ReviewCfg {
            repo_related,
            ..ReviewCfg::default()
        }
    }

    #[test]
    fn lists_saved_overviews_and_configured_related_sorted_by_name() {
        let saved = [
            key("github.com", "acme/zeta-worker"),
            key("github.com", "acme/Alpha-api"),
        ];
        let cfg = review(&[("github.com/acme/mid-portal", &["acme/zeta-worker"])]);
        let list = project_list(&saved, &cfg);
        assert_eq!(
            list,
            vec![
                key("github.com", "acme/Alpha-api"),
                key("github.com", "acme/mid-portal"),
                key("github.com", "acme/zeta-worker"),
            ]
        );
    }

    #[test]
    fn a_repo_with_overview_and_related_appears_once() {
        let saved = [key("github.com", "acme/api")];
        let cfg = review(&[("github.com/acme/api", &["acme/web"])]);
        assert_eq!(project_list(&saved, &cfg), saved.to_vec());
    }

    #[test]
    fn empty_related_lists_and_malformed_keys_are_ignored() {
        let cfg = review(&[
            ("github.com/acme/none", &[]),
            ("not-a-key", &["acme/web"]),
            ("github.com/acme", &["acme/web"]),
        ]);
        assert!(project_list(&[], &cfg).is_empty());
    }

    #[test]
    fn nothing_saved_and_nothing_configured_gives_an_empty_group() {
        assert!(project_list(&[], &ReviewCfg::default()).is_empty());
    }

    #[test]
    fn same_name_in_two_hosts_stays_deterministic() {
        let saved = [key("ghe.local", "acme/api"), key("github.com", "acme/api")];
        let list = project_list(&saved, &ReviewCfg::default());
        assert_eq!(list[0].host, "ghe.local");
        assert_eq!(list[1].host, "github.com");
    }

    #[test]
    fn repo_key_parsing_round_trips_the_config_format() {
        assert_eq!(
            parse_repo_key("github.com/acme/api"),
            Some(key("github.com", "acme/api"))
        );
        assert_eq!(parse_repo_key("/acme/api"), None);
    }

    #[test]
    fn repo_url_points_at_the_host() {
        assert_eq!(
            repo_url(&key("github.com", "acme/api")),
            "https://github.com/acme/api"
        );
    }
}
