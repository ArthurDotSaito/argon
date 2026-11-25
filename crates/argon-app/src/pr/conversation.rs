use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use argon_core::{PrKey, Side, SubjectKind};
use argon_github::{PrDetail, TimelineItem, WorkflowRun};
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::{
    App, AppContext as _, ClickEvent, Context, Div, Entity, EventEmitter, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div, px,
};

use super::activity::{ActivityFilter, ActivityOrder};
use super::activity_view::link;
use super::comment_check::{SentComment, may_have_landed};
use super::labels;
use super::timeline_item::{MarkdownBlock, strip_html_comments};
use crate::services::AppServices;
use crate::ui;

pub const CONTENT_MAX_WIDTH: f32 = 920.;
const TIMELINE_MAX_WIDTH: f32 = 760.;
const COMMENT_MIN_ROWS: usize = 3;
const COMMENT_MAX_ROWS: usize = 12;
const DESCRIPTION_AVATAR: f32 = 20.;
const DESCRIPTION_FOLDED_HEIGHT: f32 = 140.;
const LONG_DESCRIPTION_LINES: usize = 8;
const LONG_DESCRIPTION_CHARS: usize = 600;

pub enum ConversationEvent {
    Commented,
    CommentConfirmed,
    Recheck,
    OpenOnGithub,
    OpenCommits,
    OpenChecks,
    RerunFailed,
    StartReview,
    Reveal { path: String, line: u32, side: Side },
}

pub enum ComposerEvent {
    Commented,
    CommentConfirmed,
    Recheck,
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
    description_open: bool,
    description_long: bool,
    pub(super) bodies: Vec<Option<MarkdownBlock>>,
    pub(super) thread_bodies: Vec<Vec<MarkdownBlock>>,
    pub(super) hunks: Vec<Option<String>>,
    pub(super) filter: ActivityFilter,
    pub(super) order: ActivityOrder,
    pub(super) open_folds: HashSet<SharedString>,
    pub(super) runs: Option<Rc<Vec<WorkflowRun>>>,
    pub(super) error_line: Option<SharedString>,
    pub(super) viewer: Option<String>,
    pub(super) composer: Entity<Composer>,
    scroll: ScrollHandle,
    _composer_events: Subscription,
}

impl EventEmitter<ConversationEvent> for Conversation {}

