use crate::model::{ChecksState, Issue, Mergeable, PrState, PullRequest, ReviewState};
use crate::views::{
    already_done, changes_requested, is_issue_mine, is_issue_open, is_mine, is_open_and_assigned,
    is_ready, needs_you, waiting_on_others,
};

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
    IssueAssigned,
    Mentioned,
    IssueAuthored,
    Participating,
    NewInOrg(String),
    ClosedBy(u64),
    Tracking,
}

pub fn issue_reason(issue: &Issue, me: &str) -> Reason {
    if !is_issue_open(issue)
        && let Some(linked) = issue
            .linked_prs
            .iter()
            .find(|linked| linked.state == PrState::Merged)
    {
        return Reason::ClosedBy(linked.number);
    }
    if is_open_and_assigned(issue, me) {
        return Reason::IssueAssigned;
    }
    if issue.involvement.mentioned {
        return Reason::Mentioned;
    }
    if is_issue_mine(issue, me) {
        return Reason::IssueAuthored;
    }
    if issue.involvement.commented {
        return Reason::Participating;
    }
    if is_issue_open(issue) && issue.assignees.is_empty() {
        return Reason::NewInOrg(org_of(&issue.repo).to_string());
    }
    Reason::Tracking
}

fn org_of(repo: &str) -> &str {
    repo.split_once('/').map_or(repo, |(owner, _)| owner)
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
    if needs_you(pr, me) {
        return if pr.involvement.requested {
            Reason::ReviewRequestedBy(pr.author.clone())
        } else {
            Reason::AssignedBy(None)
        };
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
    use crate::fixtures::{ME, assigned, days_ago, issue, mine, my_issue, pr, requested, review};
    use crate::model::{Involvement, IssueState, LinkedPr, PrState, ReviewDecision};

    fn involved(mentioned: bool, commented: bool) -> Involvement {
        Involvement {
            mentioned,
            commented,
            ..Involvement::default()
        }
    }

    fn linked(number: u64, state: PrState) -> LinkedPr {
        LinkedPr {
            repo: "acme/widgets".into(),
            number,
            state,
            is_draft: false,
        }
    }

    fn closed(issue: Issue) -> Issue {
        Issue {
            state: IssueState::Closed,
            closed_at: Some(days_ago(1)),
            ..issue
        }
    }

    #[test]
    fn issue_reason_picks_first_matching_rule() {
        let cases: Vec<(&str, Issue, Reason)> = vec![
            (
                "closed with a merged linked pr",
                closed(Issue {
                    linked_prs: vec![linked(7, PrState::Closed), linked(9, PrState::Merged)],
                    ..issue()
                }),
                Reason::ClosedBy(9),
            ),
            (
                "closed by a merged pr wins over assignment",
                closed(Issue {
                    assignees: vec![ME.into()],
                    linked_prs: vec![linked(9, PrState::Merged)],
                    ..issue()
                }),
                Reason::ClosedBy(9),
            ),
            (
                "open issue with a merged linked pr is not closed by it",
                Issue {
                    linked_prs: vec![linked(9, PrState::Merged)],
                    assignees: vec!["bob".into()],
                    ..issue()
                },
                Reason::Tracking,
            ),
            (
                "closed with only an unmerged linked pr",
                closed(Issue {
                    linked_prs: vec![linked(9, PrState::Closed)],
                    ..issue()
                }),
                Reason::Tracking,
            ),
            (
                "assigned to me",
                Issue {
                    assignees: vec!["Me".into()],
                    ..issue()
                },
                Reason::IssueAssigned,
            ),
            (
                "assigned wins over mention",
                Issue {
                    assignees: vec![ME.into()],
                    involvement: involved(true, true),
                    ..issue()
                },
                Reason::IssueAssigned,
            ),
            (
                "closed issue assigned to me is only tracking",
                closed(Issue {
                    assignees: vec![ME.into()],
                    ..issue()
                }),
                Reason::Tracking,
            ),
            (
                "mentioned",
                Issue {
                    assignees: vec!["bob".into()],
                    involvement: involved(true, false),
                    ..issue()
                },
                Reason::Mentioned,
            ),
            (
                "mentioned wins over authored",
                Issue {
                    involvement: involved(true, false),
                    ..my_issue()
                },
                Reason::Mentioned,
            ),
            (
                "authored by me",
                Issue {
                    assignees: vec!["bob".into()],
                    ..my_issue()
                },
                Reason::IssueAuthored,
            ),
            (
                "authored wins over unassigned",
                my_issue(),
                Reason::IssueAuthored,
            ),
            (
                "commented",
                Issue {
                    assignees: vec!["bob".into()],
                    involvement: involved(false, true),
                    ..issue()
                },
                Reason::Participating,
            ),
            (
                "commented wins over unassigned",
                Issue {
                    involvement: involved(false, true),
                    ..issue()
                },
                Reason::Participating,
            ),
            (
                "open and unassigned names the org",
                issue(),
                Reason::NewInOrg("acme".into()),
            ),
            (
                "closed and unassigned is only tracking",
                closed(issue()),
                Reason::Tracking,
            ),
            (
                "assigned to someone else",
                Issue {
                    assignees: vec!["bob".into()],
                    ..issue()
                },
                Reason::Tracking,
            ),
        ];

        for (name, issue, expected) in cases {
            assert_eq!(issue_reason(&issue, ME), expected, "{name}");
        }
    }

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
                "closed pr with review requested is only tracking",
                PullRequest {
                    state: PrState::Merged,
                    involvement: requested(),
                    ..pr()
                },
                Reason::Tracking,
            ),
            (
                "closed pr assigned to me is only tracking",
                PullRequest {
                    state: PrState::Closed,
                    involvement: assigned(),
                    ..pr()
                },
                Reason::Tracking,
            ),
            (
                "requested but already reviewed after the last commit",
                PullRequest {
                    involvement: requested(),
                    latest_reviews: vec![review(ME, ReviewState::Commented, days_ago(1))],
                    ..pr()
                },
                Reason::Tracking,
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
