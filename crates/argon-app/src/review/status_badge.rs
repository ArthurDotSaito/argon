use argon_github::FileStatus;
use gpui_kit::component::{ActiveTheme as _, StyledExt as _};
use gpui_kit::{App, Div, Hsla, ParentElement as _, Styled as _, div, px};

const BADGE_SIZE: f32 = 16.;

pub fn status_badge(status: FileStatus, cx: &App) -> Div {
    let theme = cx.theme();
    let (letter, color) = status_style(status, cx);
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
        .font_family(theme.mono_font_family.clone())
        .text_color(color)
        .child(letter)
}

fn status_style(status: FileStatus, cx: &App) -> (&'static str, Hsla) {
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
