use argon_core::{IssueRef, PullRequest};
use argon_github::PrDetail;
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Hsla, ParentElement as _, Rgba, Styled as _, div,
    hsla, px, rgb,
};

use super::labels;
use super::pr_view::{MergeState, PrView};
use crate::issue::IssueStatus;
use crate::issue::destination::Destination;

const LIGHT_LABEL_LUMINANCE: f32 = 0.6;
const ACTION_GAP: f32 = 6.;

pub struct ClosingLink {
    pub issue: IssueRef,
    pub status: Option<IssueStatus>,
    pub target: Destination,
}

pub struct HeaderProps<'a> {
    pub pr: &'a PullRequest,
    pub detail: Option<&'a PrDetail>,
    pub closing: Vec<ClosingLink>,
    pub can_merge: bool,
    pub can_review_with_ai: bool,
    pub merge_state: &'a MergeState,
    pub refreshing: bool,
    pub now: DateTime<Utc>,
    pub overview_at: Option<DateTime<Utc>>,
}

pub fn header(props: &HeaderProps<'_>, actions: Vec<AnyElement>, cx: &mut Context<PrView>) -> Div {
    let pr = props.pr;
    let refresh = Button::new("pr-refresh")
        .icon(Icon::new(IconName::RefreshCw))
        .xsmall()
        .ghost()
        .loading(props.refreshing)
        .tooltip(labels::REFRESH)
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reload(cx)));
    let theme = cx.theme();
    let title = h_flex()
        .gap_2()
        .items_baseline()
        .child(
            div()
                .text_xl()
                .font_semibold()
                .text_color(theme.foreground)
                .child(pr.title.clone()),
        )
        .child(
            div()
                .text_lg()
                .text_color(theme.muted_foreground)
                .child(format!("#{}", pr.number)),
        );
    let status = match props.merge_state {
        MergeState::Idle | MergeState::Merged => None,
        MergeState::Merging => Some(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(labels::MERGING),
        ),
        MergeState::Failed(error) => Some(
            div()
                .text_sm()
                .text_color(theme.danger)
                .child(error.clone()),
        ),
    };
    v_flex()
        .w_full()
        .flex_none()
        .px_4()
        .py_3()
        .gap_1p5()
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(breadcrumb(&pr.repo, pr.number, cx))
                .child(div().flex_1())
                .children((!actions.is_empty()).then(|| {
                    h_flex()
                        .flex_none()
                        .gap(px(ACTION_GAP))
                        .items_center()
                        .children(actions)
                }))
                .child(refresh),
        )
        .child(title)
        .children(status)
}

pub fn breadcrumb(full_repo: &str, number: u64, cx: &App) -> Div {
    let theme = cx.theme();
    let (owner, repo) = full_repo.split_once('/').unwrap_or(("", full_repo));
    let separator = || {
        div()
            .text_color(theme.muted_foreground.opacity(0.6))
            .child("/")
    };
    h_flex()
        .gap_1p5()
        .items_center()
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(owner.to_string())
        .child(separator())
        .child(repo.to_string())
        .child(separator())
        .child(
            div()
                .text_color(theme.foreground)
                .child(format!("#{number}")),
        )
}

pub fn state_pill(icon: IconName, color: Hsla, label: &'static str) -> Div {
    h_flex()
        .gap_1()
        .px_2()
        .h(px(22.))
        .items_center()
        .rounded_full()
        .bg(color.opacity(0.16))
        .border_1()
        .border_color(color.opacity(0.5))
        .text_xs()
        .font_semibold()
        .text_color(color)
        .child(Icon::new(icon).xsmall())
        .child(label)
}

pub fn label_chip(name: &str, color: Option<&str>, cx: &App) -> Div {
    let theme = cx.theme();
    let (bg, fg) = match color.and_then(parse_hex) {
        Some(rgba) => (Hsla::from(rgba), contrast_text(rgba)),
        None => (theme.muted, theme.foreground),
    };
    div()
        .flex_none()
        .px_1p5()
        .h(px(18.))
        .flex()
        .items_center()
        .rounded_full()
        .bg(bg)
        .text_xs()
        .font_medium()
        .text_color(fg)
        .child(name.to_string())
}

pub fn label_color(color: &str) -> Option<Hsla> {
    parse_hex(color).map(Hsla::from)
}

fn parse_hex(color: &str) -> Option<Rgba> {
    let hex = color.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    u32::from_str_radix(hex, 16).ok().map(rgb)
}

fn contrast_text(color: Rgba) -> Hsla {
    let luminance = 0.299 * color.r + 0.587 * color.g + 0.114 * color.b;
    if luminance > LIGHT_LABEL_LUMINANCE {
        hsla(0., 0., 0.08, 1.)
    } else {
        hsla(0., 0., 1., 1.)
    }
}
