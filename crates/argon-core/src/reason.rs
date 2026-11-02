use crate::model::{ChecksState, Mergeable, PullRequest, ReviewState};
use crate::views::{already_done, changes_requested, is_mine, is_ready, waiting_on_others};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    ReviewRequestedBy(String),
    AssignedBy(Option<String>),
    ChangesRequestedBy(String),
    WaitingOn(Vec<String>),
    ReadyToMerge,
    ApprovedByYouWaitingAuthor,
    CiFailed,
    Conflict,
    Tracking,
}

pub fn reason(pr: &PullRequest, me: &str) -> Reason {
    if changes_requested(pr, me)
        && let Some(reviewer) = latest_changes_requester(pr)
    {
        return Reason::ChangesRequestedBy(reviewer);
    }
    if is_mine(pr, me) && is_ready(pr) {
        return Reason::ReadyToMerge;
    }
    if pr.involvement.requested {
        return Reason::ReviewRequestedBy(pr.author.clone());
    }
    if pr.involvement.assigned {
        return Reason::AssignedBy(None);
    }
    if already_done(pr, me) {
        return Reason::ApprovedByYouWaitingAuthor;
    }
    if matches!(pr.checks, Some(ChecksState::Failure | ChecksState::Error)) {
        return Reason::CiFailed;
    }
    if pr.mergeable == Mergeable::Conflicting {
        return Reason::Conflict;
    }
    if waiting_on_others(pr, me) {
        return Reason::WaitingOn(pr.requested_reviewers.clone());
    }
    Reason::Tracking
}

fn latest_changes_requester(pr: &PullRequest) -> Option<String> {
    pr.latest_reviews
        .iter()
        .filter(|review| review.state == ReviewState::ChangesRequested)
        .max_by_key(|review| review.submitted_at)
        .map(|review| review.author.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{ME, assigned, days_ago, mine, pr, requested, review};
    use crate::model::ReviewDecision;

    fn changes_requested_mine() -> PullRequest {
        PullRequest {
            review_decision: Some(ReviewDecision::ChangesRequested),
            latest_reviews: vec![
                review("alice", ReviewState::ChangesRequested, days_ago(2)),
                review("bob", ReviewState::ChangesRequested, days_ago(1)),
                review("carol", ReviewState::Approved, days_ago(0)),
            ],
            ..mine()
        }
    }

    #[test]
    fn reason_picks_first_matching_rule() {
        let cases: Vec<(&str, PullRequest, Reason)> = vec![
            (
                "changes requested names the most recent requester",
                changes_requested_mine(),
                Reason::ChangesRequestedBy("bob".into()),
            ),
            (
                "changes requested wins over failing ci",
                PullRequest {
                    checks: Some(ChecksState::Failure),
                    ..changes_requested_mine()
                },
                Reason::ChangesRequestedBy("bob".into()),
            ),
            (
                "decision changes requested without a review falls through",
                PullRequest {
                    review_decision: Some(ReviewDecision::ChangesRequested),
                    ..mine()
                },
                Reason::Tracking,
            ),
            (
                "my ready pr",
                PullRequest {
                    review_decision: Some(ReviewDecision::Approved),
                    ..mine()
                },
                Reason::ReadyToMerge,
            ),
            (
                "someone else's ready pr is not ready to merge for me",
                PullRequest {
                    review_decision: Some(ReviewDecision::Approved),
                    ..pr()
                },
                Reason::Tracking,
            ),
            (
                "review requested names the author",
                PullRequest {
                    involvement: requested(),
                    ..pr()
                },
                Reason::ReviewRequestedBy("someone".into()),
            ),
            (
                "requested wins over assigned",
                PullRequest {
                    involvement: crate::model::Involvement {
                        requested: true,
                        assigned: true,
                        ..Default::default()
                    },
                    ..pr()
                },
                Reason::ReviewRequestedBy("someone".into()),
            ),
            (
                "assigned",
                PullRequest {
                    involvement: assigned(),
                    ..pr()
                },
                Reason::AssignedBy(None),
            ),
            (
                "approved by me waiting on the author",
                PullRequest {
                    review_decision: Some(ReviewDecision::Approved),
                    latest_reviews: vec![review(ME, ReviewState::Approved, days_ago(1))],
                    ..pr()
                },
                Reason::ApprovedByYouWaitingAuthor,
            ),
            (
                "failing checks",
                PullRequest {
                    checks: Some(ChecksState::Failure),
                    ..pr()
                },
                Reason::CiFailed,
            ),
            (
                "errored checks",
                PullRequest {
                    checks: Some(ChecksState::Error),
                    ..pr()
                },
                Reason::CiFailed,
            ),
            (
                "pending checks are not a failure",
                PullRequest {
                    checks: Some(ChecksState::Pending),
                    ..pr()
                },
                Reason::Tracking,
            ),
            (
                "conflict",
                PullRequest {
                    mergeable: Mergeable::Conflicting,
                    ..pr()
                },
                Reason::Conflict,
            ),
            (
                "failing checks win over conflict",
                PullRequest {
                    checks: Some(ChecksState::Failure),
                    mergeable: Mergeable::Conflicting,
                    ..pr()
                },
                Reason::CiFailed,
            ),
            (
                "waiting on requested reviewers",
                PullRequest {
                    requested_reviewers: vec!["bob".into(), "carol".into()],
                    ..mine()
                },
                Reason::WaitingOn(vec!["bob".into(), "carol".into()]),
            ),
            (
                "draft with reviewers is only tracking",
                PullRequest {
                    is_draft: true,
                    requested_reviewers: vec!["bob".into()],
                    ..mine()
                },
                Reason::Tracking,
            ),
            ("nothing applies", pr(), Reason::Tracking),
        ];

        for (name, pr, expected) in cases {
            assert_eq!(reason(&pr, ME), expected, "{name}");
        }
    }
}
