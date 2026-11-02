use argon_core::{PullRequest, Side};

use crate::convert::{
    checks_state, into_pull_request, login_or_ghost, review_state, reviewer_label,
};
use crate::detail::{
    CommitInfo, Label, PrDetail, ReviewComment, ReviewThread, ReviewerStatus, ThreadComment,
    TimelineItem,
};
use crate::detail_wire::{
    WireDetail, WireDetailCommit, WireDiffSide, WireGitActor, WireLabel, WireThread,
    WireTimelineItem,
};
use crate::wire::Actor;

pub(crate) fn into_pr_detail(wire: WireDetail, host: &str) -> PrDetail {
    let pr = into_pull_request(wire.pr, host);
    let reviewers = reviewer_statuses(&pr);
    let truncated_timeline = wire.timeline_items.has_previous_page();
    let truncated_threads = wire.review_threads.has_next_page();
    let timeline = wire
        .timeline_items
        .into_items()
        .into_iter()
        .filter_map(timeline_item)
        .collect();
    PrDetail {
        pr,
        body: wire.body.unwrap_or_default(),
        head_oid: wire.head_ref_oid,
        labels: wire.labels.into_items().into_iter().map(label).collect(),
        reviewers,
        timeline,
        truncated_timeline,
        truncated_threads,
        threads: wire
            .review_threads
            .into_items()
            .into_iter()
            .map(thread)
            .collect(),
        commits: wire
            .all_commits
            .into_items()
            .into_iter()
            .map(|node| commit_info(node.commit))
            .collect(),
    }
}

fn label(wire: WireLabel) -> Label {
    Label {
        name: wire.name,
        color: wire.color,
    }
}

fn reviewer_statuses(pr: &PullRequest) -> Vec<ReviewerStatus> {
    let mut reviewers: Vec<ReviewerStatus> = pr
        .latest_reviews
        .iter()
        .map(|review| ReviewerStatus {
            login: review.author.clone(),
            state: Some(review.state),
            requested: false,
        })
        .collect();
    for login in &pr.requested_reviewers {
        match reviewers.iter_mut().find(|status| &status.login == login) {
            Some(status) => status.requested = true,
            None => reviewers.push(ReviewerStatus {
                login: login.clone(),
                state: None,
                requested: true,
            }),
        }
    }
    reviewers
}

fn actor_login(actor: Option<Actor>) -> String {
    login_or_ghost(actor.map(|actor| actor.login))
}

fn git_author_login(author: &Option<WireGitActor>) -> Option<String> {
    author
        .as_ref()
        .and_then(|author| author.user.as_ref())
        .map(|user| user.login.clone())
}

fn git_author_name(author: &Option<WireGitActor>) -> String {
    let name = author
        .as_ref()
        .and_then(|author| author.name.clone())
        .filter(|name| !name.is_empty());
    login_or_ghost(name.or_else(|| git_author_login(author)))
}

fn commit_info(commit: WireDetailCommit) -> CommitInfo {
    CommitInfo {
        author_name: git_author_name(&commit.author),
        author_login: git_author_login(&commit.author),
        oid: commit.oid,
        headline: commit.message_headline,
        body: commit.message_body.unwrap_or_default(),
        committed_at: commit.committed_date,
        additions: commit.additions,
        deletions: commit.deletions,
        checks: commit
            .status_check_rollup
            .and_then(|rollup| checks_state(rollup.state)),
    }
}

fn thread(wire: WireThread) -> ReviewThread {
    ReviewThread {
        id: wire.id,
        resolved: wire.is_resolved,
        path: wire.path,
        line: wire.line,
        start_line: wire.start_line,
        truncated_comments: wire.comments.has_next_page(),
        side: match wire.diff_side {
            WireDiffSide::Left => Side::Left,
            WireDiffSide::Right | WireDiffSide::Unknown => Side::Right,
        },
        comments: wire
            .comments
            .into_items()
            .into_iter()
            .map(|comment| ThreadComment {
                id: comment.id,
                author: actor_login(comment.author),
                body: comment.body,
                created_at: comment.created_at,
            })
            .collect(),
    }
}

