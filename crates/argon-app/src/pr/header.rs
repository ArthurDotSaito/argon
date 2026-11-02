use argon_core::{ChecksState, PrState, PullRequest, ReviewState};
use argon_github::{Label, PrDetail};
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    App, ClickEvent, Context, Div, Hsla, InteractiveElement as _, ParentElement as _, Rgba,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, div, hsla, px, rgb,
};

use super::labels;
use super::pr_view::{MergeState, PrView};
use super::timeline_item::review_style;
use crate::ui;

const META_AVATAR: f32 = 18.;
const AI_BUTTON_HEIGHT: f32 = 24.;
const BRANCH_MAX_WIDTH: f32 = 280.;
const LIGHT_LABEL_LUMINANCE: f32 = 0.6;

pub struct HeaderProps<'a> {
    pub pr: &'a PullRequest,
    pub detail: Option<&'a PrDetail>,
    pub can_merge: bool,
    pub merge_state: &'a MergeState,
    pub refreshing: bool,
    pub now: DateTime<Utc>,
}

pub fn header(props: HeaderProps<'_>, cx: &mut Context<PrView>) -> Div {
    let theme = cx.theme();
    let pr = props.pr;
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
        .pt_3()
        .pb_3()
        .gap_2()
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(breadcrumb(pr, cx))
                .child(div().flex_1())
                .child(actions(&props, cx)),
        )
        .child(title)
        .child(meta(
            pr,
            matches!(props.merge_state, MergeState::Merged),
            props.now,
            cx,
        ))
        .child(stats(pr, props.detail, cx))
        .children(status)
}

fn breadcrumb(pr: &PullRequest, cx: &App) -> Div {
    let theme = cx.theme();
    let (owner, repo) = pr.repo.split_once('/').unwrap_or(("", pr.repo.as_str()));
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
                .child(format!("#{}", pr.number)),
        )
}

fn actions(props: &HeaderProps<'_>, cx: &mut Context<PrView>) -> Div {
    let merging = matches!(props.merge_state, MergeState::Merging);
    let open = Button::new("pr-open-github")
        .icon(Icon::new(IconName::ExternalLink))
        .label(labels::OPEN_ON_GITHUB)
        .small()
        .ghost()
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_on_github(cx)));
    let refresh = Button::new("pr-refresh")
        .icon(Icon::new(IconName::RefreshCw))
        .small()
        .ghost()
        .loading(props.refreshing)
        .tooltip(labels::REFRESH)
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reload(cx)));
    let merge = props.can_merge.then(|| {
        Button::new("pr-merge")
            .icon(Icon::new(IconName::GitMerge))
            .label(labels::MERGE)
            .small()
            .success()
            .loading(merging)
            .on_click(
                cx.listener(|this, _: &ClickEvent, window, cx| this.open_merge_dialog(window, cx)),
            )
    });
    let ai_review = ai_review_button(cx);
    h_flex()
        .gap_1()
        .items_center()
        .child(open)
        .child(refresh)
        .children(merge)
        .child(ai_review)
}

fn ai_review_button(cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    h_flex()
        .id("pr-ai-review")
        .h(px(AI_BUTTON_HEIGHT))
        .px_2p5()
        .gap_1p5()
        .items_center()
        .rounded(theme.radius)
        .bg(theme.primary.opacity(0.7))
        .text_sm()
        .font_medium()
        .text_color(theme.primary_foreground.opacity(0.85))
        .cursor_not_allowed()
        .tooltip(|window, cx| Tooltip::new(labels::AI_REVIEW_SOON).build(window, cx))
        .child(Icon::new(IconName::Sparkles).small())
        .child(labels::AI_REVIEW)
}

fn meta(pr: &PullRequest, merged: bool, now: DateTime<Utc>, cx: &App) -> Div {
    let theme = cx.theme();
    let style = if merged {
        ui::state_style_for(PrState::Merged, false, cx)
    } else {
        ui::state_style(pr, cx)
    };
    let pill = h_flex()
        .gap_1()
        .px_2()
        .h(px(22.))
        .items_center()
        .rounded_full()
        .bg(style.color.opacity(0.16))
        .border_1()
        .border_color(style.color.opacity(0.5))
        .text_xs()
        .font_semibold()
        .text_color(style.color)
        .child(Icon::new(style.icon).xsmall())
        .child(style.label);
    h_flex()
        .w_full()
        .flex_wrap()
        .gap_2()
        .items_center()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(pill)
        .child(ui::avatar(&pr.author, META_AVATAR))
        .child(
            div()
                .font_semibold()
                .text_color(theme.foreground)
                .child(pr.author.clone()),
        )
        .child(format!("· aberto {}", ui::ago(now, pr.created_at)))
        .child("·")
        .child(branch("pr-copy-base", &pr.base, cx))
        .child(div().text_color(theme.muted_foreground).child("←"))
        .child(branch("pr-copy-head", &pr.head, cx))
}

