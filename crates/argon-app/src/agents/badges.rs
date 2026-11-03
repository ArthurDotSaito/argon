use argon_core::Severity;
use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex};
use gpui_kit::{AnyElement, App, Div, Hsla, IntoElement, ParentElement as _, Styled as _, div};

use super::card_status::CardStatus;
use super::finding_set::SeverityCounts;
use super::panel_labels as labels;

pub fn status_color(status: CardStatus, cx: &App) -> Hsla {
    let theme = cx.theme();
    match status {
        CardStatus::Running => theme.info,
        CardStatus::Awaiting => theme.warning,
        CardStatus::Completed => theme.success,
        CardStatus::Failed => theme.danger,
    }
}

pub fn status_icon(status: CardStatus, cx: &App) -> AnyElement {
    let color = status_color(status, cx);
    let icon = match status {
        CardStatus::Running => {
            return Spinner::new()
                .icon(Icon::new(IconName::LoaderCircle))
                .color(color)
                .xsmall()
                .into_any_element();
        }
        CardStatus::Awaiting => IconName::Hand,
        CardStatus::Completed => IconName::CircleCheck,
        CardStatus::Failed => IconName::Ban,
    };
    Icon::new(icon)
        .xsmall()
        .text_color(color)
        .into_any_element()
}

pub fn status_chip(status: CardStatus, cx: &App) -> Div {
    h_flex()
        .gap_1()
        .items_center()
        .text_xs()
        .font_semibold()
        .text_color(status_color(status, cx))
        .child(status_icon(status, cx))
        .child(labels::status_label(status))
}

pub fn severity_color(severity: Severity, cx: &App) -> Hsla {
    let theme = cx.theme();
    match severity {
        Severity::High => theme.danger,
        Severity::Medium => theme.warning,
        Severity::Low => theme.info,
    }
}

pub fn severity_icon(severity: Severity) -> IconName {
    match severity {
        Severity::High => IconName::TriangleAlert,
        Severity::Medium => IconName::CircleAlert,
        Severity::Low => IconName::Info,
    }
}

pub fn severity_pill(severity: Severity, cx: &App) -> Div {
    let color = severity_color(severity, cx);
    h_flex()
        .flex_none()
        .gap_1()
        .items_center()
        .px_1p5()
        .rounded(cx.theme().radius)
        .bg(color.opacity(0.15))
        .text_color(color)
        .text_xs()
        .font_semibold()
        .child(Icon::new(severity_icon(severity)).xsmall())
        .child(labels::severity_label(severity))
}

pub fn severity_counts(counts: SeverityCounts, cx: &App) -> Div {
    let entries = [
        (Severity::High, counts.high),
        (Severity::Medium, counts.medium),
        (Severity::Low, counts.low),
    ];
    h_flex()
        .gap_1p5()
        .children(entries.map(|(severity, count)| {
            div()
                .text_xs()
                .font_family(cx.theme().mono_font_family.clone())
                .text_color(severity_color(severity, cx))
                .child(format!("{} {count}", labels::severity_short(severity)))
        }))
}