impl Conversation {
    pub fn new(
        key: PrKey,
        viewer: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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
            description_open: false,
            description_long: false,
            bodies: Vec::new(),
            thread_bodies: Vec::new(),
            hunks: Vec::new(),
            filter: ActivityFilter::default(),
            order: ActivityOrder::default(),
            open_folds: HashSet::new(),
            runs: None,
            error_line: None,
            viewer,
            composer,
            scroll: ScrollHandle::new(),
            _composer_events: composer_events,
        }
    }

    pub fn set_detail(&mut self, detail: Rc<PrDetail>, cx: &mut Context<Self>) {
        self.bodies = detail
            .timeline
            .iter()
            .map(|item| match item {
                TimelineItem::Comment { id, body, .. } => {
                    Some(visible_block(format!("pr-comment-{id}"), body))
                }
                TimelineItem::Review { id, body, .. } if !body.trim().is_empty() => {
                    Some(visible_block(format!("pr-review-{id}"), body))
                }
                _ => None,
            })
            .collect();
        self.thread_bodies = detail
            .threads
            .iter()
            .map(|thread| {
                thread
                    .comments
                    .iter()
                    .map(|comment| {
                        visible_block(format!("pr-thread-comment-{}", comment.id), &comment.body)
                    })
                    .collect()
            })
            .collect();
        self.hunks = thread_hunks(&detail);
        let description = strip_html_comments(&detail.body);
        self.description_long = is_long_description(&description);
        self.description = (!description.trim().is_empty())
            .then(|| MarkdownBlock::new("pr-description".to_string(), &description));
        self.composer.update(cx, |composer, cx| {
            composer.set_timeline(&detail.timeline, cx)
        });
        self.detail = Some(detail);
        cx.notify();
    }

    pub fn set_checks(
        &mut self,
        runs: Option<Rc<Vec<WorkflowRun>>>,
        error_line: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let same_runs = match (&self.runs, &runs) {
            (Some(known), Some(new)) => Rc::ptr_eq(known, new),
            (None, None) => true,
            _ => false,
        };
        if same_runs && self.error_line == error_line {
            return;
        }
        self.runs = runs;
        self.error_line = error_line;
        cx.notify();
    }

    pub fn set_viewer(&mut self, viewer: Option<String>, cx: &mut Context<Self>) {
        if self.viewer != viewer {
            self.viewer = viewer;
            cx.notify();
        }
    }

    pub(super) fn toggle_order(&mut self, cx: &mut Context<Self>) {
        self.order = match self.order {
            ActivityOrder::NewestFirst => ActivityOrder::OldestFirst,
            ActivityOrder::OldestFirst => ActivityOrder::NewestFirst,
        };
        cx.notify();
    }

    pub(super) fn set_filter(&mut self, filter: ActivityFilter, cx: &mut Context<Self>) {
        if self.filter != filter {
            self.filter = filter;
            cx.notify();
        }
    }

    pub(super) fn toggle_fold(&mut self, key: SharedString, cx: &mut Context<Self>) {
        if !self.open_folds.remove(&key) {
            self.open_folds.insert(key);
        }
        cx.notify();
    }

    fn description(&self, detail: &PrDetail, now: DateTime<Utc>, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let pr = &detail.pr;
        let header = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(ui::avatar(&pr.author, DESCRIPTION_AVATAR))
            .child(
                div()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(labels::DESCRIPTION),
            )
            .child(labels::opened_ago(&pr.author, &ui::ago(now, pr.created_at)));
        let Some(description) = &self.description else {
            return v_flex().w_full().gap_2().child(header).child(
                div()
                    .italic()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(labels::NO_DESCRIPTION),
            );
        };
        let long = self.description_long;
        let folded = long && !self.description_open;
        let body = div().w_full().min_w_0().text_sm().child(description.view());
        let body = if folded {
            body.max_h(px(DESCRIPTION_FOLDED_HEIGHT)).overflow_hidden()
        } else {
            body
        };
        let toggle = long.then(|| {
            let label = if folded {
                labels::SHOW_DESCRIPTION
            } else {
                labels::HIDE_DESCRIPTION
            };
            h_flex().child(
                link("pr-description-toggle", label, cx).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.description_open = !this.description_open;
                        cx.notify();
                    },
                )),
            )
        });
        v_flex()
            .w_full()
            .gap_2()
            .child(header)
            .child(body)
            .children(toggle)
    }
}

pub fn is_long_description(body: &str) -> bool {
    let body = body.trim();
    body.lines().count() > LONG_DESCRIPTION_LINES || body.chars().count() > LONG_DESCRIPTION_CHARS
}

fn visible_block(id: String, body: &str) -> MarkdownBlock {
    MarkdownBlock::new(id, &strip_html_comments(body))
}

fn thread_hunks(detail: &PrDetail) -> Vec<Option<String>> {
    let mut known: HashMap<(&str, &str), Option<&str>> = HashMap::new();
    let comments = detail
        .timeline
        .iter()
        .filter_map(|item| match item {
            TimelineItem::Review { comments, .. } => Some(comments),
            _ => None,
        })
        .flatten();
    for comment in comments {
        let hunk = comment.diff_hunk.as_str();
        known
            .entry((comment.path.as_str(), comment.body.as_str()))
            .and_modify(|seen| {
                if *seen != Some(hunk) {
                    *seen = None;
                }
            })
            .or_insert(Some(hunk));
    }
    detail
        .threads
        .iter()
        .map(|thread| {
            let first = thread.comments.first()?;
            known
                .get(&(thread.path.as_str(), first.body.as_str()))
                .copied()
                .flatten()
                .filter(|hunk| !hunk.is_empty())
                .map(str::to_string)
        })
        .collect()
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
        let content = v_flex()
            .w_full()
            .min_w_0()
            .max_w(px(TIMELINE_MAX_WIDTH))
            .mx_auto()
            .px_4()
            .py_4()
            .gap_5()
            .text_color(cx.theme().foreground)
            .children(self.readiness_card(&detail, cx))
            .child(self.description(&detail, now, cx))
            .child(self.activity_section(&detail, now, cx));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_descriptions_stay_open() {
        assert!(!is_long_description(""));
        assert!(!is_long_description("Uma frase.\n\nOutra frase."));
    }

    #[test]
    fn many_lines_make_a_description_long() {
        let body = (0..9)
            .map(|ix| format!("linha {ix}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(is_long_description(&body));
    }

    #[test]
    fn one_very_long_paragraph_is_long() {
        assert!(is_long_description(&"palavra ".repeat(80)));
    }
}
