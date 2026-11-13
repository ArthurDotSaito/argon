use bitflags::bitflags;
use chrono::{DateTime, Duration, Utc};

use crate::model::{
    ChecksState, Issue, IssueState, LatestReview, Mergeable, PrState, PullRequest, ReviewDecision,
    ReviewState,
};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ViewSet: u16 {
        const INBOX = 1 << 0;
        const NEEDS_YOU = 1 << 1;
        const YOUR_PRS = 1 << 2;
        const WAITING_ON_OTHERS = 1 << 3;
        const CHANGES_REQUESTED = 1 << 4;
        const READY_TO_MERGE = 1 << 5;
        const ALREADY_DONE = 1 << 6;
        const ISSUES_ASSIGNED = 1 << 7;
        const ISSUES_MENTIONED = 1 << 8;
        const ISSUES_AUTHORED = 1 << 9;
        const ISSUES_PARTICIPATING = 1 << 10;
    }
}

pub fn classify(
    pr: &PullRequest,
    me: &str,
    now: DateTime<Utc>,
    closed_window_days: i64,
) -> ViewSet {
    let rules = [
        (ViewSet::INBOX, in_inbox(pr, now, closed_window_days)),
        (ViewSet::NEEDS_YOU, needs_you(pr, me)),
        (ViewSet::YOUR_PRS, is_open_and_mine(pr, me)),
        (ViewSet::WAITING_ON_OTHERS, waiting_on_others(pr, me)),
        (ViewSet::CHANGES_REQUESTED, changes_requested(pr, me)),
        (
            ViewSet::READY_TO_MERGE,
            is_open_and_mine(pr, me) && is_ready(pr),
        ),
        (ViewSet::ALREADY_DONE, already_done(pr, me)),
    ];
    rules
        .into_iter()
        .filter(|(_, applies)| *applies)
        .fold(ViewSet::empty(), |views, (view, _)| views | view)
}

pub fn classify_issue(
    issue: &Issue,
    me: &str,
    now: DateTime<Utc>,
    closed_window_days: i64,
    last_seen: Option<DateTime<Utc>>,
) -> ViewSet {
    let in_inbox = issue_in_inbox(issue, now, closed_window_days);
    let rules = [
        (ViewSet::INBOX, in_inbox),
        (ViewSet::NEEDS_YOU, issue_needs_you(issue, me, last_seen)),
        (
            ViewSet::ISSUES_ASSIGNED,
            in_inbox && is_open_and_assigned(issue, me),
        ),
        (
            ViewSet::ISSUES_MENTIONED,
            in_inbox && issue.involvement.mentioned,
        ),
        (
            ViewSet::ISSUES_AUTHORED,
            in_inbox && is_issue_mine(issue, me),
        ),
        (
            ViewSet::ISSUES_PARTICIPATING,
            in_inbox && issue.involvement.commented && !is_issue_mine(issue, me),
        ),
    ];
    rules
        .into_iter()
        .filter(|(_, applies)| *applies)
        .fold(ViewSet::empty(), |views, (view, _)| views | view)
}

pub fn is_ready(pr: &PullRequest) -> bool {
    let approved = match pr.review_decision {
        Some(decision) => decision == ReviewDecision::Approved,
        None => pr
            .latest_reviews
            .iter()
            .any(|review| review.state == ReviewState::Approved),
    };
    let checks_ok = matches!(pr.checks, None | Some(ChecksState::Success));
    is_open(pr) && !pr.is_draft && approved && checks_ok && pr.mergeable == Mergeable::Mergeable
}

pub(crate) fn is_mine(pr: &PullRequest, me: &str) -> bool {
    pr.author.eq_ignore_ascii_case(me)
}

pub(crate) fn waiting_on_others(pr: &PullRequest, me: &str) -> bool {
    is_open_and_mine(pr, me)
        && !pr.is_draft
        && !matches!(
            pr.review_decision,
            Some(ReviewDecision::Approved | ReviewDecision::ChangesRequested)
        )
        && !pr.requested_reviewers.is_empty()
}

