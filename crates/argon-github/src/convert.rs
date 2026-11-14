use argon_core::{
    ChecksState, Involvement, Issue, IssueLabel, IssueRef, IssueState, IssueStateReason,
    LatestReview, LinkedPr, Mergeable, PrId, PrState, PullRequest, ReviewDecision, ReviewState,
    SubIssues,
};

use crate::wire::{
    WireIssue, WireIssueState, WireIssueStateReason, WireLinkedPr, WireMergeable, WirePr,
    WirePrState, WireReview, WireReviewDecision, WireReviewRequest, WireReviewState, WireReviewer,
    WireRollupState,
};

const GHOST: &str = "ghost";

pub(crate) fn into_pull_requests(nodes: Vec<Option<WirePr>>, host: &str) -> Vec<PullRequest> {
    nodes
        .into_iter()
        .flatten()
        .map(|wire| into_pull_request(wire, host))
        .collect()
}

pub(crate) fn into_pull_request(wire: WirePr, host: &str) -> PullRequest {
    let head_commit = wire
        .commits
        .into_items()
        .into_iter()
        .next()
        .map(|node| node.commit);
    let head_committed_at = head_commit
        .as_ref()
        .map_or(wire.updated_at, |commit| commit.committed_date);
    let checks = head_commit
        .and_then(|commit| commit.status_check_rollup)
        .and_then(|rollup| checks_state(rollup.state));

    PullRequest {
        id: PrId(wire.id),
        host: host.to_string(),
        repo: wire.repository.name_with_owner,
        number: wire.number,
        title: wire.title,
        url: wire.url,
        author: login_or_ghost(wire.author.map(|actor| actor.login)),
        is_draft: wire.is_draft,
        state: pr_state(wire.state),
        base: wire.base_ref_name,
        head: wire.head_ref_name,
        additions: wire.additions,
        deletions: wire.deletions,
        changed_files: wire.changed_files,
        comments: wire.comments.total_count,
        review_decision: wire.review_decision.and_then(review_decision),
        checks,
        mergeable: mergeable(wire.mergeable),
        requested_reviewers: wire
            .review_requests
            .into_items()
            .into_iter()
            .filter_map(reviewer_name)
            .collect(),
        assignees: wire
            .assignees
            .into_items()
            .into_iter()
            .map(|actor| actor.login)
            .collect(),
        latest_reviews: wire
            .latest_reviews
            .map(|reviews| reviews.into_items())
            .unwrap_or_default()
            .into_iter()
            .filter_map(latest_review)
            .collect(),
        head_committed_at,
        created_at: wire.created_at,
        updated_at: wire.updated_at,
        closed_at: wire.closed_at,
        involvement: Involvement::default(),
        closing_issues: wire
            .closing_issues_references
            .into_items()
            .into_iter()
            .map(|reference| IssueRef {
                repo: reference.repository.name_with_owner,
                number: reference.number,
            })
            .collect(),
    }
}

pub(crate) fn into_issues(nodes: Vec<Option<WireIssue>>, host: &str) -> Vec<Issue> {
    nodes
        .into_iter()
        .flatten()
        .map(|wire| into_issue(wire, host))
        .collect()
}

pub(crate) fn into_issue(wire: WireIssue, host: &str) -> Issue {
    Issue {
        id: PrId(wire.id),
        host: host.to_string(),
        repo: wire.repository.name_with_owner,
        number: wire.number,
        title: wire.title,
        url: wire.url,
        author: login_or_ghost(wire.author.map(|actor| actor.login)),
        state: issue_state(wire.state),
        state_reason: wire.state_reason.and_then(issue_state_reason),
        issue_type: wire.issue_type.map(|issue_type| issue_type.name),
        labels: wire
            .labels
            .into_items()
            .into_iter()
            .map(|label| IssueLabel {
                name: label.name,
                color: label.color,
            })
            .collect(),
        assignees: wire
            .assignees
            .into_items()
            .into_iter()
            .map(|actor| actor.login)
            .collect(),
        milestone: wire.milestone.map(|milestone| milestone.title),
        comments: wire.comments.total_count,
        linked_prs: wire
            .closed_by_pull_requests_references
            .into_items()
            .into_iter()
            .map(linked_pr)
            .collect(),
        sub_issues: wire
            .sub_issues_summary
            .filter(|summary| summary.total > 0)
            .map(|summary| SubIssues {
                total: summary.total,
                completed: summary.completed,
            }),
        created_at: wire.created_at,
        updated_at: wire.updated_at,
        closed_at: wire.closed_at,
        last_mention_at: None,
        involvement: Involvement::default(),
    }
}

