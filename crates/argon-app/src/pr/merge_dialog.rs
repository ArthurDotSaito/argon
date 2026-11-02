use argon_github::MergeMethod;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, StyledExt as _, WindowExt as _, v_flex,
};
use gpui_kit::{App, ParentElement as _, Styled as _, WeakEntity, Window, div};

use super::labels;
use super::pr_view::PrView;

const METHODS: [MergeMethod; 3] = [MergeMethod::Merge, MergeMethod::Squash, MergeMethod::Rebase];
const DIALOG_WIDTH: f32 = 460.;

pub struct MergeTarget {
    pub number: u64,
    pub head: String,
    pub base: String,
}

pub fn open(view: WeakEntity<PrView>, target: MergeTarget, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let selected = view
            .upgrade()
            .map(|view| view.read(cx).merge_method())
            .unwrap_or(MergeMethod::Merge);
        let theme = cx.theme();
        let pick = view.clone();
        let methods = RadioGroup::vertical("pr-merge-method")
            .selected_index(METHODS.iter().position(|method| *method == selected))
            .children(METHODS.iter().map(|method| {
                Radio::new(labels::merge_method(*method)).label(labels::merge_method(*method))
            }))
            .on_click(move |ix, window, cx| {
                if let Some(method) = METHODS.get(*ix).copied() {
                    _ = pick.update(cx, |view, cx| view.set_merge_method(method, cx));
                    window.refresh();
                }
            });
        let confirm_view = view.clone();
        let footer = DialogFooter::new()
            .child(
                Button::new("pr-merge-cancel")
                    .label(labels::CANCEL)
                    .small()
                    .ghost()
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("pr-merge-confirm")
                    .icon(Icon::new(IconName::GitMerge))
                    .label(labels::merge_confirm(target.number, &target.base))
                    .small()
                    .success()
                    .on_click(move |_, window, cx| {
                        _ = confirm_view.update(cx, |view, cx| view.merge(cx));
                        window.close_dialog(cx);
                    }),
            );
        dialog
            .w(gpui_kit::px(DIALOG_WIDTH))
            .title(labels::MERGE_DIALOG_TITLE)
            .child(
                v_flex()
                    .gap_3()
                    .text_sm()
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(labels::merge_summary(
                                target.number,
                                &target.head,
                                &target.base,
                            )),
                    )
                    .child(
                        div()
                            .font_semibold()
                            .text_color(theme.foreground)
                            .child(labels::MERGE_METHOD),
                    )
                    .child(methods),
            )
            .footer(footer)
    });
}