pub(crate) fn changes_requested(pr: &PullRequest, me: &str) -> bool {
    is_open_and_mine(pr, me) && pr.review_decision == Some(ReviewDecision::ChangesRequested)
}

pub(crate) fn already_done(pr: &PullRequest, me: &str) -> bool {
    is_open(pr)
        && !is_mine(pr, me)
        && my_review(pr, me).is_some_and(|review| review.state == ReviewState::Approved)
        && is_ready(pr)
}

fn is_open(pr: &PullRequest) -> bool {
    pr.state == PrState::Open
}

fn is_open_and_mine(pr: &PullRequest, me: &str) -> bool {
    is_open(pr) && is_mine(pr, me)
}

fn my_review<'a>(pr: &'a PullRequest, me: &str) -> Option<&'a LatestReview> {
    pr.latest_reviews
        .iter()
        .filter(|review| review.author.eq_ignore_ascii_case(me))
        .max_by_key(|review| review.submitted_at)
}

fn in_inbox(pr: &PullRequest, now: DateTime<Utc>, closed_window_days: i64) -> bool {
    is_open(pr)
        || pr
            .closed_at
            .is_some_and(|closed_at| now - closed_at <= Duration::days(closed_window_days))
}

pub(crate) fn needs_you(pr: &PullRequest, me: &str) -> bool {
    is_open(pr)
        && !is_mine(pr, me)
        && (pr.involvement.requested || pr.involvement.assigned)
        && my_review(pr, me).is_none_or(|review| review.submitted_at <= pr.head_committed_at)
}

pub(crate) fn is_issue_open(issue: &Issue) -> bool {
    issue.state == IssueState::Open
}

pub(crate) fn is_issue_mine(issue: &Issue, me: &str) -> bool {
    issue.author.eq_ignore_ascii_case(me)
}

pub(crate) fn is_assigned_to(issue: &Issue, me: &str) -> bool {
    issue
        .assignees
        .iter()
        .any(|assignee| assignee.eq_ignore_ascii_case(me))
}

pub(crate) fn is_open_and_assigned(issue: &Issue, me: &str) -> bool {
    is_issue_open(issue) && is_assigned_to(issue, me)
}

pub(crate) fn is_unread(issue: &Issue, last_seen: Option<DateTime<Utc>>) -> bool {
    last_seen.is_none_or(|seen| issue.updated_at > seen)
}

fn issue_in_inbox(issue: &Issue, now: DateTime<Utc>, closed_window_days: i64) -> bool {
    is_issue_open(issue)
        || issue
            .closed_at
            .is_some_and(|closed_at| now - closed_at <= Duration::days(closed_window_days))
}

