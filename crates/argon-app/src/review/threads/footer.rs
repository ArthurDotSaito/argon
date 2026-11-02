use argon_github::ReviewThread;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, ClickEvent, Div, ElementId, Entity, InteractiveElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div,
};

use super::ThreadUi;
use crate::review::labels;

pub fn thread_footer(thread: &ReviewThread, ui_state: &Entity<ThreadUi>, cx: &App) -> Div {
    let theme = cx.theme();
    let state = ui_state.read(cx);
    let footer = v_flex()
        .px_3()
        .py_2()
        .gap_1()
        .border_t_1()
        .border_color(theme.border);
    if let Some(editor) = state.reply_editor(&thread.id) {
        return footer.child(editor.clone());
    }
    let resolving = state.is_resolving(&thread.id);
    let error = state
        .error(&thread.id)
        .cloned()
        .map(|error| div().text_xs().text_color(theme.danger).child(error));
    let reply_state = ui_state.clone();
    let reply_id = thread.id.clone();
    let reply = h_flex()
        .id(ElementId::Name(
            format!("thread-reply-{}", thread.id).into(),
        ))
        .flex_1()
        .min_w_0()
        .h_7()
        .px_2()
        .gap_1p5()
        .items_center()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.input)
        .bg(theme.background)
        .text_sm()
        .text_color(theme.muted_foreground)
        .cursor_text()
        .hover(|style| style.border_color(theme.ring))
        .child(Icon::new(IconName::MessageSquareReply).xsmall())
        .child(labels::REPLY_PLACEHOLDER)
        .on_click(move |_: &ClickEvent, _, cx| {
            reply_state.update(cx, |state, cx| state.request_reply(&reply_id, cx));
        });
    let resolve_state = ui_state.clone();
    let resolve_id = thread.id.clone();
    let target = !thread.resolved;
    let (icon, label) = if thread.resolved {
        (IconName::RotateCcw, labels::UNRESOLVE)
    } else {
        (IconName::CircleCheck, labels::RESOLVE)
    };
    let resolve = Button::new(ElementId::Name(
        format!("thread-resolve-{}", thread.id).into(),
    ))
    .icon(Icon::new(icon))
    .label(label)
    .small()
    .outline()
    .loading(resolving)
    .disabled(resolving)
    .on_click(move |_, _, cx| {
        resolve_state.update(cx, |state, cx| {
            state.request_resolved(&resolve_id, target, cx)
        });
    });
    footer
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(reply)
                .child(resolve),
        )
        .children(error)
}
