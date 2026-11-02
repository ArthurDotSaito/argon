use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::diff::DiffFile;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex};
use gpui_kit::{
    AnyElement, App, Div, ElementId, Entity, IntoElement, ParentElement as _, Styled as _, div,
};

use super::{DiffView, DiffViewEvent};
use crate::review::file_diff::FileMeta;
use crate::review::labels;
use crate::review::status_badge::status_badge;
use crate::ui;

pub fn file_header(view: &Entity<DiffView>, document: &DiffFile, cx: &mut App) -> AnyElement {
    let this = view.read(cx);
    let Some(ix) = this.index.get(document.path().as_ref()).copied() else {
        return div().child(document.path().clone()).into_any_element();
    };
    let file = &this.files[ix];
    let viewed = this.viewed_enabled.then(|| {
        let view = view.clone();
        let path = file.meta.path.clone();
        Checkbox::new(ElementId::NamedInteger("diff-viewed".into(), ix as u64))
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
        .min_w_0()
        .gap_2()
        .items_center()
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
                .text_color(theme.muted_foreground)
                .child(dir),
        )
        .child(
            div()
                .flex_none()
                .font_semibold()
                .text_color(theme.foreground)
                .child(name),
        )
}
