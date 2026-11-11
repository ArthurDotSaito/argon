use std::cmp::Reverse;

use argon_core::PrKey;
use chrono::{DateTime, Utc};

use super::card_status::{CardStatus, SessionFilter};

pub trait Groupable {
    fn pr(&self) -> &PrKey;
    fn status(&self) -> CardStatus;
    fn created_at(&self) -> DateTime<Utc>;
}

#[derive(Debug)]
pub struct PrGroup<T> {
    pub pr: PrKey,
    pub sessions: Vec<T>,
    pub archived: Vec<T>,
}

fn urgency(status: CardStatus) -> u8 {
    match status {
        CardStatus::Awaiting => 0,
        CardStatus::Running => 1,
        CardStatus::Failed => 2,
        CardStatus::Completed => 3,
        CardStatus::Archived => 4,
    }
}

impl<T: Groupable> PrGroup<T> {
    fn rank(&self) -> u8 {
        self.sessions
            .iter()
            .chain(&self.archived)
            .map(|item| urgency(item.status()))
            .min()
            .unwrap_or(u8::MAX)
    }

    fn latest(&self) -> Option<DateTime<Utc>> {
        self.sessions
            .iter()
            .chain(&self.archived)
            .map(Groupable::created_at)
            .max()
    }
}

pub fn group_by_pr<T: Groupable>(items: Vec<T>, filter: SessionFilter) -> Vec<PrGroup<T>> {
    let mut groups: Vec<PrGroup<T>> = Vec::new();
    for item in items
        .into_iter()
        .filter(|item| item.status().matches(filter))
    {
        let index = match groups.iter().position(|group| group.pr == *item.pr()) {
            Some(index) => index,
            None => {
                groups.push(PrGroup {
                    pr: item.pr().clone(),
                    sessions: Vec::new(),
                    archived: Vec::new(),
                });
                groups.len() - 1
            }
        };
        let group = &mut groups[index];
        if filter == SessionFilter::All && item.status() == CardStatus::Archived {
            group.archived.push(item);
        } else {
            group.sessions.push(item);
        }
    }
    for group in &mut groups {
        group
            .sessions
            .sort_by_key(|item| Reverse(item.created_at()));
        group
            .archived
            .sort_by_key(|item| Reverse(item.created_at()));
    }
    groups.sort_by_key(|group| (group.rank(), Reverse(group.latest())));
    groups
}

#[cfg(test)]
mod tests {
    use argon_core::DEFAULT_HOST;
    use chrono::TimeZone;

    use super::*;

    struct Item {
        id: &'static str,
        pr: PrKey,
        status: CardStatus,
        created_at: DateTime<Utc>,
    }

    impl Groupable for Item {
        fn pr(&self) -> &PrKey {
            &self.pr
        }

        fn status(&self) -> CardStatus {
            self.status
        }

        fn created_at(&self) -> DateTime<Utc> {
            self.created_at
        }
    }

    fn item(id: &'static str, pr: &str, status: CardStatus, minute: u32) -> Item {
        Item {
            id,
            pr: PrKey::new(DEFAULT_HOST, pr),
            status,
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 10, minute, 0).unwrap(),
        }
    }

    fn ids(items: &[Item]) -> Vec<&'static str> {
        items.iter().map(|item| item.id).collect()
    }

    #[test]
    fn groups_are_ordered_by_urgency_then_recency() {
        let groups = group_by_pr(
            vec![
                item("done", "a", CardStatus::Completed, 50),
                item("failed", "b", CardStatus::Failed, 1),
                item("running-old", "c", CardStatus::Running, 2),
                item("awaiting", "d", CardStatus::Awaiting, 3),
                item("running-new", "e", CardStatus::Running, 9),
            ],
            SessionFilter::All,
        );
        let prs: Vec<&str> = groups.iter().map(|group| group.pr.id.0.as_str()).collect();
        assert_eq!(prs, ["d", "e", "c", "b", "a"]);
    }

    #[test]
    fn sessions_inside_a_group_are_newest_first() {
        let groups = group_by_pr(
            vec![
                item("old", "a", CardStatus::Completed, 1),
                item("new", "a", CardStatus::Running, 5),
                item("mid", "a", CardStatus::Failed, 3),
            ],
            SessionFilter::All,
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(ids(&groups[0].sessions), ["new", "mid", "old"]);
    }

    #[test]
    fn all_collapses_archived_sessions() {
        let groups = group_by_pr(
            vec![
                item("live", "a", CardStatus::Running, 1),
                item("gone-old", "a", CardStatus::Archived, 2),
                item("gone-new", "a", CardStatus::Archived, 4),
            ],
            SessionFilter::All,
        );
        assert_eq!(ids(&groups[0].sessions), ["live"]);
        assert_eq!(ids(&groups[0].archived), ["gone-new", "gone-old"]);
    }

    #[test]
    fn archived_filter_lists_them_as_sessions() {
        let groups = group_by_pr(
            vec![
                item("live", "a", CardStatus::Running, 1),
                item("gone", "a", CardStatus::Archived, 2),
            ],
            SessionFilter::Archived,
        );
        assert_eq!(ids(&groups[0].sessions), ["gone"]);
        assert!(groups[0].archived.is_empty());
    }

    #[test]
    fn group_with_only_archived_sessions_still_shows() {
        let groups = group_by_pr(
            vec![
                item("gone", "a", CardStatus::Archived, 2),
                item("live", "b", CardStatus::Completed, 1),
            ],
            SessionFilter::All,
        );
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[1].pr.id.0, "a");
        assert!(groups[1].sessions.is_empty());
        assert_eq!(ids(&groups[1].archived), ["gone"]);
    }

    #[test]
    fn items_outside_the_filter_are_dropped() {
        let groups = group_by_pr(
            vec![
                item("run", "a", CardStatus::Running, 1),
                item("done", "b", CardStatus::Completed, 2),
            ],
            SessionFilter::Running,
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(ids(&groups[0].sessions), ["run"]);
    }
}
