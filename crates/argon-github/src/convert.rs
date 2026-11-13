use argon_core::{
    ChecksState, Involvement, LatestReview, Mergeable, PrId, PrState, PullRequest, ReviewDecision,
    ReviewState,
};

use crate::wire::{
    WireMergeable, WirePr, WirePrState, WireReview, WireReviewDecision, WireReviewRequest,
    WireReviewState, WireReviewer, WireRollupState,
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
        closing_issues: Vec::new(),
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
    use crate::wire::{Envelope, HydrateData};

    const NODES_PAGE: &str = include_str!("../tests/fixtures/nodes_page.json");

    fn prs_from(json: &str) -> Vec<PullRequest> {
        let envelope: Envelope<HydrateData> = serde_json::from_str(json).unwrap();
        let parsed = envelope.into_data(Utc::now()).unwrap();
        into_pull_requests(parsed.data.nodes, "github.com")
    }

    pub(crate) fn fixture_prs() -> Vec<PullRequest> {
        prs_from(NODES_PAGE)
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
