use chrono::{DateTime, Duration, TimeZone, Utc};

use crate::model::{
    DEFAULT_HOST, Involvement, LatestReview, Mergeable, PrId, PrState, PullRequest, ReviewState,
};

pub const ME: &str = "me";

pub fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 2, 12, 0, 0).unwrap()
}

pub fn days_ago(days: i64) -> DateTime<Utc> {
    now() - Duration::days(days)
}

pub fn pr() -> PullRequest {
    PullRequest {
        id: PrId("PR_node".into()),
        host: DEFAULT_HOST.into(),
        repo: "acme/widgets".into(),
        number: 482,
        title: "Fix slot finder".into(),
        url: "https://github.com/acme/widgets/pull/482".into(),
        author: "someone".into(),
        is_draft: false,
        state: PrState::Open,
        base: "main".into(),
        head: "fix/slot-finder".into(),
        additions: 10,
        deletions: 2,
        changed_files: 3,
        comments: 0,
        review_decision: None,
        checks: None,
        mergeable: Mergeable::Mergeable,
        requested_reviewers: Vec::new(),
        assignees: Vec::new(),
        latest_reviews: Vec::new(),
        head_committed_at: days_ago(2),
        created_at: days_ago(5),
        updated_at: days_ago(1),
        closed_at: None,
        involvement: Involvement::default(),
    }
}

pub fn pr_on(host: &str, id: &str, updated_at: DateTime<Utc>) -> PullRequest {
    PullRequest {
        id: PrId(id.into()),
        host: host.into(),
        url: format!("https://{host}/acme/widgets/pull/482"),
        updated_at,
        ..pr()
    }
}

pub fn mine() -> PullRequest {
    PullRequest {
        author: ME.into(),
        ..pr()
    }
}

pub fn review(author: &str, state: ReviewState, submitted_at: DateTime<Utc>) -> LatestReview {
    LatestReview {
        author: author.into(),
        state,
        submitted_at,
    }
}

pub fn requested() -> Involvement {
    Involvement {
        requested: true,
        ..Involvement::default()
    }
}

pub fn assigned() -> Involvement {
    Involvement {
        assigned: true,
        ..Involvement::default()
    }
}
