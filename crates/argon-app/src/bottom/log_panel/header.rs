use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex,
};
use gpui_kit::{App, ClickEvent, Context, Div, ParentElement as _, Styled as _, div, px};

use super::LogPanel;
use crate::bottom::labels;
use crate::bottom::log_model::LogTarget;
use crate::bottom::run_status::{run_status_icon, run_status_label};

const HEADER_HEIGHT: f32 = 34.;
const SEARCH_WIDTH: f32 = 260.;

impl LogPanel {
    pub(super) fn header(&self, target: &LogTarget, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let hits = self.view.as_ref().map(|view| view.hits());
        let count = hits.map_or(0, |hits| hits.lines.len());
        let has_error = self
            .view
            .as_ref()
            .is_some_and(|view| view.doc().first_error.is_some());
        let counter = self
            .view
            .as_ref()
            .filter(|view| !view.query().is_empty())
            .map(|view| {
                let hits = view.hits();
                let text = if hits.lines.is_empty() {
                    labels::NO_MATCHES.to_string()
                } else {
                    labels::match_position(
                        view.current_match(),
                        hits.lines.len(),
                        hits.occurrences,
                        hits.capped,
                    )
                };
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(text)
            });
        let url = target.html_url.clone();
        h_flex()
            .flex_none()
            .h(px(HEADER_HEIGHT))
            .px_2()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(run_status_icon(target.status, cx))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(labels::title(&target.workflow, &target.job)),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(run_status_label(target.status)),
            )
            .child(div().flex_1())
            .child(
                div().w(px(SEARCH_WIDTH)).flex_none().child(
                    Input::new(&self.search)
                        .xsmall()
                        .prefix(Icon::new(IconName::Search).xsmall())
                        .suffix(div().children(counter)),
                ),
            )
            .child(
                Button::new("log-previous-match")
                    .icon(Icon::new(IconName::ArrowUp))
                    .xsmall()
                    .ghost()
                    .disabled(count == 0)
                    .tooltip(labels::PREVIOUS_MATCH)
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.step_match(false, cx)),
                    ),
            )
            .child(
                Button::new("log-next-match")
                    .icon(Icon::new(IconName::ArrowDown))
                    .xsmall()
                    .ghost()
                    .disabled(count == 0)
                    .tooltip(labels::NEXT_MATCH)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.step_match(true, cx))),
            )
            .child(
                Button::new("log-jump-error")
                    .icon(Icon::new(IconName::CircleAlert))
                    .xsmall()
                    .ghost()
                    .disabled(!has_error)
                    .tooltip(labels::JUMP_TO_ERROR)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.jump_to_error(cx))),
            )
            .child(
                Button::new("log-open-github")
                    .icon(Icon::new(IconName::ExternalLink))
                    .label(labels::OPEN_ON_GITHUB)
                    .xsmall()
                    .ghost()
                    .on_click(move |_, _, _| open_url(&url)),
            )
    }

    pub(super) fn truncated_banner(&self, cx: &App) -> Option<Div> {
        let theme = cx.theme();
        self.truncated.then(|| {
            h_flex()
                .flex_none()
                .px_2()
                .py_0p5()
                .gap_1p5()
                .items_center()
                .bg(theme.warning.opacity(0.12))
                .text_xs()
                .text_color(theme.warning)
                .child(Icon::new(IconName::TriangleAlert).xsmall())
                .child(labels::TRUNCATED)
        })
    }
}

fn open_url(url: &str) {
    if let Err(error) = open::that_detached(url) {
        tracing::warn!(%error, url, "failed to open job in browser");
    }
}
