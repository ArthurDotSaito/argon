use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    App, ClickEvent, Context, Div, InteractiveElement as _, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _, px,
};

use super::labels;
use super::project_view::ProjectView;
use crate::pr::labels as pr_labels;
use crate::ui::properties;

const RELATED_ROW_HEIGHT: f32 = 30.;
const BUTTON_GAP: f32 = 6.;

impl ProjectView {
    pub(super) fn side_panel(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let open = Button::new("project-open-github")
            .icon(Icon::new(IconName::ExternalLink))
            .label(pr_labels::OPEN_ON_GITHUB)
            .small()
            .ghost()
            .w_full()
            .on_click(cx.listener(|this, _: &ClickEvent, _, _| this.open_on_github()));
        let buttons = v_flex()
            .w_full()
            .flex_none()
            .gap(px(BUTTON_GAP))
            .child(open);
        properties::panel(cx)
            .id("project-side")
            .overflow_y_scroll()
            .child(buttons)
            .child(self.related_group(cx))
            .child(refresh_group(cx))
    }

    fn related_group(&self, cx: &mut Context<Self>) -> Div {
        let rows: Vec<Stateful<Div>> = self
            .related
            .iter()
            .enumerate()
            .map(|(ix, repo)| self.related_row(ix, repo, cx))
            .collect();
        let add_button = Button::new("project-related-add")
            .icon(Icon::new(IconName::Plus))
            .label(labels::ADD)
            .small()
            .outline()
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.add_related(window, cx)));
        let theme = cx.theme();
        let title = h_flex()
            .gap_1p5()
            .items_center()
            .text_xs()
            .font_semibold()
            .text_color(theme.muted_foreground)
            .child(Icon::new(IconName::Network).xsmall())
            .child(labels::RELATED_TITLE)
            .child(div().flex_1())
            .child(
                div()
                    .font_normal()
                    .child(labels::related_counter(self.related.len())),
            );
        let list = (!rows.is_empty()).then(|| {
            v_flex()
                .w_full()
                .overflow_hidden()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .children(rows)
        });
        let add = h_flex()
            .w_full()
            .gap_1p5()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.input).small()),
            )
            .child(add_button);
        let error = self.input_error.map(|error| {
            div()
                .text_xs()
                .text_color(theme.danger)
                .child(error.message())
        });
        properties::group(cx)
            .child(title)
            .children(list)
            .child(add)
            .children(error)
            .child(note(labels::RELATED_NOTE, cx))
    }

    fn related_row(&self, ix: usize, repo: &str, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = cx.theme();
        let target = repo.to_owned();
        h_flex()
            .id(("project-related", ix))
            .w_full()
            .h(px(RELATED_ROW_HEIGHT))
            .px_2()
            .gap_2()
            .items_center()
            .when(ix > 0, |this| this.border_t_1().border_color(theme.border))
            .child(
                Icon::new(IconName::Network)
                    .xsmall()
                    .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(theme.mono_font_family.clone())
                    .text_xs()
                    .child(repo.to_owned()),
            )
            .child(
                Button::new(("project-related-remove", ix))
                    .icon(Icon::new(IconName::Trash))
                    .xsmall()
                    .ghost()
                    .tooltip(labels::REMOVE)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.remove_related(&target, cx)
                    })),
            )
    }
}

fn refresh_group(cx: &App) -> Div {
    let theme = cx.theme();
    properties::group(cx)
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .text_xs()
                .font_semibold()
                .text_color(theme.muted_foreground)
                .child(Icon::new(IconName::RefreshCw).xsmall())
                .child(labels::REFRESH_TITLE),
        )
        .child(note(labels::REFRESH_NOTE, cx))
}

fn note(text: &'static str, cx: &App) -> Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}
