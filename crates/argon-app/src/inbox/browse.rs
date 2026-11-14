use std::collections::{HashMap, HashSet};

use argon_core::{Involvement, Issue, PrKey, ViewSet};
use argon_github::IssuePage;
use argon_store::{LocalState, Store};
use chrono::{DateTime, Utc};
use gpui_kit::SharedString;

use super::InboxView;
use super::filter::KindFilter;
use super::items::{InboxItem, LoadContext, OrgKey, Subject, add_viewer_logins};

pub(super) const PAGE_SIZE: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueScope {
    Recent,
    AllOpen,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct BrowseKey {
    pub(super) view: InboxView,
    pub(super) host: String,
    pub(super) query: String,
}

#[derive(Default)]
pub(super) struct BrowseList {
    pub(super) items: Vec<InboxItem>,
    pub(super) next: Option<String>,
    pub(super) total: u32,
    pub(super) loaded: bool,
    pub(super) error: Option<SharedString>,
}

impl BrowseList {
    pub(super) fn append(&mut self, page: BrowsePage) {
        let known: HashSet<PrKey> = self.items.iter().map(|item| item.subject.key()).collect();
        self.items.extend(
            page.items
                .into_iter()
                .filter(|item| !known.contains(&item.subject.key())),
        );
        self.next = page.next;
        self.total = page.total;
        self.loaded = true;
        self.error = None;
    }
}

pub(super) struct BrowsePage {
    items: Vec<InboxItem>,
    next: Option<String>,
    total: u32,
}

pub fn browse_query(
    view: InboxView,
    kind: KindFilter,
    org: Option<&OrgKey>,
    tracked: &[OrgKey],
) -> Option<String> {
    let viewer = match view {
        InboxView::IssuesAuthored => Some("author:@me"),
        InboxView::IssuesMentioned => Some("mentions:@me"),
        InboxView::IssuesParticipating => Some("commenter:@me -author:@me"),
        InboxView::Inbox if kind == KindFilter::Issues => None,
        _ => return None,
    };
    let orgs: Vec<&OrgKey> = match org {
        Some(org) => vec![org],
        None if viewer.is_none() => tracked.iter().collect(),
        None => Vec::new(),
    };
    if viewer.is_none() && orgs.is_empty() {
        return None;
    }
    let mut terms = vec!["is:issue is:open".to_string()];
    terms.extend(viewer.map(str::to_string));
    terms.extend(orgs.into_iter().map(|org| format!("org:{}", org.owner())));
    terms.push("sort:updated-desc".to_string());
    Some(terms.join(" "))
}

pub(super) fn load_page(
    store: &mut Store,
    key: &BrowseKey,
    page: IssuePage,
    context: &mut LoadContext,
) -> argon_store::Result<BrowsePage> {
    add_viewer_logins(store, context)?;
    let last_seen = store.last_seen_map(&key.host)?;
    Ok(BrowsePage {
        items: browse_items(page.issues, key.view, &last_seen, context),
        next: page.next,
        total: page.total,
    })
}

fn browse_items(
    issues: Vec<Issue>,
    view: InboxView,
    last_seen: &HashMap<String, DateTime<Utc>>,
    context: &LoadContext,
) -> Vec<InboxItem> {
    issues
        .into_iter()
        .map(|mut issue| {
            issue.involvement = involvement(view);
            let last_seen = last_seen.get(&issue.id.0).copied();
            let state = LocalState {
                unread: last_seen.is_none_or(|seen| issue.updated_at > seen),
                archived: false,
                last_seen,
            };
            let mut item = InboxItem::new(Subject::Issue(issue), state, context);
            item.views.remove(ViewSet::NEEDS_YOU);
            item
        })
        .collect()
}

fn involvement(view: InboxView) -> Involvement {
    Involvement {
        authored: view == InboxView::IssuesAuthored,
        mentioned: view == InboxView::IssuesMentioned,
        commented: view == InboxView::IssuesParticipating,
        ..Involvement::default()
    }
}

#[cfg(test)]
mod tests {
    use argon_core::{DEFAULT_HOST, Reason, fixtures};

    use super::*;

    const ISSUE_VIEWS: [InboxView; 3] = [
        InboxView::IssuesAuthored,
        InboxView::IssuesMentioned,
        InboxView::IssuesParticipating,
    ];
    const KINDS: [KindFilter; 3] = [KindFilter::All, KindFilter::Prs, KindFilter::Issues];

    fn org(owner: &str) -> OrgKey {
        OrgKey::new(DEFAULT_HOST, owner)
    }

    fn tracked() -> Vec<OrgKey> {
        vec![org("nuria-tech"), org("acme")]
    }

    #[test]
    fn issue_views_search_open_issues_by_their_own_qualifier() {
        let expected = [
            "is:issue is:open author:@me sort:updated-desc",
            "is:issue is:open mentions:@me sort:updated-desc",
            "is:issue is:open commenter:@me -author:@me sort:updated-desc",
        ];
        for (view, expected) in ISSUE_VIEWS.into_iter().zip(expected) {
            for kind in KINDS {
                assert_eq!(
                    browse_query(view, kind, None, &tracked()).as_deref(),
                    Some(expected),
                    "{view:?} {kind:?}"
                );
            }
        }
    }

    #[test]
    fn selected_org_narrows_the_issue_views() {
        let expected = [
            "is:issue is:open author:@me org:nuria-tech sort:updated-desc",
            "is:issue is:open mentions:@me org:nuria-tech sort:updated-desc",
            "is:issue is:open commenter:@me -author:@me org:nuria-tech sort:updated-desc",
        ];
        let selected = org("Nuria-Tech");
        for (view, expected) in ISSUE_VIEWS.into_iter().zip(expected) {
            assert_eq!(
                browse_query(view, KindFilter::All, Some(&selected), &tracked()).as_deref(),
                Some(expected),
                "{view:?}"
            );
        }
    }

    #[test]
    fn inbox_with_issues_searches_every_tracked_org_or_only_the_selected_one() {
        assert_eq!(
            browse_query(InboxView::Inbox, KindFilter::Issues, None, &tracked()).as_deref(),
            Some("is:issue is:open org:nuria-tech org:acme sort:updated-desc")
        );
        assert_eq!(
            browse_query(
                InboxView::Inbox,
                KindFilter::Issues,
                Some(&org("acme")),
                &tracked()
            )
            .as_deref(),
            Some("is:issue is:open org:acme sort:updated-desc")
        );
    }

    #[test]
    fn inbox_without_tracked_orgs_has_nothing_to_browse() {
        assert_eq!(
            browse_query(InboxView::Inbox, KindFilter::Issues, None, &[]),
            None
        );
    }

    #[test]
    fn inbox_with_all_or_prs_does_not_browse() {
        for kind in [KindFilter::All, KindFilter::Prs] {
            for selected in [None, Some(org("acme"))] {
                assert_eq!(
                    browse_query(InboxView::Inbox, kind, selected.as_ref(), &tracked()),
                    None,
                    "{kind:?} {selected:?}"
                );
            }
        }
    }

    #[test]
    fn assigned_needs_you_and_pr_views_do_not_browse() {
        let unsupported = InboxView::ALL
            .into_iter()
            .filter(|view| !ISSUE_VIEWS.contains(view) && *view != InboxView::Inbox);
        for view in unsupported {
            for kind in KINDS {
                for selected in [None, Some(org("acme"))] {
                    assert_eq!(
                        browse_query(view, kind, selected.as_ref(), &tracked()),
                        None,
                        "{view:?} {kind:?} {selected:?}"
                    );
                }
            }
        }
    }

    fn context() -> LoadContext {
        LoadContext {
            logins: HashMap::from([(DEFAULT_HOST.to_string(), fixtures::ME.to_string())]),
            now: fixtures::now(),
            closed_window_days: 7,
        }
    }

    fn issue(id: &str, days_ago: i64) -> Issue {
        fixtures::issue_on(DEFAULT_HOST, id, fixtures::days_ago(days_ago))
    }

    #[test]
    fn items_are_unread_unless_seen_after_their_last_update() {
        let issues = vec![issue("never", 1), issue("seen", 2), issue("changed", 3)];
        let last_seen = HashMap::from([
            ("seen".to_string(), fixtures::days_ago(2)),
            ("changed".to_string(), fixtures::days_ago(4)),
            ("elsewhere".to_string(), fixtures::days_ago(0)),
        ]);

        let items = browse_items(issues, InboxView::Inbox, &last_seen, &context());

        let unread: Vec<(String, bool)> = items
            .iter()
            .map(|item| (item.subject.key().id.0, item.unread))
            .collect();
        assert_eq!(
            unread,
            vec![
                ("never".to_string(), true),
                ("seen".to_string(), false),
                ("changed".to_string(), true),
            ]
        );
    }

    fn reason_in(view: InboxView, issue: Issue) -> Reason {
        browse_items(vec![issue], view, &HashMap::new(), &context())[0]
            .reason
            .clone()
    }

    #[test]
    fn items_take_the_view_involvement_for_their_reason() {
        let mut mine = issue("mine", 1);
        mine.author = fixtures::ME.to_string();
        assert_eq!(
            reason_in(InboxView::IssuesAuthored, mine),
            Reason::IssueAuthored
        );
        assert_eq!(
            reason_in(InboxView::IssuesMentioned, issue("other", 1)),
            Reason::Mentioned
        );
        assert_eq!(
            reason_in(InboxView::IssuesParticipating, issue("other", 1)),
            Reason::Participating
        );
        assert_eq!(
            reason_in(InboxView::Inbox, issue("other", 1)),
            Reason::NewInOrg("acme".to_string())
        );
    }

    #[test]
    fn items_never_land_in_needs_you() {
        let mut assigned = issue("assigned", 0);
        assigned.assignees = vec![fixtures::ME.to_string()];

        let items = browse_items(
            vec![assigned],
            InboxView::Inbox,
            &HashMap::new(),
            &context(),
        );

        assert!(!items[0].views.contains(ViewSet::NEEDS_YOU));
    }
}
