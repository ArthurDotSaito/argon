use std::collections::HashMap;

use argon_core::{
    Issue, PrKey, PullRequest, Reason, SubjectKind, ViewSet, classify, classify_issue,
    issue_reason, reason,
};
use argon_store::{LocalState, Store};
use chrono::{DateTime, Utc};
use gpui_kit::SharedString;

use super::labels;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    Pr(PullRequest),
    Issue(Issue),
}

impl Subject {
    pub fn key(&self) -> PrKey {
        match self {
            Self::Pr(pr) => pr.key(),
            Self::Issue(issue) => issue.key(),
        }
    }

    pub fn kind(&self) -> SubjectKind {
        match self {
            Self::Pr(_) => SubjectKind::Pr,
            Self::Issue(_) => SubjectKind::Issue,
        }
    }

    pub fn number(&self) -> u64 {
        match self {
            Self::Pr(pr) => pr.number,
            Self::Issue(issue) => issue.number,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Pr(pr) => &pr.title,
            Self::Issue(issue) => &issue.title,
        }
    }

    pub fn repo(&self) -> &str {
        match self {
            Self::Pr(pr) => &pr.repo,
            Self::Issue(issue) => &issue.repo,
        }
    }

    pub fn host(&self) -> &str {
        match self {
            Self::Pr(pr) => &pr.host,
            Self::Issue(issue) => &issue.host,
        }
    }

    pub fn author(&self) -> &str {
        match self {
            Self::Pr(pr) => &pr.author,
            Self::Issue(issue) => &issue.author,
        }
    }

    pub fn updated_at(&self) -> DateTime<Utc> {
        match self {
            Self::Pr(pr) => pr.updated_at,
            Self::Issue(issue) => issue.updated_at,
        }
    }

    pub fn as_pr(&self) -> Option<&PullRequest> {
        match self {
            Self::Pr(pr) => Some(pr),
            Self::Issue(_) => None,
        }
    }

    pub fn as_issue(&self) -> Option<&Issue> {
        match self {
            Self::Pr(_) => None,
            Self::Issue(issue) => Some(issue),
        }
    }

    pub fn has_key(&self, key: &PrKey) -> bool {
        self.node_id() == key.id.0 && self.host() == key.host
    }

    fn node_id(&self) -> &str {
        match self {
            Self::Pr(pr) => &pr.id.0,
            Self::Issue(issue) => &issue.id.0,
        }
    }

    fn haystack(&self) -> String {
        let mut parts = vec![
            self.title().to_string(),
            format!("{}#{}", self.repo(), self.number()),
            labels::repo_ref(self.repo(), self.number()),
            self.author().to_string(),
        ];
        match self {
            Self::Pr(pr) => parts.push(pr.head.clone()),
            Self::Issue(issue) => parts.extend(issue.labels.iter().map(|label| label.name.clone())),
        }
        parts.join("\n").to_lowercase()
    }
}

pub struct InboxItem {
    pub subject: Subject,
    pub unread: bool,
    pub views: ViewSet,
    pub reason: Reason,
    owner: String,
    haystack: String,
}

impl InboxItem {
    pub(super) fn new(subject: Subject, state: LocalState, context: &LoadContext) -> Self {
        let me = context
            .logins
            .get(&subject.host().to_lowercase())
            .map(String::as_str)
            .unwrap_or_default();
        let (now, window) = (context.now, context.closed_window_days);
        let (views, reason) = match &subject {
            Subject::Pr(pr) => (classify(pr, me, now, window), reason(pr, me)),
            Subject::Issue(issue) => (
                classify_issue(issue, me, now, window, state.last_seen),
                issue_reason(issue, me),
            ),
        };
        let repo = subject.repo();
        let owner = repo
            .split_once('/')
            .map_or(repo, |(owner, _)| owner)
            .to_lowercase();
        let haystack = subject.haystack();
        Self {
            subject,
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
        self.owner == org.owner && self.subject.host().eq_ignore_ascii_case(&org.host)
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

    pub(super) fn host(&self) -> &str {
        &self.host
    }

    pub(super) fn owner(&self) -> &str {
        &self.owner
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
    let prs = store
        .list_prs()?
        .into_iter()
        .map(|(pr, state)| (Subject::Pr(pr), state));
    let issues = store
        .list_issues()
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "failed to load issues for the inbox");
            Vec::new()
        })
        .into_iter()
        .map(|(issue, state)| (Subject::Issue(issue), state));
    let subjects: Vec<(Subject, LocalState)> = prs
        .chain(issues)
        .filter(|(_, state)| !state.archived)
        .collect();
    add_viewer_logins(store, context)?;
    let mut items: Vec<InboxItem> = subjects
        .into_iter()
        .map(|(subject, state)| InboxItem::new(subject, state, context))
        .collect();
    items.sort_by(|a, b| {
        let (a, b) = (&a.subject, &b.subject);
        b.updated_at()
            .cmp(&a.updated_at())
            .then_with(|| a.host().cmp(b.host()))
            .then_with(|| a.node_id().cmp(b.node_id()))
    });
    Ok(items)
}

pub(super) fn add_viewer_logins(
    store: &Store,
    context: &mut LoadContext,
) -> argon_store::Result<()> {
    for viewer in store.viewers()? {
        context
            .logins
            .entry(viewer.host.to_lowercase())
            .or_insert(viewer.login);
    }
    Ok(())
}