fn timeline_item(wire: WireTimelineItem) -> Option<TimelineItem> {
    let item = match wire {
        WireTimelineItem::IssueComment(comment) => TimelineItem::Comment {
            id: comment.id,
            author: actor_login(comment.author),
            body: comment.body,
            created_at: comment.created_at,
        },
        WireTimelineItem::PullRequestReview(review) => TimelineItem::Review {
            id: review.id,
            author: actor_login(review.author),
            state: review_state(review.state),
            body: review.body,
            submitted_at: review.submitted_at.unwrap_or(review.created_at),
            comments: review
                .comments
                .into_items()
                .into_iter()
                .map(|comment| ReviewComment {
                    path: comment.path,
                    line: comment.line,
                    body: comment.body,
                    diff_hunk: comment.diff_hunk,
                })
                .collect(),
        },
        WireTimelineItem::PullRequestCommit(item) => TimelineItem::Commit {
            author: git_author_name(&item.commit.author),
            oid: item.commit.oid,
            headline: item.commit.message_headline,
            committed_at: item.commit.committed_date,
        },
        WireTimelineItem::ReviewRequestedEvent(event) => TimelineItem::ReviewRequested {
            actor: actor_login(event.actor),
            reviewer: login_or_ghost(event.requested_reviewer.and_then(reviewer_label)),
            at: event.created_at,
        },
        WireTimelineItem::LabeledEvent(event) => TimelineItem::Labeled {
            actor: actor_login(event.actor),
            label: event.label.name,
            at: event.created_at,
        },
        WireTimelineItem::HeadRefForcePushedEvent(event) => TimelineItem::HeadRefForcePushed {
            actor: actor_login(event.actor),
            at: event.created_at,
        },
        WireTimelineItem::MergedEvent(event) => TimelineItem::Merged {
            actor: actor_login(event.actor),
            at: event.created_at,
        },
        WireTimelineItem::ClosedEvent(event) => TimelineItem::Closed {
            actor: actor_login(event.actor),
            at: event.created_at,
        },
        WireTimelineItem::ReadyForReviewEvent(event) => TimelineItem::ReadyForReview {
            actor: actor_login(event.actor),
            at: event.created_at,
        },
        WireTimelineItem::Unknown => return None,
    };
    Some(item)
}

#[cfg(test)]
mod tests {
    use argon_core::{ChecksState, PrId, PrState, ReviewState};
    use chrono::{DateTime, Utc};

    use super::*;
    use crate::detail_wire::DetailData;
    use crate::wire::Envelope;

