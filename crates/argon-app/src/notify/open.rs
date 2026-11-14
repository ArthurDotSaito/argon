use gpui_kit::App;

use super::toast::ToastTarget;
use crate::agents::SessionCatalog;
use crate::center::CenterItem;
use crate::inbox::InboxView;
use crate::workspace::{SessionPlacement, Workspace, subject_item};

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
                ToastTarget::Issue(key) => {
                    workspace.open_item(CenterItem::Issue(key), false, window, cx);
                }
                ToastTarget::AppUpdate => workspace.show_about(window, cx),
                ToastTarget::AgentReview(key) => {
                    let catalog = SessionCatalog::global(cx);
                    let latest = catalog
                        .as_ref()
                        .and_then(|catalog| catalog.read(cx).latest_for(&key, cx));
                    let subject = latest
                        .as_ref()
                        .zip(catalog)
                        .and_then(|(id, catalog)| catalog.read(cx).subject_of(id, cx))
                        .map_or_else(
                            || CenterItem::Pr(key.clone()),
                            |(key, kind)| subject_item(key, kind),
                        );
                    workspace.open_item(subject, false, window, cx);
                    if let Some(id) = latest {
                        workspace.open_session(id, SessionPlacement::BesideSubject, window, cx);
                    }
                }
            }
        })
        .ok();
}
