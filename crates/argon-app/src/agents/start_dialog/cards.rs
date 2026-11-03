use argon_agents::defaults::AGENT_DEFAULTS;
use argon_core::AgentKind;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::{
    ClickEvent, Context, Div, Hsla, InteractiveElement as _, ParentElement as _, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _, div, hsla, prelude::FluentBuilder as _,
    px,
};

use super::{Load, StartDialog, open_settings};
use crate::agents::detection::{AgentOption, CardState};
use crate::agents::labels;

const CARD_HEIGHT: f32 = 64.;
const GLYPH_SIZE: f32 = 26.;
const DISABLED_OPACITY: f32 = 0.55;

struct CardView {
    kind: AgentKind,
    label: &'static str,
    status: SharedString,
    tooltip: Option<SharedString>,
    selectable: bool,
    configurable: bool,
    warn: bool,
}

fn card_view(option: &AgentOption) -> CardView {
    let (status, tooltip, warn): (SharedString, Option<SharedString>, bool) = match &option.state {
        CardState::Ready { version } => (format!("v{version}").into(), None, false),
        CardState::NeedsLogin { hint } => (hint.clone().into(), Some(hint.clone().into()), true),
        CardState::Missing => (labels::NOT_FOUND.into(), None, false),
        CardState::Unsupported { reason } => (
            labels::NOT_SUPPORTED.into(),
            Some(reason.clone().into()),
            false,
        ),
        CardState::Failed { message } => {
            (message.clone().into(), Some(message.clone().into()), true)
        }
    };
    CardView {
        kind: option.spec.kind,
        label: option.spec.label,
        status,
        tooltip,
        selectable: option.state.is_selectable(),
        configurable: option.state == CardState::Missing,
        warn,
    }
}

fn glyph_color(kind: AgentKind) -> Hsla {
    match kind {
        AgentKind::ClaudeCode => hsla(18. / 360., 0.62, 0.56, 1.),
        AgentKind::Codex => hsla(0., 0., 0.72, 1.),
        AgentKind::OpenCode => hsla(205. / 360., 0.6, 0.58, 1.),
        AgentKind::CommandCode => hsla(272. / 360., 0.5, 0.64, 1.),
        AgentKind::Antigravity => hsla(145. / 360., 0.45, 0.5, 1.),
    }
}

fn glyph(kind: AgentKind, label: &str) -> Div {
    let color = glyph_color(kind);
    div()
        .flex_none()
        .size(px(GLYPH_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(color.opacity(0.18))
        .border_1()
        .border_color(color.opacity(0.5))
        .text_sm()
        .font_semibold()
        .text_color(color)
        .child(label.chars().next().unwrap_or('?').to_string())
}

impl StartDialog {
    pub(super) fn agent_cards(&self, cx: &mut Context<Self>) -> Div {
        let views: Vec<CardView> = match &self.agents {
            Load::Ready(options) => options.iter().map(card_view).collect(),
            Load::Loading | Load::Failed(_) => AGENT_DEFAULTS
                .iter()
                .map(|defaults| CardView {
                    kind: defaults.kind,
                    label: defaults.label,
                    status: labels::DETECTING.into(),
                    tooltip: None,
                    selectable: false,
                    configurable: false,
                    warn: false,
                })
                .collect(),
        };
        let error = match &self.agents {
            Load::Failed(error) => Some(
                div()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            ),
            Load::Loading | Load::Ready(_) => None,
        };
        let cards: Vec<Stateful<Div>> = views
            .into_iter()
            .enumerate()
            .map(|(ix, view)| self.card(ix, view, cx))
            .collect();
        v_flex()
            .gap_1p5()
            .child(h_flex().gap_2().children(cards))
            .children(error)
    }

    fn card(&self, ix: usize, view: CardView, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = cx.theme();
        let selected = self.agent == Some(view.kind);
        let border = if selected {
            theme.primary
        } else {
            theme.border
        };
        let status_color = if view.warn {
            theme.warning
        } else {
            theme.muted_foreground
        };
        let hover = theme.primary.opacity(0.7);
        let check = selected.then(|| Icon::new(IconName::Check).small().text_color(theme.primary));
        let configure = view.configurable.then(|| {
            Button::new(("agent-configure", ix))
                .label(labels::CONFIGURE)
                .xsmall()
                .ghost()
                .disabled(self.is_launching())
                .on_click(|_, window, cx| open_settings(window, cx))
        });
        let body = h_flex()
            .flex_1()
            .min_w_0()
            .gap_2()
            .items_center()
            .when(!view.selectable, |body| body.opacity(DISABLED_OPACITY))
            .child(glyph(view.kind, view.label))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(theme.foreground)
                            .truncate()
                            .child(view.label),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(status_color)
                            .line_clamp(2)
                            .child(view.status),
                    ),
            );
        let mut card = h_flex()
            .id(("agent-card", ix))
            .flex_1()
            .min_w_0()
            .h(px(CARD_HEIGHT))
            .px_2p5()
            .gap_1()
            .items_center()
            .rounded(theme.radius)
            .border_1()
            .border_color(border)
            .bg(if selected {
                theme.primary.opacity(0.1)
            } else {
                theme.muted.opacity(0.4)
            })
            .child(body)
            .children(check)
            .children(configure);
        if let Some(tooltip) = view.tooltip {
            card = card.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx));
        }
        if view.selectable {
            let kind = view.kind;
            card.cursor_pointer()
                .hover(move |style| style.border_color(hover))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.select_agent(kind, window, cx)
                }))
        } else {
            card.cursor_not_allowed()
        }
    }
}
