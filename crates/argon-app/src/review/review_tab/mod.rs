mod load;
mod view;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use argon_core::Side;
use argon_github::PrDetail;
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, SharedString, Subscription, Task, Window,
};

use super::diff_view::{BlockSource, DiffView, DiffViewEvent, InlineBlock};
use super::file_tree::{FileTree, FileTreeEvent};
use super::labels;
use super::threads::{ThreadUi, file_threads};

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
    tree: Entity<FileTree>,
    diff: Entity<DiffView>,
    thread_ui: Entity<ThreadUi>,
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
            cx.subscribe(&diff, |this, _, event, cx| match event {
                DiffViewEvent::ViewedToggled { path, viewed } => {
                    this.toggle_viewed(path.clone(), *viewed, cx);
                }
                DiffViewEvent::GutterClicked {
                    path,
                    line,
                    side,
                    start,
                } => tracing::debug!(path, line, ?side, ?start, "diff gutter clicked"),
            }),
            cx.observe(&thread_ui, |this, _, cx| {
                this.diff.update(cx, |_, cx| cx.notify());
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
            tree,
            diff,
            thread_ui,
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

    pub fn set_scope(&mut self, scope: ReviewScope, cx: &mut Context<Self>) {
        if scope != self.scope {
            self.files = Files::Idle;
            self.scope = scope;
        }
        self.notice = None;
        if self.active && matches!(self.files, Files::Idle | Files::Failed(_)) {
            self.load_files(false, cx);
        }
        cx.notify();
    }

    pub fn reveal(&mut self, path: &str, line: u32, side: Side, cx: &mut Context<Self>) {
        self.diff
            .update(cx, |diff, cx| diff.reveal(path, line, side, cx));
    }

    #[expect(
        dead_code,
        reason = "drafts and the inline comment editor plug in here in Task 16"
    )]
    pub fn set_blocks(
        &mut self,
        path: &str,
        source: BlockSource,
        blocks: Vec<InlineBlock>,
        cx: &mut Context<Self>,
    ) {
        self.diff
            .update(cx, |diff, cx| diff.set_blocks(path, source, blocks, cx));
    }

    fn sync_threads(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.files, Files::Ready) {
            return;
        }
        let mut threads = match (&self.scope, &self.detail) {
            (ReviewScope::All, Some(detail)) => file_threads(&detail.threads, &self.thread_ui),
            _ => HashMap::new(),
        };
        let paths: Vec<String> = self
            .diff
            .read(cx)
            .files()
            .iter()
            .map(|file| file.meta.path.clone())
            .collect();
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
