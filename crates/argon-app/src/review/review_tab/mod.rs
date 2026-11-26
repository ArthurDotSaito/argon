mod blocks;
mod editor;
mod load;
mod mutations;
mod view;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use argon_core::Side;
use argon_github::PrDetail;
use gpui_kit::component::resizable::ResizableState;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, SharedString, Subscription, Task, Window,
};

use super::composer::ReviewComposer;
use super::diff_view::{BlockSource, DiffView, DiffViewEvent};
use super::drafts::DraftsModel;
use super::file_tree::{FileTree, FileTreeEvent};
use super::labels;
use super::threads::{ThreadUi, ThreadUiEvent, file_threads};
use crate::services::{AppServices, Services};
use editor::{EditorTarget, OpenEditor};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ReviewScope {
    #[default]
    All,
    Commit {
        oid: String,
        headline: String,
    },
}

pub enum ReviewTabEvent {
    ShowAll,
    Reload,
}

enum Files {
    Idle,
    Loading,
    Ready,
    Failed(SharedString),
}

pub struct ReviewTab {
    detail: Option<Rc<PrDetail>>,
    scope: ReviewScope,
    active: bool,
    files: Files,
    loaded_head: Option<String>,
    stale: bool,
    outside_comments: usize,
    notice: Option<SharedString>,
    error: Option<SharedString>,
    epoch: u64,
    focus_pending: bool,
    pending_reveal: Option<(String, u32, Side)>,
    tree: Entity<FileTree>,
    diff: Entity<DiffView>,
    thread_ui: Entity<ThreadUi>,
    drafts: Entity<DraftsModel>,
    editor: Option<OpenEditor>,
    viewer_login: Option<String>,
    composer: Option<(Entity<ReviewComposer>, Subscription)>,
    split: Entity<ResizableState>,
    services: Option<Services>,
    load_task: Option<Task<()>>,
    viewed_inflight: HashSet<String>,
    viewed_tasks: HashMap<String, Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ReviewTabEvent> for ReviewTab {}

impl ReviewTab {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tree = cx.new(|cx| FileTree::new(window, cx));
        let diff = cx.new(DiffView::new);
        let thread_ui = cx.new(|_| ThreadUi::default());
        let drafts = cx.new(DraftsModel::new);
        let subscriptions = vec![
            cx.subscribe(&tree, |this, _, event, cx| match event {
                FileTreeEvent::Open(path) => {
                    this.diff
                        .update(cx, |diff, cx| diff.scroll_to_file(path, cx));
                }
                FileTreeEvent::ToggleViewed { path, viewed } => {
                    this.toggle_viewed(path.clone(), *viewed, cx);
                }
            }),
            cx.subscribe_in(&diff, window, |this, _, event, window, cx| match event {
                DiffViewEvent::ViewedToggled { path, viewed } => {
                    this.toggle_viewed(path.clone(), *viewed, cx);
                }
                DiffViewEvent::GutterClicked {
                    path,
                    line,
                    side,
                    start,
                    crossed_hunks,
                } => this.open_line_editor(
                    path.clone(),
                    (*line, *side),
                    *start,
                    *crossed_hunks,
                    window,
                    cx,
                ),
            }),
            cx.observe(&thread_ui, |this, _, cx| {
                this.diff.update(cx, |_, cx| cx.notify());
            }),
            cx.subscribe_in(
                &thread_ui,
                window,
                |this, _, event, window, cx| match event {
                    ThreadUiEvent::Reply(id) => this.open_reply_editor(id.clone(), window, cx),
                    ThreadUiEvent::SetResolved { id, resolved } => {
                        this.set_thread_resolved(id.clone(), *resolved, window, cx)
                    }
                },
            ),
            cx.observe(&drafts, |this, _, cx| {
                this.sync_drafts(cx);
                cx.notify();
            }),
            cx.subscribe_in(&drafts, window, |this, _, event, window, cx| {
                this.on_drafts_event(event, window, cx)
            }),
        ];
        Self {
            detail: None,
            scope: ReviewScope::All,
            active: false,
            files: Files::Idle,
            loaded_head: None,
            stale: false,
            outside_comments: 0,
            notice: None,
            error: None,
            epoch: 0,
            focus_pending: false,
            pending_reveal: None,
            tree,
            diff,
            thread_ui,
            drafts,
            editor: None,
            viewer_login: None,
            composer: None,
            split: cx.new(|_| ResizableState::default()),
            services: cx
                .try_global::<AppServices>()
                .map(|global| global.0.clone()),
            load_task: None,
            viewed_inflight: HashSet::new(),
            viewed_tasks: HashMap::new(),
            _subscriptions: subscriptions,
        }
    }

