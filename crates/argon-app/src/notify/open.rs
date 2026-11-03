use gpui_kit::App;

use super::toast::ToastTarget;
use crate::center::CenterItem;
use crate::inbox::InboxView;
use crate::workspace::Workspace;

pub(super) fn is_window_focused(cx: &mut App) -> bool {
    cx.windows()
        .into_iter()
        .filter_map(|window| window.downcast::<Workspace>())
        .any(|window| window.is_active(cx).unwrap_or(false))
}

pub(super) fn open_target(target: ToastTarget, cx: &mut App) {
    let Some(handle) = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<Workspace>())
    else {
        return;
    };
    cx.activate(true);
    handle
        .update(cx, |workspace, window, cx| {
            window.activate_window();
            match target {
                ToastTarget::NeedsYou => workspace.show_view(InboxView::NeedsYou, cx),
                ToastTarget::Pr(key) => {
                    workspace.open_item(CenterItem::Pr(key), false, window, cx);
                }
                ToastTarget::AppUpdate => workspace.show_about(window, cx),
                ToastTarget::AgentReview(key) => {
                    workspace.open_item(CenterItem::Pr(key), false, window, cx);
                    workspace.reveal_agents(window, cx);
                }
            }
        })
        .ok();
}
