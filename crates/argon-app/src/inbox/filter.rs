use argon_core::SubjectKind;

use super::InboxView;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindFilter {
    All,
    Prs,
    Issues,
}

pub fn applies_to(view: InboxView) -> bool {
    matches!(view, InboxView::Inbox | InboxView::NeedsYou)
}

pub fn passes(kind: SubjectKind, filter: KindFilter, view: InboxView) -> bool {
    if !applies_to(view) {
        return true;
    }
    match filter {
        KindFilter::All => true,
        KindFilter::Prs => kind == SubjectKind::Pr,
        KindFilter::Issues => kind == SubjectKind::Issue,
    }
}

pub fn shown_kinds(view: InboxView, filter: KindFilter) -> KindFilter {
    match view {
        InboxView::Inbox | InboxView::NeedsYou => filter,
        InboxView::IssuesAssigned
        | InboxView::IssuesMentioned
        | InboxView::IssuesAuthored
        | InboxView::IssuesParticipating => KindFilter::Issues,
        InboxView::YourPrs
        | InboxView::WaitingOnOthers
        | InboxView::ChangesRequested
        | InboxView::ReadyToMerge
        | InboxView::AlreadyDone => KindFilter::Prs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [SubjectKind; 2] = [SubjectKind::Pr, SubjectKind::Issue];
    const FILTERS: [KindFilter; 3] = [KindFilter::All, KindFilter::Prs, KindFilter::Issues];

    #[test]
    fn all_lets_every_kind_through_in_filtered_views() {
        for view in [InboxView::Inbox, InboxView::NeedsYou] {
            for kind in KINDS {
                assert!(passes(kind, KindFilter::All, view), "{view:?} {kind:?}");
            }
        }
    }

    #[test]
    fn prs_and_issues_keep_only_their_kind_in_inbox_and_needs_you() {
        let cases = [
            (SubjectKind::Pr, KindFilter::Prs, true),
            (SubjectKind::Issue, KindFilter::Prs, false),
            (SubjectKind::Pr, KindFilter::Issues, false),
            (SubjectKind::Issue, KindFilter::Issues, true),
        ];
        for view in [InboxView::Inbox, InboxView::NeedsYou] {
            for (kind, filter, expected) in cases {
                assert_eq!(
                    passes(kind, filter, view),
                    expected,
                    "{view:?} {kind:?} {filter:?}"
                );
            }
        }
    }

    #[test]
    fn other_views_ignore_the_filter() {
        let unfiltered = InboxView::ALL
            .into_iter()
            .filter(|view| !matches!(view, InboxView::Inbox | InboxView::NeedsYou));
        for view in unfiltered {
            for kind in KINDS {
                for filter in FILTERS {
                    assert!(passes(kind, filter, view), "{view:?} {kind:?} {filter:?}");
                }
            }
        }
    }

    #[test]
    fn shown_kinds_follow_the_filter_only_where_it_applies() {
        for filter in FILTERS {
            assert_eq!(shown_kinds(InboxView::Inbox, filter), filter);
            assert_eq!(shown_kinds(InboxView::NeedsYou, filter), filter);
            assert_eq!(shown_kinds(InboxView::YourPrs, filter), KindFilter::Prs);
            assert_eq!(
                shown_kinds(InboxView::IssuesMentioned, filter),
                KindFilter::Issues
            );
        }
    }
}
