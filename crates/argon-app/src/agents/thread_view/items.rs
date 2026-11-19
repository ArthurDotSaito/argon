use std::ops::Range;

use gpui_kit::assets::IconName;
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::{
    CONTENT_PADDING_X, ITEM_GAP, RADIUS, TRANSCRIPT_MAX_WIDTH, TRANSCRIPT_PADDING_Y, ThreadView,
};
use crate::agents::card_status::login_hint;
use crate::agents::finding_card::findings_block;
use crate::agents::message_text::hide_output_blocks;
use crate::agents::panel_labels as labels;
use crate::agents::permission_card::permission_card;
use crate::agents::safe_markdown::agent_markdown;
use crate::agents::transcript::Item;

const USER_PREVIEW_CHARS: usize = 480;
const CURSOR: &str = " ▍";
const USER_INDENT: f32 = 48.;
const USER_PADDING_X: f32 = 12.;
const USER_PADDING_Y: f32 = 8.;
const USER_LABEL_GAP: f32 = 2.;
const TOOL_GROUP_LIMIT: usize = 25;

fn preview(text: &str, limit: usize) -> (String, bool) {
    match text.char_indices().nth(limit) {
        Some((cut, _)) => (format!("{}…", &text[..cut]), true),
        None => (text.to_owned(), false),
    }
}

pub(super) fn group_rows(items: &[Item]) -> Vec<Range<usize>> {
    let mut rows: Vec<Range<usize>> = Vec::new();
    for (ix, item) in items.iter().enumerate() {
        match rows.last_mut() {
            Some(row)
                if matches!(item, Item::Tool(_))
                    && matches!(items[row.start], Item::Tool(_))
                    && row.len() < TOOL_GROUP_LIMIT =>
            {
                row.end = ix + 1;
            }
            _ => rows.push(ix..ix + 1),
        }
    }
    rows
}

impl ThreadView {
    pub(super) fn render_row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(range) = self.rows.get(row).cloned() else {
            return self.status_row(cx);
        };
        let model = self.model.read(cx);
        let transcript = model.transcript();
        let last = transcript.items().len().saturating_sub(1);
        let ix = range.start;
        let streaming = ix == last && transcript.is_streaming();
        let open_thought = ix == last && transcript.in_turn();
        let Some(first) = transcript.items().get(ix) else {
            return div().into_any_element();
        };
        if matches!(first, Item::Tool(_)) {
            let lines = self.tool_lines(transcript.items().get(range).unwrap_or_default());
            return transcript_row(self.tool_table(row, lines, cx));
        }
        let item = first.clone();
        let weak = self.model.downgrade();
        let element = match item {
            Item::User(text) => self.user_message(ix, row, &text, cx),
            Item::Agent(text) => agent_message(ix, &text, streaming),
            Item::Thought(thought) => self.thought(ix, row, &thought, open_thought, cx),
            Item::Tool(_) => div().into_any_element(),
            Item::Plan(entries) => self.plan(&entries, cx),
            Item::Permission(permission) => {
                permission_card(weak, &permission, cx).into_any_element()
            }
            Item::Findings {
                ids,
                outside,
                unreadable,
            } => findings_block(weak, &ids, (outside, unreadable), self.model.read(cx), cx)
                .into_any_element(),
            Item::PlanDraft(id) => self.plan_card(id, cx),
            Item::Interrupted(reason) => self.notice(labels::interrupted(&reason), cx),
            Item::Error(message) => self.error(ix, &message, cx),
        };
        transcript_row(element)
    }

    fn status_row(&self, cx: &Context<Self>) -> AnyElement {
        let model = self.model.read(cx);
        let theme = cx.theme();
        let mut row = v_flex()
            .px(px(CONTENT_PADDING_X))
            .pb(px(TRANSCRIPT_PADDING_Y - ITEM_GAP))
            .gap_1();
        if model.is_loading() {
            row = row.child(status_line(
                labels::LOADING_TRANSCRIPT.into(),
                theme.muted_foreground,
            ));
        }
        if let Some(error) = model.load_error().cloned() {
            row = row.child(status_line(error, theme.danger));
        }
        if model.transcript().is_thinking() {
            row = row.child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(Spinner::new().xsmall().color(theme.muted_foreground))
                    .child(labels::THINKING),
            );
        }
        row.into_any_element()
    }

    fn user_message(
        &self,
        ix: usize,
        row: usize,
        text: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(key.clone(), row, cx)))
        });
        v_flex()
            .ml(px(USER_INDENT))
            .px(px(USER_PADDING_X))
            .py(px(USER_PADDING_Y))
            .gap(px(USER_LABEL_GAP))
            .rounded(px(RADIUS))
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
                .px_3()
                .py_2p5()
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
            .px_3()
            .py_2p5()
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

