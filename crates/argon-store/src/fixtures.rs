use argon_core::{Involvement, Mergeable, PrId, PrState, PullRequest};
use chrono::{DateTime, Duration, TimeZone, Utc};

pub fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 2, 12, 0, 0).unwrap()
}

pub fn days_ago(days: i64) -> DateTime<Utc> {
    now() - Duration::days(days)
}

pub fn pr(id: &str, host: &str, updated_at: DateTime<Utc>) -> PullRequest {
    PullRequest {
        id: PrId(id.into()),
        host: host.into(),
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
        head_committed_at: days_ago(10),
        created_at: days_ago(20),
        updated_at,
        closed_at: None,
        involvement: Involvement::default(),
    }
}
