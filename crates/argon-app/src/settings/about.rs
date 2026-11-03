use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    Context, Entity, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Subscription, Window, div,
};

use super::labels;
use super::ui::{card, field, note, section};
use crate::agents::AgentSessions;
use crate::review::{Confirm, confirm};
use crate::ui::ago;
use crate::update::{self, CAN_INSTALL, UpdateModel, UpdateState, labels as update_labels};

const CURRENT_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

pub struct AboutSection {
    model: Entity<UpdateModel>,
    _observation: Subscription,
}

impl AboutSection {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let model = update::entity(cx);
        let observation = cx.observe(&model, |_, _, cx| cx.notify());
        Self {
            model,
            _observation: observation,
        }
    }

    fn request_install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = AgentSessions::global(cx).map_or(0, |sessions| {
            let activity = sessions.read(cx).activity(cx);
            activity.running + activity.awaiting
        });
        let model = self.model.clone();
        if active == 0 {
            model.update(cx, |model, cx| model.install(cx));
            return;
        }
        let request = Confirm {
            title: labels::UPDATE_CONFIRM_TITLE,
            message: labels::update_stops_agents(active),
            confirm: labels::CONFIRM,
        };
        confirm(request, window, cx, move |_, cx| {
            model.update(cx, |model, cx| model.install(cx));
        });
    }
}

fn value(text: impl Into<SharedString>) -> impl IntoElement {
    div().text_sm().child(text.into())
}

impl Render for AboutSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (state, offered, last_checked) = {
            let model = self.model.read(cx);
            (
                model.state().clone(),
                model.offered().cloned(),
                model.last_checked(),
            )
        };
        let theme = cx.theme();
        let tone = match &state {
            UpdateState::UpToDate => theme.success,
            UpdateState::Available(_) | UpdateState::Downloading { .. } => theme.primary,
            UpdateState::Unavailable(_) => theme.warning,
            UpdateState::Failed(_) => theme.danger,
            UpdateState::Idle | UpdateState::Checking | UpdateState::Installing => {
                theme.muted_foreground
            }
        };
        let checked = last_checked
            .map(|at| labels::checked_ago(&ago(Utc::now(), at.with_timezone(&Utc))))
            .unwrap_or_else(|| update_labels::IDLE.to_string());
        let release = offered.as_ref();

        let check = Button::new("settings-about-check")
            .icon(Icon::new(IconName::RefreshCw))
            .label(labels::CHECK_UPDATES)
            .small()
            .outline()
            .loading(state == UpdateState::Checking)
            .disabled(state.is_busy())
            .on_click(cx.listener(|this, _, _, cx| {
                this.model.update(cx, |model, cx| model.check(cx));
            }));
        let release_notes = release.map(|release| {
            let url = release.html_url.clone();
            Button::new("settings-about-notes")
                .icon(Icon::new(IconName::ExternalLink))
                .label(labels::WHATS_NEW)
                .small()
                .ghost()
                .on_click(move |_, _, _| {
                    if let Err(error) = open::that_detached(&url) {
                        tracing::warn!(%error, url, "failed to open release notes in browser");
                    }
                })
        });
        let install = release.map(|_| {
            Button::new("settings-about-install")
                .icon(Icon::new(IconName::ArrowUp))
                .label(labels::UPDATE)
                .small()
                .primary()
                .loading(matches!(state, UpdateState::Downloading { .. }))
                .disabled(!CAN_INSTALL || state.is_busy())
                .on_click(cx.listener(|this, _, window, cx| this.request_install(window, cx)))
        });

        let details = card(cx)
            .child(field(labels::APP, value(labels::APP_NAME), cx))
            .child(field(labels::VERSION, value(CURRENT_VERSION), cx))
            .child(field(labels::LAST_CHECK, value(checked), cx))
            .child(field(
                labels::STATUS,
                div().text_sm().text_color(tone).child(state.status_text()),
                cx,
            ))
            .children(release.map(|release| {
                field(
                    labels::NEW_VERSION,
                    value(format!("v{}", release.version)),
                    cx,
                )
            }));
        let actions = h_flex()
            .gap_2()
            .items_center()
            .child(check)
            .children(release_notes)
            .children(install);
        let debug_note = (release.is_some() && !CAN_INSTALL)
            .then(|| note(labels::INSTALLED_APP_ONLY, cx.theme().muted_foreground));

        section(labels::ABOUT, labels::ABOUT_HINT, cx)
            .child(details)
            .child(v_flex().gap_1().child(actions).children(debug_note))
    }
}
