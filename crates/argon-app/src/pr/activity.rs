use std::collections::HashSet;

use argon_core::ReviewState;
use argon_github::{PrDetail, ReviewThread, TimelineItem};
use chrono::{DateTime, NaiveDate, Utc};

use super::timeline_item::timestamp;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ActivityFilter {
    #[default]
    Conversation,
    All,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ActivityOrder {
    #[default]
    NewestFirst,
    OldestFirst,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ActivityKind {
    Comment { ix: usize },
    Review { ix: usize },
    Thread { ix: usize },
    Commits { ixs: Vec<usize> },
    Event { ix: usize },
    Opened,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ActivityEntry {
    pub kind: ActivityKind,
    pub at: DateTime<Utc>,
    pub talk: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ActivityDay {
    pub date: NaiveDate,
    pub entries: Vec<ActivityEntry>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Hidden {
    pub commits: usize,
    pub bot: usize,
    pub events: usize,
}

impl Hidden {
    pub fn total(&self) -> usize {
        self.commits + self.bot + self.events
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Activity {
    pub days: Vec<ActivityDay>,
    pub hidden: Hidden,
    pub talk_count: usize,
    pub total_count: usize,
}

pub fn build(
    detail: &PrDetail,
    filter: ActivityFilter,
    order: ActivityOrder,
    local_day: impl Fn(DateTime<Utc>) -> NaiveDate,
) -> Activity {
    let bots = bot_logins(detail);
    let mut entries = vec![ActivityEntry {
        kind: ActivityKind::Opened,
        at: detail.pr.created_at,
        talk: false,
    }];
    entries.extend(
        detail
            .timeline
            .iter()
            .enumerate()
            .filter_map(|(ix, item)| timeline_entry(ix, item)),
    );
    entries.extend(
        detail
            .threads
            .iter()
            .enumerate()
            .filter_map(|(ix, thread)| thread_entry(ix, thread, &bots)),
    );
    entries.sort_by_key(|entry| entry.at);
    let entries = group_commits(entries, &local_day);

    let talk_count = entries.iter().filter(|entry| entry.talk).count();
    let total_count = entries.iter().map(weight).sum();
    let mut hidden = Hidden::default();
    let mut days: Vec<ActivityDay> = Vec::new();
    for entry in entries {
        if filter == ActivityFilter::Conversation && !entry.talk {
            hide(&mut hidden, &entry);
            continue;
        }
        let date = local_day(entry.at);
        match days.last_mut() {
            Some(day) if day.date == date => day.entries.push(entry),
            _ => days.push(ActivityDay {
                date,
                entries: vec![entry],
            }),
        }
    }
    if order == ActivityOrder::NewestFirst {
        days.reverse();
        for day in &mut days {
            day.entries.reverse();
        }
    }
    Activity {
        days,
        hidden,
        talk_count,
        total_count,
    }
}

fn bot_logins(detail: &PrDetail) -> HashSet<String> {
    detail
        .timeline
        .iter()
        .filter_map(|item| match item {
            TimelineItem::Comment {
                author,
                author_is_bot: true,
                ..
            }
            | TimelineItem::Review {
                author,
                author_is_bot: true,
                ..
            } => Some(author.to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

fn timeline_entry(ix: usize, item: &TimelineItem) -> Option<ActivityEntry> {
    let (kind, talk) = match item {
        TimelineItem::Comment { author_is_bot, .. } => {
            (ActivityKind::Comment { ix }, !author_is_bot)
        }
        TimelineItem::Review {
            state,
            body,
            author_is_bot,
            ..
        } => {
            let silent = matches!(state, ReviewState::Commented | ReviewState::Pending)
                && body.trim().is_empty();
            if silent {
                return None;
            }
            (ActivityKind::Review { ix }, !author_is_bot)
        }
        TimelineItem::Commit { .. } => (ActivityKind::Commits { ixs: vec![ix] }, false),
        TimelineItem::Merged { .. } | TimelineItem::Closed { .. } => {
            (ActivityKind::Event { ix }, true)
        }
        TimelineItem::ReviewRequested { .. }
        | TimelineItem::Labeled { .. }
        | TimelineItem::HeadRefForcePushed { .. }
        | TimelineItem::ReadyForReview { .. } => (ActivityKind::Event { ix }, false),
        TimelineItem::Assigned { .. }
        | TimelineItem::Unassigned { .. }
        | TimelineItem::Unlabeled { .. }
        | TimelineItem::Reopened { .. }
        | TimelineItem::CrossReferenced { .. }
        | TimelineItem::Milestoned { .. } => return None,
    };
    Some(ActivityEntry {
        kind,
        at: timestamp(item),
        talk,
    })
}

fn thread_entry(ix: usize, thread: &ReviewThread, bots: &HashSet<String>) -> Option<ActivityEntry> {
    let at = thread
        .comments
        .iter()
        .map(|comment| comment.created_at)
        .max()?;
    let only_bots = thread
        .comments
        .iter()
        .all(|comment| bots.contains(&comment.author.to_ascii_lowercase()));
    Some(ActivityEntry {
        kind: ActivityKind::Thread { ix },
        at,
        talk: !only_bots,
    })
}

fn group_commits(
    entries: Vec<ActivityEntry>,
    local_day: &impl Fn(DateTime<Utc>) -> NaiveDate,
) -> Vec<ActivityEntry> {
    let mut grouped: Vec<ActivityEntry> = Vec::with_capacity(entries.len());
    for entry in entries {
        if let ActivityKind::Commits { ixs } = &entry.kind
            && let Some(ActivityEntry {
                kind: ActivityKind::Commits { ixs: group },
                at,
                ..
            }) = grouped.last_mut()
            && local_day(*at) == local_day(entry.at)
        {
            group.extend(ixs);
            *at = entry.at;
            continue;
        }
        grouped.push(entry);
    }
    grouped
}

fn weight(entry: &ActivityEntry) -> usize {
    match &entry.kind {
        ActivityKind::Commits { ixs } => ixs.len(),
        _ => 1,
    }
}

fn hide(hidden: &mut Hidden, entry: &ActivityEntry) {
    match &entry.kind {
        ActivityKind::Commits { ixs } => hidden.commits += ixs.len(),
        ActivityKind::Comment { .. }
        | ActivityKind::Review { .. }
        | ActivityKind::Thread { .. } => hidden.bot += 1,
        ActivityKind::Event { .. } | ActivityKind::Opened => hidden.events += 1,
    }
}

#[cfg(test)]
mod tests {
    use argon_core::{Side, fixtures};
    use argon_github::ThreadComment;
    use chrono::Duration;

    use super::*;

    fn base() -> DateTime<Utc> {
        "2026-11-17T12:00:00Z".parse().unwrap()
    }

    fn at(hours: i64) -> DateTime<Utc> {
        base() + Duration::hours(hours)
    }

    fn utc_day(at: DateTime<Utc>) -> NaiveDate {
        at.date_naive()
    }

    fn comment(author: &str, bot: bool, hours: i64) -> TimelineItem {
        TimelineItem::Comment {
            id: format!("c-{author}-{hours}"),
            author: author.into(),
            author_is_bot: bot,
            body: "texto".into(),
            created_at: at(hours),
            url: None,
        }
    }

    fn review(author: &str, bot: bool, state: ReviewState, body: &str, hours: i64) -> TimelineItem {
        TimelineItem::Review {
            id: format!("r-{author}-{hours}"),
            author: author.into(),
            author_is_bot: bot,
            state,
            body: body.into(),
            submitted_at: at(hours),
            comments: Vec::new(),
        }
    }

    fn commit(hours: i64) -> TimelineItem {
        TimelineItem::Commit {
            oid: format!("{hours:040}"),
            headline: "feat: algo".into(),
            author: "Marina".into(),
            committed_at: at(hours),
        }
    }

    fn labeled(hours: i64) -> TimelineItem {
        TimelineItem::Labeled {
            actor: "ci".into(),
            label: "size:XL".into(),
            at: at(hours),
        }
    }

    fn thread(comments: &[(&str, i64)]) -> ReviewThread {
        ReviewThread {
            id: "t".into(),
            resolved: false,
            outdated: false,
            path: "src/cache.rs".into(),
            line: Some(42),
            start_line: None,
            original_line: None,
            original_start_line: None,
            side: Side::Right,
            start_side: None,
            comments: comments
                .iter()
                .map(|(author, hours)| ThreadComment {
                    id: format!("tc-{author}-{hours}"),
                    author: (*author).into(),
                    body: "texto".into(),
                    created_at: at(*hours),
                })
                .collect(),
            truncated_comments: false,
        }
    }

    fn detail(timeline: Vec<TimelineItem>, threads: Vec<ReviewThread>) -> PrDetail {
        let mut pr = fixtures::pr();
        pr.created_at = base();
        PrDetail {
            pr,
            body: String::new(),
            head_oid: "abc".into(),
            base_oid: None,
            labels: Vec::new(),
            reviewers: Vec::new(),
            timeline,
            truncated_timeline: false,
            threads,
            truncated_threads: false,
            commits: Vec::new(),
        }
    }

    fn all(detail: &PrDetail, order: ActivityOrder) -> Activity {
        build(detail, ActivityFilter::All, order, utc_day)
    }

    fn kinds(activity: &Activity) -> Vec<ActivityKind> {
        activity
            .days
            .iter()
            .flat_map(|day| day.entries.iter().map(|entry| entry.kind.clone()))
            .collect()
    }

    #[test]
    fn newest_first_puts_the_latest_day_and_entry_on_top() {
        let detail = detail(
            vec![comment("ana", false, 1), comment("vitor", false, 30)],
            Vec::new(),
        );
        let activity = all(&detail, ActivityOrder::NewestFirst);
        assert_eq!(
            kinds(&activity),
            [
                ActivityKind::Comment { ix: 1 },
                ActivityKind::Comment { ix: 0 },
                ActivityKind::Opened,
            ]
        );
        let dates: Vec<NaiveDate> = activity.days.iter().map(|day| day.date).collect();
        assert_eq!(dates, [utc_day(at(30)), utc_day(at(1))]);
    }

    #[test]
    fn oldest_first_is_the_exact_reverse() {
        let detail = detail(
            vec![comment("ana", false, 1), comment("vitor", false, 30)],
            Vec::new(),
        );
        let mut newest = kinds(&all(&detail, ActivityOrder::NewestFirst));
        newest.reverse();
        assert_eq!(kinds(&all(&detail, ActivityOrder::OldestFirst)), newest);
    }

    #[test]
    fn neighbouring_commits_become_one_group() {
        let detail = detail(vec![commit(1), commit(2), commit(3)], Vec::new());
        let activity = all(&detail, ActivityOrder::OldestFirst);
        assert_eq!(
            kinds(&activity),
            [
                ActivityKind::Opened,
                ActivityKind::Commits { ixs: vec![0, 1, 2] },
            ]
        );
        assert_eq!(activity.days[0].entries[1].at, at(3));
        assert_eq!(activity.total_count, 4);
    }

    #[test]
    fn a_comment_between_commits_splits_the_group() {
        let detail = detail(
            vec![commit(1), comment("ana", false, 2), commit(3)],
            Vec::new(),
        );
        assert_eq!(
            kinds(&all(&detail, ActivityOrder::OldestFirst)),
            [
                ActivityKind::Opened,
                ActivityKind::Commits { ixs: vec![0] },
                ActivityKind::Comment { ix: 1 },
                ActivityKind::Commits { ixs: vec![2] },
            ]
        );
    }

    #[test]
    fn commits_on_different_days_stay_in_separate_groups() {
        let detail = detail(vec![commit(1), commit(30)], Vec::new());
        let activity = all(&detail, ActivityOrder::OldestFirst);
        assert_eq!(activity.days.len(), 2);
        assert_eq!(
            activity.days[1].entries,
            [ActivityEntry {
                kind: ActivityKind::Commits { ixs: vec![1] },
                at: at(30),
                talk: false,
            }]
        );
    }

    #[test]
    fn a_thread_sits_where_its_last_message_is() {
        let detail = detail(
            vec![comment("vitor", false, 20)],
            vec![thread(&[("ana", 2), ("marina", 40)])],
        );
        assert_eq!(
            kinds(&all(&detail, ActivityOrder::NewestFirst))[0],
            ActivityKind::Thread { ix: 0 }
        );
    }

    #[test]
    fn conversation_filter_keeps_people_and_counts_what_it_hides() {
        let detail = detail(
            vec![
                commit(1),
                commit(2),
                labeled(3),
                comment("pullfrog", true, 4),
                comment("vitor", false, 5),
                review("ana", false, ReviewState::Approved, "", 6),
            ],
            vec![thread(&[("pullfrog", 4)]), thread(&[("ana", 7)])],
        );
        let activity = build(
            &detail,
            ActivityFilter::Conversation,
            ActivityOrder::OldestFirst,
            utc_day,
        );
        assert_eq!(
            kinds(&activity),
            [
                ActivityKind::Comment { ix: 4 },
                ActivityKind::Review { ix: 5 },
                ActivityKind::Thread { ix: 1 },
            ]
        );
        assert_eq!(
            activity.hidden,
            Hidden {
                commits: 2,
                bot: 2,
                events: 2,
            }
        );
        assert_eq!(activity.talk_count, 3);
        assert_eq!(activity.total_count, 9);
    }

    #[test]
    fn a_comment_only_review_without_a_body_is_left_to_its_threads() {
        let detail = detail(
            vec![
                review("ana", false, ReviewState::Commented, "  ", 1),
                review("ana", false, ReviewState::Pending, "", 2),
                review("ana", false, ReviewState::Commented, "olha isso", 3),
                review("ana", false, ReviewState::ChangesRequested, "", 4),
            ],
            Vec::new(),
        );
        assert_eq!(
            kinds(&all(&detail, ActivityOrder::OldestFirst)),
            [
                ActivityKind::Opened,
                ActivityKind::Review { ix: 2 },
                ActivityKind::Review { ix: 3 },
            ]
        );
    }

    #[test]
    fn merging_and_closing_count_as_conversation() {
        let detail = detail(
            vec![TimelineItem::Merged {
                actor: "marina".into(),
                at: at(5),
            }],
            Vec::new(),
        );
        let activity = build(
            &detail,
            ActivityFilter::Conversation,
            ActivityOrder::NewestFirst,
            utc_day,
        );
        assert_eq!(kinds(&activity), [ActivityKind::Event { ix: 0 }]);
        assert_eq!(activity.hidden.events, 1);
    }

    #[test]
    fn events_the_screen_never_draws_are_not_counted() {
        let detail = detail(
            vec![TimelineItem::Reopened {
                actor: "marina".into(),
                at: at(5),
            }],
            Vec::new(),
        );
        let activity = all(&detail, ActivityOrder::NewestFirst);
        assert_eq!(kinds(&activity), [ActivityKind::Opened]);
        assert_eq!(activity.total_count, 1);
    }
}
