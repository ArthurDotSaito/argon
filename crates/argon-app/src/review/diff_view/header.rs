use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::diff::DiffFile;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::{
    AnyElement, App, ElementId, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, div,
};

use super::{DiffView, DiffViewEvent};
use crate::review::labels;

pub fn viewed_toggle(view: &Entity<DiffView>, document: &DiffFile, cx: &mut App) -> AnyElement {
    let this = view.read(cx);
    let path = document.path().to_string();
    let Some(ix) = this
        .index
        .get(&path)
        .copied()
        .filter(|_| this.viewed_enabled)
    else {
        return div().into_any_element();
    };
    let saving = this.saving.contains(&path);
    let viewed = this.files[ix].viewed;
    let view = view.clone();
    let checkbox = Checkbox::new(ElementId::NamedInteger("diff-viewed".into(), ix as u64))
        .label(if saving {
            labels::SAVING
        } else {
            labels::VIEWED
        })
        .checked(viewed)
        .disabled(saving)
        .small()
        .on_click(move |checked, _, cx| {
            let event = DiffViewEvent::ViewedToggled {
                path: path.clone(),
                viewed: *checked,
            };
            view.update(cx, |_, cx| cx.emit(event));
        });
    if saving {
        div()
            .id(ElementId::NamedInteger(
                "diff-viewed-saving".into(),
                ix as u64,
            ))
            .child(checkbox)
            .tooltip(|window, cx| Tooltip::new(labels::SAVING_VIEWED).build(window, cx))
            .into_any_element()
    } else {
        checkbox.into_any_element()
    }
}
