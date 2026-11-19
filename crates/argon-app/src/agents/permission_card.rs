use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{App, Div, ParentElement as _, SharedString, Styled as _, WeakEntity, div};

use super::panel_labels as labels;
use super::thread_model::ThreadModel;
use super::transcript::{PermissionItem, PermissionState};

fn option_button(
    model: WeakEntity<ThreadModel>,
    request_id: &str,
    option: &argon_agents::PermissionOption,
) -> Button {
    let label = labels::permission_option(&option.kind, &option.label);
    let id = SharedString::from(format!("perm-{request_id}-{}", option.id));
    let button = Button::new(id).label(label.clone()).xsmall();
    let button = if option.kind.starts_with("allow") {
        button.primary()
    } else {
        button.outline()
    };
    let request_id = request_id.to_owned();
    let option_id = option.id.clone();
    button.on_click(move |_, _, cx| {
        _ = model.update(cx, |model, cx| {
            model.respond(&request_id, &option_id, &label, cx)
        });
    })
}

pub fn permission_card(model: WeakEntity<ThreadModel>, item: &PermissionItem, cx: &App) -> Div {
    let theme = cx.theme();
    let pending = item.state == PermissionState::Pending;
    let accent = if pending { theme.warning } else { theme.border };
    let footer = match &item.state {
        PermissionState::Pending => h_flex().flex_wrap().gap_1p5().children(
            item.options
                .iter()
                .map(|option| option_button(model.clone(), &item.request_id, option)),
        ),
        PermissionState::Answered(label) => h_flex()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(labels::answered(label)),
        PermissionState::Expired => h_flex()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(labels::PERMISSION_EXPIRED),
    };
    v_flex()
        .px_3()
        .py_2p5()
        .gap_1p5()
        .rounded(theme.radius)
        .border_1()
        .border_color(accent)
        .when(pending, |this| this.bg(theme.warning.opacity(0.08)))
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .text_xs()
                .font_semibold()
                .text_color(if pending {
                    theme.warning
                } else {
                    theme.muted_foreground
                })
                .child(Icon::new(IconName::Hand).xsmall())
                .child(labels::PERMISSION_TITLE)
                .child(
                    div()
                        .font_normal()
                        .text_color(theme.muted_foreground)
                        .child(format!("· {}", labels::tool_kind(&item.kind))),
                ),
        )
        .child(
            div()
                .text_sm()
                .font_family(theme.mono_font_family.clone())
                .child(item.title.clone()),
        )
        .child(footer)
}
