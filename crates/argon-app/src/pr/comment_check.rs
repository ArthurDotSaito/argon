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
        self.landed_id(timeline).is_some()
    }

    pub fn landed_id<'a>(&self, timeline: &'a [TimelineItem]) -> Option<&'a str> {
        let window = Duration::minutes(LANDED_WINDOW_MINUTES);
        timeline.iter().find_map(|item| match item {
            TimelineItem::Comment {
                id,
                author,
                body,
                created_at,
            } if author.eq_ignore_ascii_case(&self.author)
                && body.trim() == self.body
                && *created_at >= self.sent_at - window
                && *created_at <= self.sent_at + window =>
            {
                Some(id.as_str())
            }
            _ => None,
        })
    }
}

pub fn is_transport_error(error: &ServiceError) -> bool {
    matches!(error, ServiceError::Github(GithubError::Http(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(id: &str, author: &str, body: &str, created_at: DateTime<Utc>) -> TimelineItem {
        TimelineItem::Comment {
            id: id.into(),
            author: author.into(),
            body: body.into(),
            created_at,
        }
    }

    #[test]
    fn landed_id_finds_the_matching_comment_near_the_send_time() {
        let sent = SentComment::new("Ana".into(), "Plano".into());
        let near = sent.sent_at + Duration::seconds(30);
        let timeline = [
            comment(
                "IC_old",
                "ana",
                "Plano",
                sent.sent_at - Duration::minutes(10),
            ),
            comment("IC_other", "bruno", "Plano", near),
            comment("IC_new", "ana", "Plano\n", near),
        ];
        assert_eq!(sent.landed_id(&timeline), Some("IC_new"));
        assert!(sent.landed_in(&timeline));
    }

    #[test]
    fn landed_id_ignores_a_different_body() {
        let sent = SentComment::new("ana".into(), "Plano".into());
        let timeline = [comment("IC_1", "ana", "Outro", sent.sent_at)];
        assert_eq!(sent.landed_id(&timeline), None);
        assert!(!sent.landed_in(&timeline));
    }
}