fn linked_pr(wire: WireLinkedPr) -> LinkedPr {
    LinkedPr {
        repo: wire.repository.name_with_owner,
        number: wire.number,
        state: pr_state(wire.state),
        is_draft: wire.is_draft,
    }
}

pub(crate) fn issue_state(state: WireIssueState) -> IssueState {
    match state {
        WireIssueState::Open | WireIssueState::Unknown => IssueState::Open,
        WireIssueState::Closed => IssueState::Closed,
    }
}

fn issue_state_reason(reason: WireIssueStateReason) -> Option<IssueStateReason> {
    match reason {
        WireIssueStateReason::Completed => Some(IssueStateReason::Completed),
        WireIssueStateReason::NotPlanned => Some(IssueStateReason::NotPlanned),
        WireIssueStateReason::Duplicate => Some(IssueStateReason::Duplicate),
        WireIssueStateReason::Reopened => Some(IssueStateReason::Reopened),
        WireIssueStateReason::Unknown => None,
    }
}

pub(crate) fn login_or_ghost(login: Option<String>) -> String {
    login.unwrap_or_else(|| GHOST.to_string())
}

fn reviewer_name(request: WireReviewRequest) -> Option<String> {
    reviewer_label(request.requested_reviewer?)
}

pub(crate) fn reviewer_label(reviewer: WireReviewer) -> Option<String> {
    match (reviewer.slug, reviewer.organization, reviewer.login) {
        (Some(slug), Some(organization), _) => Some(format!("{}/{slug}", organization.login)),
        (_, _, login) => login,
    }
}

fn latest_review(review: WireReview) -> Option<LatestReview> {
    Some(LatestReview {
        author: login_or_ghost(review.author.map(|actor| actor.login)),
        state: review_state(review.state),
        submitted_at: review.submitted_at?,
    })
}

fn pr_state(state: WirePrState) -> PrState {
    match state {
        WirePrState::Open => PrState::Open,
        WirePrState::Closed => PrState::Closed,
        WirePrState::Merged => PrState::Merged,
        WirePrState::Unknown => PrState::Open,
    }
}

fn review_decision(decision: WireReviewDecision) -> Option<ReviewDecision> {
    match decision {
        WireReviewDecision::Approved => Some(ReviewDecision::Approved),
        WireReviewDecision::ChangesRequested => Some(ReviewDecision::ChangesRequested),
        WireReviewDecision::Unknown => None,
        WireReviewDecision::ReviewRequired => Some(ReviewDecision::ReviewRequired),
    }
}

fn mergeable(mergeable: WireMergeable) -> Mergeable {
    match mergeable {
        WireMergeable::Mergeable => Mergeable::Mergeable,
        WireMergeable::Conflicting => Mergeable::Conflicting,
        WireMergeable::Unknown => Mergeable::Unknown,
    }
}

pub(crate) fn review_state(state: WireReviewState) -> ReviewState {
    match state {
        WireReviewState::Approved => ReviewState::Approved,
        WireReviewState::ChangesRequested => ReviewState::ChangesRequested,
        WireReviewState::Commented | WireReviewState::Unknown => ReviewState::Commented,
        WireReviewState::Dismissed => ReviewState::Dismissed,
        WireReviewState::Pending => ReviewState::Pending,
    }
}

