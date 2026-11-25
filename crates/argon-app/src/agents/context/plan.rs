use argon_core::{PlanComment, PlanContext};
use argon_github::{IssueDetail, TimelineItem};

pub fn plan_context(detail: &IssueDetail, base: &str, base_sha: &str) -> PlanContext {
    let issue = &detail.issue;
    PlanContext {
        issue_title: issue.title.clone(),
        issue_number: issue.number,
        issue_url: issue.url.clone(),
        issue_body: detail.body.clone(),
        repo: issue.repo.clone(),
        labels: issue
            .labels
            .iter()
            .map(|label| label.name.clone())
            .collect(),
        assignees: issue.assignees.clone(),
        milestone: issue.milestone.clone(),
        comments: detail.timeline.iter().filter_map(plan_comment).collect(),
        base: base.to_owned(),
        base_sha: base_sha.to_owned(),
    }
}

fn plan_comment(item: &TimelineItem) -> Option<PlanComment> {
    match item {
        TimelineItem::Comment {
            author,
            body,
            created_at,
            ..
        } => Some(PlanComment {
            author: author.clone(),
            created_at: *created_at,
            body: body.clone(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use argon_core::{IssueLabel, fixtures};
    use chrono::Duration;

    use super::*;

    fn detail(timeline: Vec<TimelineItem>) -> IssueDetail {
        let mut issue = fixtures::issue();
        issue.labels = vec![
            IssueLabel {
                name: "bug".into(),
                color: "d73a4a".into(),
            },
            IssueLabel {
                name: "ui".into(),
                color: "0e8a16".into(),
            },
        ];
        issue.assignees = vec!["ana".into()];
        issue.milestone = Some("v1.2".into());
        IssueDetail {
            issue,
            body: "O slot final some.".into(),
            timeline,
            linked_prs: Vec::new(),
            sub_issues: Vec::new(),
            truncated_timeline: false,
        }
    }

    #[test]
    fn issue_fields_and_base_fill_the_context() {
        let context = plan_context(&detail(Vec::new()), "main", "abc123");
        assert_eq!(context.issue_title, "Slot finder skips the last free slot");
        assert_eq!(context.issue_number, 311);
        assert_eq!(
            context.issue_url,
            "https://github.com/acme/widgets/issues/311"
        );
        assert_eq!(context.issue_body, "O slot final some.");
        assert_eq!(context.repo, "acme/widgets");
        assert_eq!(context.labels, ["bug", "ui"]);
        assert_eq!(context.assignees, ["ana"]);
        assert_eq!(context.milestone.as_deref(), Some("v1.2"));
        assert!(context.comments.is_empty());
        assert_eq!(context.base, "main");
        assert_eq!(context.base_sha, "abc123");
    }

    #[test]
    fn only_timeline_comments_become_plan_comments() {
        let at = fixtures::now();
        let timeline = vec![
            TimelineItem::Comment {
                id: "c1".into(),
                author: "bruno".into(),
                author_is_bot: false,
                body: "Acontece no Windows.".into(),
                created_at: at,
                url: None,
            },
            TimelineItem::Labeled {
                actor: "ana".into(),
                label: "bug".into(),
                at,
            },
            TimelineItem::Comment {
                id: "c2".into(),
                author: "carla".into(),
                author_is_bot: false,
                body: "Confirmo.".into(),
                created_at: at + Duration::minutes(5),
                url: None,
            },
        ];
        let context = plan_context(&detail(timeline), "main", "abc123");
        assert_eq!(
            context.comments,
            [
                PlanComment {
                    author: "bruno".into(),
                    created_at: at,
                    body: "Acontece no Windows.".into(),
                },
                PlanComment {
                    author: "carla".into(),
                    created_at: at + Duration::minutes(5),
                    body: "Confirmo.".into(),
                },
            ]
        );
    }
}
