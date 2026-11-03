use argon_agents::ToolStatus;
use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div,
};

use super::ThreadView;
use crate::agents::panel_labels as labels;
use crate::agents::transcript::{ThoughtItem, ToolItem};

const OUTPUT_LIMIT: usize = 20_000;

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

fn link(id: SharedString, text: &'static str, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .text_xs()
        .text_color(cx.theme().link)
        .cursor_pointer()
        .child(text)
}

impl ThreadView {
    pub(super) fn thought(
        &self,
        ix: usize,
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
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(key.clone(), ix, cx))),
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

    pub(super) fn tool(&self, ix: usize, tool: &ToolItem, cx: &mut Context<Self>) -> AnyElement {
        let key: SharedString = format!("out:{}", tool.id).into();
        let expanded = self.is_expanded(&key);
        let theme = cx.theme();
        let title = if tool.title.is_empty() {
            labels::tool_kind(&tool.kind).to_owned()
        } else {
            tool.title.clone()
        };
        let toggle = tool.output.as_ref().map(|_| {
            let label = if expanded {
                labels::HIDE_OUTPUT
            } else {
                labels::SHOW_OUTPUT
            };
            let key = key.clone();
            link(
                SharedString::from(format!("tool-toggle-{}", tool.id)),
                label,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle(key.clone(), ix, cx)))
        });
        let output = tool.output.as_ref().filter(|_| expanded).map(|output| {
            div()
                .id(SharedString::from(format!("tool-output-{}", tool.id)))
                .max_h_64()
                .overflow_y_scroll()
                .p_1p5()
                .rounded(theme.radius)
                .bg(theme.secondary)
                .text_xs()
                .font_family(theme.mono_font_family.clone())
                .child(clipped(output))
        });
        v_flex()
            .gap_1()
            .px_1p5()
            .py_1()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .text_xs()
                    .font_family(theme.mono_font_family.clone())
                    .child(
                        Icon::new(kind_icon(&tool.kind))
                            .xsmall()
                            .text_color(theme.muted_foreground),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .children(toggle)
                    .child(status_icon(tool.status, cx)),
            )
            .children(output)
            .into_any_element()
    }

    pub(super) fn plan(&self, entries: &[String], cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        v_flex()
            .gap_1()
            .p_2()
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
