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
pub use labels::{PR_FALLBACK_TITLE, needs_you_status};
pub use model::{InboxEvent, InboxHandle, InboxModel};
pub use views_panel::{VIEWS_PANEL, ViewsPanel};

pub fn pr_tab_title(key: &PrKey, cx: &App) -> SharedString {
    InboxModel::global(cx)
        .and_then(|model| {
            let pr = model.read(cx).pr(key)?;
            Some(labels::pr_tab_title(pr.number, &pr.title).into())
        })
        .unwrap_or_else(|| labels::PR_FALLBACK_TITLE.into())
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
}

impl InboxView {
    pub const ALL: [Self; 7] = [
        Self::Inbox,
        Self::NeedsYou,
        Self::YourPrs,
        Self::WaitingOnOthers,
        Self::ChangesRequested,
        Self::ReadyToMerge,
        Self::AlreadyDone,
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
        }
    }

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|view| *view == self)
            .unwrap_or_default()
    }
}
