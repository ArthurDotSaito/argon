use std::collections::HashMap;

use argon_core::{PullRequest, Reason, ViewSet, classify, reason};
use argon_store::{LocalState, Store};
use chrono::{DateTime, Utc};
use gpui_kit::SharedString;

use super::labels;

pub struct InboxItem {
    pub pr: PullRequest,
    pub unread: bool,
    pub views: ViewSet,
    pub reason: Reason,
    owner: String,
    haystack: String,
}

impl InboxItem {
    fn new(pr: PullRequest, state: LocalState, context: &LoadContext) -> Self {
        let me = context
            .logins
            .get(&pr.host.to_lowercase())
            .map(String::as_str)
            .unwrap_or_default();
        let views = classify(&pr, me, context.now, context.closed_window_days);
        let reason = reason(&pr, me);
        let owner = pr
            .repo
            .split_once('/')
            .map_or(pr.repo.as_str(), |(owner, _)| owner)
            .to_lowercase();
        let haystack = [
            pr.title.clone(),
            format!("{}#{}", pr.repo, pr.number),
            labels::repo_ref(&pr.repo, pr.number),
            pr.author.clone(),
            pr.head.clone(),
        ]
        .join("\n")
        .to_lowercase();
        Self {
            pr,
            unread: state.unread,
            views,
            reason,
            owner,
            haystack,
        }
    }

    pub(super) fn matches(&self, query: &str) -> bool {
        self.haystack.contains(query)
    }

    pub(super) fn belongs_to(&self, org: &OrgKey) -> bool {
        self.owner == org.owner && self.pr.host.eq_ignore_ascii_case(&org.host)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrgKey {
    host: String,
    owner: String,
}

impl OrgKey {
    pub(super) fn new(host: &str, owner: &str) -> Self {
        Self {
            host: host.to_lowercase(),
            owner: owner.to_lowercase(),
        }
    }
}

pub struct OrgEntry {
    pub key: OrgKey,
    pub name: SharedString,
    pub count: usize,
    pub admin: bool,
}

pub struct AccountEntry {
    pub login: SharedString,
    pub host: SharedString,
}

pub(super) struct LoadContext {
    pub(super) logins: HashMap<String, String>,
    pub(super) now: DateTime<Utc>,
    pub(super) closed_window_days: i64,
}

pub(super) fn load_items(
    store: &mut Store,
    context: &mut LoadContext,
) -> argon_store::Result<Vec<InboxItem>> {
    let prs: Vec<(PullRequest, LocalState)> = store
        .list_prs()?
        .into_iter()
        .filter(|(_, state)| !state.archived)
        .collect();
    for (pr, _) in &prs {
        if pr.involvement.authored {
            context
                .logins
                .entry(pr.host.to_lowercase())
                .or_insert_with(|| pr.author.clone());
        }
    }
    let mut items: Vec<InboxItem> = prs
        .into_iter()
        .map(|(pr, state)| InboxItem::new(pr, state, context))
        .collect();
    items.sort_by(|a, b| {
        b.pr.updated_at
            .cmp(&a.pr.updated_at)
            .then_with(|| a.pr.id.0.cmp(&b.pr.id.0))
    });
    Ok(items)
}
