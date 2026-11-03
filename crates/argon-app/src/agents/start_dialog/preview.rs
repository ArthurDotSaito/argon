use std::ops::Range;

use gpui_kit::assets::IconName;
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, Div, IntoElement, ParentElement as _, SharedString, Styled as _, div,
    px, uniform_list,
};

use super::StartDialog;
use crate::agents::labels;

const PROMPT_HEIGHT: f32 = 210.;
const ROW_HEIGHT: f32 = 17.;
const KEY_WIDTH: f32 = 52.;

fn quote(arg: &str) -> String {
    if arg.contains(char::is_whitespace) {
        format!("\"{arg}\"")
    } else {
        arg.to_owned()
    }
}

fn pane_header(title: &'static str, trailing: impl IntoElement, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .w_full()
        .px_2()
        .h(px(26.))
        .gap_1p5()
        .items_center()
        .border_b_1()
        .border_color(theme.border)
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(Icon::new(IconName::SquareTerminal).xsmall())
        .child(
            div()
                .font_semibold()
                .text_color(theme.foreground)
                .child(title),
        )
        .child(div().flex_1())
        .child(trailing)
}

fn pane(cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .w_full()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .overflow_hidden()
}

fn key_value(key: &'static str, value: impl Into<SharedString>, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .gap_2()
        .child(
            div()
                .flex_none()
                .w(px(KEY_WIDTH))
                .text_color(theme.muted_foreground)
                .child(key),
        )
        .child(div().flex_1().min_w_0().truncate().child(value.into()))
}

impl StartDialog {
    pub(super) fn prompt_preview(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let lines = self
            .preview
            .as_ref()
            .map_or(0, |preview| preview.lines.len());
        let chars = self.preview.as_ref().map_or(0, |preview| preview.chars);
        let count = labels::char_count(chars, lines);
        let body: AnyElement = if lines == 0 {
            div()
                .size_full()
                .p_2()
                .text_color(theme.muted_foreground)
                .child(labels::BUILDING_CONTEXT)
                .into_any_element()
        } else {
            div()
                .relative()
                .size_full()
                .child(
                    uniform_list(
                        "ai-prompt-preview",
                        lines,
                        cx.processor(|this, range: Range<usize>, _, cx| {
                            this.preview_rows(range, cx)
                        }),
                    )
                    .track_scroll(&self.preview_scroll)
                    .size_full(),
                )
                .vertical_scrollbar(&self.preview_scroll)
                .into_any_element()
        };
        let diff_note = self.preview.as_ref().filter(|_| lines > 0).map(|preview| {
            h_flex()
                .gap_1p5()
                .items_center()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(Icon::new(IconName::FileDiff).xsmall())
                .child(if preview.diff_inline {
                    labels::DIFF_INLINE
                } else {
                    labels::DIFF_IN_WORKTREE
                })
        });
        v_flex()
            .w_full()
            .gap_1()
            .child(
                pane(cx)
                    .child(pane_header(labels::PROMPT_PREVIEW, count, cx))
                    .child(
                        div()
                            .h(px(PROMPT_HEIGHT))
                            .py_1()
                            .font_family(theme.mono_font_family.clone())
                            .text_xs()
                            .child(body),
                    ),
            )
            .children(diff_note)
    }

    fn preview_rows(&self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme();
        range
            .filter_map(|ix| self.preview.as_ref()?.lines.get(ix))
            .map(|line| {
                div()
                    .h(px(ROW_HEIGHT))
                    .px_2()
                    .whitespace_nowrap()
                    .truncate()
                    .text_color(theme.foreground)
                    .child(line.clone())
                    .into_any_element()
            })
            .collect()
    }

    pub(super) fn command_preview(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let Some(option) = self.selected_option() else {
            return pane(cx)
                .child(pane_header(labels::COMMAND_PREVIEW, div(), cx))
                .child(
                    div()
                        .p_2()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(labels::PICK_AGENT),
                );
        };
        let spec = &option.spec;
        let command: String = std::iter::once(quote(&spec.command.display().to_string()))
            .chain(spec.args.iter().map(|arg| quote(arg)))
            .collect::<Vec<_>>()
            .join(" ");
        let cwd = match (&self.session_id, Self::sessions(cx)) {
            (Ok(id), Some(sessions)) => sessions
                .read(cx)
                .worktrees()
                .planned_path(&self.pr.host, &self.pr.repo, self.pr.number, id)
                .map(|path| path.display().to_string())
                .unwrap_or_else(|error| error.to_string()),
            _ => String::new(),
        };
        let model = match (&self.model, self.model_label()) {
            (Some(id), Some(label)) if &label != id => format!("{id} · {label}"),
            (Some(id), _) => id.clone(),
            (None, _) => labels::no_models(spec.label),
        };
        let env = (!spec.env.is_empty()).then(|| {
            key_value(
                labels::ENV_LINE,
                spec.env.keys().cloned().collect::<Vec<_>>().join(", "),
                cx,
            )
        });
        let copy = Clipboard::new("ai-copy-command")
            .value(SharedString::from(command.clone()))
            .xsmall();
        pane(cx)
            .child(pane_header(labels::COMMAND_PREVIEW, copy, cx))
            .child(
                v_flex()
                    .p_2()
                    .gap_0p5()
                    .font_family(theme.mono_font_family.clone())
                    .text_xs()
                    .text_color(theme.foreground)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().text_color(theme.success).child("$"))
                            .child(div().flex_1().min_w_0().truncate().child(command)),
                    )
                    .child(key_value(labels::CWD, cwd, cx))
                    .child(key_value(labels::MODEL_LINE, model, cx))
                    .children(env),
            )
    }
}
