use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex};
use gpui_kit::{App, Div, ParentElement as _, SharedString, Styled as _, Window, div};

use super::card_status::{CardStatus, ResumeAction};
use super::panel_labels as labels;
use super::sessions::{AgentSessions, ResumeState};
use crate::review::{Confirm, confirm};
use crate::services::{AppServices, Services};

#[derive(Debug, Clone)]
pub struct SessionControls {
    pub id: String,
    pub status: CardStatus,
    pub resume: Option<ResumeAction>,
    pub worktree: Option<PathBuf>,
}

fn services(cx: &App) -> Option<Services> {
    cx.try_global::<AppServices>()
        .map(|global| global.0.clone())
}

fn resume(id: String, cx: &mut App) {
    let (Some(sessions), Some(services)) = (AgentSessions::global(cx), services(cx)) else {
        return;
    };
    sessions.update(cx, |sessions, cx| sessions.resume(id, &services, cx));
}

fn confirm_archive(id: String, worktree: Option<PathBuf>, window: &mut Window, cx: &mut App) {
    let request = Confirm {
        title: labels::ARCHIVE_TITLE,
        message: labels::ARCHIVE_MESSAGE.to_owned(),
        confirm: labels::ARCHIVE,
    };
    confirm(request, window, cx, move |_, cx| {
        let (Some(sessions), Some(services)) = (AgentSessions::global(cx), services(cx)) else {
            return;
        };
        let (id, worktree) = (id.clone(), worktree.clone());
        sessions.update(cx, |sessions, cx| {
            sessions.archive(id, worktree, &services, cx)
        });
    });
}

pub fn controls(controls: &SessionControls, prefix: &str, cx: &App) -> Option<Div> {
    let state = AgentSessions::global(cx)
        .and_then(|sessions| sessions.read(cx).resume_state(&controls.id).cloned());
    let resuming = matches!(state, Some(ResumeState::Running));
    let resume = controls.resume.map(|action| {
        let label = match action {
            ResumeAction::Continue => labels::CONTINUE,
            ResumeAction::Retry => labels::RETRY,
        };
        let id = controls.id.clone();
        Button::new(SharedString::from(format!(
            "{prefix}-resume-{}",
            controls.id
        )))
        .icon(Icon::new(IconName::Play))
        .label(if resuming { labels::RESUMING } else { label })
        .xsmall()
        .outline()
        .loading(resuming)
        .disabled(resuming)
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            resume(id.clone(), cx)
        })
    });
    let archivable = AgentSessions::global(cx)
        .is_none_or(|sessions| sessions.read(cx).can_archive(&controls.id));
    let archive = (controls.status != CardStatus::Archived).then(|| {
        let (id, worktree) = (controls.id.clone(), controls.worktree.clone());
        Button::new(SharedString::from(format!(
            "{prefix}-archive-{}",
            controls.id
        )))
        .icon(Icon::new(IconName::Archive))
        .xsmall()
        .ghost()
        .tooltip(labels::ARCHIVE_TOOLTIP)
        .disabled(!archivable)
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            confirm_archive(id.clone(), worktree.clone(), window, cx)
        })
    });
    let error = match state {
        Some(ResumeState::Failed(error)) => Some(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(cx.theme().danger)
                .child(labels::resume_failed(&error)),
        ),
        _ => None,
    };
    if resume.is_none() && archive.is_none() && error.is_none() {
        return None;
    }
    Some(
        h_flex()
            .gap_1p5()
            .items_center()
            .children(error)
            .child(div().flex_1())
            .children(resume)
            .children(archive),
    )
}
