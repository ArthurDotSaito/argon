use argon_agents::Support;
use argon_agents::defaults::agent_defaults;
use argon_core::PermissionMode;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::select::Select;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::{
    Context, Div, Hsla, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window,
    div,
};

use super::fields::env_summary;
use super::form::{AgentForm, Status};
use crate::agents::{CardState, FULL_ACCESS_RISK};
use crate::settings::labels;
use crate::settings::ui::{card, field, indented, mono, note};

impl AgentForm {
    fn status_line(&self, cx: &Context<Self>) -> (SharedString, Hsla) {
        let theme = cx.theme();
        if let Support::NotYet { reason } = &self.support {
            return (reason.clone().into(), theme.muted_foreground);
        }
        match &self.status {
            Status::Detecting => (labels::DETECTING.into(), theme.muted_foreground),
            Status::Failed(error) => (error.clone(), theme.danger),
            Status::Done(state) => match state {
                CardState::Ready { version } => (labels::found(version).into(), theme.success),
                CardState::NeedsLogin { hint } => (hint.clone().into(), theme.warning),
                CardState::Missing => (labels::NOT_FOUND.into(), theme.warning),
                CardState::Unsupported { reason } => {
                    (reason.clone().into(), theme.muted_foreground)
                }
                CardState::Failed { message } => {
                    (labels::detection_failed(message).into(), theme.danger)
                }
            },
        }
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        let (status, color) = self.status_line(cx);
        let testing = matches!(self.status, Status::Detecting);
        let test = self.is_supported().then(|| {
            Button::new(("settings-agent-test", self.kind as usize))
                .icon(Icon::new(IconName::Play))
                .label(labels::TEST)
                .xsmall()
                .outline()
                .loading(testing)
                .disabled(testing)
                .on_click(cx.listener(|this, _, window, cx| this.test(window, cx)))
        });
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().font_semibold().child(self.label))
                    .child(note(status, color)),
            )
            .children(test)
    }

    fn env_rows(&self, cx: &mut Context<Self>) -> Div {
        let rows: Vec<Div> = self
            .env
            .iter()
            .enumerate()
            .map(|(ix, row)| {
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(div().w_48().child(Input::new(&row.key).small()))
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&row.value).small().mask_toggle()),
                    )
                    .child(
                        Button::new(("settings-env-remove", ix))
                            .icon(Icon::new(IconName::Trash))
                            .xsmall()
                            .ghost()
                            .tooltip(labels::REMOVE)
                            .on_click(cx.listener(move |this, _, _, cx| this.remove_env(ix, cx))),
                    )
            })
            .collect();
        let add = Button::new(("settings-env-add", self.kind as usize))
            .icon(Icon::new(IconName::Plus))
            .label(labels::ADD_ENV)
            .xsmall()
            .ghost()
            .on_click(cx.listener(|this, _, window, cx| {
                this.push_env(String::new(), String::new(), window, cx)
            }));
        v_flex().gap_1().children(rows).child(h_flex().child(add))
    }

    fn permission_mode_control(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let risk = (self.permission_mode == PermissionMode::FullAccess)
            .then(|| note(FULL_ACCESS_RISK, theme.warning));
        v_flex()
            .gap_1()
            .child(
                div()
                    .w_64()
                    .child(Select::new(&self.permission_select).small()),
            )
            .child(note(
                labels::permission_mode_hint(self.permission_mode),
                theme.muted_foreground,
            ))
            .children(risk)
    }

    fn use_acp_control(&self, cx: &mut Context<Self>) -> Div {
        let toggle = Switch::new(("settings-agent-use-acp", self.kind as usize))
            .checked(self.use_acp)
            .small()
            .on_click(cx.listener(|this, on: &bool, _, cx| {
                this.use_acp = *on;
                cx.notify();
            }));
        v_flex()
            .gap_1()
            .child(h_flex().child(toggle))
            .child(note(labels::USE_ACP_NOTE, cx.theme().muted_foreground))
    }

    fn actions(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let dirty = self.is_dirty(cx);
        let invalid = self.draft(cx).err().map(|error| error.message());
        let message = self
            .error
            .clone()
            .or(invalid.map(SharedString::from))
            .map(|error| note(error, theme.danger))
            .or_else(|| dirty.then(|| note(labels::UNSAVED, theme.warning)));
        h_flex()
            .gap_2()
            .items_center()
            .child(
                Button::new(("settings-agent-save", self.kind as usize))
                    .label(labels::SAVE)
                    .small()
                    .primary()
                    .disabled(!dirty)
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            )
            .child(
                Button::new(("settings-agent-discard", self.kind as usize))
                    .label(labels::DISCARD)
                    .small()
                    .ghost()
                    .disabled(!dirty)
                    .on_click(cx.listener(|this, _, window, cx| this.discard(window, cx))),
            )
            .children(message)
    }
}

impl Render for AgentForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.header(cx);
        if !self.is_supported() {
            return card(cx).child(header);
        }
        let theme = cx.theme();
        let detected = self.detected.as_ref().map(|spec| {
            let env = (!spec.env.is_empty())
                .then(|| mono(format!("env  {}", env_summary(&spec.env)), cx));
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .text_color(theme.muted_foreground)
                .child(mono(
                    format!("{} {}", labels::DETECTED_AT, spec.command.display()),
                    cx,
                ))
                .children(env)
        });
        let model = Select::new(&self.model_select).small();
        let use_acp = agent_defaults(self.kind)
            .acp_fallback
            .is_some()
            .then(|| field(labels::USE_ACP, self.use_acp_control(cx), cx));
        card(cx)
            .child(header)
            .child(field(
                labels::COMMAND,
                Input::new(&self.command).small(),
                cx,
            ))
            .children(detected.map(indented))
            .child(field(labels::ARGS, Input::new(&self.args).small(), cx))
            .child(field(labels::ENV, self.env_rows(cx), cx))
            .child(field(labels::DEFAULT_MODEL, model, cx))
            .child(field(
                labels::PERMISSION_MODE,
                self.permission_mode_control(cx),
                cx,
            ))
            .children(use_acp)
            .child(self.actions(cx))
    }
}
