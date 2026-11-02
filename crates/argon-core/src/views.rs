use bitflags::bitflags;
use chrono::{DateTime, Duration, Utc};

use crate::model::{
    ChecksState, LatestReview, Mergeable, PrState, PullRequest, ReviewDecision, ReviewState,
};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ViewSet: u8 {
        const INBOX = 1 << 0;
        const NEEDS_YOU = 1 << 1;
        const YOUR_PRS = 1 << 2;
        const WAITING_ON_OTHERS = 1 << 3;
        const CHANGES_REQUESTED = 1 << 4;
        const READY_TO_MERGE = 1 << 5;
        const ALREADY_DONE = 1 << 6;
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
        .find(|review| review.author.eq_ignore_ascii_case(me))
}

fn in_inbox(pr: &PullRequest, now: DateTime<Utc>, closed_window_days: i64) -> bool {
    is_open(pr)
        || pr
            .closed_at
            .is_some_and(|closed_at| now - closed_at <= Duration::days(closed_window_days))
}

fn needs_you(pr: &PullRequest, me: &str) -> bool {
    is_open(pr)
        && !is_mine(pr, me)
        && (pr.involvement.requested || pr.involvement.assigned)
        && my_review(pr, me).is_none_or(|review| review.submitted_at <= pr.head_committed_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{ME, assigned, days_ago, mine, now, pr, requested, review};

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
