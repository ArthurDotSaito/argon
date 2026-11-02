use argon_core::Hunk;
use gpui_kit::assets::IconName;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, Div, ElementId, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::row::NUMBER_WIDTH;
use super::{DiffMode, DiffView, DiffViewEvent};
use crate::review::file_diff::{FileBody, FileMeta};
use crate::review::labels;
use crate::review::status_badge::status_badge;
use crate::ui;

pub(super) const FILE_HEADER_HEIGHT: f32 = 36.;
const HUNK_HEADER_HEIGHT: f32 = 24.;
const FILE_GAP: f32 = 12.;

pub(super) fn file_header(
    view: &Entity<DiffView>,
    this: &DiffView,
    ix: usize,
    sticky: bool,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let file = &this.files[ix];
    let id_prefix = if sticky { "diff-sticky" } else { "diff-file" };
    let collapse_view = view.clone();
    let chevron = if file.collapsed {
        IconName::ChevronRight
    } else {
        IconName::ChevronDown
    };
    let toggle = div()
        .id(ElementId::NamedInteger(
            format!("{id_prefix}-collapse").into(),
            ix as u64,
        ))
        .flex_none()
        .p_0p5()
        .rounded(theme.radius)
        .cursor_pointer()
        .hover(|style| style.bg(theme.muted))
        .child(
            Icon::new(chevron)
                .small()
                .text_color(theme.muted_foreground),
        )
        .on_click(move |_: &ClickEvent, _, cx| {
            collapse_view.update(cx, |this, cx| this.toggle_collapsed(ix, cx));
        });
    let viewed = this.viewed_enabled.then(|| {
        let view = view.clone();
        let path = file.meta.path.clone();
        Checkbox::new(ElementId::NamedInteger(
            format!("{id_prefix}-viewed").into(),
            ix as u64,
        ))
        .label(labels::VIEWED)
        .checked(file.viewed)
        .small()
        .on_click(move |checked, _, cx| {
            let event = DiffViewEvent::ViewedToggled {
                path: path.clone(),
                viewed: *checked,
            };
            view.update(cx, |_, cx| cx.emit(event));
        })
    });
    h_flex()
        .w_full()
        .h(px(FILE_HEADER_HEIGHT))
        .px_2()
        .gap_2()
        .items_center()
        .bg(theme.secondary)
        .border_y_1()
        .border_color(theme.border)
        .text_sm()
        .child(toggle)
        .child(status_badge(file.meta.status, cx))
        .child(path_label(&file.meta, cx))
        .child(
            div()
                .flex_none()
                .child(ui::diff_stat(file.meta.additions, file.meta.deletions, cx)),
        )
        .child(div().flex_1())
        .children(viewed)
        .into_any_element()
}

fn path_label(meta: &FileMeta, cx: &App) -> Div {
    let theme = cx.theme();
    let (dir, name) = match meta.path.rsplit_once('/') {
        Some((dir, name)) => (format!("{dir}/"), name.to_string()),
        None => (String::new(), meta.path.clone()),
    };
    let previous = meta.previous_path.as_ref().map(|previous| {
        div()
            .flex_none()
            .text_color(theme.muted_foreground)
            .child(labels::renamed_from(previous))
    });
    h_flex()
        .min_w_0()
        .gap_1()
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .children(previous)
        .child(
            div()
                .min_w_0()
                .truncate()
                .child(div().text_color(theme.muted_foreground).child(dir)),
        )
        .child(
            div()
                .flex_none()
                .font_semibold()
                .text_color(theme.foreground)
                .child(name),
        )
}

pub(super) fn hunk_header(hunk: &Hunk, mode: DiffMode, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let gutters = match mode {
        DiffMode::Inline => 2.,
        DiffMode::Split => 1.,
    };
    let range = format!(
        "@@ -{},{} +{},{} @@",
        hunk.old_start, hunk.old_len, hunk.new_start, hunk.new_len
    );
    h_flex()
        .w_full()
        .h(px(HUNK_HEADER_HEIGHT))
        .pl(px(NUMBER_WIDTH * gutters))
        .gap_2()
        .items_center()
        .bg(theme.info.opacity(0.08))
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(div().flex_none().child(range))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(theme.foreground.opacity(0.7))
                .child(hunk.header.clone()),
        )
        .into_any_element()
}

pub(super) fn notice(this: &DiffView, ix: usize, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (icon, message, link) = match this.files[ix].body {
        FileBody::Missing => (IconName::TriangleAlert, labels::DIFF_TOO_LARGE, true),
        FileBody::Invalid => (IconName::TriangleAlert, labels::DIFF_INVALID, true),
        FileBody::Empty | FileBody::Ready(_) => (IconName::Info, labels::DIFF_EMPTY, false),
    };
    let url = this.github_url.clone().filter(|_| link);
    let link = url.map(|url| {
        div()
            .id(ElementId::NamedInteger(
                "diff-open-github".into(),
                ix as u64,
            ))
            .text_color(theme.link)
            .cursor_pointer()
            .hover(|style| style.underline())
            .child(labels::OPEN_ON_GITHUB)
            .on_click(move |_: &ClickEvent, _, _| {
                if let Err(error) = open::that_detached(&url) {
                    tracing::warn!(%error, url, "failed to open diff in browser");
                }
            })
    });
    h_flex()
        .w_full()
        .px_4()
        .py_3()
        .gap_2()
        .items_center()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(Icon::new(icon).small())
        .child(message)
        .children(link)
        .into_any_element()
}

pub(super) fn outdated_header(cx: &App) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .w_full()
        .px_4()
        .pt_2()
        .pb_1()
        .gap_2()
        .items_center()
        .text_xs()
        .font_semibold()
        .text_color(theme.muted_foreground)
        .child(Icon::new(IconName::Clock).xsmall())
        .child(labels::OUTDATED_COMMENTS)
        .into_any_element()
}

pub(super) fn file_gap() -> AnyElement {
    div().w_full().h(px(FILE_GAP)).into_any_element()
}
