use argon_core::{Issue, PullRequest};
use argon_github::SubIssueRow;

use crate::center::CenterItem;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    Argon(CenterItem),
    Browser(String),
}

pub fn linked_pr(pr: &PullRequest, stored: bool) -> Destination {
    if stored {
        Destination::Argon(CenterItem::Pr(pr.key()))
    } else {
        Destination::Browser(pr.url.clone())
    }
}

pub fn sub_issue<'a>(
    host: &str,
    sub: &SubIssueRow,
    stored: impl IntoIterator<Item = &'a Issue>,
) -> Destination {
    stored
        .into_iter()
        .find(|issue| {
            issue.number == sub.number
                && issue.repo.eq_ignore_ascii_case(&sub.repo)
                && issue.host.eq_ignore_ascii_case(host)
        })
        .map(|issue| Destination::Argon(CenterItem::Issue(issue.key())))
        .unwrap_or_else(|| Destination::Browser(issue_url(host, &sub.repo, sub.number)))
}

fn issue_url(host: &str, repo: &str, number: u64) -> String {
    format!("https://{host}/{repo}/issues/{number}")
}

#[cfg(test)]
mod tests {
    use argon_core::IssueState;
    use argon_core::fixtures::{self, issue_on};

    use super::*;

    fn sub(repo: &str, number: u64) -> SubIssueRow {
        SubIssueRow {
            repo: repo.into(),
            number,
            title: "Child".into(),
            state: IssueState::Open,
        }
    }

    fn stored_issue(host: &str, id: &str, repo: &str, number: u64) -> Issue {
        Issue {
            repo: repo.into(),
            number,
            ..issue_on(host, id, fixtures::now())
        }
    }

    #[test]
    fn a_stored_pr_opens_in_argon() {
        let pr = fixtures::pr();
        assert_eq!(
            linked_pr(&pr, true),
            Destination::Argon(CenterItem::Pr(pr.key()))
        );
    }

    #[test]
    fn a_pr_outside_the_store_opens_its_url() {
        let pr = fixtures::pr();
        assert_eq!(linked_pr(&pr, false), Destination::Browser(pr.url.clone()));
    }

    #[test]
    fn a_stored_sub_issue_opens_in_argon_matching_repo_without_case() {
        let stored = [
            stored_issue("github.com", "OTHER", "acme/widgets", 8),
            stored_issue("github.com", "CHILD", "Acme/Widgets", 7),
        ];
        assert_eq!(
            sub_issue("github.com", &sub("acme/widgets", 7), &stored),
            Destination::Argon(CenterItem::Issue(stored[1].key()))
        );
    }

    #[test]
    fn a_sub_issue_on_another_host_is_not_the_stored_one() {
        let stored = [stored_issue("ghe.acme.io", "CHILD", "acme/widgets", 7)];
        assert_eq!(
            sub_issue("github.com", &sub("acme/widgets", 7), &stored),
            Destination::Browser("https://github.com/acme/widgets/issues/7".into())
        );
    }

    #[test]
    fn a_sub_issue_outside_the_store_opens_on_its_host() {
        assert_eq!(
            sub_issue("ghe.acme.io", &sub("acme/api", 12), []),
            Destination::Browser("https://ghe.acme.io/acme/api/issues/12".into())
        );
    }
}
