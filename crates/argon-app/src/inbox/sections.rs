use argon_core::{DateBucket, ViewSet, date_bucket};
use chrono::{DateTime, Local, Utc};
use gpui_kit::SharedString;

use super::InboxView;
use super::items::InboxItem;
use super::labels;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InboxRow {
    Header { title: SharedString, count: usize },
    Item { position: usize },
    LoadMore,
    BrowseMore,
}

#[derive(Default)]
pub struct Sections {
    pub rows: Vec<InboxRow>,
    pub visible: Vec<usize>,
    pub row_of: Vec<usize>,
}

impl Sections {
    pub fn build(
        items: &[InboxItem],
        matches: Vec<usize>,
        view: InboxView,
        now: DateTime<Local>,
    ) -> Sections {
        let entries = matches
            .into_iter()
            .map(|ix| (ix, items[ix].views, items[ix].subject.updated_at()));
        let mut sections = Sections::default();
        for (group, indexes) in group(entries, view, now) {
            sections.push(group.title(), &indexes);
        }
        sections
    }

    fn push(&mut self, title: &'static str, group: &[usize]) {
        if group.is_empty() {
            return;
        }
        self.rows.push(InboxRow::Header {
            title: title.into(),
            count: group.len(),
        });
        for ix in group {
            self.row_of.push(self.rows.len());
            self.rows.push(InboxRow::Item {
                position: self.visible.len(),
            });
            self.visible.push(*ix);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    NeedsYou,
    Date(DateBucket),
}

impl Group {
    fn title(self) -> &'static str {
        match self {
            Self::NeedsYou => labels::NEEDS_YOU_SECTION,
            Self::Date(bucket) => labels::bucket_title(bucket),
        }
    }
}

fn group(
    entries: impl IntoIterator<Item = (usize, ViewSet, DateTime<Utc>)>,
    view: InboxView,
    now: DateTime<Local>,
) -> Vec<(Group, Vec<usize>)> {
    let needs_you_first = view == InboxView::Inbox;
    let mut needs_you = Vec::new();
    let mut groups: Vec<(Group, Vec<usize>)> = Vec::new();
    for (ix, views, updated_at) in entries {
        if needs_you_first && views.contains(ViewSet::NEEDS_YOU) {
            needs_you.push(ix);
            continue;
        }
        let bucket = Group::Date(date_bucket(now, updated_at.with_timezone(&Local)));
        match groups.last_mut() {
            Some((last, group)) if *last == bucket => group.push(ix),
            _ => groups.push((bucket, vec![ix])),
        }
    }
    if !needs_you.is_empty() {
        groups.insert(0, (Group::NeedsYou, needs_you));
    }
    groups
}

#[cfg(test)]
mod tests {
    use argon_core::{DEFAULT_HOST, fixtures};

    use super::*;
    use crate::inbox::items::Subject;

    fn pr(id: &str, days_ago: i64) -> Subject {
        Subject::Pr(fixtures::pr_on(
            DEFAULT_HOST,
            id,
            fixtures::days_ago(days_ago),
        ))
    }

    fn issue(id: &str, days_ago: i64) -> Subject {
        Subject::Issue(fixtures::issue_on(
            DEFAULT_HOST,
            id,
            fixtures::days_ago(days_ago),
        ))
    }

    fn entries(subjects: &[(Subject, ViewSet)]) -> Vec<(usize, ViewSet, DateTime<Utc>)> {
        subjects
            .iter()
            .enumerate()
            .map(|(ix, (subject, views))| (ix, *views, subject.updated_at()))
            .collect()
    }

    fn now() -> DateTime<Local> {
        fixtures::now().with_timezone(&Local)
    }

    fn mixed() -> Vec<(Subject, ViewSet)> {
        let needs_you = ViewSet::INBOX | ViewSet::NEEDS_YOU;
        vec![
            (pr("pr-today", 0), ViewSet::INBOX),
            (issue("issue-today", 0), needs_you),
            (issue("issue-yesterday", 1), ViewSet::INBOX),
            (pr("pr-yesterday", 1), needs_you),
            (pr("pr-week", 3), ViewSet::INBOX),
            (issue("issue-old", 30), ViewSet::INBOX),
        ]
    }

    #[test]
    fn prs_and_issues_share_date_buckets_in_update_order() {
        let subjects: Vec<(Subject, ViewSet)> = mixed()
            .into_iter()
            .map(|(subject, _)| (subject, ViewSet::INBOX))
            .collect();
        assert_eq!(
            group(entries(&subjects), InboxView::Inbox, now()),
            vec![
                (Group::Date(DateBucket::Today), vec![0, 1]),
                (Group::Date(DateBucket::Yesterday), vec![2, 3]),
                (Group::Date(DateBucket::LastSevenDays), vec![4]),
                (Group::Date(DateBucket::Older), vec![5]),
            ]
        );
    }

    #[test]
    fn needs_you_comes_first_in_inbox_with_both_kinds() {
        assert_eq!(
            group(entries(&mixed()), InboxView::Inbox, now()),
            vec![
                (Group::NeedsYou, vec![1, 3]),
                (Group::Date(DateBucket::Today), vec![0]),
                (Group::Date(DateBucket::Yesterday), vec![2]),
                (Group::Date(DateBucket::LastSevenDays), vec![4]),
                (Group::Date(DateBucket::Older), vec![5]),
            ]
        );
    }

    #[test]
    fn other_views_only_group_by_date() {
        for view in [InboxView::NeedsYou, InboxView::IssuesAssigned] {
            assert_eq!(
                group(entries(&mixed()), view, now()),
                vec![
                    (Group::Date(DateBucket::Today), vec![0, 1]),
                    (Group::Date(DateBucket::Yesterday), vec![2, 3]),
                    (Group::Date(DateBucket::LastSevenDays), vec![4]),
                    (Group::Date(DateBucket::Older), vec![5]),
                ],
                "{view:?}"
            );
        }
    }
}
