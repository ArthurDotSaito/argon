use argon_github::{OrgInfo, Viewer};
use argon_store::{CachedOrg, CachedViewer};

pub(super) fn viewer_from_cache(cached: &CachedViewer) -> Viewer {
    Viewer {
        login: cached.login.clone(),
        orgs: cached
            .orgs
            .iter()
            .map(|org| OrgInfo {
                login: org.login.clone(),
                can_admin: org.can_admin,
            })
            .collect(),
    }
}

pub(super) fn cache_orgs(viewer: &Viewer) -> Vec<CachedOrg> {
    viewer
        .orgs
        .iter()
        .map(|org| CachedOrg {
            login: org.login.clone(),
            can_admin: org.can_admin,
        })
        .collect()
}

pub(super) fn orgs_for_host(tracked: &[String], host: &str) -> Vec<String> {
    tracked
        .iter()
        .filter_map(|entry| entry.split_once('/'))
        .filter(|(entry_host, _)| *entry_host == host)
        .map(|(_, org)| org.to_string())
        .collect()
}

pub(super) fn admin_orgs(viewers: &[(String, Viewer)]) -> Vec<String> {
    viewers
        .iter()
        .flat_map(|(host, viewer)| {
            viewer
                .orgs
                .iter()
                .filter(|org| org.can_admin)
                .map(move |org| format!("{host}/{}", org.login))
        })
        .collect()
}

pub(super) fn merge_tracked(tracked: &mut Vec<String>, additions: Vec<String>) {
    for entry in additions {
        if !tracked
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&entry))
        {
            tracked.push(entry);
        }
    }
}
