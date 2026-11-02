use chrono::{DateTime, Local};

const RECENT_DAYS: i64 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DateBucket {
    Today,
    Yesterday,
    LastSevenDays,
    Older,
}

pub fn date_bucket(now: DateTime<Local>, ts: DateTime<Local>) -> DateBucket {
    let days_ago = (now.date_naive() - ts.date_naive()).num_days();
    match days_ago {
        ..=0 => DateBucket::Today,
        1 => DateBucket::Yesterday,
        2..=RECENT_DAYS => DateBucket::LastSevenDays,
        _ => DateBucket::Older,
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
    fn date_bucket_groups_by_local_day_and_last_seven_days() {
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
                "timestamp slightly in the future is today",
                local(2026, 11, 4, 23, 59),
                local(2026, 11, 5, 0, 2),
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
                "monday sees sunday as yesterday",
                local(2026, 11, 2, 9, 0),
                local(2026, 11, 1, 22, 0),
                DateBucket::Yesterday,
            ),
            (
                "two days ago",
                local(2026, 11, 5, 12, 0),
                local(2026, 11, 3, 18, 0),
                DateBucket::LastSevenDays,
            ),
            (
                "monday sees the previous friday as recent",
                local(2026, 11, 2, 9, 0),
                local(2026, 10, 30, 22, 0),
                DateBucket::LastSevenDays,
            ),
            (
                "exactly seven days ago is still recent",
                local(2026, 11, 9, 8, 0),
                local(2026, 11, 2, 23, 0),
                DateBucket::LastSevenDays,
            ),
            (
                "eight days ago is older",
                local(2026, 11, 9, 23, 0),
                local(2026, 11, 1, 0, 30),
                DateBucket::Older,
            ),
            (
                "recent days across a year boundary",
                local(2027, 1, 2, 12, 0),
                local(2026, 12, 28, 12, 0),
                DateBucket::LastSevenDays,
            ),
            (
                "same date in another year",
                local(2026, 11, 4, 12, 0),
                local(2025, 11, 4, 12, 0),
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
