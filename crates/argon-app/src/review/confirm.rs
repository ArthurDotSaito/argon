use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _};
use gpui_kit::{App, ParentElement as _, SharedString, Styled as _, Window, div, px};

use super::labels;

const DIALOG_WIDTH: f32 = 420.;

pub struct Confirm<T = &'static str> {
    pub title: T,
    pub message: String,
    pub confirm: &'static str,
}

pub fn confirm<T: Into<SharedString>>(
    request: Confirm<T>,
    window: &mut Window,
    cx: &mut App,
    on_confirm: impl Fn(&mut Window, &mut App) + 'static,
) {
    let on_confirm = Rc::new(on_confirm);
    let title: SharedString = request.title.into();
    let (message, confirm_label) = (request.message, request.confirm);
    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme();
        let on_confirm = on_confirm.clone();
        let footer = DialogFooter::new()
            .child(
                Button::new("confirm-cancel")
                    .label(labels::CANCEL)
                    .small()
                    .ghost()
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("confirm-ok")
                    .label(confirm_label)
                    .small()
                    .danger()
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        on_confirm(window, cx);
                    }),
            );
        dialog
            .w(px(DIALOG_WIDTH))
            .title(title.clone())
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(message.clone()),
            )
            .footer(footer)
    });
}
