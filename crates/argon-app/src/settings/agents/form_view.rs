use argon_agents::defaults::agent_defaults;
use argon_agents::{Support, outdated_version};
use std::time::SystemTime;

use argon_agents::AccountUsage;
use argon_core::PermissionMode;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::select::Select;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::{
    App, Context, Div, Hsla, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Window, div, prelude::FluentBuilder as _, px,
};

use super::fields::env_summary;
use super::form::{AgentForm, Status};
use crate::agents::{
    AgentSessions, CardState, FULL_ACCESS_RISK, Tone, UsageDetail, UsageLine, UsageState,
    WindowRow, detail, needs_consent, open_usage_url, tone_color,
};
use crate::settings::labels;
use crate::settings::ui::{card, field, indented, mono, note};

const BAR_WIDTH: f32 = 160.;
const LABEL_COLUMN: f32 = 72.;
const PERCENT_COLUMN: f32 = 36.;

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
                CardState::Ready { version } => match outdated_version(self.kind, version) {
                    Some(tested) => (
                        labels::found_outdated(version, &tested).into(),
                        theme.warning,
                    ),
                    None => (labels::found(version).into(), theme.success),
                },
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

    fn usage_rows(&self, rows: Vec<WindowRow>, cx: &App) -> Div {
        let theme = cx.theme();
        let kind = self.kind as usize;
        let rows: Vec<Div> = rows
            .into_iter()
            .enumerate()
            .map(|(ix, row)| {
                let color = tone_color(row.tone, cx);
                let bar = Progress::new(SharedString::from(format!("usage-bar-{kind}-{ix}")))
                    .value(row.fraction * 100.)
                    .xsmall()
                    .w(px(BAR_WIDTH))
                    .when(row.tone != Tone::Muted, |bar| bar.color(color));
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_none()
                            .w(px(LABEL_COLUMN))
                            .text_xs()
                            .text_color(theme.foreground)
                            .child(row.label),
                    )
                    .child(bar)
                    .child(
                        div()
                            .flex_none()
                            .w(px(PERCENT_COLUMN))
                            .text_xs()
                            .text_color(color)
                            .child(format!("{}%", row.percent)),
                    )
                    .children(
                        row.renews
                            .map(|renews| note(renews, theme.muted_foreground)),
                    )
            })
            .collect();
        v_flex().gap_1().children(rows)
    }

    fn usage_status(&self, line: UsageLine, cx: &App) -> Div {
        match line.link {
            Some(url) => h_flex().child(
                Button::new(("settings-agent-usage-link", self.kind as usize))
                    .label(line.text)
                    .xsmall()
                    .link()
                    .on_click(move |_, _, _| open_usage_url(&url)),
            ),
            None => div().child(note(line.text, tone_color(line.tone, cx))),
        }
    }

    fn plan_consent_control(&self, cx: &mut Context<Self>) -> Div {
        let toggle = Switch::new(("settings-agent-read-plan-usage", self.kind as usize))
            .checked(self.read_plan_usage)
            .small()
            .on_click(cx.listener(|this, on: &bool, _, cx| this.set_plan_usage_consent(*on, cx)));
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(toggle)
                    .child(div().text_sm().child(labels::READ_OPENCODE_LOGIN)),
            )
            .child(note(
                labels::READ_OPENCODE_LOGIN_NOTE,
                cx.theme().muted_foreground,
            ))
    }

    fn usage_block(&self, cx: &mut Context<Self>) -> Option<Div> {
        let consent = needs_consent(self.kind).then(|| self.plan_consent_control(cx));
        let details = match &self.status {
            Status::Done(CardState::Ready { .. }) => self.usage_details(cx),
            Status::Detecting => None,
            Status::Done(_) | Status::Failed(_) => {
                Some(note(labels::USAGE_NEEDS_AGENT, cx.theme().muted_foreground))
            }
        };
        if details.is_none() && consent.is_none() {
            return None;
        }
        Some(v_flex().gap_2().children(details).children(consent))
    }

    fn usage_details(&self, cx: &mut Context<Self>) -> Option<Div> {
        let sessions = AgentSessions::global(cx)?;
        let (shown, refreshing, refreshable) = {
            let sessions = sessions.read(cx);
            let state = sessions.usage_state(self.kind, self.read_plan_usage);
            let refreshable = matches!(
                state,
                UsageState::Loading
                    | UsageState::Shown(AccountUsage::Ok { .. } | AccountUsage::Unavailable(_))
            );
            (
                detail(state, SystemTime::now()),
                sessions.usage_refreshing(self.kind),
                refreshable,
            )
        };
        let body = match shown? {
            UsageDetail::Windows { plan, rows } => v_flex()
                .gap_1()
                .child(div().text_sm().font_semibold().child(plan))
                .child(self.usage_rows(rows, cx)),
            UsageDetail::Line(line) => self.usage_status(line, cx),
        };
        let refresh = refreshable.then(|| {
            Button::new(("settings-agent-usage-refresh", self.kind as usize))
                .icon(Icon::new(IconName::RefreshCw))
                .label(labels::REFRESH_USAGE)
                .xsmall()
                .ghost()
                .loading(refreshing)
                .disabled(refreshing)
                .on_click(cx.listener(|this, _, _, cx| this.refresh_usage(cx)))
        });
        Some(
            h_flex()
                .gap_2()
                .items_start()
                .child(div().flex_1().min_w_0().child(body))
                .children(refresh),
        )
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
        let usage = self.usage_block(cx);
        let override_warning = self.override_warning(cx);
        let warning_color = cx.theme().warning;
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
            .children(override_warning.map(|text| indented(note(text, warning_color))))
            .child(field(labels::ENV, self.env_rows(cx), cx))
            .child(field(labels::DEFAULT_MODEL, model, cx))
            .child(field(
                labels::PERMISSION_MODE,
                self.permission_mode_control(cx),
                cx,
            ))
            .children(use_acp)
            .children(usage.map(|usage| field(labels::PLAN_AND_USAGE, usage, cx)))
            .child(self.actions(cx))
    }
}
