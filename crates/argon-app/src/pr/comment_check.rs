use argon_github::{GithubError, TimelineItem};
use chrono::{DateTime, Duration, Utc};

use crate::services::ServiceError;

const LANDED_WINDOW_MINUTES: i64 = 2;

pub struct SentComment {
    author: String,
    body: String,
    sent_at: DateTime<Utc>,
}

impl SentComment {
    pub fn new(author: String, body: String) -> Self {
        Self {
            author,
            body,
            sent_at: Utc::now(),
        }
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn landed_in(&self, timeline: &[TimelineItem]) -> bool {
        let window = Duration::minutes(LANDED_WINDOW_MINUTES);
        timeline.iter().any(|item| match item {
            TimelineItem::Comment {
                author,
                body,
                created_at,
                ..
            } => {
                author.eq_ignore_ascii_case(&self.author)
                    && body.trim() == self.body
                    && *created_at >= self.sent_at - window
                    && *created_at <= self.sent_at + window
            }
            _ => false,
        })
    }
}

pub fn is_transport_error(error: &ServiceError) -> bool {
    matches!(error, ServiceError::Github(GithubError::Http(_)))
}
