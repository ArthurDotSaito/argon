use argon_core::PullRequest;
use gpui_kit::App;

use super::PrView;
use crate::center::CenterItem;
use crate::issue::IssueStatus;
use crate::issue::destination::{self, Destination};
use crate::pr::header::ClosingLink;

impl PrView {
    pub(super) fn closing_links(&self, pr: &PullRequest, cx: &App) -> Vec<ClosingLink> {
        let inbox = self.inbox.read(cx);
        pr.closing_issues
            .iter()
            .map(|closing| {
                let stored = inbox
                    .items()
                    .iter()
                    .filter_map(|item| item.subject.as_issue());
                let target = destination::closing_issue(&pr.host, closing, stored);
                let status = match &target {
                    Destination::Argon(CenterItem::Issue(key)) => inbox
                        .issue(key)
                        .map(|issue| IssueStatus::of(issue.state, issue.state_reason)),
                    _ => None,
                };
                ClosingLink {
                    issue: closing.clone(),
                    status,
                    target,
                }
            })
            .collect()
    }
}
