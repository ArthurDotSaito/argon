mod header;
mod items;
mod layout;
mod render;
mod row;
mod selection;

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use argon_core::Side;
use gpui_kit::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, ListAlignment, ListOffset,
    ListState, SharedString, Task, Window, px,
};

pub use items::DiffMode;
use items::{BlockAnchor, Item};
use selection::{Drag, Flash, Selection};

use super::file_diff::FileDiff;
use super::highlight::FileHighlights;

const LIST_OVERDRAW: f32 = 600.;
const REVEAL_CONTEXT_ROWS: usize = 6;
const FLASH_DURATION: Duration = Duration::from_millis(1500);

pub enum DiffViewEvent {
    GutterClicked {
        path: String,
        line: u32,
        side: Side,
        start: Option<(u32, Side)>,
    },
    ViewedToggled {
        path: String,
        viewed: bool,
    },
}

pub type BlockRender = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

#[derive(Clone)]
pub struct InlineBlock {
    pub anchor: (u32, Side),
    pub start: Option<(u32, Side)>,
    pub key: SharedString,
    pub render: BlockRender,
}

impl InlineBlock {
    pub fn new(anchor: (u32, Side), key: impl Into<SharedString>, render: BlockRender) -> Self {
        Self {
            anchor,
            start: None,
            key: key.into(),
            render,
        }
    }

    pub fn with_start(mut self, start: Option<(u32, Side)>) -> Self {
        self.start = start.filter(|start| *start != self.anchor);
        self
    }

    fn layout(&self) -> BlockAnchor {
        BlockAnchor {
            anchor: self.anchor,
            start: self.start,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ScrollTarget {
    File(usize),
    Anchor {
        file: usize,
        anchor: (u32, Side),
        context: usize,
    },
}

#[derive(Default)]
struct FileBlocks {
    anchored: Vec<InlineBlock>,
    outdated: Vec<InlineBlock>,
}

pub struct DiffView {
    files: Vec<FileDiff>,
    index: HashMap<String, usize>,
    blocks: HashMap<String, FileBlocks>,
    mode: DiffMode,
    items: Vec<Item>,
    ranges: Vec<Range<usize>>,
    dirty: BTreeSet<usize>,
    list: ListState,
    ready: bool,
    viewed_enabled: bool,
    selection: Option<Selection>,
    drag: Option<Drag>,
    flash: Option<Flash>,
    flash_seq: u64,
    pending_reveal: Option<(String, u32, Side)>,
    pending_scroll: Option<ScrollTarget>,
    github_url: Option<String>,
    focus_handle: FocusHandle,
    flash_task: Option<Task<()>>,
}

impl EventEmitter<DiffViewEvent> for DiffView {}

impl Focusable for DiffView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl DiffView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            files: Vec::new(),
            index: HashMap::new(),
            blocks: HashMap::new(),
            mode: DiffMode::default(),
            items: Vec::new(),
            ranges: Vec::new(),
            dirty: BTreeSet::new(),
            list: ListState::new(0, ListAlignment::Top, px(LIST_OVERDRAW)),
            ready: false,
            viewed_enabled: true,
            selection: None,
            drag: None,
            flash: None,
            flash_seq: 0,
            pending_reveal: None,
            pending_scroll: None,
            github_url: None,
            focus_handle: cx.focus_handle(),
            flash_task: None,
        }
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.files.clear();
        self.index.clear();
        self.items.clear();
        self.ranges.clear();
        self.dirty.clear();
        self.selection = None;
        self.drag = None;
        self.ready = false;
        self.pending_scroll = None;
        self.flash = None;
        self.list.reset(0);
        cx.notify();
    }

    pub fn set_files(&mut self, files: Vec<FileDiff>, cx: &mut Context<Self>) {
        self.index = files
            .iter()
            .enumerate()
            .map(|(ix, file)| (file.meta.path.clone(), ix))
            .collect();
        self.files = files;
        self.ready = true;
        self.selection = None;
        self.rebuild_all();
        self.list.scroll_to(ListOffset::default());
        if let Some((path, line, side)) = self.pending_reveal.take() {
            self.reveal(&path, line, side, cx);
        }
        cx.notify();
    }

    pub fn files(&self) -> &[FileDiff] {
        &self.files
    }

    pub fn set_github_url(&mut self, url: Option<String>) {
        self.github_url = url;
    }

