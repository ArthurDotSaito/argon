mod browse;
mod filter;
mod inbox_view;
mod items;
mod labels;
mod model;
mod row;
mod sections;
mod views_panel;

use argon_core::{PrKey, ViewSet};
use gpui_kit::{App, SharedString};

pub use inbox_view::InboxPane;
pub use labels::{ISSUE_FALLBACK_TITLE, PR_FALLBACK_TITLE, needs_you_status, repo_ref};
pub use model::{InboxEvent, InboxHandle, InboxModel};
pub(crate) use row::pr_row;
pub use views_panel::{VIEWS_PANEL, ViewsPanel};

pub fn pr_tab_title(key: &PrKey, cx: &App) -> SharedString {
    tab_title(key, labels::PR_FALLBACK_TITLE, cx)
}

pub fn issue_tab_title(key: &PrKey, cx: &App) -> SharedString {
    tab_title(key, labels::ISSUE_FALLBACK_TITLE, cx)
}

fn tab_title(key: &PrKey, fallback: &'static str, cx: &App) -> SharedString {
    InboxModel::global(cx)
        .and_then(|model| {
            let subject = model.read(cx).subject(key)?;
            Some(labels::pr_tab_title(subject.number(), subject.title()).into())
        })
        .unwrap_or_else(|| fallback.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InboxView {
    Inbox,
    NeedsYou,
    YourPrs,
    WaitingOnOthers,
    ChangesRequested,
    ReadyToMerge,
    AlreadyDone,
    IssuesAssigned,
    IssuesMentioned,
    IssuesAuthored,
    IssuesParticipating,
}

impl InboxView {
    pub const ALL: [Self; 11] = [
        Self::Inbox,
        Self::NeedsYou,
        Self::YourPrs,
        Self::WaitingOnOthers,
        Self::ChangesRequested,
        Self::ReadyToMerge,
        Self::AlreadyDone,
        Self::IssuesAssigned,
        Self::IssuesMentioned,
        Self::IssuesAuthored,
        Self::IssuesParticipating,
    ];

    pub fn view_set(self) -> ViewSet {
        match self {
            Self::Inbox => ViewSet::INBOX,
            Self::NeedsYou => ViewSet::NEEDS_YOU,
            Self::YourPrs => ViewSet::YOUR_PRS,
            Self::WaitingOnOthers => ViewSet::WAITING_ON_OTHERS,
            Self::ChangesRequested => ViewSet::CHANGES_REQUESTED,
            Self::ReadyToMerge => ViewSet::READY_TO_MERGE,
            Self::AlreadyDone => ViewSet::ALREADY_DONE,
            Self::IssuesAssigned => ViewSet::ISSUES_ASSIGNED,
            Self::IssuesMentioned => ViewSet::ISSUES_MENTIONED,
            Self::IssuesAuthored => ViewSet::ISSUES_AUTHORED,
            Self::IssuesParticipating => ViewSet::ISSUES_PARTICIPATING,
        }
    }

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|view| *view == self)
            .unwrap_or_default()
    }
}