fn issue_needs_you(issue: &Issue, me: &str, last_seen: Option<DateTime<Utc>>) -> bool {
    let newly_mentioned = issue
        .last_mention_at
        .is_some_and(|mention| last_seen.is_none_or(|seen| mention > seen));
    is_issue_open(issue)
        && !is_issue_mine(issue, me)
        && ((is_assigned_to(issue, me) && is_unread(issue, last_seen)) || newly_mentioned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{
        ME, assigned, days_ago, issue, mine, my_issue, now, pr, requested, review,
    };
    use crate::model::Involvement;

    const WINDOW_DAYS: i64 = 7;

    fn closed(state: PrState, closed_at: Option<DateTime<Utc>>) -> PullRequest {
        PullRequest {
            state,
            closed_at,
            ..pr()
        }
    }

    fn ready_mine() -> PullRequest {
        PullRequest {
            review_decision: Some(ReviewDecision::Approved),
            ..mine()
        }
    }

    fn approved_by_me() -> PullRequest {
        PullRequest {
            review_decision: Some(ReviewDecision::Approved),
            latest_reviews: vec![review(ME, ReviewState::Approved, days_ago(1))],
            ..pr()
        }
    }

    #[test]
    fn classify_assigns_expected_views() {
        let inbox = ViewSet::INBOX;
        let yours = ViewSet::INBOX | ViewSet::YOUR_PRS;
        let cases: Vec<(&str, PullRequest, ViewSet)> = vec![
            ("plain open pr of someone else", pr(), inbox),
            (
                "review requested",
                PullRequest {
                    involvement: requested(),
                    ..pr()
                },
                inbox | ViewSet::NEEDS_YOU,
            ),
            (
                "assigned",
                PullRequest {
                    involvement: assigned(),
                    ..pr()
                },
                inbox | ViewSet::NEEDS_YOU,
            ),
            (
                "requested but authored by me",
                PullRequest {
                    involvement: requested(),
                    ..mine()
                },
                yours,
            ),
            (
                "my review before the last commit keeps needs you",
                PullRequest {
                    involvement: requested(),
                    latest_reviews: vec![review(ME, ReviewState::Commented, days_ago(3))],
                    ..pr()
                },
                inbox | ViewSet::NEEDS_YOU,
            ),
            (
                "my review after the last commit leaves needs you",
                PullRequest {
                    involvement: requested(),
                    latest_reviews: vec![review(ME, ReviewState::Commented, days_ago(1))],
                    ..pr()
                },
                inbox,
            ),
            (
                "my review exactly at the last commit keeps needs you",
                PullRequest {
                    involvement: requested(),
                    latest_reviews: vec![review(ME, ReviewState::Commented, days_ago(2))],
                    ..pr()
                },
                inbox | ViewSet::NEEDS_YOU,
            ),
            (
                "review by someone else after the commit keeps needs you",
                PullRequest {
                    involvement: requested(),
                    latest_reviews: vec![review("other", ReviewState::Commented, days_ago(1))],
                    ..pr()
                },
                inbox | ViewSet::NEEDS_YOU,
            ),
            (
                "login comparison ignores case for author",
                PullRequest {
                    author: "ME".into(),
                    involvement: requested(),
                    ..pr()
                },
                yours,
            ),
            (
                "login comparison ignores case for review author",
                PullRequest {
                    involvement: requested(),
                    latest_reviews: vec![review("Me", ReviewState::Commented, days_ago(1))],
                    ..pr()
                },
                inbox,
            ),
            (
                "closed inside the window",
                closed(PrState::Closed, Some(days_ago(3))),
                inbox,
            ),
            (
                "merged inside the window",
                closed(PrState::Merged, Some(days_ago(6))),
                inbox,
            ),
            (
                "closed exactly at the window edge",
                closed(PrState::Closed, Some(days_ago(WINDOW_DAYS))),
                inbox,
            ),
            (
                "closed outside the window",
                closed(PrState::Closed, Some(days_ago(WINDOW_DAYS + 1))),
                ViewSet::empty(),
            ),
            (
                "merged outside the window",
                closed(PrState::Merged, Some(days_ago(30))),
                ViewSet::empty(),
            ),
            (
                "closed without closed_at",
                closed(PrState::Closed, None),
                ViewSet::empty(),
            ),
            (
                "closed pr requested from me",
                PullRequest {
                    involvement: requested(),
                    ..closed(PrState::Closed, Some(days_ago(1)))
                },
                inbox,
            ),
            (
                "closed pr authored by me",
                PullRequest {
                    author: ME.into(),
                    ..closed(PrState::Merged, Some(days_ago(1)))
                },
                inbox,
            ),
            ("my open pr without reviewers", mine(), yours),
            (
                "my draft pr with reviewers is not waiting",
                PullRequest {
                    is_draft: true,
                    requested_reviewers: vec!["bob".into()],
                    ..mine()
                },
                yours,
            ),
            (
                "my pr with pending reviewers and no decision",
                PullRequest {
                    requested_reviewers: vec!["bob".into()],
                    ..mine()
                },
                yours | ViewSet::WAITING_ON_OTHERS,
            ),
            (
                "my pr with review required and reviewers",
                PullRequest {
                    review_decision: Some(ReviewDecision::ReviewRequired),
                    requested_reviewers: vec!["bob".into()],
                    ..mine()
                },
                yours | ViewSet::WAITING_ON_OTHERS,
            ),
            (
                "my approved pr with reviewers is not waiting but is ready",
                PullRequest {
                    requested_reviewers: vec!["bob".into()],
                    ..ready_mine()
                },
                yours | ViewSet::READY_TO_MERGE,
            ),
            (
                "my pr with changes requested",
                PullRequest {
                    review_decision: Some(ReviewDecision::ChangesRequested),
                    requested_reviewers: vec!["bob".into()],
                    ..mine()
                },
                yours | ViewSet::CHANGES_REQUESTED,
            ),
            (
                "someone else's pr with changes requested",
                PullRequest {
                    review_decision: Some(ReviewDecision::ChangesRequested),
                    ..pr()
                },
                inbox,
            ),
            (
                "my approved pr is ready to merge",
                ready_mine(),
                yours | ViewSet::READY_TO_MERGE,
            ),
            (
                "my approved pr with failing checks is not ready",
                PullRequest {
                    checks: Some(ChecksState::Failure),
                    ..ready_mine()
                },
                yours,
            ),
            (
                "someone else's ready pr is not ready to merge for me",
                PullRequest {
                    review_decision: Some(ReviewDecision::Approved),
                    ..pr()
                },
                inbox,
            ),
            (
                "approved by me and ready",
                approved_by_me(),
                inbox | ViewSet::ALREADY_DONE,
            ),
            (
                "approved by me with approval before the last commit",
                PullRequest {
                    involvement: requested(),
                    latest_reviews: vec![review(ME, ReviewState::Approved, days_ago(3))],
                    ..approved_by_me()
                },
                inbox | ViewSet::NEEDS_YOU | ViewSet::ALREADY_DONE,
            ),
            (
                "older approval superseded by my newer changes requested",
                PullRequest {
                    latest_reviews: vec![
                        review(ME, ReviewState::Approved, days_ago(3)),
                        review(ME, ReviewState::ChangesRequested, days_ago(1)),
                    ],
                    ..approved_by_me()
                },
                inbox,
            ),
            (
                "newer approval supersedes my older changes requested",
                PullRequest {
                    latest_reviews: vec![
                        review(ME, ReviewState::Approved, days_ago(1)),
                        review(ME, ReviewState::ChangesRequested, days_ago(3)),
                    ],
                    ..approved_by_me()
                },
                inbox | ViewSet::ALREADY_DONE,
            ),
            (
                "approved by me but checks failing",
                PullRequest {
                    checks: Some(ChecksState::Failure),
                    ..approved_by_me()
                },
                inbox,
            ),
            (
                "commented by me is not already done",
                PullRequest {
                    latest_reviews: vec![review(ME, ReviewState::Commented, days_ago(1))],
                    review_decision: Some(ReviewDecision::Approved),
                    ..pr()
                },
                inbox,
            ),
            (
                "approved by someone else is not already done",
                PullRequest {
                    latest_reviews: vec![review("bob", ReviewState::Approved, days_ago(1))],
                    ..pr()
                },
                inbox,
            ),
            (
                "closed pr approved by me is not already done",
                PullRequest {
                    state: PrState::Merged,
                    closed_at: Some(days_ago(1)),
                    ..approved_by_me()
                },
                inbox,
            ),
        ];

        for (name, pr, expected) in cases {
            assert_eq!(classify(&pr, ME, now(), WINDOW_DAYS), expected, "{name}");
        }
    }

    fn assigned_issue() -> Issue {
        Issue {
            assignees: vec![ME.into()],
            ..issue()
        }
    }

    fn closed_issue(closed_at: Option<DateTime<Utc>>) -> Issue {
        Issue {
            state: IssueState::Closed,
            closed_at,
            ..issue()
        }
    }

    fn involvement_of(mentioned: bool, commented: bool) -> Involvement {
        Involvement {
            mentioned,
            commented,
            ..Involvement::default()
        }
    }

    #[test]
    fn classify_issue_assigns_expected_views() {
        let inbox = ViewSet::INBOX;
        let seen = Some(days_ago(2));
        let cases: Vec<(&str, Issue, Option<DateTime<Utc>>, ViewSet)> = vec![
            ("plain open issue", issue(), seen, inbox),
            (
                "assigned and unread",
                assigned_issue(),
                seen,
                inbox | ViewSet::NEEDS_YOU | ViewSet::ISSUES_ASSIGNED,
            ),
            (
                "assigned and already read",
                assigned_issue(),
                Some(days_ago(0)),
                inbox | ViewSet::ISSUES_ASSIGNED,
            ),
            (
                "assigned and updated exactly at last seen is read",
                assigned_issue(),
                Some(days_ago(1)),
                inbox | ViewSet::ISSUES_ASSIGNED,
            ),
            (
                "assigned and never seen",
                assigned_issue(),
                None,
                inbox | ViewSet::NEEDS_YOU | ViewSet::ISSUES_ASSIGNED,
            ),
            (
                "assignee login ignores case",
                Issue {
                    assignees: vec!["ME".into()],
                    ..issue()
                },
                seen,
                inbox | ViewSet::NEEDS_YOU | ViewSet::ISSUES_ASSIGNED,
            ),
            (
                "assigned to another person",
                Issue {
                    assignees: vec!["bob".into()],
                    ..issue()
                },
                seen,
                inbox,
            ),
            (
                "assigned to me but authored by me",
                Issue {
                    assignees: vec![ME.into()],
                    ..my_issue()
                },
                seen,
                inbox | ViewSet::ISSUES_ASSIGNED | ViewSet::ISSUES_AUTHORED,
            ),
            (
                "old mention with newer activity stays out",
                Issue {
                    involvement: involvement_of(true, false),
                    last_mention_at: Some(days_ago(3)),
                    ..issue()
                },
                seen,
                inbox | ViewSet::ISSUES_MENTIONED,
            ),
            (
                "new mention is needs you",
                Issue {
                    involvement: involvement_of(true, false),
                    last_mention_at: Some(days_ago(1)),
                    ..issue()
                },
                seen,
                inbox | ViewSet::NEEDS_YOU | ViewSet::ISSUES_MENTIONED,
            ),
            (
                "mention at exactly last seen is not new",
                Issue {
                    involvement: involvement_of(true, false),
                    last_mention_at: Some(days_ago(2)),
                    ..issue()
                },
                seen,
                inbox | ViewSet::ISSUES_MENTIONED,
            ),
            (
                "mention with no last seen counts as new",
                Issue {
                    involvement: involvement_of(true, false),
                    last_mention_at: Some(days_ago(3)),
                    ..issue()
                },
                None,
                inbox | ViewSet::NEEDS_YOU | ViewSet::ISSUES_MENTIONED,
            ),
            (
                "mentioned flag without mention timestamp",
                Issue {
                    involvement: involvement_of(true, false),
                    ..issue()
                },
                None,
                inbox | ViewSet::ISSUES_MENTIONED,
            ),
            (
                "new mention on my own issue never needs you",
                Issue {
                    involvement: involvement_of(true, false),
                    last_mention_at: Some(days_ago(0)),
                    ..my_issue()
                },
                seen,
                inbox | ViewSet::ISSUES_MENTIONED | ViewSet::ISSUES_AUTHORED,
            ),
            (
                "new mention on a closed issue does not need you",
                Issue {
                    involvement: involvement_of(true, false),
                    last_mention_at: Some(days_ago(0)),
                    ..closed_issue(Some(days_ago(3)))
                },
                seen,
                inbox | ViewSet::ISSUES_MENTIONED,
            ),
            (
                "authored by me",
                my_issue(),
                seen,
                inbox | ViewSet::ISSUES_AUTHORED,
            ),
            (
                "author login ignores case",
                Issue {
                    author: "ME".into(),
                    ..issue()
                },
                seen,
                inbox | ViewSet::ISSUES_AUTHORED,
            ),
            (
                "commented on another person's issue",
                Issue {
                    involvement: involvement_of(false, true),
                    ..issue()
                },
                seen,
                inbox | ViewSet::ISSUES_PARTICIPATING,
            ),
            (
                "commented on my own issue is not participating",
                Issue {
                    involvement: involvement_of(false, true),
                    ..my_issue()
                },
                seen,
                inbox | ViewSet::ISSUES_AUTHORED,
            ),
            (
                "closed inside the window",
                closed_issue(Some(days_ago(3))),
                seen,
                inbox,
            ),
            (
                "closed exactly at the window edge",
                closed_issue(Some(days_ago(WINDOW_DAYS))),
                seen,
                inbox,
            ),
            (
                "closed outside the window",
                closed_issue(Some(days_ago(WINDOW_DAYS + 1))),
                seen,
                ViewSet::empty(),
            ),
            (
                "closed without closed_at",
                closed_issue(None),
                seen,
                ViewSet::empty(),
            ),
            (
                "closed assigned issue keeps only the inbox",
                Issue {
                    assignees: vec![ME.into()],
                    ..closed_issue(Some(days_ago(1)))
                },
                seen,
                inbox,
            ),
            (
                "closed authored issue inside the window",
                Issue {
                    author: ME.into(),
                    ..closed_issue(Some(days_ago(1)))
                },
                seen,
                inbox | ViewSet::ISSUES_AUTHORED,
            ),
            (
                "closed authored issue outside the window has no views",
                Issue {
                    author: ME.into(),
                    ..closed_issue(Some(days_ago(30)))
                },
                seen,
                ViewSet::empty(),
            ),
            (
                "closed participating issue outside the window has no views",
                Issue {
                    involvement: involvement_of(true, true),
                    ..closed_issue(Some(days_ago(30)))
                },
                seen,
                ViewSet::empty(),
            ),
        ];

        for (name, issue, last_seen, expected) in cases {
            assert_eq!(
                classify_issue(&issue, ME, now(), WINDOW_DAYS, last_seen),
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn is_ready_follows_the_merge_rule() {
        let approved_review = || vec![review("bob", ReviewState::Approved, days_ago(1))];
        let cases: Vec<(&str, PullRequest, bool)> = vec![
            ("approved decision with no checks", ready_mine(), true),
            (
                "approved decision with successful checks",
                PullRequest {
                    checks: Some(ChecksState::Success),
                    ..ready_mine()
                },
                true,
            ),
            (
                "no decision with an approving review",
                PullRequest {
                    latest_reviews: approved_review(),
                    ..mine()
                },
                true,
            ),
            ("no decision and no review", mine(), false),
            (
                "no decision with only a comment review",
                PullRequest {
                    latest_reviews: vec![review("bob", ReviewState::Commented, days_ago(1))],
                    ..mine()
                },
                false,
            ),
            (
                "review required overrides an approving review",
                PullRequest {
                    review_decision: Some(ReviewDecision::ReviewRequired),
                    latest_reviews: approved_review(),
                    ..mine()
                },
                false,
            ),
            (
                "changes requested",
                PullRequest {
                    review_decision: Some(ReviewDecision::ChangesRequested),
                    ..mine()
                },
                false,
            ),
            (
                "draft is never ready",
                PullRequest {
                    is_draft: true,
                    ..ready_mine()
                },
                false,
            ),
            (
                "pending checks",
                PullRequest {
                    checks: Some(ChecksState::Pending),
                    ..ready_mine()
                },
                false,
            ),
            (
                "failing checks",
                PullRequest {
                    checks: Some(ChecksState::Failure),
                    ..ready_mine()
                },
                false,
            ),
            (
                "errored checks",
                PullRequest {
                    checks: Some(ChecksState::Error),
                    ..ready_mine()
                },
                false,
            ),
            (
                "conflicting",
                PullRequest {
                    mergeable: Mergeable::Conflicting,
                    ..ready_mine()
                },
                false,
            ),
            (
                "mergeable unknown",
                PullRequest {
                    mergeable: Mergeable::Unknown,
                    ..ready_mine()
                },
                false,
            ),
            (
                "merged",
                PullRequest {
                    state: PrState::Merged,
                    ..ready_mine()
                },
                false,
            ),
            (
                "closed",
                PullRequest {
                    state: PrState::Closed,
                    ..ready_mine()
                },
                false,
            ),
        ];

        for (name, pr, expected) in cases {
            assert_eq!(is_ready(&pr), expected, "{name}");
        }
    }
}
