use std::collections::HashSet;

use argon_core::{Issue, PrState};
use argon_github::{IssueDetail, TimelineItem};
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::{AnyElement, App, Div, Hsla, IntoElement, ParentElement as _, Styled as _, div};

use super::header::{IssueStatus, reference_chip, status_style};
use super::labels;
use crate::inbox::InboxModel;
use crate::pr::{self, MarkdownBlock, card, card_body, card_text, event, labels as pr_labels};
use crate::ui;

#[derive(Clone, Copy)]
enum SourceState {
    Pr { state: PrState, is_draft: bool },
    Issue(IssueStatus),
}

struct Reference {
    linked: bool,
    source: Option<SourceState>,
}

struct Entry {
    ix: usize,
    body: Option<MarkdownBlock>,
    reference: Option<Reference>,
}

impl Entry {
    fn new(ix: usize, detail: &IssueDetail, inbox: &InboxModel) -> Self {
        let item = &detail.timeline[ix];
        let body = match item {
            TimelineItem::Comment { id, body, .. } => {
                Some(MarkdownBlock::new(format!("issue-comment-{id}"), body))
            }
            _ => None,
        };
        let reference = match item {
            TimelineItem::CrossReferenced {
                repo,
                number,
                is_pull_request,
                ..
            } => Some(Reference {
                linked: *is_pull_request
                    && detail.issue.linked_prs.iter().any(|linked| {
                        linked.number == *number && linked.repo.eq_ignore_ascii_case(repo)
                    }),
                source: source_state(repo, *number, *is_pull_request, detail, inbox),
            }),
            _ => None,
        };
        Self {
            ix,
            body,
            reference,
        }
    }
}

#[derive(Default)]
pub struct Timeline {
    description: Option<MarkdownBlock>,
    entries: Vec<Entry>,
}

impl Timeline {
    pub fn new(detail: &IssueDetail, inbox: &InboxModel) -> Self {
        let description = (!detail.body.trim().is_empty())
            .then(|| MarkdownBlock::new("issue-description".to_string(), &detail.body));
        let entries = visible_order(&detail.timeline)
            .into_iter()
            .map(|ix| Entry::new(ix, detail, inbox))
            .collect();
        Self {
            description,
            entries,
        }
    }

    pub fn description(&self, issue: &Issue, now: DateTime<Utc>, cx: &App) -> Div {
        let theme = cx.theme();
        let body = match &self.description {
            Some(description) => description.view().into_any_element(),
            None => div()
                .italic()
                .text_color(theme.muted_foreground)
                .child(pr_labels::NO_DESCRIPTION)
                .into_any_element(),
        };
        card(
            &issue.author,
            card_body(
                &issue.author,
                div().child(labels::OPENED_THIS_ISSUE),
                issue.created_at,
                now,
                cx,
            )
            .child(card_text(body)),
        )
    }

    pub fn items(&self, detail: &IssueDetail, now: DateTime<Utc>, cx: &App) -> Div {
        v_flex()
            .w_full()
            .gap_3()
            .children(self.entries.iter().filter_map(|entry| {
                let item = detail.timeline.get(entry.ix)?;
                timeline_item(item, entry, &detail.issue, now, cx)
            }))
    }
}

fn visible_order(timeline: &[TimelineItem]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..timeline.len()).collect();
    order.sort_by_key(|ix| pr::timestamp(&timeline[*ix]));
    let mut sources = HashSet::new();
    order.retain(|ix| match &timeline[*ix] {
        TimelineItem::CrossReferenced { repo, number, .. } => {
            sources.insert((repo.to_lowercase(), *number))
        }
        _ => true,
    });
    order
}

fn source_state(
    repo: &str,
    number: u64,
    is_pull_request: bool,
    detail: &IssueDetail,
    inbox: &InboxModel,
) -> Option<SourceState> {
    let host = detail.issue.host.as_str();
    let same = |item_host: &str, item_repo: &str, item_number: u64| {
        item_number == number
            && item_repo.eq_ignore_ascii_case(repo)
            && item_host.eq_ignore_ascii_case(host)
    };
    let subjects = inbox.items().iter().map(|item| &item.subject);
    if is_pull_request {
        return detail
            .linked_prs
            .iter()
            .chain(subjects.filter_map(|subject| subject.as_pr()))
            .find(|pr| same(&pr.host, &pr.repo, pr.number))
            .map(|pr| SourceState::Pr {
                state: pr.state,
                is_draft: pr.is_draft,
            });
    }
    detail
        .sub_issues
        .iter()
        .find(|sub| same(host, &sub.repo, sub.number))
        .map(|sub| SourceState::Issue(IssueStatus::of(sub.state, None)))
        .or_else(|| {
            subjects
                .filter_map(|subject| subject.as_issue())
                .find(|issue| same(&issue.host, &issue.repo, issue.number))
                .map(|issue| SourceState::Issue(IssueStatus::of(issue.state, issue.state_reason)))
        })
}

