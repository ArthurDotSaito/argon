use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Subscription, Window, div,
};

use super::drafts::comment_body;
use super::labels;
use crate::actions::CancelComment;
use crate::keymap::COMMENT_EDITOR_CONTEXT;

const MIN_ROWS: usize = 3;
const MAX_ROWS: usize = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorKind {
    NewComment,
    EditDraft,
    Reply,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorAction {
    AddToReview,
    CommentNow,
    Save,
    Reply,
}

impl EditorKind {
    fn primary(self) -> EditorAction {
        match self {
            Self::NewComment => EditorAction::AddToReview,
            Self::EditDraft => EditorAction::Save,
            Self::Reply => EditorAction::Reply,
        }
    }
}

pub enum CommentEditorEvent {
    Submit(EditorAction),
    Cancel,
}

pub struct CommentEditor {
    kind: EditorKind,
    caption: Option<SharedString>,
    notice: Option<SharedString>,
    input: Entity<TextareaState>,
    busy: Option<EditorAction>,
    error: Option<SharedString>,
    focus_pending: bool,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<CommentEditorEvent> for CommentEditor {}

impl CommentEditor {
    pub fn new(
        kind: EditorKind,
        caption: Option<SharedString>,
        notice: Option<SharedString>,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = match kind {
            EditorKind::Reply => labels::REPLY_PLACEHOLDER,
            EditorKind::NewComment | EditorKind::EditDraft => labels::EDITOR_PLACEHOLDER,
        };
        let initial = initial.to_string();
        let input = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx)
                .placeholder(placeholder)
                .auto_grow(MIN_ROWS, MAX_ROWS);
            state.set_value(initial, window, cx);
            state
        });
        let subscriptions = [
            cx.observe(&input, |_, _, cx| cx.notify()),
            cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                if let InputEvent::PressEnter {
                    secondary: true, ..
                } = event
                {
                    this.request(this.kind.primary(), cx);
                }
            }),
        ];
        Self {
            kind,
            caption,
            notice,
            input,
            busy: None,
            error: None,
            focus_pending: true,
            _subscriptions: subscriptions,
        }
    }

    pub fn body(&self, cx: &App) -> Option<String> {
        comment_body(&self.input.read(cx).value())
    }

    pub fn has_text(&self, cx: &App) -> bool {
        self.body(cx).is_some()
    }

    pub fn is_busy(&self) -> bool {
        self.busy.is_some()
    }

    pub fn focus(&mut self, cx: &mut Context<Self>) {
        self.focus_pending = true;
        cx.notify();
    }

    pub fn set_busy(&mut self, busy: Option<EditorAction>, cx: &mut Context<Self>) {
        self.busy = busy;
        if busy.is_some() {
            self.error = None;
        }
        cx.notify();
    }

    pub fn fail(&mut self, error: SharedString, cx: &mut Context<Self>) {
        self.busy = None;
        self.error = Some(error);
        cx.notify();
    }

    fn request(&mut self, action: EditorAction, cx: &mut Context<Self>) {
        if self.busy.is_none() && self.has_text(cx) {
            cx.emit(CommentEditorEvent::Submit(action));
        }
    }

    fn button(&self, action: EditorAction, empty: bool, cx: &mut Context<Self>) -> Button {
        let (id, label) = match action {
            EditorAction::AddToReview => ("comment-editor-add", labels::ADD_TO_REVIEW),
            EditorAction::CommentNow => ("comment-editor-now", labels::COMMENT_NOW),
            EditorAction::Save => ("comment-editor-save", labels::SAVE),
            EditorAction::Reply => ("comment-editor-reply", labels::REPLY),
        };
        let button = Button::new(id)
            .label(label)
            .small()
            .loading(self.busy == Some(action))
            .disabled(empty || self.busy.is_some())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.request(action, cx)));
        match action {
            EditorAction::CommentNow => button.outline().tooltip(labels::COMMENT_NOW_TOOLTIP),
            EditorAction::AddToReview => button.primary().tooltip(labels::ADD_TO_REVIEW_SHORTCUT),
            EditorAction::Save | EditorAction::Reply => button.primary(),
        }
    }
}

impl Render for CommentEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.focus_pending) {
            let handle = self.input.focus_handle(cx);
            window.defer(cx, move |window, cx| window.focus(&handle, cx));
        }
        let empty = !self.has_text(cx);
        let actions = match self.kind {
            EditorKind::NewComment => vec![EditorAction::CommentNow, EditorAction::AddToReview],
            EditorKind::EditDraft => vec![EditorAction::Save],
            EditorKind::Reply => vec![EditorAction::Reply],
        };
        let buttons: Vec<Button> = actions
            .into_iter()
            .map(|action| self.button(action, empty, cx))
            .collect();
        let busy = self.busy.is_some();
        let theme = cx.theme();
        let caption = self.caption.clone().map(|caption| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(caption)
        });
        let notice = self
            .notice
            .clone()
            .map(|notice| div().text_xs().text_color(theme.warning).child(notice));
        let error = self.error.clone().map(|error| {
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(theme.danger)
                .child(error)
        });
        let cancel = Button::new("comment-editor-cancel")
            .label(labels::CANCEL)
            .small()
            .ghost()
            .disabled(busy)
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(CommentEditorEvent::Cancel)));
        v_flex()
            .key_context(COMMENT_EDITOR_CONTEXT)
            .on_action(cx.listener(|this, _: &CancelComment, _, cx| {
                if !this.is_busy() {
                    cx.emit(CommentEditorEvent::Cancel);
                }
            }))
            .w_full()
            .p_2()
            .gap_2()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.primary.opacity(0.6))
            .bg(theme.background)
            .text_sm()
            .children(caption)
            .children(notice)
            .child(Textarea::new(&self.input).disabled(busy))
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .children(error)
                    .child(div().flex_1())
                    .child(cancel)
                    .children(buttons),
            )
    }
}
