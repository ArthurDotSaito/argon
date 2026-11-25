use std::collections::HashMap;

use argon_core::{PullRequest, Side, mentions_login};
use chrono::{DateTime, Utc};

use crate::convert::{
    checks_state, into_issue, into_pull_request, issue_state, login_or_ghost, review_state,
    reviewer_label,
};
use crate::detail::{
    CommitInfo, IssueDetail, Label, PrDetail, ReviewComment, ReviewThread, ReviewerStatus,
    SubIssueRow, ThreadComment, TimelineItem,
};
use crate::detail_wire::{
    WireAssignee, WireDetail, WireDetailCommit, WireDiffSide, WireGitActor, WireIssueDetail,
    WireLabel, WireMentionIssue, WireReferenced, WireThread, WireTimelineItem,
};
use crate::wire::Actor;

const BOT_TYPENAME: &str = "Bot";

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
        base_oid: wire.base_ref_oid.filter(|oid| !oid.is_empty()),
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

pub(crate) fn into_issue_detail(wire: WireIssueDetail, host: &str) -> IssueDetail {
    let truncated_timeline = wire.timeline_items.has_previous_page();
    IssueDetail {
        issue: into_issue(wire.issue, host),
        body: wire.body.unwrap_or_default(),
        timeline: wire
            .timeline_items
            .into_items()
            .into_iter()
            .filter_map(timeline_item)
            .collect(),
        linked_prs: wire
            .linked_pull_requests
            .into_items()
            .into_iter()
            .map(|pr| into_pull_request(pr, host))
            .collect(),
        sub_issues: wire
            .sub_issues
            .into_items()
            .into_iter()
            .map(|sub| SubIssueRow {
                repo: sub.repository.name_with_owner,
                number: sub.number,
                title: sub.title,
                state: issue_state(sub.state),
            })
            .collect(),
        truncated_timeline,
    }
}

pub(crate) fn latest_mention_times(
    issues: impl IntoIterator<Item = WireMentionIssue>,
    login: &str,
    since: &HashMap<String, DateTime<Utc>>,
) -> HashMap<String, DateTime<Utc>> {
    issues
        .into_iter()
        .filter_map(|issue| {
            let id = issue.id.clone();
            let limit = since.get(&id).copied();
            latest_mention_time(issue, login, limit).map(|at| (id, at))
        })
        .collect()
}

