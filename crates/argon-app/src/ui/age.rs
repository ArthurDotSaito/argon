use chrono::{DateTime, Local, Utc};

pub fn age_label(now: DateTime<Utc>, ts: DateTime<Utc>) -> String {
    Age::of(now, ts).label()
}

pub fn ago(now: DateTime<Utc>, ts: DateTime<Utc>) -> String {
    match Age::of(now, ts) {
        Age::Now => "agora".to_string(),
        Age::Yesterday => "ontem".to_string(),
        age => format!("há {}", age.label()),
    }
}

enum Age {
    Now,
    Minutes(i64),
    Hours(i64),
    Yesterday,
    Days(i64),
    Weeks(i64),
    Months(i64),
}

impl Age {
    fn of(now: DateTime<Utc>, ts: DateTime<Utc>) -> Age {
        let minutes = (now - ts).num_minutes().max(0);
        let days = (now.with_timezone(&Local).date_naive() - ts.with_timezone(&Local).date_naive())
            .num_days()
            .max(0);
        match (minutes, days) {
            (0, _) => Age::Now,
            (1..60, _) => Age::Minutes(minutes),
            (_, 0) => Age::Hours(minutes / 60),
            (_, 1) => Age::Yesterday,
            (_, 2..7) => Age::Days(days),
            (_, 7..30) => Age::Weeks(days / 7),
            _ => Age::Months(days / 30),
        }
    }

    fn label(&self) -> String {
        match self {
            Age::Now => "agora".to_string(),
            Age::Minutes(minutes) => format!("{minutes} min"),
            Age::Hours(hours) => format!("{hours} h"),
            Age::Yesterday => "ontem".to_string(),
            Age::Days(days) => format!("{days} d"),
            Age::Weeks(weeks) => format!("{weeks} sem"),
            Age::Months(months) => format!("{months} mês"),
        }
    }
}