fn source_style(source: Option<SourceState>, is_pull_request: bool, cx: &App) -> (IconName, Hsla) {
    match source {
        Some(SourceState::Pr { state, is_draft }) => {
            let style = ui::state_style_for(state, is_draft, cx);
            (style.icon, style.color)
        }
        Some(SourceState::Issue(status)) => status_style(status, cx),
        None if is_pull_request => (IconName::GitPullRequest, cx.theme().muted_foreground),
        None => (IconName::CircleDot, cx.theme().muted_foreground),
    }
}

fn label_change(text: String, label: &str, issue: &Issue, cx: &App) -> Div {
    let color = issue
        .labels
        .iter()
        .find(|known| known.name.eq_ignore_ascii_case(label))
        .map(|known| known.color.as_str());
    h_flex()
        .gap_1p5()
        .items_center()
        .child(text)
        .child(pr::label_chip(label, color, cx))
}

fn timeline_item(
    item: &TimelineItem,
    entry: &Entry,
    issue: &Issue,
    now: DateTime<Utc>,
    cx: &App,
) -> Option<AnyElement> {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let element = match item {
        TimelineItem::Comment {
            author, created_at, ..
        } => card(
            author,
            card_body(
                author,
                div().child(pr_labels::COMMENTED),
                *created_at,
                now,
                cx,
            )
            .children(entry.body.as_ref().map(|body| card_text(body.view()))),
        )
        .into_any_element(),
        TimelineItem::Labeled { actor, label, at } => event(
            IconName::Tag,
            muted,
            label_change(labels::labeled(actor), label, issue, cx),
            *at,
            now,
            cx,
        ),
        TimelineItem::Unlabeled { actor, label, at } => event(
            IconName::Tag,
            muted,
            label_change(labels::unlabeled(actor), label, issue, cx),
            *at,
            now,
            cx,
        ),
        TimelineItem::Assigned {
            actor,
            assignee,
            at,
        } => event(
            IconName::UserPlus,
            muted,
            div().child(labels::assigned(actor, assignee)),
            *at,
            now,
            cx,
        ),
        TimelineItem::Unassigned {
            actor,
            assignee,
            at,
        } => event(
            IconName::UserMinus,
            muted,
            div().child(labels::unassigned(actor, assignee)),
            *at,
            now,
            cx,
        ),
        TimelineItem::Closed { actor, at } => event(
            IconName::CircleCheck,
            theme.magenta,
            div().child(labels::closed(actor)),
            *at,
            now,
            cx,
        ),
        TimelineItem::Reopened { actor, at } => event(
            IconName::CircleDot,
            theme.success,
            div().child(labels::reopened(actor)),
            *at,
            now,
            cx,
        ),
        TimelineItem::Milestoned {
            actor,
            milestone,
            at,
        } => event(
            IconName::Milestone,
            muted,
            div().child(labels::milestoned(actor, milestone)),
            *at,
            now,
            cx,
        ),
        TimelineItem::CrossReferenced {
            actor,
            repo,
            number,
            is_pull_request,
            at,
        } => {
            let reference = entry.reference.as_ref()?;
            let (icon, color) = source_style(reference.source, *is_pull_request, cx);
            let chip = reference_chip(
                icon,
                color,
                labels::reference(&issue.repo, repo, *number),
                cx,
            );
            event(
                IconName::Link,
                muted,
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(labels::cross_referenced(actor, reference.linked))
                    .child(chip),
                *at,
                now,
                cx,
            )
        }
        TimelineItem::Review { .. }
        | TimelineItem::Commit { .. }
        | TimelineItem::ReviewRequested { .. }
        | TimelineItem::HeadRefForcePushed { .. }
        | TimelineItem::Merged { .. }
        | TimelineItem::ReadyForReview { .. } => return None,
    };
    Some(element)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 1, 10, minute, 0).unwrap()
    }

    fn cross_ref(repo: &str, number: u64, minute: u32) -> TimelineItem {
        TimelineItem::CrossReferenced {
            actor: "ana".into(),
            repo: repo.into(),
            number,
            is_pull_request: true,
            at: at(minute),
        }
    }

    fn reopened(minute: u32) -> TimelineItem {
        TimelineItem::Reopened {
            actor: "ana".into(),
            at: at(minute),
        }
    }

    #[test]
    fn items_follow_their_timestamps() {
        let timeline = vec![reopened(30), reopened(10), reopened(20)];
        assert_eq!(visible_order(&timeline), vec![1, 2, 0]);
    }

    #[test]
    fn a_source_referenced_twice_shows_once_at_its_first_reference() {
        let timeline = vec![
            cross_ref("acme/api", 7, 20),
            cross_ref("acme/api", 7, 5),
            cross_ref("Acme/API", 7, 40),
        ];
        assert_eq!(visible_order(&timeline), vec![1]);
    }

    #[test]
    fn different_sources_are_kept() {
        let timeline = vec![
            cross_ref("acme/api", 7, 1),
            cross_ref("acme/web", 7, 2),
            cross_ref("acme/api", 8, 3),
            reopened(4),
        ];
        assert_eq!(visible_order(&timeline), vec![0, 1, 2, 3]);
    }
}
