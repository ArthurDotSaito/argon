use argon_core::{Issue, IssueRef, PullRequest};
use argon_github::SubIssueRow;
use gpui_kit::{App, Window};

use crate::center::CenterItem;
use crate::workspace::Workspace;

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
    issue_by_number(host, &sub.repo, sub.number, stored)
}

pub fn closing_issue<'a>(
    host: &str,
    closing: &IssueRef,
    stored: impl IntoIterator<Item = &'a Issue>,
) -> Destination {
    issue_by_number(host, &closing.repo, closing.number, stored)
}

pub fn open(target: &Destination, window: &mut Window, cx: &mut App) {
    match target {
        Destination::Argon(item) => {
            let Some(workspace) = Workspace::global(cx) else {
                return;
            };
            let item = item.clone();
            _ = workspace.update(cx, |workspace, cx| {
                workspace.open_item(item, false, window, cx)
            });
        }
        Destination::Browser(url) => {
            if let Err(error) = open::that_detached(url) {
                tracing::warn!(%error, url, "failed to open link in browser");
            }
        }
    }
}

fn issue_by_number<'a>(
    host: &str,
    repo: &str,
    number: u64,
    stored: impl IntoIterator<Item = &'a Issue>,
) -> Destination {
    stored
        .into_iter()
        .find(|issue| {
            issue.number == number
                && issue.repo.eq_ignore_ascii_case(repo)
                && issue.host.eq_ignore_ascii_case(host)
        })
        .map(|issue| Destination::Argon(CenterItem::Issue(issue.key())))
        .unwrap_or_else(|| Destination::Browser(issue_url(host, repo, number)))
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
    fn a_stored_closing_issue_opens_in_argon() {
        let stored = [stored_issue("github.com", "CLOSED", "acme/widgets", 76)];
        let closing = IssueRef {
            repo: "acme/widgets".into(),
            number: 76,
        };
        assert_eq!(
            closing_issue("github.com", &closing, &stored),
            Destination::Argon(CenterItem::Issue(stored[0].key()))
        );
    }

    #[test]
    fn a_closing_issue_outside_the_store_opens_its_url() {
        let closing = IssueRef {
            repo: "acme/api".into(),
            number: 3,
        };
        assert_eq!(
            closing_issue("github.com", &closing, []),
            Destination::Browser("https://github.com/acme/api/issues/3".into())
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
