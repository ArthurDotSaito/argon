use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, StyledExt as _, WindowExt as _, v_flex,
};
use gpui_kit::{App, ParentElement as _, Styled as _, WeakEntity, Window, div, px};

use super::ActionsTab;
use super::labels;

const DIALOG_WIDTH: f32 = 440.;

pub fn open(
    tab: WeakEntity<ActionsTab>,
    workflows: Vec<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let count = workflows.len();
    let names = workflows.join(", ");
    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme();
        let confirm_tab = tab.clone();
        let footer = DialogFooter::new()
            .child(
                Button::new("actions-rerun-cancel")
                    .label(labels::CANCEL)
                    .small()
                    .ghost()
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("actions-rerun-confirm")
                    .icon(Icon::new(IconName::RotateCw))
                    .label(labels::rerun_confirm(count))
                    .small()
                    .primary()
                    .on_click(move |_, window, cx| {
                        _ = confirm_tab.update(cx, |tab, cx| tab.rerun_failed(cx));
                        window.close_dialog(cx);
                    }),
            );
        dialog
            .w(px(DIALOG_WIDTH))
            .title(labels::RERUN_TITLE)
            .child(
                v_flex()
                    .gap_2()
                    .text_sm()
                    .child(
                        div()
                            .font_semibold()
                            .text_color(theme.foreground)
                            .child(labels::rerun_question(&names)),
                    )
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(labels::rerun_hint(count)),
                    ),
            )
            .footer(footer)
    });
}
