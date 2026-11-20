use argon_core::RepoKey;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::{App, Window};

use super::{labels, related};
use crate::center::CenterItem;
use crate::config_watch;
use crate::review::{Confirm, DraftsBus, confirm};
use crate::services::{AppServices, flatten};
use crate::workspace::Workspace;

pub fn request_removal(key: RepoKey, window: &mut Window, cx: &mut App) {
    let request = Confirm {
        title: labels::remove_project_title(&key.repo),
        message: labels::REMOVE_PROJECT_NOTE.to_owned(),
        confirm: labels::REMOVE_PROJECT_CONFIRM,
    };
    confirm(request, window, cx, move |window, cx| {
        remove(key.clone(), window, cx)
    });
}

fn remove(key: RepoKey, window: &mut Window, cx: &mut App) {
    let Some(services) = cx
        .try_global::<AppServices>()
        .map(|global| global.0.clone())
    else {
        return;
    };
    let (host, repo) = (key.host.clone(), key.repo.clone());
    window
        .spawn(cx, async move |cx| {
            let deleted = flatten(
                services
                    .store()
                    .call(move |store| store.delete_project_overview(&host, &repo))
                    .await,
            );
            _ = cx.update(|window, cx| match deleted {
                Ok(_) => finish_removal(&key, window, cx),
                Err(error) => {
                    tracing::warn!(%error, "failed to remove the project overview");
                    window.push_notification(
                        Notification::error(labels::remove_project_failed(&error.to_string())),
                        cx,
                    );
                }
            });
        })
        .detach();
}

fn finish_removal(key: &RepoKey, window: &mut Window, cx: &mut App) {
    config_watch::update(cx, |config| {
        related::forget_related(&mut config.review.repo_related, &key.host, &key.repo);
    });
    if let Some(workspace) = Workspace::global(cx) {
        _ = workspace.update(cx, |workspace, cx| {
            workspace.close_item(&CenterItem::Project(key.clone()), window, cx)
        });
    }
    DraftsBus::notify_projects(cx);
}
