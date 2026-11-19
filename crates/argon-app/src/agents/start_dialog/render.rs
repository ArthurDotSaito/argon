use argon_core::SessionKind;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, WindowExt as _, h_flex,
    v_flex,
};
use gpui_kit::{
    App, ClickEvent, ClipboardItem, Context, Div, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, div,
};

use super::launch::Status;
use super::{Phase, StartDialog, Subject};
use crate::agents::labels;

fn section(title: &'static str, body: impl IntoElement, cx: &App) -> Div {
    v_flex()
        .gap_1p5()
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(cx.theme().muted_foreground)
                .child(title),
        )
        .child(body)
}

impl StartDialog {
    pub(super) fn title(&self, cx: &App) -> Div {
        let theme = cx.theme();
        let (title, context) = match &self.subject {
            Subject::Pr(pr) => (
                SharedString::from(labels::DIALOG_TITLE),
                labels::context_line(&pr.repo, pr.number, &pr.head),
            ),
            Subject::Issue(issue) => (
                SharedString::from(labels::plan_title(issue.number)),
                issue.repo.clone(),
            ),
        };
        h_flex()
            .gap_2()
            .items_center()
            .child(Icon::new(IconName::Sparkles).text_color(theme.primary))
            .child(title)
            .child(
                div()
                    .text_xs()
                    .font_normal()
                    .font_family(theme.mono_font_family.clone())
                    .text_color(theme.muted_foreground)
                    .child(context),
            )
    }

    fn error_row(text: SharedString, cx: &App) -> Div {
        let copied = text.clone();
        h_flex()
            .w_full()
            .gap_1p5()
            .items_start()
            .text_xs()
            .text_color(cx.theme().danger)
            .child(Icon::new(IconName::TriangleAlert).xsmall())
            .child(div().flex_1().min_w_0().child(text))
            .child(
                Button::new("ai-review-copy-error")
                    .icon(Icon::new(IconName::Copy))
                    .label(labels::COPY_ERROR)
                    .xsmall()
                    .ghost()
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copied.to_string()))
                    }),
            )
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let busy = self.is_launching();
        let mut error = None;
        let status = self.status().and_then(|status| match status {
            Status::Busy(text) => Some(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(Icon::new(IconName::LoaderCircle).xsmall())
                    .child(div().min_w_0().truncate().child(text)),
            ),
            Status::Notice(text) => Some(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(Icon::new(IconName::Info).xsmall())
                    .child(div().min_w_0().truncate().child(text)),
            ),
            Status::Error(text) => {
                error = Some(text);
                None
            }
        });
        let buttons = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(div().flex_1().min_w_0().children(status))
            .child(if busy {
                Button::new("ai-review-cancel-start")
                    .label(labels::CANCEL_START)
                    .small()
                    .outline()
                    .disabled(matches!(self.phase, Phase::Cancelling))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_start(cx)))
            } else {
                Button::new("ai-review-cancel")
                    .label(labels::CANCEL)
                    .small()
                    .ghost()
                    .on_click(|_, window, cx| window.close_dialog(cx))
            })
            .child(
                Button::new("ai-review-start")
                    .icon(Icon::new(IconName::Play))
                    .label(match self.kind() {
                        SessionKind::Review => labels::START,
                        SessionKind::Plan => labels::START_PLAN,
                        SessionKind::Summary => labels::START_SUMMARY,
                    })
                    .small()
                    .primary()
                    .loading(busy)
                    .disabled(!self.can_start())
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.start(cx))),
            );
        v_flex()
            .w_full()
            .gap_2()
            .children(error.map(|text| Self::error_row(text, cx)))
            .child(buttons)
    }
}

impl Render for StartDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cards = self.agent_cards(cx);
        let selects = self.selects(cx);
        let options = self.options(cx);
        let warning = self.permissions_warning(cx);
        let prompt = self.prompt_preview(cx);
        let command = self.command_preview(cx);
        let footer = self.footer(cx);
        v_flex()
            .w_full()
            .gap_3()
            .text_sm()
            .child(section(labels::AGENT, cards, cx))
            .child(selects)
            .child(options)
            .child(warning)
            .child(prompt)
            .child(command)
            .child(footer)
    }
}
