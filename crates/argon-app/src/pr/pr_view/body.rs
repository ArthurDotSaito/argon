use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, div,
    px,
};

use super::{Load, PrTab, PrView};
use crate::inbox::PR_FALLBACK_TITLE;
use crate::pr::conversation::{self, CONTENT_MAX_WIDTH};
use crate::pr::labels;

const TAB_HEIGHT: f32 = 34.;
const ERROR_MAX_WIDTH: f32 = 560.;

impl PrView {
    fn tab_count(&self, tab: PrTab, cx: &App) -> Option<usize> {
        match tab {
            PrTab::Conversation => self.detail.as_deref().map(conversation::count),
            PrTab::Commits => self.commits.read(cx).count(),
            PrTab::Actions => self.actions.read(cx).count(),
            PrTab::Review => self.review.read(cx).count(),
        }
    }

    pub(super) fn tab_bar(&self, cx: &mut Context<Self>) -> Div {
        let items: Vec<Stateful<Div>> = PrTab::ALL
            .iter()
            .enumerate()
            .map(|(ix, tab)| self.tab_item(ix, *tab, cx))
            .collect();
        h_flex().px_2().gap_1().items_end().children(items)
    }

    fn tab_item(&self, ix: usize, tab: PrTab, cx: &mut Context<Self>) -> Stateful<Div> {
        let selected = self.tab == tab;
        let count = self.tab_count(tab, cx);
        tab_button(("pr-tab", ix), tab.icon(), tab.title(), count, selected, cx)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_tab(tab, cx)))
    }

    pub(super) fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.detail.is_some() {
            return match self.tab {
                PrTab::Conversation => self.conversation.clone().into_any_element(),
                PrTab::Commits => self.commits.clone().into_any_element(),
                PrTab::Actions => self.actions.clone().into_any_element(),
                PrTab::Review => self.review.clone().into_any_element(),
            };
        }
        match &self.load {
            Load::Failed(message) => {
                let retry = self.retry_button("pr-retry", cx);
                failure_panel(message.clone(), retry, cx).into_any_element()
            }
            Load::Loading | Load::Ready => skeleton().into_any_element(),
        }
    }

    pub(super) fn error_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let Load::Failed(message) = &self.load else {
            return None;
        };
        self.detail.as_ref()?;
        let retry = self.retry_button("pr-retry-banner", cx);
        Some(failure_banner(message.clone(), retry, cx))
    }

    fn retry_button(&self, id: &'static str, cx: &mut Context<Self>) -> Button {
        retry_button(id).on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reload(cx)))
    }
}

pub fn tab_button(
    id: impl Into<ElementId>,
    icon: IconName,
    title: &'static str,
    count: Option<usize>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let count = count.map(|count| {
        div()
            .px_1p5()
            .rounded_full()
            .bg(theme.muted)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(count.to_string())
    });
    h_flex()
        .id(id)
        .h(px(TAB_HEIGHT))
        .px_3()
        .gap_1p5()
        .items_center()
        .border_b_2()
        .border_color(if selected {
            theme.primary
        } else {
            gpui_kit::transparent_black()
        })
        .text_sm()
        .text_color(if selected {
            theme.foreground
        } else {
            theme.muted_foreground
        })
        .cursor_pointer()
        .hover(|style| style.text_color(theme.foreground))
        .child(Icon::new(icon).small())
        .child(title)
        .children(count)
}

pub fn failure_banner(message: SharedString, retry: Button, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .mx_4()
        .mb_2()
        .px_3()
        .py_1p5()
        .gap_2()
        .items_center()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.danger.opacity(0.5))
        .bg(theme.danger.opacity(0.08))
        .text_sm()
        .child(
            Icon::new(IconName::CircleAlert)
                .small()
                .text_color(theme.danger),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(theme.foreground)
                .child(message),
        )
        .child(retry)
}

pub fn fallback_header(title: &'static str, note: &'static str, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .px_4()
        .pt_3()
        .pb_3()
        .gap_1()
        .child(
            div()
                .text_xl()
                .font_semibold()
                .text_color(theme.foreground)
                .child(title),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(note),
        )
}

pub(super) fn pr_fallback_header(cx: &App) -> Div {
    fallback_header(PR_FALLBACK_TITLE, labels::NOT_IN_INBOX, cx)
}

pub fn failure_panel(message: SharedString, retry: Button, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .child(
            Icon::new(IconName::CircleAlert)
                .large()
                .text_color(theme.danger),
        )
        .child(
            div()
                .max_w(px(ERROR_MAX_WIDTH))
                .text_sm()
                .text_center()
                .text_color(theme.foreground)
                .child(message),
        )
        .child(retry)
}

pub fn retry_button(id: &'static str) -> Button {
    Button::new(id)
        .icon(Icon::new(IconName::RotateCw))
        .label(labels::RETRY)
        .small()
        .outline()
}

pub fn skeleton() -> Div {
    v_flex()
        .w_full()
        .max_w(px(CONTENT_MAX_WIDTH))
        .mx_auto()
        .p_4()
        .gap_3()
        .child(Skeleton::new().w(px(320.)).h(px(16.)))
        .child(Skeleton::new().w_full().h(px(96.)))
        .child(Skeleton::new().w(px(480.)).h(px(14.)))
        .child(Skeleton::new().w_full().h(px(72.)))
        .child(Skeleton::new().w(px(400.)).h(px(14.)))
}
