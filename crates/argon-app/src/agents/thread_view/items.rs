use gpui_kit::assets::IconName;
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div,
};

use super::ThreadView;
use crate::agents::card_status::login_hint;
use crate::agents::finding_card::findings_block;
use crate::agents::message_text::hide_findings_block;
use crate::agents::panel_labels as labels;
use crate::agents::permission_card::permission_card;
use crate::agents::transcript::Item;
use crate::pr::markdown;

const USER_PREVIEW_CHARS: usize = 480;
const CURSOR: &str = " ▍";

fn preview(text: &str, limit: usize) -> (String, bool) {
    match text.char_indices().nth(limit) {
        Some((cut, _)) => (format!("{}…", &text[..cut]), true),
        None => (text.to_owned(), false),
    }
}

impl ThreadView {
    pub(super) fn render_items(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let model = self.model.read(cx);
        let transcript = model.transcript().clone();
        let findings = model.findings().clone();
        let loading = model.is_loading();
        let load_error = model.load_error().cloned();
        let busy: Vec<i64> = findings_busy(&transcript, |id| model.is_finding_busy(id));
        let weak = self.model.downgrade();
        let items = transcript.items();
        let last = items.len().saturating_sub(1);
        let mut out = Vec::with_capacity(items.len() + 2);
        for (ix, item) in items.iter().enumerate() {
            let element = match item {
                Item::User(text) => self.user_message(ix, text, cx),
                Item::Agent(text) => {
                    let streaming = ix == last && transcript.is_streaming();
                    agent_message(ix, text, streaming)
                }
                Item::Thought(thought) => {
                    let open = ix == last && transcript.in_turn();
                    self.thought(ix, thought, open, cx)
                }
                Item::Tool(tool) => self.tool(tool, cx),
                Item::Plan(entries) => self.plan(entries, cx),
                Item::Permission(permission) => {
                    permission_card(weak.clone(), permission, cx).into_any_element()
                }
                Item::Findings { ids, skipped } => {
                    findings_block(weak.clone(), ids, *skipped, &findings, &busy, cx)
                        .into_any_element()
                }
                Item::Interrupted(reason) => self.notice(labels::interrupted(reason), cx),
                Item::Error(message) => self.error(ix, message, cx),
            };
            out.push(element);
        }
        let theme = cx.theme();
        if loading {
            out.push(status_line(
                labels::LOADING_TRANSCRIPT.into(),
                theme.muted_foreground,
            ));
        }
        if let Some(error) = load_error {
            out.push(status_line(error, theme.danger));
        }
        if transcript.is_thinking() {
            out.push(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(Spinner::new().xsmall().color(theme.muted_foreground))
                    .child(labels::THINKING)
                    .into_any_element(),
            );
        }
        out
    }

    fn user_message(&self, ix: usize, text: &str, cx: &mut Context<Self>) -> AnyElement {
        let key: SharedString = format!("user:{ix}").into();
        let expanded = self.is_expanded(&key);
        let (shown, truncated) = if expanded {
            (text.to_owned(), true)
        } else {
            preview(text, USER_PREVIEW_CHARS)
        };
        let theme = cx.theme();
        let toggle = truncated.then(|| {
            div()
                .id(SharedString::from(format!("user-toggle-{ix}")))
                .text_xs()
                .text_color(theme.link)
                .cursor_pointer()
                .child(if expanded {
                    labels::SHOW_LESS
                } else {
                    labels::SHOW_ALL
                })
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(key.clone(), cx)))
        });
        v_flex()
            .ml_6()
            .p_2()
            .gap_1()
            .rounded(theme.radius)
            .bg(theme.secondary)
            .text_sm()
            .child(div().child(shown))
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(labels::YOU)
                    .children(toggle),
            )
            .into_any_element()
    }

    fn notice(&self, text: String, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        h_flex()
            .gap_1p5()
            .items_center()
            .text_xs()
            .italic()
            .text_color(theme.muted_foreground)
            .child(Icon::new(IconName::Square).xsmall())
            .child(text)
            .into_any_element()
    }

    fn error(&self, ix: usize, message: &str, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        if let Some(hint) = login_hint(message) {
            return v_flex()
                .p_2()
                .gap_1p5()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.warning)
                .bg(theme.warning.opacity(0.08))
                .text_sm()
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .font_semibold()
                        .text_color(theme.warning)
                        .child(Icon::new(IconName::Lock).xsmall())
                        .child(labels::LOGIN_TITLE),
                )
                .child(div().child(hint.hint))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .px_1p5()
                                .rounded(theme.radius)
                                .bg(theme.secondary)
                                .font_family(theme.mono_font_family.clone())
                                .text_xs()
                                .child(hint.command.clone()),
                        )
                        .child(
                            Clipboard::new(SharedString::from(format!("agent-login-copy-{ix}")))
                                .value(SharedString::from(hint.command))
                                .tooltip(labels::COPY_COMMAND)
                                .xsmall(),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(labels::COPY_COMMAND),
                        ),
                )
                .into_any_element();
        }
        h_flex()
            .p_2()
            .gap_1p5()
            .items_start()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.danger.opacity(0.6))
            .bg(theme.danger.opacity(0.08))
            .text_xs()
            .text_color(theme.danger)
            .child(Icon::new(IconName::CircleX).xsmall())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_family(theme.mono_font_family.clone())
                    .child(message.to_owned()),
            )
            .into_any_element()
    }
}

fn findings_busy(
    transcript: &crate::agents::transcript::Transcript,
    is_busy: impl Fn(i64) -> bool,
) -> Vec<i64> {
    transcript
        .items()
        .iter()
        .filter_map(|item| match item {
            Item::Findings { ids, .. } => Some(ids.iter().copied()),
            _ => None,
        })
        .flatten()
        .filter(|id| is_busy(*id))
        .collect()
}

fn agent_message(ix: usize, text: &str, streaming: bool) -> AnyElement {
    let text = hide_findings_block(text);
    let body = if streaming {
        format!("{text}{CURSOR}")
    } else {
        text
    };
    div()
        .text_sm()
        .child(markdown(format!("agent-msg-{ix}").into(), body.into()))
        .into_any_element()
}

fn status_line(text: SharedString, color: gpui_kit::Hsla) -> AnyElement {
    div()
        .text_xs()
        .text_color(color)
        .child(text)
        .into_any_element()
}
