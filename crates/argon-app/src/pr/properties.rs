use argon_core::{PrState, PullRequest, Reason, ReviewState};
use argon_github::{Label, PrDetail};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, InteractiveElement as _, IntoElement as _,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, div,
    px,
};

use super::header::{ClosingLink, HeaderProps, label_chip, state_pill};
use super::labels;
use super::pr_view::{MergeState, PrView};
use super::timeline_item::review_style;
use crate::agents;
use crate::inbox::{reason_line, reason_tone};
use crate::issue::{self, destination};
use crate::ui::{self, properties};

const AVATAR_SIZE: f32 = 18.;
const AI_BUTTON_HEIGHT: f32 = 24.;
const BUTTON_GAP: f32 = 6.;
const VALUE_GAP: f32 = 6.;
const CHIP_HEIGHT: f32 = 22.;

pub fn panel(
    props: &HeaderProps<'_>,
    reason: Option<&Reason>,
    cx: &mut Context<PrView>,
) -> Stateful<Div> {
    let buttons = v_flex()
        .w_full()
        .flex_none()
        .gap(px(BUTTON_GAP))
        .children(action_buttons(props, true, true, cx));
    let closing = closing_row(props, cx);
    let summary = summary(props, reason, cx).children(closing);
    let review = review_group(props, cx);
    properties::panel(cx)
        .id("pr-properties")
        .overflow_y_scroll()
        .child(buttons)
        .child(summary)
        .child(review)
}

pub fn action_buttons(
    props: &HeaderProps<'_>,
    show_review: bool,
    full_width: bool,
    cx: &mut Context<PrView>,
) -> Vec<AnyElement> {
    let merging = matches!(props.merge_state, MergeState::Merging);
    let ai_review = ai_review_button(props.can_review_with_ai, full_width, cx);
    let review = (show_review && props.pr.state == PrState::Open).then(|| {
        Button::new("pr-review")
            .icon(Icon::new(IconName::FileDiff))
            .label(labels::REVIEW)
            .small()
            .outline()
            .when(full_width, |button| button.w_full())
            .tooltip(labels::REVIEW_TOOLTIP)
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.start_review(window, cx)))
            .into_any_element()
    });
    let merge = props.can_merge.then(|| {
        Button::new("pr-merge")
            .icon(Icon::new(IconName::GitMerge))
            .label(labels::MERGE)
            .small()
            .success()
            .when(full_width, |button| button.w_full())
            .loading(merging)
            .on_click(
                cx.listener(|this, _: &ClickEvent, window, cx| this.open_merge_dialog(window, cx)),
            )
            .into_any_element()
    });
    let open = Button::new("pr-open-github")
        .icon(Icon::new(IconName::ExternalLink))
        .label(labels::OPEN_ON_GITHUB)
        .small()
        .ghost()
        .when(full_width, |button| button.w_full())
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_on_github(cx)))
        .into_any_element();
    std::iter::once(ai_review)
        .chain(review)
        .chain(merge)
        .chain(std::iter::once(open))
        .collect()
}

fn ai_review_button(enabled: bool, full_width: bool, cx: &mut Context<PrView>) -> AnyElement {
    if enabled {
        return Button::new("pr-ai-review")
            .icon(Icon::new(IconName::Sparkles))
            .label(labels::AI_REVIEW)
            .small()
            .primary()
            .when(full_width, |button| button.w_full())
            .tooltip(agents::BUTTON_TOOLTIP)
            .on_click(
                cx.listener(|this, _: &ClickEvent, window, cx| this.open_ai_review(window, cx)),
            )
            .into_any_element();
    }
    let theme = cx.theme();
    h_flex()
        .id("pr-ai-review")
        .when(full_width, |this| this.w_full())
        .flex_none()
        .h(px(AI_BUTTON_HEIGHT))
        .px_2p5()
        .gap_1p5()
        .items_center()
        .justify_center()
        .rounded(theme.radius)
        .bg(theme.primary.opacity(0.7))
        .text_sm()
        .font_medium()
        .text_color(theme.primary_foreground.opacity(0.85))
        .cursor_not_allowed()
        .tooltip(|window, cx| Tooltip::new(agents::CLOSED_TOOLTIP).build(window, cx))
        .child(Icon::new(IconName::Sparkles).small())
        .child(labels::AI_REVIEW)
        .into_any_element()
}

fn merged(props: &HeaderProps<'_>) -> bool {
    props.pr.state == PrState::Merged || matches!(props.merge_state, MergeState::Merged)
}

fn summary(props: &HeaderProps<'_>, reason: Option<&Reason>, cx: &App) -> Div {
    let theme = cx.theme();
    let pr = props.pr;
    let style = if merged(props) {
        ui::state_style_for(PrState::Merged, false, cx)
    } else {
        ui::state_style(pr, cx)
    };
    let pill = state_pill(style.icon, style.color, style.label).flex_none();
    let state = values().child(pill).child(
        div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(ui::ago(props.now, pr.created_at)),
    );
    let reason_row = reason.map(|reason| {
        properties::row(
            labels::PROPERTY_REASON,
            div()
                .min_w_0()
                .font_medium()
                .text_color(ui::tone_color(reason_tone(reason), cx))
                .child(reason_line(reason, props.now, pr.updated_at)),
            cx,
        )
    });
    let author = values()
        .child(ui::avatar(&pr.author, AVATAR_SIZE))
        .child(div().min_w_0().truncate().child(pr.author.clone()));
    let branches = values()
        .child(branch("pr-copy-head", &pr.head, cx))
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(labels::BRANCH_JOINER),
        )
        .child(branch("pr-copy-base", &pr.base, cx));
    properties::group(cx)
        .child(properties::row(labels::PROPERTY_STATE, state, cx))
        .children(reason_row)
        .child(properties::row(labels::PROPERTY_AUTHOR, author, cx))
        .child(properties::row(labels::PROPERTY_BRANCHES, branches, cx))
}

