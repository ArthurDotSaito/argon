use chrono::{DateTime, Datelike, Local};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DateBucket {
    Today,
    Yesterday,
    ThisWeek,
    Older,
}

pub fn date_bucket(now: DateTime<Local>, ts: DateTime<Local>) -> DateBucket {
    let today = now.date_naive();
    let day = ts.date_naive();
    if day == today {
        DateBucket::Today
    } else if today.pred_opt() == Some(day) {
        DateBucket::Yesterday
    } else if day.iso_week() == today.iso_week() {
        DateBucket::ThisWeek
    } else {
        DateBucket::Older
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn local(year: i32, month: u32, day: u32, hour: u32, min: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, min, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn date_bucket_groups_by_local_day_and_iso_week() {
        let cases = [
            (
                "same day",
                local(2026, 11, 4, 15, 0),
                local(2026, 11, 4, 8, 30),
                DateBucket::Today,
            ),
            (
                "one minute after midnight still today",
                local(2026, 11, 4, 0, 1),
                local(2026, 11, 4, 0, 0),
                DateBucket::Today,
            ),
            (
                "midnight rollover moves to yesterday",
                local(2026, 11, 4, 0, 5),
                local(2026, 11, 3, 23, 55),
                DateBucket::Yesterday,
            ),
            (
                "yesterday morning",
                local(2026, 11, 4, 23, 59),
                local(2026, 11, 3, 0, 0),
                DateBucket::Yesterday,
            ),
            (
                "monday of the same week",
                local(2026, 11, 5, 12, 0),
                local(2026, 11, 2, 9, 0),
                DateBucket::ThisWeek,
            ),
            (
                "two days ago on the same week",
                local(2026, 11, 5, 12, 0),
                local(2026, 11, 3, 18, 0),
                DateBucket::ThisWeek,
            ),
            (
                "sunday before is the previous week",
                local(2026, 11, 4, 12, 0),
                local(2026, 11, 1, 12, 0),
                DateBucket::Older,
            ),
            (
                "monday sees sunday as yesterday not older",
                local(2026, 11, 2, 9, 0),
                local(2026, 11, 1, 22, 0),
                DateBucket::Yesterday,
            ),
            (
                "monday sees friday as older",
                local(2026, 11, 2, 9, 0),
                local(2026, 10, 30, 22, 0),
                DateBucket::Older,
            ),
            (
                "sunday sees monday of the same iso week",
                local(2026, 11, 8, 20, 0),
                local(2026, 11, 2, 7, 0),
                DateBucket::ThisWeek,
            ),
            (
                "week across a year boundary",
                local(2027, 1, 1, 12, 0),
                local(2026, 12, 29, 12, 0),
                DateBucket::ThisWeek,
            ),
            (
                "same week number in another year",
                local(2026, 11, 4, 12, 0),
                local(2025, 11, 5, 12, 0),
                DateBucket::Older,
            ),
            (
                "weeks ago",
                local(2026, 11, 4, 12, 0),
                local(2026, 9, 1, 12, 0),
                DateBucket::Older,
            ),
        ];

        for (name, now, ts, expected) in cases {
            assert_eq!(date_bucket(now, ts), expected, "{name}");
        }
    }
}
