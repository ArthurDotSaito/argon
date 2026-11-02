use std::rc::Rc;

use argon_core::PrKey;
use argon_github::{PrDetail, TimelineItem};
use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AppContext as _, ClickEvent, Context, Div, Entity, EventEmitter, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, Window, div, px,
};

use super::labels;
use super::timeline_item::{self, card, card_body, card_text, markdown, timeline_item};
use crate::services::AppServices;

const CONTENT_MAX_WIDTH: f32 = 920.;
const COMMENT_MIN_ROWS: usize = 3;
const COMMENT_MAX_ROWS: usize = 12;

pub enum ConversationEvent {
    Commented,
    OpenOnGithub,
}

pub fn count(detail: &PrDetail) -> usize {
    detail
        .timeline
        .iter()
        .filter(|item| timeline_item::is_conversation(item))
        .count()
}

pub struct Conversation {
    key: PrKey,
    detail: Option<Rc<PrDetail>>,
    timeline: Vec<usize>,
    input: Entity<TextareaState>,
    sending: bool,
    error: Option<SharedString>,
    send_task: Option<Task<()>>,
    scroll: ScrollHandle,
}

impl EventEmitter<ConversationEvent> for Conversation {}

impl Conversation {
    pub fn new(key: PrKey, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(labels::COMMENT_PLACEHOLDER)
                .auto_grow(COMMENT_MIN_ROWS, COMMENT_MAX_ROWS)
        });
        cx.observe(&input, |_, _, cx| cx.notify()).detach();
        Self {
            key,
            detail: None,
            timeline: Vec::new(),
            input,
            sending: false,
            error: None,
            send_task: None,
            scroll: ScrollHandle::new(),
        }
    }

    pub fn set_detail(&mut self, detail: Rc<PrDetail>, cx: &mut Context<Self>) {
        let mut order: Vec<usize> = (0..detail.timeline.len()).collect();
        order.sort_by_key(|ix| timeline_item::timestamp(&detail.timeline[*ix]));
        self.timeline = order;
        self.detail = Some(detail);
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.input.read(cx).value().trim().to_string();
        if body.is_empty() || self.sending {
            return;
        }
        let Some(services) = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone())
        else {
            self.error = Some(labels::comment_failed(labels::SERVICES_UNAVAILABLE).into());
            cx.notify();
            return;
        };
        self.sending = true;
        self.error = None;
        let id = self.key.id.clone();
        let task = services.spawn_github(cx, self.key.host.clone(), move |client| async move {
            client.add_comment(&id, &body).await
        });
        self.send_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.sending = false;
                match result {
                    Ok(()) => {
                        this.input
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        cx.emit(ConversationEvent::Commented);
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to add pr comment");
                        this.error = Some(labels::comment_failed(&error.to_string()).into());
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn description(&self, detail: &PrDetail, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let pr = &detail.pr;
        let body = if detail.body.trim().is_empty() {
            div()
                .italic()
                .text_color(theme.muted_foreground)
                .child(labels::NO_DESCRIPTION)
                .into_any_element()
        } else {
            markdown("pr-description", &detail.body).into_any_element()
        };
        card(
            &pr.author,
            card_body(
                &pr.author,
                div().child(labels::OPENED_THIS_PR),
                pr.created_at,
                Utc::now(),
                cx,
            )
            .child(card_text(body)),
        )
    }

    fn truncated_notice(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        h_flex()
            .w_full()
            .gap_2()
            .px_3()
            .py_2()
            .items_center()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.warning.opacity(0.5))
            .bg(theme.warning.opacity(0.08))
            .text_sm()
            .text_color(theme.foreground)
            .child(
                Icon::new(IconName::TriangleAlert)
                    .small()
                    .text_color(theme.warning),
            )
            .child(labels::TIMELINE_TRUNCATED)
            .child(div().text_color(theme.muted_foreground).child("·"))
            .child(
                div()
                    .id("pr-timeline-open-github")
                    .text_color(theme.link)
                    .cursor_pointer()
                    .hover(|style| style.underline())
                    .child(labels::OPEN_ON_GITHUB)
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                        cx.emit(ConversationEvent::OpenOnGithub)
                    })),
            )
    }

    fn composer(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let empty = self.input.read(cx).value().trim().is_empty();
        let error = self.error.clone().map(|error| {
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .text_color(theme.danger)
                .child(error)
        });
        v_flex()
            .w_full()
            .gap_2()
            .child(Textarea::new(&self.input).disabled(self.sending))
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .children(error)
                    .child(div().flex_1())
                    .child(
                        Button::new("pr-comment-submit")
                            .label(labels::COMMENT)
                            .small()
                            .primary()
                            .loading(self.sending)
                            .disabled(empty)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.submit(window, cx)
                            })),
                    ),
            )
    }
}

impl Render for Conversation {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scroller = div()
            .id("pr-conversation-scroll")
            .size_full()
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .vertical_scrollbar(&self.scroll);
        let Some(detail) = self.detail.clone() else {
            return scroller;
        };
        let now = Utc::now();
        let items: Vec<&TimelineItem> = self
            .timeline
            .iter()
            .filter_map(|ix| detail.timeline.get(*ix))
            .collect();
        let notice = detail.truncated_timeline.then(|| self.truncated_notice(cx));
        let timeline = v_flex().w_full().gap_3().children(
            items
                .into_iter()
                .map(|item| timeline_item(item, &detail.labels, now, cx)),
        );
        let content = v_flex()
            .w_full()
            .min_w_0()
            .max_w(px(CONTENT_MAX_WIDTH))
            .mx_auto()
            .px_4()
            .py_4()
            .gap_4()
            .text_color(cx.theme().foreground)
            .child(self.description(&detail, cx))
            .children(notice)
            .child(timeline)
            .child(self.composer(cx));
        scroller.child(content)
    }
}