    pub fn set_viewed_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.viewed_enabled = enabled;
        cx.notify();
    }

    pub fn set_highlights(
        &mut self,
        path: &str,
        highlights: Arc<FileHighlights>,
        cx: &mut Context<Self>,
    ) {
        if let Some(file) = self.file_mut(path) {
            file.highlights = Some(highlights);
            cx.notify();
        }
    }

    pub fn set_viewed(&mut self, path: &str, viewed: bool, cx: &mut Context<Self>) {
        let Some(&ix) = self.index.get(path) else {
            return;
        };
        let file = &mut self.files[ix];
        if file.viewed != viewed {
            file.viewed = viewed;
            file.collapsed = viewed;
            self.dirty.insert(ix);
        }
        cx.notify();
    }

    pub fn toggle_collapsed(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(file) = self.files.get_mut(ix) {
            file.collapsed = !file.collapsed;
            self.dirty.insert(ix);
            cx.notify();
        }
    }

    pub fn mode(&self) -> DiffMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        if mode == self.mode {
            return;
        }
        let keep = self.top_anchor();
        self.mode = mode;
        self.rebuild_all();
        self.pending_scroll = keep.map(|(file, anchor)| match anchor {
            Some(anchor) => ScrollTarget::Anchor {
                file,
                anchor,
                context: 0,
            },
            None => ScrollTarget::File(file),
        });
        cx.notify();
    }

    pub fn toggle_mode(&mut self, cx: &mut Context<Self>) {
        self.set_mode(self.mode.toggled(), cx);
    }

    pub fn set_inline_blocks(
        &mut self,
        path: &str,
        blocks: Vec<InlineBlock>,
        cx: &mut Context<Self>,
    ) {
        let entry = self.blocks.entry(path.to_string()).or_default();
        let same_layout = layout_key(&entry.anchored) == layout_key(&blocks);
        entry.anchored = blocks;
        if same_layout {
            cx.notify();
        } else {
            self.mark_dirty(path, cx);
        }
    }

    pub fn set_outdated_blocks(
        &mut self,
        path: &str,
        blocks: Vec<InlineBlock>,
        cx: &mut Context<Self>,
    ) {
        let entry = self.blocks.entry(path.to_string()).or_default();
        let same_layout = entry.outdated.len() == blocks.len();
        entry.outdated = blocks;
        if same_layout {
            cx.notify();
        } else {
            self.mark_dirty(path, cx);
        }
    }

    pub fn scroll_to_file(&mut self, path: &str, cx: &mut Context<Self>) {
        if let Some(&ix) = self.index.get(path) {
            self.pending_scroll = Some(ScrollTarget::File(ix));
            cx.notify();
        }
    }

    pub fn reveal(&mut self, path: &str, line: u32, side: Side, cx: &mut Context<Self>) {
        if !self.ready {
            self.pending_reveal = Some((path.to_string(), line, side));
            return;
        }
        let Some(&ix) = self.index.get(path) else {
            return;
        };
        if self.files[ix].collapsed {
            self.files[ix].collapsed = false;
            self.dirty.insert(ix);
        }
        self.pending_scroll = Some(ScrollTarget::Anchor {
            file: ix,
            anchor: (line, side),
            context: REVEAL_CONTEXT_ROWS,
        });
        self.start_flash(ix, (line, side), cx);
        cx.notify();
    }

    fn file_mut(&mut self, path: &str) -> Option<&mut FileDiff> {
        let ix = *self.index.get(path)?;
        self.files.get_mut(ix)
    }

    fn mark_dirty(&mut self, path: &str, cx: &mut Context<Self>) {
        if let Some(&ix) = self.index.get(path) {
            self.dirty.insert(ix);
            cx.notify();
        }
    }

    fn block(
        &self,
        file: usize,
        block: usize,
        outdated: bool,
    ) -> Option<(SharedString, BlockRender)> {
        let blocks = self.blocks.get(&self.files.get(file)?.meta.path)?;
        let list = if outdated {
            &blocks.outdated
        } else {
            &blocks.anchored
        };
        list.get(block)
            .map(|block| (block.key.clone(), block.render.clone()))
    }
}

fn layout_key(blocks: &[InlineBlock]) -> Vec<(&str, BlockAnchor)> {
    blocks
        .iter()
        .map(|block| (block.key.as_ref(), block.layout()))
        .collect()
}
