use argon_core::{PrState, PullRequest};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::{App, Hsla, Styled as _};

pub struct StateStyle {
    pub icon: IconName,
    pub color: Hsla,
    pub label: &'static str,
}

pub fn state_style(pr: &PullRequest, cx: &App) -> StateStyle {
    state_style_for(pr.state, pr.is_draft, cx)
}

pub fn state_style_for(state: PrState, is_draft: bool, cx: &App) -> StateStyle {
    let theme = cx.theme();
    let (icon, color, label) = match state {
        PrState::Open if is_draft => (
            IconName::GitPullRequestDraft,
            theme.muted_foreground,
            "Draft",
        ),
        PrState::Open => (IconName::GitPullRequest, theme.success, "Open"),
        PrState::Merged => (IconName::GitMerge, theme.magenta, "Merged"),
        PrState::Closed => (IconName::GitPullRequestClosed, theme.danger, "Closed"),
    };
    StateStyle { icon, color, label }
}

pub fn state_icon(pr: &PullRequest, cx: &App) -> Icon {
    let style = state_style(pr, cx);
    Icon::new(style.icon).small().text_color(style.color)
}