fn transcript_row(element: AnyElement) -> AnyElement {
    div()
        .px(px(CONTENT_PADDING_X))
        .pb(px(ITEM_GAP))
        .child(div().max_w(px(TRANSCRIPT_MAX_WIDTH)).child(element))
        .into_any_element()
}

fn agent_message(ix: usize, text: &str, streaming: bool) -> AnyElement {
    let text = hide_output_blocks(text);
    let body = if streaming {
        format!("{text}{CURSOR}")
    } else {
        text
    };
    div()
        .text_sm()
        .child(agent_markdown(
            SharedString::from(format!("agent-msg-{ix}")),
            &body,
        ))
        .into_any_element()
}

fn status_line(text: SharedString, color: gpui_kit::Hsla) -> AnyElement {
    div()
        .text_xs()
        .text_color(color)
        .child(text)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use argon_agents::ToolStatus;

    use super::*;
    use crate::agents::transcript::{PermissionItem, PermissionState, ToolItem};

    fn tool(n: usize) -> Item {
        Item::Tool(ToolItem {
            id: format!("tool-{n}"),
            title: String::new(),
            kind: "read".into(),
            status: ToolStatus::Completed,
            output: None,
        })
    }

    fn permission(id: &str) -> Item {
        Item::Permission(PermissionItem {
            request_id: id.into(),
            title: String::new(),
            kind: String::new(),
            options: Vec::new(),
            state: PermissionState::Pending,
        })
    }

    #[test]
    fn no_items_make_no_rows() {
        assert!(group_rows(&[]).is_empty());
    }

    #[test]
    fn consecutive_tools_share_a_row() {
        let items = [
            Item::User("hi".into()),
            tool(0),
            tool(1),
            Item::Agent("done".into()),
            tool(2),
        ];
        assert_eq!(group_rows(&items), vec![0..1, 1..3, 3..4, 4..5]);
    }

    #[test]
    fn only_tools_are_grouped() {
        let items = [permission("a"), permission("b")];
        assert_eq!(group_rows(&items), vec![0..1, 1..2]);
    }

    #[test]
    fn tool_groups_are_capped() {
        let items: Vec<Item> = (0..30).map(tool).collect();
        assert_eq!(
            group_rows(&items),
            vec![0..TOOL_GROUP_LIMIT, TOOL_GROUP_LIMIT..30]
        );
    }

    #[test]
    fn appending_items_only_touches_the_last_row() {
        let mut items: Vec<Item> = vec![Item::User("go".into())];
        items.extend((0..27).map(tool));
        items.push(Item::Agent("ok".into()));
        items.push(permission("p"));
        items.extend((27..30).map(tool));
        let all = group_rows(&items);
        for len in 0..items.len() {
            let rows = group_rows(&items[..len]);
            let settled = &rows[..rows.len().saturating_sub(1)];
            assert_eq!(settled, &all[..settled.len()], "prefix of {len} items");
            assert!(rows.len() <= group_rows(&items[..len + 1]).len());
        }
    }
}
