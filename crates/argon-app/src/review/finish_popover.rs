use argon_core::{ReviewEvent, Side};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, WindowExt as _, h_flex,
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, ClickEvent, Context, Div, Entity, EventEmitter, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::confirm::{Confirm, confirm};
use super::drafts::{DraftsModel, EventOptions, SubmitState, summarize};
use super::labels;

const DIALOG_WIDTH: f32 = 560.;
const LIST_MAX_HEIGHT: f32 = 180.;
const MIN_ROWS: usize = 4;
const MAX_ROWS: usize = 12;
const EVENTS: [ReviewEvent; 3] = [
    ReviewEvent::Comment,
    ReviewEvent::Approve,
    ReviewEvent::RequestChanges,
];

pub enum FinishEvent {
    Reveal { path: String, line: u32, side: Side },
}

pub struct FinishReview {
    drafts: Entity<DraftsModel>,
    input: Entity<TextareaState>,
    event: ReviewEvent,
    options: EventOptions,
    focus_pending: bool,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<FinishEvent> for FinishReview {}

pub fn open(
    drafts: Entity<DraftsModel>,
    options: EventOptions,
    window: &mut Window,
    cx: &mut App,
) -> Entity<FinishReview> {
    let finish = cx.new(|cx| FinishReview::new(drafts, options, window, cx));
    let content = finish.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let sending = content.read(cx).drafts.read(cx).is_sending();
        dialog
            .w(px(DIALOG_WIDTH))
            .title(labels::FINISH_REVIEW)
            .overlay_closable(false)
            .keyboard(!sending)
            .close_button(!sending)
            .child(content.clone())
    });
    finish
}

impl FinishReview {
    fn new(
        drafts: Entity<DraftsModel>,
        options: EventOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let review = drafts.read(cx).review().clone();
        let input = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx)
                .placeholder(labels::GENERAL_COMMENT_PLACEHOLDER)
                .auto_grow(MIN_ROWS, MAX_ROWS);
            state.set_value(review.body, window, cx);
            state
        });
        let subscriptions = [
            cx.observe(&drafts, |_, _, cx| cx.notify()),
            cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.save(cx);
                }
            }),
        ];
        Self {
            drafts,
            input,
            event: options.coerce(review.event),
            options,
            focus_pending: true,
            _subscriptions: subscriptions,
        }
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

    fn submit(&mut self, cx: &mut Context<Self>) {
        let body = self.body(cx);
        let event = self.event;
        self.drafts
            .update(cx, |drafts, cx| drafts.submit(body, event, cx));
    }

    fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let drafts = self.drafts.clone();
        let count = drafts.read(cx).drafts().len();
        confirm(
            Confirm {
                title: labels::DISCARD_REVIEW_TITLE,
                message: labels::discard_review_message(count),
                confirm: labels::DISCARD,
            },
            window,
            cx,
            move |window, cx| {
                window.close_dialog(cx);
                drafts.update(cx, |drafts, cx| drafts.discard(cx));
            },
        );
    }

    fn events(&self, sending: bool, cx: &mut Context<Self>) -> RadioGroup {
        let options = self.options;
        let radios = EVENTS.iter().map(|event| {
            let (label, hint) = match event {
                ReviewEvent::Comment => (labels::EVENT_COMMENT, labels::EVENT_COMMENT_HINT),
                ReviewEvent::Approve => (labels::EVENT_APPROVE, labels::EVENT_APPROVE_HINT),
                ReviewEvent::RequestChanges => (
                    labels::EVENT_REQUEST_CHANGES,
                    labels::EVENT_REQUEST_CHANGES_HINT,
                ),
            };
            let allowed = options.allows(*event);
            Radio::new(label)
                .label(format!("{label} — {hint}"))
                .disabled(!allowed)
                .tooltip(if allowed { hint } else { labels::OWN_PR })
        });
        RadioGroup::vertical("finish-review-event")
            .selected_index(EVENTS.iter().position(|event| *event == self.event))
            .disabled(sending)
            .children(radios)
            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                if let Some(event) = EVENTS.get(*ix).copied() {
                    this.set_event(event, cx);
                }
            }))
    }

    fn pending(&self, sending: bool, cx: &mut Context<Self>) -> Div {
        let items = summarize(self.drafts.read(cx).drafts());
        let theme = cx.theme();
        let title = div()
            .font_semibold()
            .text_color(theme.foreground)
            .child(format!("{} ({})", labels::PENDING_COMMENTS, items.len()));
        if items.is_empty() {
            return v_flex().gap_1().child(title).child(
                div()
                    .text_color(theme.muted_foreground)
                    .child(labels::NO_PENDING),
            );
        }
        let link = theme.link;
        let rows = items.into_iter().enumerate().map(|(ix, item)| {
            let (path, line, side) = (item.path, item.line, item.side);
            h_flex()
                .id(("finish-draft", ix))
                .gap_1p5()
                .items_center()
                .text_color(link)
                .cursor_pointer()
                .hover(|style| style.underline())
                .child(Icon::new(IconName::MessageSquare).xsmall())
                .child(div().min_w_0().truncate().child(item.label))
                .when(!sending, |row| {
                    row.on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                        window.close_dialog(cx);
                        cx.emit(FinishEvent::Reveal {
                            path: path.clone(),
                            line,
                            side,
                        });
                    }))
                })
        });
        v_flex().gap_1().child(title).child(
            v_flex()
                .id("finish-review-drafts")
                .max_h(px(LIST_MAX_HEIGHT))
                .overflow_y_scroll()
                .gap_1()
                .children(rows),
        )
    }

    fn footer(&self, sending: bool, cx: &mut Context<Self>) -> Div {
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                Button::new("finish-review-discard")
                    .icon(Icon::new(IconName::Trash))
                    .label(labels::DISCARD_REVIEW)
                    .small()
                    .ghost()
                    .disabled(sending)
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, window, cx| this.discard(window, cx)),
                    ),
            )
            .child(div().flex_1())
            .child(
                Button::new("finish-review-cancel")
                    .label(labels::CANCEL)
                    .small()
                    .ghost()
                    .disabled(sending)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("finish-review-submit")
                    .icon(Icon::new(IconName::Send))
                    .label(labels::SUBMIT_REVIEW)
                    .small()
                    .primary()
                    .loading(sending)
                    .disabled(sending)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.submit(cx))),
            )
    }
}

impl Render for FinishReview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.focus_pending) {
            let handle = self.input.focus_handle(cx);
            window.defer(cx, move |window, cx| window.focus(&handle, cx));
        }
        let state = self.drafts.read(cx).submit_state().clone();
        let sending = state == SubmitState::Sending;
        let events = self.events(sending, cx);
        let pending = self.pending(sending, cx);
        let footer = self.footer(sending, cx);
        let theme = cx.theme();
        let own_pr = (!self.options.judge).then(|| {
            h_flex()
                .gap_1p5()
                .items_center()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(Icon::new(IconName::Info).xsmall())
                .child(labels::OWN_PR)
        });
        let error = match state {
            SubmitState::Failed(message) => Some(
                h_flex()
                    .gap_2()
                    .items_start()
                    .text_color(theme.danger)
                    .child(Icon::new(IconName::CircleAlert).small())
                    .child(div().flex_1().min_w_0().child(message)),
            ),
            SubmitState::Idle | SubmitState::Sending => None,
        };
        v_flex()
            .w_full()
            .gap_3()
            .text_sm()
            .child(Textarea::new(&self.input).disabled(sending))
            .child(events)
            .children(own_pr)
            .child(pending)
            .children(error)
            .child(footer)
    }
}
