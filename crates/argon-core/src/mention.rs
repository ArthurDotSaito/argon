use chrono::{DateTime, Utc};

use crate::model::Issue;
use crate::views::{is_assigned_to, is_issue_mine, is_issue_open, is_unread};

pub fn is_mention_candidate(issue: &Issue, me: &str, last_seen: Option<DateTime<Utc>>) -> bool {
    is_issue_open(issue)
        && issue.involvement.mentioned
        && !is_assigned_to(issue, me)
        && !is_issue_mine(issue, me)
        && is_unread(issue, last_seen)
}

pub fn mentions_login(text: &str, login: &str) -> bool {
    if login.is_empty() {
        return false;
    }
    let text = text.to_ascii_lowercase();
    let login = login.to_ascii_lowercase();
    text.match_indices('@').any(|(at, _)| {
        let preceded_by_word = text[..at]
            .chars()
            .next_back()
            .is_some_and(is_before_blocker);
        let rest = &text[at + 1..];
        !preceded_by_word
            && rest.starts_with(&login)
            && !rest[login.len()..]
                .chars()
                .next()
                .is_some_and(is_after_blocker)
    })
}

fn is_before_blocker(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '-')
}

fn is_after_blocker(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{ME, days_ago, issue, my_issue};
    use crate::model::{Involvement, IssueState};

    fn mentioned_issue() -> Issue {
        Issue {
            involvement: Involvement {
                mentioned: true,
                ..Involvement::default()
            },
            ..issue()
        }
    }

    #[test]
    fn mention_candidate_follows_the_rule() {
        let seen = Some(days_ago(2));
        let cases: Vec<(&str, Issue, Option<DateTime<Utc>>, bool)> = vec![
            ("mentioned and updated since", mentioned_issue(), seen, true),
            ("mentioned and never seen", mentioned_issue(), None, true),
            (
                "mentioned but already read",
                mentioned_issue(),
                Some(days_ago(0)),
                false,
            ),
            (
                "mentioned and updated exactly at last seen",
                mentioned_issue(),
                Some(days_ago(1)),
                false,
            ),
            ("not mentioned", issue(), seen, false),
            (
                "closed",
                Issue {
                    state: IssueState::Closed,
                    ..mentioned_issue()
                },
                seen,
                false,
            ),
            (
                "assigned to me",
                Issue {
                    assignees: vec!["ME".into()],
                    ..mentioned_issue()
                },
                seen,
                false,
            ),
            (
                "assigned to another person",
                Issue {
                    assignees: vec!["bob".into()],
                    ..mentioned_issue()
                },
                seen,
                true,
            ),
            (
                "mine",
                Issue {
                    involvement: mentioned_issue().involvement,
                    ..my_issue()
                },
                seen,
                false,
            ),
        ];

        for (name, issue, last_seen, expected) in cases {
            assert_eq!(
                is_mention_candidate(&issue, ME, last_seen),
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn mentions_login_matches_whole_handles_only() {
        let cases = [
            ("plain mention", "ping @me please", true),
            ("mention at the start", "@me", true),
            ("uppercase text", "ping @ME", true),
            ("mixed case login", "ping @Me", true),
            ("sentence end", "thanks @me.", true),
            ("comma", "hi @me, look", true),
            ("after newline", "hi\n@me", true),
            ("after open paren", "(@me)", true),
            ("email address", "write to x@me.com", false),
            ("email with dotted prefix", "a.b@me", false),
            ("longer login with dash", "cc @me-other", false),
            ("longer login", "cc @mex", false),
            ("longer login with underscore", "cc @me_bot", false),
            ("longer login with digit", "cc @me2", false),
            ("login only after another handle", "@other @me", true),
            ("missing at sign", "me", false),
            ("at sign alone", "@", false),
            ("later valid mention after a bad one", "x@me and @me", true),
            ("empty text", "", false),
            ("non ascii neighbours", "olá @me!", true),
            (
                "team mention of an org with the same name",
                "cc @me/devs",
                false,
            ),
            ("inside inline code", "run `@me`", true),
        ];

        for (name, text, expected) in cases {
            assert_eq!(mentions_login(text, "me"), expected, "{name}");
        }
    }

    #[test]
    fn mentions_login_rejects_empty_login() {
        assert!(!mentions_login("@", ""));
    }
}
