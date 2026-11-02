use argon_github::{FileStatus, PrFile};
use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{App, Div, Hsla, ParentElement as _, Styled as _, div, px};

use crate::ui;

const BADGE_SIZE: f32 = 18.;
const DIFF_WIDTH: f32 = 96.;
const RENAME_ARROW: &str = "→";

pub fn commit_files_list(files: &[PrFile], cx: &App) -> Div {
    let theme = cx.theme();
    let rows: Vec<Div> = files
        .iter()
        .enumerate()
        .map(|(ix, file)| file_row(ix, file, cx))
        .collect();
    v_flex()
        .w_full()
        .overflow_hidden()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .children(rows)
}

fn file_row(ix: usize, file: &PrFile, cx: &App) -> Div {
    let theme = cx.theme();
    let (letter, color) = status_badge(file.status, cx);
    let path = match &file.previous_path {
        Some(previous) => format!("{previous} {RENAME_ARROW} {}", file.path),
        None => file.path.clone(),
    };
    let row = h_flex()
        .w_full()
        .min_w_0()
        .px_3()
        .py_1p5()
        .gap_2()
        .items_center()
        .child(
            div()
                .size(px(BADGE_SIZE))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme.radius)
                .bg(color.opacity(0.15))
                .text_xs()
                .font_semibold()
                .text_color(color)
                .child(letter),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_sm()
                .font_family(theme.mono_font_family.clone())
                .text_color(theme.foreground)
                .child(path),
        )
        .child(
            h_flex()
                .w(px(DIFF_WIDTH))
                .flex_none()
                .justify_end()
                .child(ui::diff_stat(file.additions, file.deletions, cx)),
        );
    if ix > 0 {
        row.border_t_1().border_color(theme.border)
    } else {
        row
    }
}

fn status_badge(status: FileStatus, cx: &App) -> (&'static str, Hsla) {
    let theme = cx.theme();
    match status {
        FileStatus::Added => ("A", theme.success),
        FileStatus::Removed => ("D", theme.danger),
        FileStatus::Renamed => ("R", theme.primary),
        FileStatus::Copied => ("C", theme.primary),
        FileStatus::Modified | FileStatus::Changed => ("M", theme.warning),
        FileStatus::Unchanged => ("·", theme.muted_foreground),
    }
}
