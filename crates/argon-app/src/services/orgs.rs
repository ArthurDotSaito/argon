use argon_github::Viewer;

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