    pub fn set_detail(&mut self, detail: Rc<PrDetail>, cx: &mut Context<Self>) {
        if let ReviewScope::Commit { oid, .. } = &self.scope
            && !detail.commits.iter().any(|commit| commit.oid == *oid)
        {
            self.notice = Some(labels::commit_gone(oid).into());
            self.scope = ReviewScope::All;
            self.files = Files::Idle;
        }
        let head_moved = self.scope == ReviewScope::All
            && matches!(self.files, Files::Ready)
            && self.loaded_head.as_deref() != Some(detail.head_oid.as_str());
        let (key, number, head) = (detail.pr.key(), detail.pr.number, detail.head_oid.clone());
        self.drafts
            .update(cx, |drafts, cx| drafts.set_pr(key, number, head, cx));
        self.thread_ui.update(cx, |ui, cx| ui.settle(cx));
        self.detail = Some(detail);
        self.stale |= head_moved;
        if self.active && matches!(self.files, Files::Idle) {
            self.load_files(false, cx);
        } else if self.active && self.stale {
            self.load_files(true, cx);
        } else {
            self.sync_threads(cx);
        }
        cx.notify();
    }

    pub fn count(&self) -> Option<usize> {
        self.detail
            .as_ref()
            .map(|detail| detail.pr.changed_files as usize)
    }

    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        self.focus_pending = active;
        if active && matches!(self.files, Files::Idle | Files::Failed(_)) {
            self.load_files(false, cx);
        } else if active && self.stale {
            self.load_files(true, cx);
        }
        cx.notify();
    }

    pub fn set_viewer_login(&mut self, login: Option<String>, cx: &mut Context<Self>) {
        if self.viewer_login != login {
            self.viewer_login = login;
            self.sync_drafts(cx);
            cx.notify();
        }
    }

    pub fn set_scope(&mut self, scope: ReviewScope, cx: &mut Context<Self>) {
        if scope != ReviewScope::All {
            if self
                .editor
                .as_ref()
                .is_some_and(|open| open.entity.read(cx).is_busy())
            {
                self.notice = Some(labels::WAIT_FOR_SEND.into());
                cx.notify();
                return;
            }
            self.close_editor(cx);
        }
        if scope != self.scope {
            self.files = Files::Idle;
            self.scope = scope;
        }
        self.notice = None;
        if !self.dock_visible(cx) {
            self.collapse_composer(None, cx);
        }
        if self.active && matches!(self.files, Files::Idle | Files::Failed(_)) {
            self.load_files(false, cx);
        }
        cx.notify();
    }

    pub fn reveal(&mut self, path: &str, line: u32, side: Side, cx: &mut Context<Self>) {
        if self.scope == ReviewScope::All && matches!(self.files, Files::Ready) {
            self.diff
                .update(cx, |diff, cx| diff.reveal(path, line, side, cx));
        } else {
            self.pending_reveal = Some((path.to_string(), line, side));
        }
    }

    pub fn selected_line(&self, cx: &App) -> Option<(String, u32)> {
        if let Some(line) = self.diff.read(cx).selected_line(cx) {
            return Some(line);
        }
        match self.editor.as_ref().map(|open| &open.target) {
            Some(EditorTarget::Line { path, anchor, .. }) => Some((path.clone(), anchor.0)),
            _ => None,
        }
    }

    fn dock_visible(&self, cx: &App) -> bool {
        self.scope == ReviewScope::All || self.drafts.read(cx).in_progress()
    }

    fn diff_paths(&self, cx: &Context<Self>) -> Vec<String> {
        self.diff
            .read(cx)
            .files()
            .iter()
            .map(|file| file.meta.path.clone())
            .collect()
    }

    fn sync_blocks(&mut self, cx: &mut Context<Self>) {
        self.sync_threads(cx);
        self.sync_drafts(cx);
        self.sync_editor(cx);
    }

    fn sync_threads(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.files, Files::Ready) {
            return;
        }
        let mut threads = match (&self.scope, &self.detail) {
            (ReviewScope::All, Some(detail)) => file_threads(&detail.threads, &self.thread_ui),
            _ => HashMap::new(),
        };
        let paths = self.diff_paths(cx);
        self.diff.update(cx, |diff, cx| {
            for path in &paths {
                let blocks = threads.remove(path).unwrap_or_default();
                diff.set_blocks(path, BlockSource::Threads, blocks, cx);
            }
        });
        self.outside_comments = match (&self.scope, &self.detail) {
            (ReviewScope::All, Some(detail)) => detail
                .threads
                .iter()
                .filter(|thread| !paths.contains(&thread.path))
                .count(),
            _ => 0,
        };
    }
}
