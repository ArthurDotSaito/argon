use argon_core::{DraftComment, DraftSource, Side, single_comment_submission};
use futures::FutureExt as _;
use futures::future::LocalBoxFuture;
use gpui_kit::component::notification::Notification;
use gpui_kit::{App, AppContext as _, Context, Entity, SharedString, Subscription, Window};

use super::{ReviewScope, ReviewTab, ReviewTabEvent};
use crate::review::comment_editor::{CommentEditor, CommentEditorEvent, EditorAction, EditorKind};
use crate::review::confirm::{Confirm, confirm};
use crate::review::drafts::comment_body;
use crate::review::labels;
use crate::review::mutation::run_detached;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorTarget {
    Line {
        path: String,
        anchor: (u32, Side),
        start: Option<(u32, Side)>,
        draft: Option<i64>,
    },
    Reply {
        thread: String,
    },
}

pub struct OpenEditor {
    pub(super) entity: Entity<CommentEditor>,
    pub(super) target: EditorTarget,
    initial: Option<String>,
    _subscription: Subscription,
}

impl OpenEditor {
    fn dirty(&self, cx: &App) -> bool {
        self.entity.read(cx).body(cx) != self.initial
    }
}

type Job = LocalBoxFuture<'static, Result<bool, SharedString>>;

impl ReviewTab {
    pub(super) fn open_line_editor(
        &mut self,
        path: String,
        anchor: (u32, Side),
        start: Option<(u32, Side)>,
        crossed_hunks: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.scope != ReviewScope::All {
            return;
        }
        let target = EditorTarget::Line {
            path,
            anchor,
            start,
            draft: None,
        };
        let notice = crossed_hunks.then(|| labels::CROSSED_HUNKS.into());
        self.request_editor(target, String::new(), notice, window, cx);
    }

    pub(super) fn open_reply_editor(
        &mut self,
        thread: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_editor(
            EditorTarget::Reply { thread },
            String::new(),
            None,
            window,
            cx,
        );
    }

    pub(super) fn request_editor(
        &mut self,
        target: EditorTarget,
        initial: String,
        notice: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = &self.editor {
            if open.entity.read(cx).is_busy() {
                return;
            }
            if open.target == target {
                open.entity.update(cx, |editor, cx| editor.focus(cx));
                return;
            }
            if open.dirty(cx) {
                let tab = cx.weak_entity();
                let request = Confirm {
                    title: labels::DISCARD_COMMENT_TITLE,
                    message: labels::DISCARD_COMMENT_MESSAGE.to_string(),
                    confirm: labels::DISCARD,
                };
                confirm(request, window, cx, move |window, cx| {
                    _ = tab.update(cx, |tab, cx| {
                        tab.replace_editor(target.clone(), &initial, notice.clone(), window, cx)
                    });
                });
                return;
            }
        }
        self.replace_editor(target, &initial, notice, window, cx);
    }

    fn replace_editor(
        &mut self,
        target: EditorTarget,
        initial: &str,
        notice: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_editor(cx);
        let (kind, caption) = match &target {
            EditorTarget::Line {
                anchor,
                start,
                draft,
                ..
            } => (
                if draft.is_some() {
                    EditorKind::EditDraft
                } else {
                    EditorKind::NewComment
                },
                Some(labels::editor_target(anchor.0, start.map(|start| start.0)).into()),
            ),
            EditorTarget::Reply { .. } => (EditorKind::Reply, None),
        };
        let entity = cx.new(|cx| CommentEditor::new(kind, caption, notice, initial, window, cx));
        let subscription = cx.subscribe_in(&entity, window, |this, _, event, window, cx| {
            this.on_editor_event(event, window, cx)
        });
        if let EditorTarget::Reply { thread } = &target {
            let reply = Some((thread.clone(), entity.clone()));
            self.thread_ui.update(cx, |ui, cx| ui.set_reply(reply, cx));
        }
        self.editor = Some(OpenEditor {
            entity,
            target,
            initial: comment_body(initial),
            _subscription: subscription,
        });
        self.sync_drafts(cx);
        self.sync_editor(cx);
        cx.notify();
    }

    pub(super) fn close_editor(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.editor.take() else {
            return;
        };
        if let EditorTarget::Reply { .. } = open.target {
            self.thread_ui.update(cx, |ui, cx| ui.set_reply(None, cx));
        }
        self.sync_drafts(cx);
        self.sync_editor(cx);
        cx.notify();
    }

