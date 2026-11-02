use std::ops::Range;

use argon_github::CommitInfo;
use chrono::{Datelike as _, Local, NaiveDate};

const WEEKDAYS: [&str; 7] = ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"];
const MONTHS: [&str; 12] = [
    "jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez",
];
const TODAY: &str = "Hoje";
const YESTERDAY: &str = "Ontem";

pub struct DayGroup {
    pub day: NaiveDate,
    pub commits: Range<usize>,
}

pub fn local_day(commit: &CommitInfo) -> NaiveDate {
    commit.committed_at.with_timezone(&Local).date_naive()
}

pub fn group_by_day(days: &[NaiveDate]) -> Vec<DayGroup> {
    let mut groups: Vec<DayGroup> = Vec::new();
    for (ix, day) in days.iter().enumerate() {
        match groups.last_mut() {
            Some(group) if group.day == *day => group.commits.end = ix + 1,
            _ => groups.push(DayGroup {
                day: *day,
                commits: ix..ix + 1,
            }),
        }
    }
    groups
}

pub fn day_label(day: NaiveDate, today: NaiveDate) -> String {
    match (today - day).num_days() {
        0 => TODAY.to_string(),
        1 => YESTERDAY.to_string(),
        _ => {
            let weekday = WEEKDAYS[day.weekday().num_days_from_monday() as usize];
            let month = MONTHS[day.month0() as usize];
            if day.year() == today.year() {
                format!("{weekday}, {} {month}", day.day())
            } else {
                format!("{weekday}, {} {month} {}", day.day(), day.year())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn groups_consecutive_commits_of_the_same_day_keeping_order() {
        let days = [
            date(2026, 10, 28),
            date(2026, 10, 28),
            date(2026, 10, 30),
            date(2026, 11, 2),
            date(2026, 11, 2),
            date(2026, 11, 2),
        ];

        let groups = group_by_day(&days);

        let shape: Vec<_> = groups.iter().map(|g| (g.day, g.commits.clone())).collect();
        assert_eq!(
            shape,
            vec![
                (date(2026, 10, 28), 0..2),
                (date(2026, 10, 30), 2..3),
                (date(2026, 11, 2), 3..6),
            ]
        );
    }

    #[test]
    fn empty_input_has_no_groups() {
        assert!(group_by_day(&[]).is_empty());
    }

    #[test]
    fn splits_again_when_a_day_reappears_out_of_order() {
        let days = [date(2026, 11, 1), date(2026, 11, 2), date(2026, 11, 1)];

        assert_eq!(group_by_day(&days).len(), 3);
    }

    #[test]
    fn labels_today_and_yesterday() {
        let today = date(2026, 11, 2);

        assert_eq!(day_label(today, today), "Hoje");
        assert_eq!(day_label(date(2026, 11, 1), today), "Ontem");
    }

    #[test]
    fn labels_older_days_with_weekday_and_month() {
        let today = date(2026, 11, 2);

        assert_eq!(day_label(date(2026, 10, 28), today), "qua, 28 out");
        assert_eq!(day_label(date(2026, 3, 1), today), "dom, 1 mar");
    }

    #[test]
    fn labels_days_of_other_years_with_the_year() {
        let today = date(2026, 1, 5);

        assert_eq!(day_label(date(2025, 12, 31), today), "qua, 31 dez 2025");
    }

    #[test]
    fn labels_the_day_after_new_year_as_yesterday() {
        let today = date(2026, 1, 1);

        assert_eq!(day_label(date(2025, 12, 31), today), "Ontem");
    }
}