fn latest_mention_time(
    issue: WireMentionIssue,
    login: &str,
    since: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    let counts = |author: &Option<Actor>, body: &str, at: DateTime<Utc>| {
        since.is_none_or(|limit| at > limit)
            && !author
                .as_ref()
                .is_some_and(|actor| actor.login.eq_ignore_ascii_case(login))
            && mentions_login(body, login)
    };
    let from_comments = issue
        .timeline_items
        .into_items()
        .into_iter()
        .filter(|comment| counts(&comment.author, &comment.body, comment.created_at))
        .map(|comment| comment.created_at);
    let from_body = counts(
        &issue.author,
        issue.body.as_deref().unwrap_or_default(),
        issue.created_at,
    )
    .then_some(issue.created_at);
    from_comments.chain(from_body).max()
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

fn actor_is_bot(actor: &Option<Actor>) -> bool {
    actor
        .as_ref()
        .is_some_and(|actor| actor.typename.as_deref() == Some(BOT_TYPENAME))
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

fn diff_side(side: WireDiffSide) -> Side {
    match side {
        WireDiffSide::Left => Side::Left,
        WireDiffSide::Right | WireDiffSide::Unknown => Side::Right,
    }
}

fn thread(wire: WireThread) -> ReviewThread {
    ReviewThread {
        id: wire.id,
        resolved: wire.is_resolved,
        outdated: wire.is_outdated,
        path: wire.path,
        line: wire.line,
        start_line: wire.start_line,
        original_line: wire.original_line,
        original_start_line: wire.original_start_line,
        truncated_comments: wire.comments.has_next_page(),
        side: diff_side(wire.diff_side),
        start_side: wire.start_diff_side.map(diff_side),
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

fn assignee_login(assignee: Option<WireAssignee>) -> String {
    login_or_ghost(assignee.and_then(|assignee| assignee.login))
}

fn cross_reference(
    actor: Option<Actor>,
    referenced: WireReferenced,
    at: DateTime<Utc>,
) -> Option<TimelineItem> {
    let (item, is_pull_request) = match referenced {
        WireReferenced::Issue(item) => (item, false),
        WireReferenced::PullRequest(item) => (item, true),
        WireReferenced::Unknown => return None,
    };
    Some(TimelineItem::CrossReferenced {
        actor: actor_login(actor),
        repo: item.repository.name_with_owner,
        number: item.number,
        is_pull_request,
        at,
    })
}

fn timeline_item(wire: WireTimelineItem) -> Option<TimelineItem> {
    let item = match wire {
        WireTimelineItem::IssueComment(comment) => TimelineItem::Comment {
            id: comment.id,
            author_is_bot: actor_is_bot(&comment.author),
            author: actor_login(comment.author),
            body: comment.body,
            created_at: comment.created_at,
            url: comment.url,
        },
        WireTimelineItem::PullRequestReview(review) => TimelineItem::Review {
            id: review.id,
            author_is_bot: actor_is_bot(&review.author),
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
        WireTimelineItem::AssignedEvent(event) => TimelineItem::Assigned {
            actor: actor_login(event.actor),
            assignee: assignee_login(event.assignee),
            at: event.created_at,
        },
        WireTimelineItem::UnassignedEvent(event) => TimelineItem::Unassigned {
            actor: actor_login(event.actor),
            assignee: assignee_login(event.assignee),
            at: event.created_at,
        },
        WireTimelineItem::UnlabeledEvent(event) => TimelineItem::Unlabeled {
            actor: actor_login(event.actor),
            label: event.label.name,
            at: event.created_at,
        },
        WireTimelineItem::ReopenedEvent(event) => TimelineItem::Reopened {
            actor: actor_login(event.actor),
            at: event.created_at,
        },
        WireTimelineItem::CrossReferencedEvent(event) => {
            return cross_reference(event.actor, event.source, event.created_at);
        }
        WireTimelineItem::ConnectedEvent(event) => {
            return cross_reference(event.actor, event.subject, event.created_at);
        }
        WireTimelineItem::MilestonedEvent(event) => TimelineItem::Milestoned {
            actor: actor_login(event.actor),
            milestone: event.milestone_title,
            at: event.created_at,
        },
        WireTimelineItem::Unknown => return None,
    };
    Some(item)
}

#[cfg(test)]
mod tests {
    use argon_core::{ChecksState, IssueState, PrId, PrState, ReviewState};
    use chrono::{DateTime, Utc};

    use super::*;
    use crate::detail_wire::{DetailData, IssueDetailData, WireIssueNode, WireMentionsData};
    use crate::wire::Envelope;

    const DETAIL: &str = include_str!("../tests/fixtures/detail.json");

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }

    fn comment_by(author: &str) -> TimelineItem {
        let json = format!(
            r#"{{"__typename":"IssueComment","id":"c","author":{author},"body":"x","createdAt":"2026-10-27T13:10:00Z"}}"#
        );
        timeline_item(serde_json::from_str(&json).unwrap()).unwrap()
    }

    fn review_by(author: &str) -> TimelineItem {
        let json = format!(
            r#"{{"__typename":"PullRequestReview","id":"r","author":{author},"state":"COMMENTED","body":"x","submittedAt":null,"createdAt":"2026-10-27T13:10:00Z"}}"#
        );
        timeline_item(serde_json::from_str(&json).unwrap()).unwrap()
    }

    #[test]
    fn only_bot_authors_are_flagged_as_bots() {
        let bot = r#"{"__typename":"Bot","login":"pullfrog"}"#;
        let user = r#"{"__typename":"User","login":"mcouto"}"#;
        let untyped = r#"{"login":"mcouto"}"#;
        let flagged = |item: TimelineItem| match item {
            TimelineItem::Comment { author_is_bot, .. }
            | TimelineItem::Review { author_is_bot, .. } => author_is_bot,
            other => panic!("unexpected item {other:?}"),
        };
        assert!(flagged(comment_by(bot)));
        assert!(flagged(review_by(bot)));
        for author in [user, untyped, "null"] {
            assert!(!flagged(comment_by(author)), "comment by {author}");
            assert!(!flagged(review_by(author)), "review by {author}");
        }
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
        assert_eq!(
            detail.base_oid.as_deref(),
            Some("9b1e44c07a2f5d3e8c6b0a1d2e3f4a5b6c7d8e9f")
        );
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
                author_is_bot: false,
                body: "Ready for a first pass.".into(),
                created_at: at("2026-10-27T13:10:00Z"),
                url: None,
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
            ..
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

        assert!(!open.outdated);
        assert!(outdated.outdated);
    }

    #[test]
    fn threads_carry_start_side_and_original_anchors() {
        let threads = detail().threads;
        assert_eq!(threads[0].start_side, Some(Side::Left));
        assert_eq!(threads[0].side, Side::Right);
        assert_eq!(
            (threads[0].original_line, threads[0].original_start_line),
            (Some(41), Some(38))
        );
        assert_eq!(threads[1].start_side, None);
        assert_eq!(
            (threads[1].original_line, threads[1].original_start_line),
            (Some(12), None)
        );
        let outdated = &threads[2];
        assert_eq!(outdated.line, None);
        assert_eq!(
            (outdated.original_line, outdated.original_start_line),
            (Some(7), Some(5))
        );
        assert_eq!(outdated.start_side, Some(Side::Right));
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

    const ISSUE_DETAIL: &str = include_str!("../tests/fixtures/issue_detail.json");
    const ISSUE_MENTIONS: &str = include_str!("../tests/fixtures/issue_mentions.json");
    const LOGIN: &str = "arthur-saito";

    fn issue_detail_from(json: &str) -> IssueDetail {
        let envelope: Envelope<IssueDetailData> = serde_json::from_str(json).unwrap();
        match envelope.into_data(Utc::now()).unwrap().data.node.unwrap() {
            WireIssueNode::Issue(wire) => into_issue_detail(*wire, "github.com"),
            WireIssueNode::Other => panic!("expected an issue node"),
        }
    }

    fn issue_detail() -> IssueDetail {
        issue_detail_from(ISSUE_DETAIL)
    }

    #[test]
    fn issue_part_reuses_the_inbox_conversion() {
        let detail = issue_detail();
        assert_eq!(detail.issue.id, PrId("I_kwDOOrbita0101".into()));
        assert_eq!(detail.issue.host, "github.com");
        assert_eq!(detail.issue.repo, "orbita-health/atlas");
        assert_eq!(detail.issue.number, 101);
        assert_eq!(detail.issue.state, IssueState::Open);
        assert_eq!(
            detail.issue.state_reason,
            Some(argon_core::IssueStateReason::Reopened)
        );
        assert_eq!(detail.issue.issue_type.as_deref(), Some("Bug"));
        assert_eq!(detail.issue.milestone.as_deref(), Some("Q4 stability"));
        assert_eq!(detail.issue.assignees, vec!["lfarias", "dnogueira"]);
        assert_eq!(detail.issue.labels.len(), 2);
        assert_eq!(detail.issue.comments, 3);
        assert_eq!(detail.issue.sub_issues.map(|sub| sub.total), Some(3));
        assert_eq!(detail.issue.linked_prs.len(), 1);
        assert!(
            detail
                .body
                .starts_with("Large clinics hit the 30s timeout.")
        );
        assert!(detail.body.contains("- seen on prod"));
        assert!(detail.truncated_timeline);
    }

    #[test]
    fn issue_timeline_maps_every_item_type_and_skips_unknown_and_null_nodes() {
        let timeline = issue_detail().timeline;
        assert_eq!(timeline.len(), 13);

        assert_eq!(
            timeline[0],
            TimelineItem::Comment {
                id: "IC_kwDOOrbita11".into(),
                author: "lfarias".into(),
                author_is_bot: false,
                body: "I can take a look. cc @mcouto".into(),
                created_at: at("2026-10-20T09:00:00Z"),
                url: Some(
                    "https://github.com/orbita-health/atlas/issues/101#issuecomment-11".into()
                ),
            }
        );
        assert_eq!(
            timeline[1],
            TimelineItem::Assigned {
                actor: "mcouto".into(),
                assignee: "lfarias".into(),
                at: at("2026-10-20T09:05:00Z"),
            }
        );
        assert_eq!(
            timeline[2],
            TimelineItem::Assigned {
                actor: "ghost".into(),
                assignee: "ghost".into(),
                at: at("2026-10-20T09:06:00Z"),
            }
        );
        assert_eq!(
            timeline[3],
            TimelineItem::Unassigned {
                actor: "lfarias".into(),
                assignee: "pbarros".into(),
                at: at("2026-10-21T10:00:00Z"),
            }
        );
        assert_eq!(
            timeline[4],
            TimelineItem::Labeled {
                actor: "mcouto".into(),
                label: "bug".into(),
                at: at("2026-10-21T10:05:00Z"),
            }
        );
        assert_eq!(
            timeline[5],
            TimelineItem::Unlabeled {
                actor: "mcouto".into(),
                label: "triage".into(),
                at: at("2026-10-21T10:06:00Z"),
            }
        );
        assert_eq!(
            timeline[6],
            TimelineItem::Milestoned {
                actor: "dnogueira".into(),
                milestone: "Q4 stability".into(),
                at: at("2026-10-22T11:00:00Z"),
            }
        );
        assert_eq!(
            timeline[7],
            TimelineItem::CrossReferenced {
                actor: "mcouto".into(),
                repo: "orbita-health/atlas".into(),
                number: 412,
                is_pull_request: true,
                at: at("2026-10-27T13:05:12Z"),
            }
        );
        assert_eq!(
            timeline[8],
            TimelineItem::CrossReferenced {
                actor: "dnogueira".into(),
                repo: "orbita-health/billing".into(),
                number: 87,
                is_pull_request: false,
                at: at("2026-10-28T08:00:00Z"),
            }
        );
        assert_eq!(
            timeline[9],
            TimelineItem::CrossReferenced {
                actor: "lfarias".into(),
                repo: "orbita-health/atlas".into(),
                number: 430,
                is_pull_request: true,
                at: at("2026-10-29T08:00:00Z"),
            }
        );
        assert_eq!(
            timeline[10],
            TimelineItem::Closed {
                actor: "lfarias".into(),
                at: at("2026-10-30T09:00:00Z"),
            }
        );
        assert_eq!(
            timeline[11],
            TimelineItem::Reopened {
                actor: "mcouto".into(),
                at: at("2026-10-30T09:30:00Z"),
            }
        );
        assert!(matches!(
            &timeline[12],
            TimelineItem::Comment { author, .. } if author == "ghost"
        ));
    }

    #[test]
    fn issue_linked_prs_are_hydrated_with_the_pull_request_fields() {
        let linked = issue_detail().linked_prs;
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].id, PrId("PR_kwDOOrbita0412".into()));
        assert_eq!(linked[0].number, 412);
        assert_eq!(linked[0].checks, Some(ChecksState::Failure));
        assert_eq!(linked[0].host, "github.com");
        assert_eq!(linked[0].assignees, vec!["mcouto"]);
        assert_eq!(linked[0].closing_issues.len(), 1);
    }

    #[test]
    fn issue_sub_issues_keep_repo_title_and_state() {
        let sub_issues = issue_detail().sub_issues;
        assert_eq!(
            sub_issues,
            vec![
                SubIssueRow {
                    repo: "orbita-health/atlas".into(),
                    number: 102,
                    title: "Cap concurrent slot lookups".into(),
                    state: IssueState::Closed,
                },
                SubIssueRow {
                    repo: "orbita-health/atlas".into(),
                    number: 103,
                    title: "Surface timeout in the clinic dashboard".into(),
                    state: IssueState::Open,
                },
                SubIssueRow {
                    repo: "orbita-health/observability".into(),
                    number: 12,
                    title: "Expose retry metrics".into(),
                    state: IssueState::Open,
                },
            ]
        );
    }

    #[test]
    fn issue_detail_defaults_when_connections_and_body_are_missing() {
        let json = ISSUE_DETAIL
            .replace("\"hasPreviousPage\": true", "\"hasPreviousPage\": false")
            .replace(
                "\"body\": \"Large clinics hit the 30s timeout.\\n\\n- repro: 500 slots\\n- seen on prod\"",
                "\"body\": null",
            )
            .replace("\"subIssues\": {", "\"subIssuesGone\": {");
        assert_ne!(json, ISSUE_DETAIL);
        let detail = issue_detail_from(&json);
        assert!(!detail.truncated_timeline);
        assert_eq!(detail.body, "");
        assert!(detail.sub_issues.is_empty());
    }

    #[test]
    fn issue_node_of_another_type_is_not_an_issue() {
        let envelope: Envelope<IssueDetailData> =
            serde_json::from_str(r#"{ "data": { "node": { "__typename": "PullRequest" } } }"#)
                .unwrap();
        let node = envelope.into_data(Utc::now()).unwrap().data.node.unwrap();
        assert!(matches!(node, WireIssueNode::Other));
    }

    #[test]
    fn missing_issue_node_deserializes_to_none() {
        let envelope: Envelope<IssueDetailData> =
            serde_json::from_str(r#"{ "data": { "node": null } }"#).unwrap();
        assert!(envelope.into_data(Utc::now()).unwrap().data.node.is_none());
    }

    fn mentions(since: &[(&str, &str)]) -> HashMap<String, DateTime<Utc>> {
        let envelope: Envelope<WireMentionsData> = serde_json::from_str(ISSUE_MENTIONS).unwrap();
        let issues = envelope.into_data(Utc::now()).unwrap().data.into_issues();
        let since = since
            .iter()
            .map(|(id, value)| (id.to_string(), at(value)))
            .collect();
        latest_mention_times(issues, LOGIN, &since)
    }

    #[test]
    fn mentions_by_someone_else_after_since_count() {
        let found = mentions(&[("I_kwDOOrbita0101", "2026-11-02T00:00:00Z")]);
        assert_eq!(
            found.get("I_kwDOOrbita0101"),
            Some(&at("2026-11-02T09:00:00Z"))
        );
    }

    #[test]
    fn own_comments_old_mentions_and_non_mentions_do_not_count() {
        let found = mentions(&[
            ("I_kwDOOrbita0101", "2026-11-02T00:00:00Z"),
            ("I_kwDOOrbita0087", "2026-10-12T00:00:00Z"),
        ]);
        assert!(!found.contains_key("I_kwDOOrbita0087"));
        assert!(!found.contains_key("I_kwDOOrbita0130"));
        assert!(!found.contains_key("I_kwDOOrbita0064"));
    }

    #[test]
    fn latest_of_several_mentions_wins_over_later_own_and_plain_comments() {
        let found = mentions(&[("I_kwDOOrbita0101", "2026-10-01T00:00:00Z")]);
        assert_eq!(
            found.get("I_kwDOOrbita0101"),
            Some(&at("2026-11-02T09:00:00Z"))
        );
    }

    #[test]
    fn issue_body_created_after_since_counts_and_missing_since_means_no_limit() {
        let found = mentions(&[("I_kwDOOrbita0087", "2026-10-05T00:00:00Z")]);
        assert_eq!(
            found.get("I_kwDOOrbita0087"),
            Some(&at("2026-10-11T09:00:00Z"))
        );
        assert_eq!(
            found.get("I_kwDOOrbita0092"),
            Some(&at("2026-11-02T07:30:00Z"))
        );
    }

    #[test]
    fn issue_body_alone_is_enough_when_it_is_newer_than_since() {
        let found = mentions(&[("I_kwDOOrbita0092", "2026-11-01T00:00:00Z")]);
        assert_eq!(
            found.get("I_kwDOOrbita0092"),
            Some(&at("2026-11-02T07:30:00Z"))
        );
        let found = mentions(&[("I_kwDOOrbita0092", "2026-11-03T00:00:00Z")]);
        assert!(!found.contains_key("I_kwDOOrbita0092"));
    }

    #[test]
    fn malformed_and_empty_nodes_are_skipped_in_the_mentions_page() {
        let envelope: Envelope<WireMentionsData> = serde_json::from_str(ISSUE_MENTIONS).unwrap();
        let ids: Vec<String> = envelope
            .into_data(Utc::now())
            .unwrap()
            .data
            .into_issues()
            .into_iter()
            .map(|issue| issue.id)
            .collect();
        assert_eq!(
            ids,
            vec![
                "I_kwDOOrbita0101",
                "I_kwDOOrbita0087",
                "I_kwDOOrbita0130",
                "I_kwDOOrbita0092",
                "I_kwDOOrbita0064",
            ]
        );
    }
}