    const DETAIL: &str = include_str!("../tests/fixtures/detail.json");

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }

    fn detail_from(json: &str) -> PrDetail {
        let envelope: Envelope<DetailData> = serde_json::from_str(json).unwrap();
        let wire = envelope.into_data(Utc::now()).unwrap().data.node.unwrap();
        into_pr_detail(wire, "github.com")
    }

    fn detail() -> PrDetail {
        detail_from(DETAIL)
    }

    #[test]
    fn pull_request_part_reuses_the_inbox_conversion() {
        let detail = detail();
        assert_eq!(detail.pr.id, PrId("PR_kwDOOrbita0482".into()));
        assert_eq!(detail.pr.host, "github.com");
        assert_eq!(detail.pr.repo, "orbita-health/atlas");
        assert_eq!(detail.pr.number, 482);
        assert_eq!(detail.pr.state, PrState::Open);
        assert_eq!(detail.pr.checks, Some(ChecksState::Failure));
        assert_eq!(detail.pr.head_committed_at, at("2026-10-31T20:11:09Z"));
        assert_eq!(detail.head_oid, "3fa91c0de4b27a85d6e1f09c4b7a2d83e5f61a90");
        assert!(detail.body.starts_with("Adds a retry budget"));
        assert!(detail.body.contains("- jittered backoff"));
    }

    #[test]
    fn labels_are_mapped() {
        let labels: Vec<(String, String)> = detail()
            .labels
            .into_iter()
            .map(|label| (label.name, label.color))
            .collect();
        assert_eq!(
            labels,
            vec![
                ("needs-review".to_string(), "fbca04".to_string()),
                ("backend".to_string(), "0e8a16".to_string()),
            ]
        );
    }

    #[test]
    fn reviewers_merge_latest_reviews_with_pending_requests_and_teams() {
        let reviewers = detail().reviewers;
        let summary: Vec<(&str, Option<ReviewState>, bool)> = reviewers
            .iter()
            .map(|reviewer| (reviewer.login.as_str(), reviewer.state, reviewer.requested))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("dnogueira", Some(ReviewState::ChangesRequested), false),
                ("lfarias", Some(ReviewState::Commented), true),
                ("orbita-health/platform", None, true),
                ("pbarros", None, true),
            ]
        );
    }

    #[test]
    fn timeline_maps_every_item_type_and_skips_unknown_and_null_nodes() {
        let detail = detail();
        assert!(detail.truncated_timeline);
        assert_eq!(detail.timeline.len(), 15);

        assert_eq!(
            detail.timeline[0],
            TimelineItem::Commit {
                oid: "a1c4e7f0b92d3586714fe0ab39c85d2e61f7b034".into(),
                headline: "Add retry budget type".into(),
                author: "Marina Couto".into(),
                committed_at: at("2026-10-27T12:40:00Z"),
            }
        );
        assert_eq!(
            detail.timeline[1],
            TimelineItem::Comment {
                id: "IC_kwDOOrbita01".into(),
                author: "mcouto".into(),
                body: "Ready for a first pass.".into(),
                created_at: at("2026-10-27T13:10:00Z"),
            }
        );
        assert_eq!(
            detail.timeline[2],
            TimelineItem::Labeled {
                actor: "mcouto".into(),
                label: "needs-review".into(),
                at: at("2026-10-27T13:11:00Z"),
            }
        );
        assert_eq!(
            detail.timeline[3],
            TimelineItem::ReviewRequested {
                actor: "mcouto".into(),
                reviewer: "lfarias".into(),
                at: at("2026-10-27T13:12:00Z"),
            }
        );
        assert!(matches!(
            &detail.timeline[4],
            TimelineItem::ReviewRequested { reviewer, .. } if reviewer == "orbita-health/platform"
        ));
        assert!(matches!(
            &detail.timeline[5],
            TimelineItem::ReviewRequested { reviewer, .. } if reviewer == "ghost"
        ));
        assert_eq!(
            detail.timeline[7],
            TimelineItem::HeadRefForcePushed {
                actor: "ghost".into(),
                at: at("2026-10-31T20:12:00Z"),
            }
        );
        assert_eq!(
            detail.timeline[8],
            TimelineItem::ReadyForReview {
                actor: "mcouto".into(),
                at: at("2026-10-31T20:30:00Z"),
            }
        );
        assert_eq!(
            detail.timeline[13],
            TimelineItem::Merged {
                actor: "mcouto".into(),
                at: at("2026-11-01T09:00:00Z"),
            }
        );
        assert_eq!(
            detail.timeline[14],
            TimelineItem::Closed {
                actor: "mcouto".into(),
                at: at("2026-11-01T09:00:01Z"),
            }
        );
    }

    #[test]
    fn review_item_carries_code_comments_and_outdated_lines() {
        let detail = detail();
        let TimelineItem::Review {
            id,
            author,
            state,
            body,
            submitted_at,
            comments,
        } = &detail.timeline[6]
        else {
            panic!("expected a review, got {:?}", detail.timeline[6]);
        };
        assert_eq!(id, "PRR_kwDOOrbita01");
        assert_eq!(author, "dnogueira");
        assert_eq!(*state, ReviewState::ChangesRequested);
        assert_eq!(body, "A few things to address before merging.");
        assert_eq!(*submitted_at, at("2026-10-30T17:22:45Z"));
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].path, "src/retry/budget.ts");
        assert_eq!(comments[0].line, Some(41));
        assert!(comments[0].diff_hunk.starts_with("@@ -35,6 +35,10 @@"));
        assert_eq!(comments[1].path, "docs/retry.md");
        assert_eq!(comments[1].line, None);
    }

    #[test]
    fn review_without_submission_time_uses_creation_time_and_ghost_author() {
        let detail = detail();
        assert!(matches!(
            &detail.timeline[9],
            TimelineItem::Review { state: ReviewState::Approved, comments, .. } if comments.is_empty()
        ));
        let TimelineItem::Review {
            author,
            state,
            submitted_at,
            ..
        } = &detail.timeline[10]
        else {
            panic!("expected a review, got {:?}", detail.timeline[10]);
        };
        assert_eq!(author, "ghost");
        assert_eq!(*state, ReviewState::Commented);
        assert_eq!(*submitted_at, at("2026-10-31T22:00:00Z"));
        assert!(matches!(
            &detail.timeline[11],
            TimelineItem::Comment { author, .. } if author == "ghost"
        ));
        assert!(matches!(
            &detail.timeline[12],
            TimelineItem::Commit { author, .. } if author == "ghost"
        ));
    }

    #[test]
    fn threads_map_sides_resolution_ranges_and_ghost_authors() {
        let threads = detail().threads;
        assert_eq!(threads.len(), 3);

        let open = &threads[0];
        assert_eq!(open.id, "PRRT_kwDOOrbita01");
        assert!(!open.resolved);
        assert_eq!(open.path, "src/retry/budget.ts");
        assert_eq!((open.line, open.start_line), (Some(41), Some(38)));
        assert_eq!(open.side, Side::Right);
        assert_eq!(open.comments.len(), 2);
        assert_eq!(open.comments[0].author, "dnogueira");
        assert_eq!(open.comments[1].author, "ghost");
        assert_eq!(open.comments[1].created_at, at("2026-10-30T18:02:10Z"));

        let resolved = &threads[1];
        assert!(resolved.resolved);
        assert_eq!(resolved.side, Side::Left);
        assert_eq!((resolved.line, resolved.start_line), (Some(12), None));

        let outdated = &threads[2];
        assert_eq!(outdated.line, None);
        assert_eq!(outdated.side, Side::Right);
    }

    #[test]
    fn commits_map_authors_stats_and_check_states() {
        let commits = detail().commits;
        assert_eq!(commits.len(), 3);

        let first = &commits[0];
        assert_eq!(first.oid, "a1c4e7f0b92d3586714fe0ab39c85d2e61f7b034");
        assert_eq!(first.headline, "Add retry budget type");
        assert_eq!(
            first.body,
            "Tracks attempts per request.\n\nNo behavior change yet."
        );
        assert_eq!(first.author_name, "Marina Couto");
        assert_eq!(first.author_login.as_deref(), Some("mcouto"));
        assert_eq!(first.committed_at, at("2026-10-27T12:40:00Z"));
        assert_eq!((first.additions, first.deletions), (120, 4));
        assert_eq!(first.checks, Some(ChecksState::Success));

        let second = &commits[1];
        assert_eq!(second.author_name, "Marina Couto");
        assert_eq!(second.author_login, None);
        assert_eq!(second.checks, Some(ChecksState::Failure));

        let third = &commits[2];
        assert_eq!(third.author_name, "ghost");
        assert_eq!(third.author_login, None);
        assert_eq!(third.body, "");
        assert_eq!(third.checks, None);
    }

    #[test]
    fn untruncated_timeline_and_missing_body_use_defaults() {
        let json = DETAIL
            .replace("\"hasPreviousPage\": true", "\"hasPreviousPage\": false")
            .replace(
                "\"body\": \"Adds a retry budget so the slot finder stops hammering the scheduling API.\\n\\n- budget per request\\n- jittered backoff\"",
                "\"body\": null",
            );
        let detail = detail_from(&json);
        assert!(!detail.truncated_timeline);
        assert_eq!(detail.body, "");
    }

    #[test]
    fn truncation_flags_come_from_the_page_info_of_each_connection() {
        let detail = detail();
        assert!(detail.truncated_timeline);
        assert!(!detail.truncated_threads);
        let flags: Vec<bool> = detail
            .threads
            .iter()
            .map(|thread| thread.truncated_comments)
            .collect();
        assert_eq!(flags, vec![true, false, false]);

        let json = DETAIL.replace(
            "\"reviewThreads\": {
        \"pageInfo\": { \"hasNextPage\": false }",
            "\"reviewThreads\": {
        \"pageInfo\": { \"hasNextPage\": true }",
        );
        assert!(detail_from(&json).truncated_threads);
    }

    #[test]
    fn nested_field_errors_leave_the_field_empty_and_keep_the_detail() {
        let json = DETAIL
            .replace(
                "\"labels\": {
        \"nodes\": [
          { \"name\": \"needs-review\", \"color\": \"fbca04\" },
          { \"name\": \"backend\", \"color\": \"0e8a16\" }
        ]
      }",
                "\"labels\": null",
            )
            .replace("\"reviewThreads\": {", "\"reviewThreadsGone\": {")
            .replacen("\"data\": {", "\"errors\": [{ \"type\": \"FORBIDDEN\", \"path\": [\"node\", \"labels\"], \"message\": \"saml\" }, { \"type\": \"FORBIDDEN\", \"path\": [\"node\", \"reviewThreads\"], \"message\": \"saml\" }],
  \"data\": {", 1);
        assert!(json.contains("\"labels\": null"));
        let envelope: Envelope<DetailData> = serde_json::from_str(&json).unwrap();
        let parsed = envelope.into_data(Utc::now()).unwrap();
        assert_eq!(parsed.dropped_nodes, 0);
        assert_eq!(
            parsed.partial_paths,
            vec!["node.labels", "node.reviewThreads"]
        );
        let detail = into_pr_detail(parsed.data.node.unwrap(), "github.com");
        assert!(detail.labels.is_empty());
        assert!(detail.threads.is_empty());
        assert_eq!(detail.commits.len(), 3);
        assert_eq!(detail.timeline.len(), 15);
    }

    #[test]
    fn malformed_nodes_inside_a_connection_are_skipped() {
        let json = DETAIL.replace(
            "\"label\": { \"name\": \"needs-review\" },",
            "\"label\": null,",
        );
        assert_ne!(json, DETAIL);
        let detail = detail_from(&json);
        assert_eq!(detail.timeline.len(), 14);
        assert!(
            !detail
                .timeline
                .iter()
                .any(|item| matches!(item, TimelineItem::Labeled { .. }))
        );
    }

    #[test]
    fn null_review_comments_connection_becomes_empty() {
        let json = DETAIL.replace(
            "\"comments\": { \"nodes\": [] }
          },
          {
            \"__typename\": \"PullRequestReview\",
            \"id\": \"PRR_kwDOOrbita03\"",
            "\"comments\": null
          },
          {
            \"__typename\": \"PullRequestReview\",
            \"id\": \"PRR_kwDOOrbita03\"",
        );
        assert_ne!(json, DETAIL);
        let detail = detail_from(&json);
        assert!(matches!(
            &detail.timeline[9],
            TimelineItem::Review { comments, .. } if comments.is_empty()
        ));
    }

    #[test]
    fn errors_on_the_node_itself_or_at_the_root_still_fail() {
        let node_error = r#"{
            "data": { "node": null },
            "errors": [{ "type": "FORBIDDEN", "path": ["node"], "message": "blocked" }]
        }"#;
        let envelope: Envelope<DetailData> = serde_json::from_str(node_error).unwrap();
        assert!(envelope.into_data(Utc::now()).is_err());

        let root_error = DETAIL.replacen(
            "\"data\": {",
            "\"errors\": [{ \"type\": \"INTERNAL\", \"message\": \"boom\" }],
  \"data\": {",
            1,
        );
        let envelope: Envelope<DetailData> = serde_json::from_str(&root_error).unwrap();
        assert!(envelope.into_data(Utc::now()).is_err());
    }

    #[test]
    fn nested_error_with_null_data_fails() {
        let json = r#"{
            "data": null,
            "errors": [{ "type": "FORBIDDEN", "path": ["node", "labels"], "message": "blocked" }]
        }"#;
        let envelope: Envelope<DetailData> = serde_json::from_str(json).unwrap();
        assert!(envelope.into_data(Utc::now()).is_err());
    }

    #[test]
    fn unknown_enum_values_do_not_fail_the_detail() {
        let json = DETAIL
            .replace("\"CHANGES_REQUESTED\"", "\"BRAND_NEW\"")
            .replace("\"FAILURE\"", "\"BRAND_NEW\"")
            .replace("\"RIGHT\"", "\"BRAND_NEW\"");
        let detail = detail_from(&json);
        assert_eq!(detail.threads[0].side, Side::Right);
        assert_eq!(detail.commits[1].checks, None);
        assert!(matches!(
            &detail.timeline[6],
            TimelineItem::Review {
                state: ReviewState::Commented,
                ..
            }
        ));
    }

    #[test]
    fn missing_node_deserializes_to_none() {
        let envelope: Envelope<DetailData> =
            serde_json::from_str(r#"{ "data": { "node": null } }"#).unwrap();
        assert!(envelope.into_data(Utc::now()).unwrap().data.node.is_none());
    }
}
