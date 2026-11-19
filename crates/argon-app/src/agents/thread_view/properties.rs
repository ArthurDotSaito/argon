use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Div, InteractiveElement as _, IntoElement as _, ParentElement as _,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, div,
};

use super::ThreadView;
use crate::agents::badges::{severity_pills, status_chip};
use crate::agents::labels::{PLAN_MODE_LOCKED, permission_mode, permission_mode_hint};
use crate::agents::panel_labels as labels;
use crate::agents::session_actions::controls;
use crate::bottom::duration_label;
use crate::ui::properties;

impl ThreadView {
    pub(super) fn properties(&self, reference: SharedString, cx: &App) -> Stateful<Div> {
        let model = self.model.read(cx);
        let info = model.info();
        let theme = cx.theme();
        let composer = self.live_composer(cx).map(|composer| composer.read(cx));
        let mode = model.permission_mode();
        let model_value = match composer {
            Some(composer) => div()
                .w_full()
                .child(composer.model_select(cx))
                .into_any_element(),
            None => text(info.model_label.clone()),
        };
        let permission_value = match (composer, mode) {
            (Some(composer), _) => Some(
                div()
                    .w_full()
                    .child(composer.permission_select(cx))
                    .into_any_element(),
            ),
            (None, Some(mode)) => Some(text(permission_mode(mode))),
            (None, None) => None,
        };
        let hint = if model.is_permission_mode_locked() {
            Some(PLAN_MODE_LOCKED)
        } else {
            mode.map(permission_mode_hint)
        }
        .map(|hint| properties::row("", note(hint, theme.muted_foreground), cx));
        let turn_hint = (composer.is_some() && model.transcript().in_turn()).then(|| {
            properties::row(
                "",
                note(labels::PERMISSION_MODE_TURN_HINT, theme.warning),
                cx,
            )
        });
        let session = properties::group(cx)
            .border_t_0()
            .pt_0()
            .child(properties::row(
                labels::PROPERTY_STATUS,
                status_chip(model.card_status(), cx),
                cx,
            ))
            .child(properties::row(
                labels::PROPERTY_TIME,
                text(duration_label(model.elapsed(Utc::now()))),
                cx,
            ))
            .child(properties::row(
                labels::subject_property(info.kind),
                div()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .font_family(theme.mono_font_family.clone())
                    .child(reference),
                cx,
            ));
        let agent = properties::group(cx)
            .child(properties::row(
                labels::PROPERTY_AGENT,
                text(info.agent_label),
                cx,
            ))
            .child(properties::row(labels::PROPERTY_MODEL, model_value, cx))
            .children(
                permission_value
                    .map(|value| properties::row(labels::PROPERTY_PERMISSION, value, cx)),
            )
            .children(hint)
            .children(turn_hint);
        let findings = properties::group(cx).child(properties::row(
            labels::PROPERTY_FINDINGS,
            severity_pills(model.counts(), cx),
            cx,
        ));
        properties::panel(cx)
            .id("agent-session-properties")
            .overflow_y_scroll()
            .child(session)
            .child(agent)
            .child(findings)
            .child(div().flex_1())
            .children(self.actions(cx))
    }

    fn actions(&self, cx: &App) -> Option<Div> {
        let session_controls = self.session_controls(cx);
        let notice = self.model.read(cx).notice().cloned().map(|notice| {
            h_flex()
                .gap_1p5()
                .items_start()
                .text_xs()
                .text_color(cx.theme().warning)
                .child(Icon::new(IconName::TriangleAlert).xsmall())
                .child(div().flex_1().min_w_0().child(notice))
        });
        let buttons = controls(&session_controls, "thread", cx);
        (notice.is_some() || buttons.is_some()).then(|| {
            v_flex()
                .flex_none()
                .gap_2()
                .children(notice)
                .children(buttons)
        })
    }
}

fn text(value: impl Into<SharedString>) -> AnyElement {
    div()
        .min_w_0()
        .truncate()
        .child(value.into())
        .into_any_element()
}

fn note(value: &'static str, color: gpui_kit::Hsla) -> Div {
    div().w_full().text_xs().text_color(color).child(value)
}