pub(crate) fn checks_state(state: WireRollupState) -> Option<ChecksState> {
    match state {
        WireRollupState::Success => Some(ChecksState::Success),
        WireRollupState::Failure => Some(ChecksState::Failure),
        WireRollupState::Pending | WireRollupState::Expected => Some(ChecksState::Pending),
        WireRollupState::Error => Some(ChecksState::Error),
        WireRollupState::Unknown => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use chrono::{DateTime, Utc};

    use super::*;
    use crate::wire::{Envelope, HydrateData, IssueHydrateData};

    const NODES_PAGE: &str = include_str!("../tests/fixtures/nodes_page.json");

    fn prs_from(json: &str) -> Vec<PullRequest> {
        let envelope: Envelope<HydrateData> = serde_json::from_str(json).unwrap();
        let parsed = envelope.into_data(Utc::now()).unwrap();
        into_pull_requests(parsed.data.nodes, "github.com")
    }

    pub(crate) fn fixture_prs() -> Vec<PullRequest> {
        prs_from(NODES_PAGE)
    }

    const ISSUE_NODES: &str = include_str!("../tests/fixtures/issue_nodes.json");

    pub(crate) fn fixture_issues() -> Vec<Issue> {
        let envelope: Envelope<IssueHydrateData> = serde_json::from_str(ISSUE_NODES).unwrap();
        let parsed = envelope.into_data(Utc::now()).unwrap();
        into_issues(parsed.data.nodes, "github.com")
    }

    fn issue_by_number(issues: &[Issue], number: u64) -> &Issue {
        issues.iter().find(|issue| issue.number == number).unwrap()
    }

    #[test]
    fn issue_fixture_maps_every_issue() {
        let numbers: Vec<u64> = fixture_issues().iter().map(|issue| issue.number).collect();
        assert_eq!(numbers, vec![101, 87, 64, 130, 92]);
    }

    #[test]
    fn open_issue_with_type_labels_assignees_and_milestone() {
        let issues = fixture_issues();
        let issue = issue_by_number(&issues, 101);
        assert_eq!(issue.id, PrId("I_kwDOOrbita0101".into()));
        assert_eq!(issue.host, "github.com");
        assert_eq!(issue.repo, "orbita-health/atlas");
        assert_eq!(issue.title, "Scheduling client times out on large clinics");
        assert_eq!(
            issue.url,
            "https://github.com/orbita-health/atlas/issues/101"
        );
        assert_eq!(issue.author, "mcouto");
        assert_eq!(issue.state, IssueState::Open);
        assert_eq!(issue.state_reason, None);
        assert_eq!(issue.issue_type.as_deref(), Some("Bug"));
        assert_eq!(
            issue.labels,
            vec![
                IssueLabel {
                    name: "bug".into(),
                    color: "d73a4a".into()
                },
                IssueLabel {
                    name: "scheduling".into(),
                    color: "0e8a16".into()
                },
            ]
        );
        assert_eq!(issue.assignees, vec!["lfarias", "dnogueira"]);
        assert_eq!(issue.milestone.as_deref(), Some("Q4 stability"));
        assert_eq!(issue.comments, 3);
        assert!(issue.linked_prs.is_empty());
        assert_eq!(issue.sub_issues, None);
        assert_eq!(issue.created_at, at("2026-10-20T08:00:00Z"));
        assert_eq!(issue.updated_at, at("2026-11-02T10:15:00Z"));
        assert_eq!(issue.closed_at, None);
        assert_eq!(issue.last_mention_at, None);
        assert_eq!(issue.involvement, Involvement::default());
    }

    #[test]
    fn closed_issue_keeps_state_reason_and_linked_merged_pr() {
        let issues = fixture_issues();
        let completed = issue_by_number(&issues, 87);
        assert_eq!(completed.state, IssueState::Closed);
        assert_eq!(completed.state_reason, Some(IssueStateReason::Completed));
        assert_eq!(completed.closed_at, Some(at("2026-10-30T09:00:00Z")));
        assert_eq!(completed.issue_type, None);
        assert_eq!(completed.milestone, None);
        assert_eq!(
            completed.linked_prs,
            vec![LinkedPr {
                repo: "orbita-health/atlas".into(),
                number: 398,
                state: PrState::Merged,
                is_draft: false,
            }]
        );
    }

    #[test]
    fn issue_closed_as_not_planned() {
        let issues = fixture_issues();
        let issue = issue_by_number(&issues, 64);
        assert_eq!(issue.state, IssueState::Closed);
        assert_eq!(issue.state_reason, Some(IssueStateReason::NotPlanned));
        assert!(issue.assignees.is_empty());
    }

    #[test]
    fn issue_with_open_draft_pr_linked() {
        let issues = fixture_issues();
        let issue = issue_by_number(&issues, 130);
        assert_eq!(
            issue.linked_prs,
            vec![LinkedPr {
                repo: "orbita-health/atlas".into(),
                number: 412,
                state: PrState::Open,
                is_draft: true,
            }]
        );
    }

    #[test]
    fn issue_with_sub_issues_from_deleted_author() {
        let issues = fixture_issues();
        let issue = issue_by_number(&issues, 92);
        assert_eq!(issue.author, "ghost");
        assert_eq!(issue.state_reason, Some(IssueStateReason::Reopened));
        assert_eq!(
            issue.sub_issues,
            Some(SubIssues {
                total: 5,
                completed: 2
            })
        );
    }

    #[test]
    fn unknown_issue_enum_values_fall_back_without_failing_the_page() {
        let json = ISSUE_NODES
            .replace("\"NOT_PLANNED\"", "\"BRAND_NEW\"")
            .replace("\"CLOSED\"", "\"BRAND_NEW\"");
        let envelope: Envelope<IssueHydrateData> = serde_json::from_str(&json).unwrap();
        let issues = into_issues(
            envelope.into_data(Utc::now()).unwrap().data.nodes,
            "github.com",
        );
        assert_eq!(issues.len(), 5);
        assert_eq!(issue_by_number(&issues, 64).state, IssueState::Open);
        assert_eq!(issue_by_number(&issues, 64).state_reason, None);
    }

    #[test]
    fn pull_request_closing_issues_come_from_the_fixture() {
        let prs = fixture_prs();
        assert_eq!(
            by_number(&prs, 412).closing_issues,
            vec![
                IssueRef {
                    repo: "orbita-health/atlas".into(),
                    number: 130
                },
                IssueRef {
                    repo: "orbita-health/atlas".into(),
                    number: 101
                },
            ]
        );
        assert!(by_number(&prs, 398).closing_issues.is_empty());
    }

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }

    fn by_number(prs: &[PullRequest], number: u64) -> &PullRequest {
        prs.iter().find(|pr| pr.number == number).unwrap()
    }

    #[test]
    fn search_page_fixture_maps_every_pull_request() {
        let prs = fixture_prs();
        let numbers: Vec<u64> = prs.iter().map(|pr| pr.number).collect();
        assert_eq!(numbers, vec![412, 398, 77, 61, 80]);
    }

    #[test]
    fn open_pr_with_team_reviewer_and_failing_checks() {
        let prs = fixture_prs();
        let pr = by_number(&prs, 412);
        assert_eq!(pr.id, PrId("PR_kwDOOrbita0412".into()));
        assert_eq!(pr.host, "github.com");
        assert_eq!(pr.repo, "orbita-health/atlas");
        assert_eq!(pr.title, "Add retry budget to scheduling client");
        assert_eq!(pr.url, "https://github.com/orbita-health/atlas/pull/412");
        assert_eq!(pr.author, "mcouto");
        assert!(!pr.is_draft);
        assert_eq!(pr.state, PrState::Open);
        assert_eq!(
            (pr.base.as_str(), pr.head.as_str()),
            ("main", "feat/retry-budget")
        );
        assert_eq!((pr.additions, pr.deletions, pr.changed_files), (214, 37, 9));
        assert_eq!(pr.comments, 6);
        assert_eq!(pr.review_decision, Some(ReviewDecision::ChangesRequested));
        assert_eq!(pr.checks, Some(ChecksState::Failure));
        assert_eq!(pr.mergeable, Mergeable::Conflicting);
        assert_eq!(
            pr.requested_reviewers,
            vec!["lfarias", "orbita-health/platform"]
        );
        assert_eq!(pr.assignees, vec!["mcouto"]);
        assert_eq!(
            pr.latest_reviews,
            vec![
                LatestReview {
                    author: "dnogueira".into(),
                    state: ReviewState::ChangesRequested,
                    submitted_at: at("2026-10-30T17:22:45Z"),
                },
                LatestReview {
                    author: "lfarias".into(),
                    state: ReviewState::Commented,
                    submitted_at: at("2026-10-31T08:03:10Z"),
                },
            ]
        );
        assert_eq!(pr.head_committed_at, at("2026-10-31T20:11:09Z"));
        assert_eq!(pr.created_at, at("2026-10-27T13:05:11Z"));
        assert_eq!(pr.updated_at, at("2026-11-01T09:40:02Z"));
        assert_eq!(pr.closed_at, None);
        assert_eq!(pr.involvement, Involvement::default());
    }

    #[test]
    fn merged_pr_from_deleted_author_without_checks() {
        let prs = fixture_prs();
        let pr = by_number(&prs, 398);
        assert_eq!(pr.author, "ghost");
        assert_eq!(pr.state, PrState::Merged);
        assert_eq!(pr.closed_at, Some(at("2026-10-29T11:00:00Z")));
        assert_eq!(pr.checks, None);
        assert_eq!(pr.review_decision, None);
        assert_eq!(pr.mergeable, Mergeable::Unknown);
        assert!(pr.requested_reviewers.is_empty());
        assert!(pr.assignees.is_empty());
        assert!(pr.latest_reviews.is_empty());
    }

    #[test]
    fn draft_pr_with_successful_checks_and_approval() {
        let prs = fixture_prs();
        let pr = by_number(&prs, 77);
        assert!(pr.is_draft);
        assert_eq!(pr.checks, Some(ChecksState::Success));
        assert_eq!(pr.review_decision, Some(ReviewDecision::Approved));
        assert_eq!(pr.mergeable, Mergeable::Mergeable);
        assert_eq!(pr.latest_reviews[0].state, ReviewState::Approved);
    }

    #[test]
    fn closed_pr_without_commits_falls_back_to_updated_at() {
        let prs = fixture_prs();
        let pr = by_number(&prs, 61);
        assert_eq!(pr.state, PrState::Closed);
        assert_eq!(pr.checks, None);
        assert_eq!(pr.review_decision, Some(ReviewDecision::ReviewRequired));
        assert_eq!(pr.head_committed_at, pr.updated_at);
    }

    #[test]
    fn rollup_states_map_to_checks_state() {
        let prs = fixture_prs();
        assert_eq!(by_number(&prs, 80).checks, Some(ChecksState::Pending));
        assert_eq!(
            checks_state(WireRollupState::Expected),
            Some(ChecksState::Pending)
        );
        assert_eq!(
            checks_state(WireRollupState::Error),
            Some(ChecksState::Error)
        );
    }

    #[test]
    fn unknown_enum_values_fall_back_without_failing_the_page() {
        let json = NODES_PAGE
            .replace("\"EXPECTED\"", "\"BRAND_NEW\"")
            .replace("\"CONFLICTING\"", "\"BRAND_NEW\"")
            .replace("\"COMMENTED\"", "\"BRAND_NEW\"")
            .replace("\"CHANGES_REQUESTED\"", "\"BRAND_NEW\"")
            .replace("\"MERGED\"", "\"BRAND_NEW\"");
        let prs = prs_from(&json);
        assert_eq!(prs.len(), 5);
        assert_eq!(by_number(&prs, 80).checks, None);
        assert_eq!(by_number(&prs, 412).mergeable, Mergeable::Unknown);
        assert_eq!(by_number(&prs, 412).review_decision, None);
        assert_eq!(
            by_number(&prs, 412).latest_reviews[1].state,
            ReviewState::Commented
        );
        assert_eq!(by_number(&prs, 398).state, PrState::Open);
    }
}
