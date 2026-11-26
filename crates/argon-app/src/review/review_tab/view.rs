use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::resizable::{resizable_panel, v_resizable};
use gpui_kit::component::{ActiveTheme as _, Icon, Selectable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Focusable as _, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, px,
};

use super::{Files, ReviewScope, ReviewTab, ReviewTabEvent};
use crate::review::diff_view::DiffMode;
use crate::review::labels;
use crate::review::review_bar::{BarProps, review_bar};

const TREE_WIDTH: f32 = 280.;
const TOOLBAR_HEIGHT: f32 = 36.;
const COMPOSER_HEIGHT: f32 = 320.;
const COMPOSER_MIN: f32 = 220.;
const COMPOSER_MAX: f32 = 600.;

impl ReviewTab {
    fn commit_banner(&self, oid: &str, headline: &str, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        h_flex()
            .w_full()
            .flex_none()
            .px_4()
            .py_2()
            .gap_2()
            .items_center()
            .bg(theme.muted)
            .border_b_1()
            .border_color(theme.border)
            .text_sm()
            .child(
                Icon::new(IconName::GitCommitHorizontal)
                    .small()
                    .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.foreground)
                    .child(labels::commit_banner(oid, headline)),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .child(labels::SEPARATOR),
            )
            .child(
                div()
                    .id("review-show-all")
                    .flex_none()
                    .text_color(theme.link)
                    .cursor_pointer()
                    .hover(|style| style.underline())
                    .child(labels::REVIEW_SHOW_ALL)
                    .on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ReviewTabEvent::ShowAll)),
                    ),
            )
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> Div {
        let mode = self.diff.read(cx).mode();
        let theme = cx.theme();
        let hint = matches!(self.scope, ReviewScope::Commit { .. }).then(|| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(labels::COMMIT_COMMENT_HINT)
        });
        let mode_button = |id: &'static str, label: &'static str, target: DiffMode| {
            Button::new(id)
                .label(label)
                .xsmall()
                .ghost()
                .selected(mode == target)
                .tooltip(labels::MODE_SHORTCUT)
        };
        let split = mode_button("review-mode-split", labels::SPLIT, DiffMode::Split).on_click(
            cx.listener(|this, _: &ClickEvent, _, cx| {
                this.diff
                    .update(cx, |diff, cx| diff.set_mode(DiffMode::Split, cx));
                cx.notify();
            }),
        );
        let inline = mode_button("review-mode-inline", labels::INLINE, DiffMode::Inline).on_click(
            cx.listener(|this, _: &ClickEvent, _, cx| {
                this.diff
                    .update(cx, |diff, cx| diff.set_mode(DiffMode::Inline, cx));
                cx.notify();
            }),
        );
        h_flex()
            .w_full()
            .flex_none()
            .h(px(TOOLBAR_HEIGHT))
            .px_3()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .children(hint)
            .child(div().flex_1())
            .child(split)
            .child(inline)
    }

    fn stale_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let visible = self.scope == ReviewScope::All
            && matches!(self.files, Files::Ready)
            && self.drafts.read(cx).stale();
        if !visible {
            return None;
        }
        let reload = div()
            .id("review-stale-reload")
            .flex_none()
            .text_color(cx.theme().link)
            .cursor_pointer()
            .hover(|style| style.underline())
            .child(labels::RELOAD)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reload_diff(cx)));
        let warning = cx.theme().warning;
        Some(
            banner(
                labels::DIFF_STALE.into(),
                warning,
                IconName::TriangleAlert,
                cx,
            )
            .child(reload),
        )
    }

    fn bar(&self, cx: &mut Context<Self>) -> Div {
        let drafts = self.drafts.read(cx);
        let props = BarProps {
            count: drafts.drafts().len(),
            in_progress: drafts.in_progress(),
            head_moved: drafts.head_moved(),
            sending: drafts.is_sending(),
        };
        let on_finish =
            cx.listener(|this, _: &ClickEvent, window, cx| this.open_finish(window, cx));
        review_bar(props, on_finish, cx)
    }

    fn content(&self, body: Div, cx: &mut Context<Self>) -> AnyElement {
        match &self.composer {
            Some((composer, _)) => v_resizable("review-composer-split")
                .with_state(&self.split)
                .child(resizable_panel().child(body))
                .child(
                    resizable_panel()
                        .size(px(COMPOSER_HEIGHT))
                        .size_range(px(COMPOSER_MIN)..px(COMPOSER_MAX))
                        .fixed()
                        .child(composer.clone()),
                )
                .into_any_element(),
            None => v_flex()
                .size_full()
                .child(div().flex_1().min_h_0().child(body))
                .children(self.dock_visible(cx).then(|| self.bar(cx)))
                .into_any_element(),
        }
    }

    fn body(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        match &self.files {
            Files::Idle | Files::Loading => {
                centered(labels::LOADING_FILES.into(), theme.muted_foreground)
            }
            Files::Failed(message) => centered(message.clone(), theme.danger).child(
                div()
                    .id("review-files-retry")
                    .text_sm()
                    .text_color(theme.link)
                    .cursor_pointer()
                    .hover(|style| style.underline())
                    .child(labels::RETRY)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.load_files(false, cx);
                        cx.notify();
                    })),
            ),
            Files::Ready => h_flex()
                .size_full()
                .items_start()
                .child(
                    div()
                        .w(px(TREE_WIDTH))
                        .h_full()
                        .flex_none()
                        .border_r_1()
                        .border_color(theme.border)
                        .child(self.tree.clone()),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.toolbar(cx))
                        .child(div().flex_1().min_h_0().child(self.diff.clone())),
                ),
        }
    }
}

fn centered(message: SharedString, color: Hsla) -> Div {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .text_sm()
        .text_color(color)
        .child(message)
}

fn banner(message: SharedString, color: Hsla, icon: IconName, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .w_full()
        .flex_none()
        .px_4()
        .py_2()
        .gap_2()
        .items_center()
        .border_b_1()
        .border_color(color.opacity(0.5))
        .bg(color.opacity(0.08))
        .text_sm()
        .text_color(theme.foreground)
        .child(Icon::new(icon).small().text_color(color))
        .child(message)
}

impl Render for ReviewTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_pending && matches!(self.files, Files::Ready) {
            self.focus_pending = false;
            let handle = self.diff.focus_handle(cx);
            window.defer(cx, move |window, cx| window.focus(&handle, cx));
        }
        let warning = cx.theme().warning;
        let danger = cx.theme().danger;
        let notice = self
            .notice
            .clone()
            .map(|notice| banner(notice, warning, IconName::TriangleAlert, cx));
        let error = self
            .error
            .clone()
            .map(|error| banner(error, danger, IconName::CircleAlert, cx));
        let info = cx.theme().info;
        let outside = (self.outside_comments > 0).then(|| {
            banner(
                labels::outside_comments(self.outside_comments).into(),
                info,
                IconName::MessageSquare,
                cx,
            )
        });
        let commit = match self.scope.clone() {
            ReviewScope::Commit { oid, headline } => Some(self.commit_banner(&oid, &headline, cx)),
            ReviewScope::All => None,
        };
        let stale = self.stale_banner(cx);
        let body = self.body(cx);
        let content = self.content(body, cx);
        v_flex()
            .size_full()
            .children(notice)
            .children(stale)
            .children(commit)
            .children(error)
            .children(outside)
            .child(div().flex_1().min_h_0().child(content))
    }
}
