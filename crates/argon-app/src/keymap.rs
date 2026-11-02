use gpui_kit::{App, KeyBinding};

use crate::actions::{CloseActiveItem, ToggleBottomDock, ToggleLeftDock, ToggleRightDock};

pub const WORKSPACE_CONTEXT: &str = "Workspace";

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-b", ToggleLeftDock, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new("ctrl-alt-b", ToggleRightDock, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new("ctrl-j", ToggleBottomDock, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new("ctrl-w", CloseActiveItem, Some(WORKSPACE_CONTEXT)),
    ]);
}