fn branch(id: &'static str, name: &str, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .gap_1()
        .px_1p5()
        .h(px(22.))
        .items_center()
        .rounded(theme.radius)
        .bg(theme.muted)
        .border_1()
        .border_color(theme.border)
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .text_color(theme.foreground)
        .child(
            Icon::new(IconName::GitBranch)
                .xsmall()
                .text_color(theme.muted_foreground),
        )
        .child(
            div()
                .max_w(px(BRANCH_MAX_WIDTH))
                .truncate()
                .child(name.to_string()),
        )
        .child(
            Clipboard::new(id)
                .value(SharedString::from(name.to_string()))
                .tooltip(labels::COPY_BRANCH)
                .xsmall(),
        )
}

fn stats(pr: &PullRequest, detail: Option<&PrDetail>, cx: &App) -> Div {
    let theme = cx.theme();
    let diff = h_flex()
        .gap_1p5()
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .child(
            div()
                .text_color(theme.success)
                .child(format!("+{}", pr.additions)),
        )
        .child(
            div()
                .text_color(theme.danger)
                .child(format!("−{}", pr.deletions)),
        );
    let files = h_flex()
        .gap_1()
        .items_center()
        .child(Icon::new(IconName::File).xsmall())
        .child(labels::changed_files(pr.changed_files));
    let labels_row = detail
        .filter(|detail| !detail.labels.is_empty())
        .map(|detail| label_list(&detail.labels, cx));
    h_flex()
        .w_full()
        .flex_wrap()
        .gap_x_4()
        .gap_y_1p5()
        .items_center()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(checks_summary(pr.checks, cx))
        .child(diff)
        .child(files)
        .child(reviewers(pr, detail, cx))
        .children(labels_row)
}

fn checks_summary(checks: Option<ChecksState>, cx: &App) -> Div {
    let theme = cx.theme();
    let (icon, color, text) = match checks {
        Some(ChecksState::Success) => (IconName::CircleCheck, theme.success, "CI ok"),
        Some(ChecksState::Failure | ChecksState::Error) => {
            (IconName::CircleX, theme.danger, "CI falhou")
        }
        Some(ChecksState::Pending) => (IconName::LoaderCircle, theme.warning, "CI em andamento"),
        None => (IconName::Minus, theme.muted_foreground, "sem CI"),
    };
    h_flex()
        .gap_1()
        .items_center()
        .child(Icon::new(icon).xsmall().text_color(color))
        .child(text)
}

struct ReviewerEntry {
    login: String,
    state: Option<ReviewState>,
    requested: bool,
}

fn reviewer_entries(pr: &PullRequest, detail: Option<&PrDetail>) -> Vec<ReviewerEntry> {
    if let Some(detail) = detail {
        return detail
            .reviewers
            .iter()
            .map(|reviewer| ReviewerEntry {
                login: reviewer.login.clone(),
                state: reviewer.state,
                requested: reviewer.requested,
            })
            .collect();
    }
    let mut entries: Vec<ReviewerEntry> = pr
        .requested_reviewers
        .iter()
        .map(|login| ReviewerEntry {
            login: login.clone(),
            state: None,
            requested: true,
        })
        .collect();
    for review in &pr.latest_reviews {
        if !entries
            .iter()
            .any(|entry| entry.login.eq_ignore_ascii_case(&review.author))
        {
            entries.push(ReviewerEntry {
                login: review.author.clone(),
                state: Some(review.state),
                requested: false,
            });
        }
    }
    entries
}

fn reviewers(pr: &PullRequest, detail: Option<&PrDetail>, cx: &App) -> Div {
    let theme = cx.theme();
    let entries = reviewer_entries(pr, detail);
    let row = h_flex()
        .gap_2()
        .items_center()
        .child(Icon::new(IconName::Users).xsmall());
    if entries.is_empty() {
        return row.child(labels::NO_REVIEWERS);
    }
    row.children(entries.into_iter().enumerate().map(|(ix, entry)| {
        let (icon, color) = match labels::settled_review(entry.state, entry.requested) {
            Some(state) => review_style(state, cx),
            None => (IconName::CircleDashed, theme.warning),
        };
        let tooltip: SharedString =
            labels::reviewer_tooltip(&entry.login, entry.state, entry.requested).into();
        h_flex()
            .id(("pr-reviewer", ix))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .gap_1()
            .items_center()
            .child(Icon::new(icon).xsmall().text_color(color))
            .child(
                div()
                    .text_color(theme.foreground)
                    .child(entry.login.clone()),
            )
    }))
}

fn label_list(pr_labels: &[Label], cx: &App) -> Div {
    h_flex().gap_1().flex_wrap().items_center().children(
        pr_labels
            .iter()
            .map(|label| label_chip(&label.name, Some(label.color.as_str()), cx)),
    )
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