fn review_group(props: &HeaderProps<'_>, cx: &App) -> Div {
    let theme = cx.theme();
    let pr = props.pr;
    let changes = values()
        .child(ui::diff_stat(pr.additions, pr.deletions, cx))
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(labels::in_changed_files(pr.changed_files)),
        );
    let labels_row = props
        .detail
        .filter(|detail| !detail.labels.is_empty())
        .map(|detail| properties::row(labels::PROPERTY_LABELS, label_list(&detail.labels, cx), cx));
    properties::group(cx)
        .child(properties::row(
            labels::PROPERTY_CI,
            ui::checks_label(pr.checks, cx),
            cx,
        ))
        .children(reviewer_rows(pr, props.detail, cx))
        .child(properties::row(labels::PROPERTY_CHANGES, changes, cx))
        .children(labels_row)
}

fn values() -> Div {
    h_flex()
        .w_full()
        .min_w_0()
        .flex_wrap()
        .items_center()
        .gap(px(VALUE_GAP))
}

fn closing_row(props: &HeaderProps<'_>, cx: &mut Context<PrView>) -> Option<Div> {
    let pr = props.pr;
    let merged = merged(props);
    let abandoned = pr.state == PrState::Closed && !merged;
    if abandoned || props.closing.is_empty() {
        return None;
    }
    let chips: Vec<_> = props
        .closing
        .iter()
        .enumerate()
        .map(|(ix, link)| closing_chip(ix, &pr.repo, merged, link, cx))
        .collect();
    Some(properties::row(
        labels::PROPERTY_ISSUES,
        values().children(chips),
        cx,
    ))
}

fn closing_chip(
    ix: usize,
    pr_repo: &str,
    merged: bool,
    link: &ClosingLink,
    cx: &mut Context<PrView>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (icon, color) = match link.status {
        Some(status) => issue::status_style(status, cx),
        None => (IconName::CircleDot, theme.muted_foreground),
    };
    let target = link.target.clone();
    h_flex()
        .id(("pr-closing-issue", ix))
        .max_w_full()
        .min_w_0()
        .gap_1()
        .px_2()
        .h(px(CHIP_HEIGHT))
        .items_center()
        .rounded_full()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .text_xs()
        .text_color(theme.foreground)
        .cursor_pointer()
        .hover(|style| style.border_color(theme.primary))
        .tooltip(|window, cx| Tooltip::new(labels::OPEN_ISSUE).build(window, cx))
        .child(Icon::new(icon).xsmall().text_color(color))
        .child(div().min_w_0().truncate().child(labels::closing_issue(
            &link.issue,
            pr_repo,
            merged,
        )))
        .on_click(
            cx.listener(move |_, _: &ClickEvent, window, cx| {
                destination::open(&target, window, cx)
            }),
        )
}

fn branch(id: &'static str, name: &str, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .max_w_full()
        .min_w_0()
        .gap_1()
        .px_1p5()
        .h(px(CHIP_HEIGHT))
        .items_center()
        .rounded(theme.radius)
        .bg(theme.background)
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
        .child(div().min_w_0().truncate().child(name.to_string()))
        .child(
            Clipboard::new(id)
                .value(SharedString::from(name.to_string()))
                .tooltip(labels::COPY_BRANCH)
                .xsmall(),
        )
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

fn reviewer_rows(pr: &PullRequest, detail: Option<&PrDetail>, cx: &App) -> Vec<Div> {
    let theme = cx.theme();
    let entries = reviewer_entries(pr, detail);
    if entries.is_empty() {
        return vec![properties::row(
            labels::PROPERTY_REVIEWERS,
            div()
                .text_color(theme.muted_foreground)
                .child(labels::NO_REVIEWERS),
            cx,
        )];
    }
    entries
        .into_iter()
        .enumerate()
        .map(|(ix, entry)| {
            let color = match labels::settled_review(entry.state, entry.requested) {
                Some(state) => review_style(state, cx).1,
                None => theme.muted_foreground,
            };
            let value = h_flex()
                .flex_1()
                .min_w_0()
                .gap_1p5()
                .items_center()
                .child(ui::avatar(&entry.login, AVATAR_SIZE))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(entry.login.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(color)
                        .child(labels::reviewer_status(entry.state, entry.requested)),
                );
            let label = if ix == 0 {
                labels::PROPERTY_REVIEWERS
            } else {
                ""
            };
            properties::row(label, value, cx)
        })
        .collect()
}

fn label_list(pr_labels: &[Label], cx: &App) -> Div {
    h_flex().gap_1().flex_wrap().items_center().children(
        pr_labels
            .iter()
            .map(|label| label_chip(&label.name, Some(label.color.as_str()), cx)),
    )
}
