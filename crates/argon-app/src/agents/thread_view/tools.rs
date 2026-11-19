use argon_agents::ToolStatus;
use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::{RADIUS, ThreadView};
use crate::agents::panel_labels as labels;
use crate::agents::transcript::{Item, ThoughtItem, ToolItem};
use crate::theme::density;

const OUTPUT_LIMIT: usize = 20_000;
const TOOL_ROW_HEIGHT: f32 = 30.;
const TOOL_PADDING_X: f32 = 10.;
const TOOL_GAP: f32 = 10.;
const KIND_WIDTH: f32 = 64.;
const DIVIDER_OPACITY: f32 = 0.45;

fn kind_icon(kind: &str) -> IconName {
    match kind {
        "read" => IconName::FileText,
        "edit" => IconName::Pencil,
        "delete" => IconName::Trash,
        "move" => IconName::ArrowRight,
        "search" => IconName::Search,
        "execute" => IconName::Terminal,
        "think" => IconName::Brain,
        "fetch" => IconName::Globe,
        _ => IconName::Wrench,
    }
}

fn status_icon(status: ToolStatus, cx: &App) -> AnyElement {
    let theme = cx.theme();
    match status {
        ToolStatus::Pending => Icon::new(IconName::Circle)
            .xsmall()
            .text_color(theme.muted_foreground)
            .into_any_element(),
        ToolStatus::InProgress => Spinner::new()
            .icon(Icon::new(IconName::LoaderCircle))
            .xsmall()
            .color(theme.warning)
            .into_any_element(),
        ToolStatus::Completed => Icon::new(IconName::Check)
            .xsmall()
            .text_color(theme.success)
            .into_any_element(),
        ToolStatus::Failed => Icon::new(IconName::CircleX)
            .xsmall()
            .text_color(theme.danger)
            .into_any_element(),
    }
}

fn clipped(output: &str) -> String {
    match output.char_indices().nth(OUTPUT_LIMIT) {
        Some((cut, _)) => format!("{}\n…", &output[..cut]),
        None => output.to_owned(),
    }
}

pub(super) struct ToolLine {
    id: String,
    title: String,
    kind: String,
    status: ToolStatus,
    has_output: bool,
    output: Option<String>,
}

fn output_key(id: &str) -> SharedString {
    format!("out:{id}").into()
}

fn link(id: SharedString, text: &'static str, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .text_xs()
        .text_color(cx.theme().link)
        .cursor_pointer()
        .child(text)
}

impl ThreadView {
    pub(super) fn thought(
        &self,
        ix: usize,
        row: usize,
        thought: &ThoughtItem,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key: SharedString = format!("thought:{ix}").into();
        let expanded = self.is_expanded(&key);
        let title = match (open, thought.duration()) {
            (true, _) => labels::THINKING_NOW.to_owned(),
            (false, Some(duration)) => labels::thought_for(duration.as_secs().max(1)),
            (false, None) => labels::THOUGHT.to_owned(),
        };
        let theme = cx.theme();
        let chevron = if expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .id(SharedString::from(format!("thought-{ix}")))
                    .gap_1p5()
                    .items_center()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .cursor_pointer()
                    .child(Icon::new(chevron).xsmall())
                    .child(Icon::new(IconName::Brain).xsmall())
                    .child(title)
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(key.clone(), row, cx))),
            )
            .children(expanded.then(|| {
                div()
                    .ml_4()
                    .pl_2()
                    .border_l_2()
                    .border_color(theme.border)
                    .text_xs()
                    .italic()
                    .text_color(theme.muted_foreground)
                    .child(thought.text.clone())
            }))
            .into_any_element()
    }

    pub(super) fn tool_lines(&self, items: &[Item]) -> Vec<ToolLine> {
        items
            .iter()
            .filter_map(|item| match item {
                Item::Tool(tool) => Some(self.tool_line_data(tool)),
                _ => None,
            })
            .collect()
    }

    fn tool_line_data(&self, tool: &ToolItem) -> ToolLine {
        let expanded = self.is_expanded(&output_key(&tool.id));
        ToolLine {
            id: tool.id.clone(),
            title: tool.title.clone(),
            kind: tool.kind.clone(),
            status: tool.status,
            has_output: tool.output.is_some(),
            output: tool.output.as_deref().filter(|_| expanded).map(clipped),
        }
    }

    pub(super) fn tool_table(
        &self,
        row: usize,
        tools: Vec<ToolLine>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let lines: Vec<Div> = tools
            .into_iter()
            .enumerate()
            .map(|(n, tool)| self.tool_line(row, n == 0, tool, cx))
            .collect();
        v_flex()
            .rounded(px(RADIUS))
            .border_1()
            .border_color(cx.theme().border)
            .overflow_hidden()
            .children(lines)
            .into_any_element()
    }

    fn tool_line(&self, row: usize, first: bool, tool: ToolLine, cx: &mut Context<Self>) -> Div {
        let key = output_key(&tool.id);
        let expanded = tool.output.is_some();
        let toggle = tool.has_output.then(|| {
            let label = if expanded {
                labels::HIDE_OUTPUT
            } else {
                labels::SHOW_OUTPUT
            };
            link(
                SharedString::from(format!("tool-toggle-{}", tool.id)),
                label,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle(key.clone(), row, cx)))
        });
        let theme = cx.theme();
        let divider = theme.border.opacity(DIVIDER_OPACITY);
        let output = tool.output.map(|output| {
            div()
                .id(SharedString::from(format!("tool-output-{}", tool.id)))
                .max_h_64()
                .overflow_y_scroll()
                .px(px(TOOL_PADDING_X))
                .py_1p5()
                .border_t_1()
                .border_color(divider)
                .bg(theme.secondary)
                .text_xs()
                .font_family(theme.mono_font_family.clone())
                .child(output)
        });
        v_flex()
            .when(!first, |this| this.border_t_1().border_color(divider))
            .child(
                h_flex()
                    .h(px(density::scaled(TOOL_ROW_HEIGHT, cx)))
                    .px(px(TOOL_PADDING_X))
                    .gap(px(TOOL_GAP))
                    .items_center()
                    .text_xs()
                    .child(
                        Icon::new(kind_icon(&tool.kind))
                            .xsmall()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .w(px(KIND_WIDTH))
                            .flex_none()
                            .truncate()
                            .text_color(theme.muted_foreground)
                            .child(labels::tool_kind(&tool.kind)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(theme.mono_font_family.clone())
                            .child(tool.title),
                    )
                    .children(toggle)
                    .child(status_icon(tool.status, cx)),
            )
            .children(output)
    }

    pub(super) fn plan(&self, entries: &[String], cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        v_flex()
            .gap_1()
            .px_3()
            .py_2p5()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .text_xs()
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .font_semibold()
                    .child(Icon::new(IconName::ListChecks).xsmall())
                    .child(labels::PLAN),
            )
            .children(entries.iter().map(|entry| {
                h_flex()
                    .gap_1p5()
                    .items_start()
                    .child(
                        Icon::new(IconName::Square)
                            .xsmall()
                            .text_color(theme.muted_foreground),
                    )
                    .child(div().flex_1().min_w_0().child(entry.clone()))
            }))
            .into_any_element()
    }
}
