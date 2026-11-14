use std::rc::Rc;

use argon_core::{PrKey, SubjectKind};
use argon_github::{PrDetail, TimelineItem};
use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, ClickEvent, Context, Div, Entity, EventEmitter, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div, px,
};

use super::comment_check::{SentComment, may_have_landed};
use super::labels;
use super::timeline_item::{self, Entry, MarkdownBlock, card, card_body, card_text, timeline_item};
use crate::services::AppServices;

pub const CONTENT_MAX_WIDTH: f32 = 920.;
const COMMENT_MIN_ROWS: usize = 3;
const COMMENT_MAX_ROWS: usize = 12;

pub enum ConversationEvent {
    Commented,
    CommentConfirmed,
    Recheck,
    OpenOnGithub,
}

pub enum ComposerEvent {
    Commented,
    CommentConfirmed,
    Recheck,
}

pub fn count(detail: &PrDetail) -> usize {
    detail
        .timeline
        .iter()
        .filter(|item| timeline_item::is_conversation(item))
        .count()
}

pub struct Composer {
    subject: PrKey,
    kind: SubjectKind,
    input: Entity<TextareaState>,
    sending: bool,
    error: Option<SharedString>,
    notice: Option<SharedString>,
    unconfirmed: Option<SentComment>,
    clear_input: Option<String>,
    send_task: Option<Task<()>>,
}

impl EventEmitter<ComposerEvent> for Composer {}

impl Composer {
    pub fn new(
        subject: PrKey,
        kind: SubjectKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(labels::COMMENT_PLACEHOLDER)
                .auto_grow(COMMENT_MIN_ROWS, COMMENT_MAX_ROWS)
        });
        cx.observe(&input, |_, _, cx| cx.notify()).detach();
        Self {
            subject,
            kind,
            input,
            sending: false,
            error: None,
            notice: None,
            unconfirmed: None,
            clear_input: None,
            send_task: None,
        }
    }

    pub fn set_timeline(&mut self, timeline: &[TimelineItem], cx: &mut Context<Self>) {
        if let Some(sent) = self.unconfirmed.take()
            && sent.landed_in(timeline)
        {
            self.error = None;
            self.notice = Some(labels::COMMENT_SENT.into());
            self.clear_input = Some(sent.body().to_string());
            cx.emit(ComposerEvent::CommentConfirmed);
            cx.notify();
        }
    }

    fn submit_id(&self) -> &'static str {
        match self.kind {
            SubjectKind::Pr => "pr-comment-submit",
            SubjectKind::Issue => "issue-comment-submit",
        }
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
        self.notice = None;
        self.unconfirmed = None;
        let author = services
            .accounts()
            .into_iter()
            .find(|account| account.host.eq_ignore_ascii_case(&self.subject.host))
            .map(|account| account.login);
        let sent_body = body.clone();
        let id = self.subject.id.clone();
        let kind = self.kind;
        let task = services.spawn_github(cx, self.subject.host.clone(), move |client| async move {
            client.add_comment(&id, &body).await
        });
        self.send_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.sending = false;
                match result {
                    Ok(_) => {
                        this.input
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        cx.emit(ComposerEvent::Commented);
                    }
                    Err(error) => {
                        tracing::warn!(%error, ?kind, "failed to add comment");
                        this.error = Some(labels::comment_failed(&error.to_string()).into());
                        if let Some(author) = author.filter(|_| may_have_landed(&error)) {
                            this.unconfirmed = Some(SentComment::new(author, sent_body));
                            cx.emit(ComposerEvent::Recheck);
                        }
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(sent) = self.clear_input.take()
            && self.input.read(cx).value().trim() == sent
        {
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
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
        let notice = self.notice.clone().map(|notice| {
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .text_color(theme.success)
                .child(notice)
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
                    .children(notice)
                    .child(div().flex_1())
                    .child(
                        Button::new(self.submit_id())
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

pub struct Conversation {
    detail: Option<Rc<PrDetail>>,
    description: Option<MarkdownBlock>,
    timeline: Vec<Entry>,
    composer: Entity<Composer>,
    scroll: ScrollHandle,
    _composer_events: Subscription,
}

impl EventEmitter<ConversationEvent> for Conversation {}

impl Conversation {
    pub fn new(key: PrKey, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| Composer::new(key, SubjectKind::Pr, window, cx));
        let composer_events = cx.subscribe(&composer, |_, _, event, cx| {
            cx.emit(match event {
                ComposerEvent::Commented => ConversationEvent::Commented,
                ComposerEvent::CommentConfirmed => ConversationEvent::CommentConfirmed,
                ComposerEvent::Recheck => ConversationEvent::Recheck,
            })
        });
        Self {
            detail: None,
            description: None,
            timeline: Vec::new(),
            composer,
            scroll: ScrollHandle::new(),
            _composer_events: composer_events,
        }
    }

    pub fn set_detail(&mut self, detail: Rc<PrDetail>, cx: &mut Context<Self>) {
        let mut order: Vec<usize> = (0..detail.timeline.len()).collect();
        order.sort_by_key(|ix| timeline_item::timestamp(&detail.timeline[*ix]));
        self.timeline = order
            .into_iter()
            .map(|ix| Entry::new(ix, &detail.timeline[ix]))
            .collect();
        self.description = (!detail.body.trim().is_empty())
            .then(|| MarkdownBlock::new("pr-description".to_string(), &detail.body));
        self.composer.update(cx, |composer, cx| {
            composer.set_timeline(&detail.timeline, cx)
        });
        self.detail = Some(detail);
        cx.notify();
    }

    fn description(&self, detail: &PrDetail, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let pr = &detail.pr;
        let body = match &self.description {
            Some(description) => description.view().into_any_element(),
            None => div()
                .italic()
                .text_color(theme.muted_foreground)
                .child(labels::NO_DESCRIPTION)
                .into_any_element(),
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
        let notice = detail.truncated_timeline.then(|| {
            let open =
                cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ConversationEvent::OpenOnGithub));
            truncated_notice("pr-timeline-open-github", open, cx)
        });
        let timeline = v_flex()
            .w_full()
            .gap_3()
            .children(self.timeline.iter().filter_map(|entry| {
                let item = detail.timeline.get(entry.ix)?;
                timeline_item(item, entry, &detail.labels, now, cx)
            }));
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
            .child(self.composer.clone());
        scroller.child(content)
    }
}

pub fn truncated_notice(
    id: &'static str,
    on_open: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
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
                .id(id)
                .text_color(theme.link)
                .cursor_pointer()
                .hover(|style| style.underline())
                .child(labels::OPEN_ON_GITHUB)
                .on_click(on_open),
        )
}
