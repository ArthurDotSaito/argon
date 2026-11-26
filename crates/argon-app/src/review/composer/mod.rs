mod view;

use argon_core::{ReviewEvent, Side};
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Subscription, Window,
};

use super::confirm::{Confirm, confirm};
use super::drafts::{DraftsModel, EventOptions};
use super::labels;

const EVENTS: [ReviewEvent; 3] = [
    ReviewEvent::Comment,
    ReviewEvent::Approve,
    ReviewEvent::RequestChanges,
];

pub enum ComposerEvent {
    Reveal { path: String, line: u32, side: Side },
    Reload,
    Collapse,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ComposerTab {
    Write,
    Preview,
}

pub struct ReviewComposer {
    drafts: Entity<DraftsModel>,
    input: Entity<TextareaState>,
    event: ReviewEvent,
    options: EventOptions,
    tab: ComposerTab,
    comments_open: bool,
    suspects: Vec<i64>,
    focus_handle: FocusHandle,
    focus_pending: bool,
    _subscriptions: [Subscription; 3],
}

impl EventEmitter<ComposerEvent> for ReviewComposer {}

impl ReviewComposer {
    pub fn new(
        drafts: Entity<DraftsModel>,
        options: EventOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let review = drafts.read(cx).review().clone();
        let suspects = drafts.read(cx).suspects().to_vec();
        let input = cx.new(|cx| {
            let mut state =
                TextareaState::new(window, cx).placeholder(labels::GENERAL_COMMENT_PLACEHOLDER);
            state.set_value(review.body, window, cx);
            state
        });
        let subscriptions =
            [
                cx.observe(&drafts, |this, drafts, cx| {
                    let suspects = drafts.read(cx).suspects();
                    if suspects != this.suspects.as_slice() {
                        this.suspects = suspects.to_vec();
                        this.comments_open |= !this.suspects.is_empty();
                    }
                    cx.notify();
                }),
                cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::Change => this.save(cx),
                        InputEvent::PressEnter {
                            secondary: true, ..
                        } => this.submit(window, cx),
                        _ => {}
                    }
                }),
                cx.observe(&input, |_, _, cx| cx.notify()),
            ];
        Self {
            drafts,
            input,
            event: options.coerce(review.event),
            options,
            tab: ComposerTab::Write,
            comments_open: !suspects.is_empty(),
            suspects,
            focus_handle: cx.focus_handle(),
            focus_pending: true,
            _subscriptions: subscriptions,
        }
    }

    pub fn focus(&mut self, cx: &mut Context<Self>) {
        self.tab = ComposerTab::Write;
        self.focus_pending = true;
        cx.notify();
    }

    pub fn without_focus(&mut self) {
        self.focus_pending = false;
    }

    pub fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus_handle.contains_focused(window, cx)
    }

    fn body(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let body = self.body(cx);
        let event = self.event;
        self.drafts
            .update(cx, |drafts, cx| drafts.save_review(body, event, cx));
    }

    fn set_event(&mut self, event: ReviewEvent, cx: &mut Context<Self>) {
        if self.options.allows(event) && event != self.event {
            self.event = event;
            self.save(cx);
            cx.notify();
        }
    }

    fn set_tab(&mut self, tab: ComposerTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.focus_pending = true;
            cx.notify();
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let drafts = self.drafts.read(cx);
        if drafts.is_sending() || drafts.stale() {
            return;
        }
        let body = self.body(cx);
        let event = self.event;
        let handle = window.window_handle();
        self.drafts
            .update(cx, |drafts, cx| drafts.submit(body, event, handle, cx));
    }

    fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let drafts = self.drafts.clone();
        let this = cx.entity().downgrade();
        let count = drafts.read(cx).drafts().len();
        confirm(
            Confirm {
                title: labels::DISCARD_REVIEW_TITLE,
                message: labels::discard_review_message(count),
                confirm: labels::DISCARD,
            },
            window,
            cx,
            move |_, cx| {
                drafts.update(cx, |drafts, cx| drafts.discard(cx));
                _ = this.update(cx, |_, cx| cx.emit(ComposerEvent::Collapse));
            },
        );
    }
}