    fn on_editor_event(
        &mut self,
        event: &CommentEditorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            CommentEditorEvent::Submit(action) => self.submit_editor(*action, window, cx),
            CommentEditorEvent::Cancel => {
                let Some(open) = &self.editor else {
                    return;
                };
                if !open.dirty(cx) {
                    self.close_editor(cx);
                    return;
                }
                let tab = cx.weak_entity();
                let request = Confirm {
                    title: labels::DISCARD_COMMENT_TITLE,
                    message: labels::DISCARD_COMMENT_MESSAGE.to_string(),
                    confirm: labels::DISCARD,
                };
                confirm(request, window, cx, move |_, cx| {
                    _ = tab.update(cx, |tab, cx| tab.close_editor(cx));
                });
            }
        }
    }

    fn submit_editor(&mut self, action: EditorAction, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = &self.editor else {
            return;
        };
        let Some(body) = open.entity.read(cx).body(cx) else {
            return;
        };
        let entity = open.entity.clone();
        let target = open.target.clone();
        let Some(number) = self.detail.as_ref().map(|detail| detail.pr.number) else {
            return;
        };
        let job = match self.editor_job(action, target, body, cx) {
            Ok(job) => job,
            Err(error) => {
                entity.update(cx, |editor, cx| editor.fail(error, cx));
                return;
            }
        };
        entity.update(cx, |editor, cx| editor.set_busy(Some(action), cx));
        let applied = entity.clone();
        run_detached(
            window.window_handle(),
            cx,
            job,
            move |this, result, cx| match result {
                Ok(reload) => {
                    if this
                        .editor
                        .as_ref()
                        .is_some_and(|open| open.entity == applied)
                    {
                        this.close_editor(cx);
                    } else {
                        applied.update(cx, |editor, cx| editor.set_busy(None, cx));
                    }
                    if reload {
                        cx.emit(ReviewTabEvent::Reload);
                    }
                }
                Err(error) => applied.update(cx, |editor, cx| editor.fail(error, cx)),
            },
            move |result| match result {
                Ok(_) if action == EditorAction::Reply => {
                    Notification::success(labels::reply_sent_in(number))
                }
                Ok(_) => Notification::success(labels::comment_sent_in(number)),
                Err(error) => Notification::error(labels::mutation_failed_in(number, &error)),
            },
        );
    }

    fn editor_job(
        &mut self,
        action: EditorAction,
        target: EditorTarget,
        body: String,
        cx: &mut Context<Self>,
    ) -> Result<Job, SharedString> {
        let unavailable = || SharedString::from(labels::SERVICES_UNAVAILABLE);
        let detail = self.detail.clone().ok_or_else(unavailable)?;
        match (action, target) {
            (
                EditorAction::Save,
                EditorTarget::Line {
                    draft: Some(id), ..
                },
            ) => {
                let task = self
                    .drafts
                    .update(cx, |drafts, cx| drafts.update_body(id, body, cx));
                Ok(task.map(|result| result.map(|()| false)).boxed_local())
            }
            (
                action @ (EditorAction::AddToReview | EditorAction::CommentNow),
                EditorTarget::Line {
                    path,
                    anchor,
                    start,
                    draft: None,
                },
            ) => {
                let draft = DraftComment {
                    id: 0,
                    pr: detail.pr.key(),
                    path,
                    line: anchor.0,
                    side: anchor.1,
                    start_line: start.map(|start| start.0),
                    start_side: start.map(|start| start.1),
                    body,
                    source: DraftSource::User,
                    session_id: None,
                    head_oid: self.loaded_head.clone(),
                };
                if action == EditorAction::AddToReview {
                    let task = self.drafts.update(cx, |drafts, cx| drafts.add(draft, cx));
                    return Ok(task.map(|result| result.map(|()| false)).boxed_local());
                }
                let head = self.displayed_head(cx).ok_or(labels::DIFF_STALE)?;
                let submission = single_comment_submission(&draft);
                let id = detail.pr.id.clone();
                let task = self
                    .github_detached(move |client| async move {
                        client.submit_review(&id, &head, &submission).await
                    })
                    .ok_or_else(unavailable)?;
                Ok(task
                    .map(|result| {
                        result
                            .map(|_| true)
                            .map_err(|error| labels::comment_failed(&error.to_string()).into())
                    })
                    .boxed_local())
            }
            (EditorAction::Reply, EditorTarget::Reply { thread }) => {
                let task = self
                    .github_detached(move |client| async move {
                        client.reply_thread(&thread, &body).await
                    })
                    .ok_or_else(unavailable)?;
                Ok(task
                    .map(|result| {
                        result
                            .map(|()| true)
                            .map_err(|error| labels::reply_failed(&error.to_string()).into())
                    })
                    .boxed_local())
            }
            _ => Err(unavailable()),
        }
    }
}
