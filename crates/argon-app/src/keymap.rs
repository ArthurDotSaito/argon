use gpui_kit::{App, KeyBinding};

use crate::actions::{
    ArchiveSelected, ClearFilter, CloseActiveItem, FocusFilter, OpenSelected, SelectNext,
    SelectPrevious, ToggleBottomDock, ToggleLeftDock, ToggleRead, ToggleRightDock,
};

pub const WORKSPACE_CONTEXT: &str = "Workspace";
pub const INBOX_CONTEXT: &str = "Inbox";
pub const INBOX_LIST_CONTEXT: &str = "InboxList";

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-b", ToggleLeftDock, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new("ctrl-alt-b", ToggleRightDock, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new("ctrl-j", ToggleBottomDock, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new("ctrl-w", CloseActiveItem, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new("j", SelectNext, Some(INBOX_LIST_CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(INBOX_LIST_CONTEXT)),
        KeyBinding::new("k", SelectPrevious, Some(INBOX_LIST_CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(INBOX_LIST_CONTEXT)),
        KeyBinding::new("enter", OpenSelected, Some(INBOX_LIST_CONTEXT)),
        KeyBinding::new("e", ArchiveSelected, Some(INBOX_LIST_CONTEXT)),
        KeyBinding::new("u", ToggleRead, Some(INBOX_LIST_CONTEXT)),
        KeyBinding::new("/", FocusFilter, Some(INBOX_LIST_CONTEXT)),
        KeyBinding::new("escape", ClearFilter, Some(INBOX_CONTEXT)),
    ]);
}
