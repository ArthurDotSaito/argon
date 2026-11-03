mod blocks;
mod convert;
mod header;
mod render;

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use argon_core::Side;
use gpui_kit::component::diff::{
    DiffEvent, DiffLinePosition, DiffLineRange, DiffMode as KitMode, DiffState,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, Subscription, Task,
};

use blocks::BlockSet;
#[cfg(test)]
pub use blocks::BlockTarget;
pub use blocks::{BlockRender, BlockSource, InlineBlock};
use convert::{gutter_range, position};

use super::file_diff::FileDiff;

const REVEAL_CONTEXT: u32 = 4;
const FLASH_STEPS: [(bool, u64); 5] = [
    (true, 350),
    (false, 200),
    (true, 350),
    (false, 200),
    (true, 400),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffMode {
    #[default]
    Split,
    Inline,
}

impl DiffMode {
    pub fn toggled(self) -> Self {
        match self {
            Self::Split => Self::Inline,
            Self::Inline => Self::Split,
        }
    }

    fn kit(self) -> KitMode {
        match self {
            Self::Split => KitMode::Split,
            Self::Inline => KitMode::Unified,
        }
    }
}

pub enum DiffViewEvent {
    GutterClicked {
        path: String,
        line: u32,
        side: Side,
        start: Option<(u32, Side)>,
        crossed_hunks: bool,
    },
    ViewedToggled {
        path: String,
        viewed: bool,
    },
}

pub struct DiffView {
    state: Entity<DiffState>,
    files: Vec<FileDiff>,
    index: HashMap<String, usize>,
    blocks: HashMap<String, BlockSet>,
    mode: DiffMode,
    ready: bool,
    viewed_enabled: bool,
    comments_enabled: bool,
    github_url: Option<String>,
    flash: Option<(String, (u32, Side))>,
    pending_reveal: Option<(String, u32, Side)>,
    saving: HashSet<String>,
    flash_task: Option<Task<()>>,
    pointer_selection: bool,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<DiffViewEvent> for DiffView {}

impl Focusable for DiffView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.state.read(cx).focus_handle(cx)
    }
}

impl DiffView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mode = DiffMode::default();
        let state = cx.new(|cx| DiffState::new(Vec::new(), cx).with_mode(mode.kit()));
        let subscriptions = [
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.subscribe(&state, |this, _, event, cx| match event {
                DiffEvent::SelectionStarted(_) => this.pointer_selection = true,
                DiffEvent::SelectionEnded(range) => this.on_selection_ended(range, cx),
                _ => {}
            }),
        ];
        Self {
            state,
            files: Vec::new(),
            index: HashMap::new(),
            blocks: HashMap::new(),
            mode,
            ready: false,
            viewed_enabled: true,
            comments_enabled: true,
            github_url: None,
            flash: None,
            pending_reveal: None,
            saving: HashSet::new(),
            flash_task: None,
            pointer_selection: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.files.clear();
        self.index.clear();
        self.ready = false;
        self.flash = None;
        self.flash_task = None;
        self.pending_reveal = None;
        self.blocks.clear();
        self.saving.clear();
        self.state
            .update(cx, |state, cx| state.set_files(Vec::new(), cx));
        cx.notify();
    }

    pub fn set_files(&mut self, files: Vec<FileDiff>, cx: &mut Context<Self>) {
        let reload = self.ready && !self.files.is_empty();
        if reload && same_documents(&self.files, &files) {
            self.replace_files(files, cx);
            return;
        }
        let previous: HashSet<String> = self.files.iter().map(|f| f.meta.path.clone()).collect();
        let state = self.state.read(cx);
        let collapsed: HashSet<String> = previous
            .iter()
            .filter(|path| state.is_file_collapsed(path))
            .cloned()
            .collect();
        let selection = state.selected_lines().filter(|_| reload);
        let focus_line = selection
            .as_ref()
            .map(|range| DiffLinePosition::new(range.path().clone(), range.end_side(), range.end()))
            .or_else(|| self.editor_line().filter(|_| reload));
        let documents: Vec<_> = files.iter().map(|file| file.document.clone()).collect();
        self.state.update(cx, |state, cx| {
            state.set_files(documents, cx);
            for file in &files {
                let path = &file.meta.path;
                let collapse = if previous.contains(path) {
                    collapsed.contains(path)
                } else {
                    file.viewed
                };
                state.set_file_collapsed(path, collapse, cx);
            }
            state.set_selected_lines(selection, cx);
            if let Some(line) = focus_line
                && !state.is_file_collapsed(line.path())
            {
                state.scroll_to_line(line, cx);
            }
        });
        self.replace_files(files, cx);
    }

    fn replace_files(&mut self, files: Vec<FileDiff>, cx: &mut Context<Self>) {
        self.index = files
            .iter()
            .enumerate()
            .map(|(ix, file)| (file.meta.path.clone(), ix))
            .collect();
        self.files = files;
        self.ready = true;
        if let Some((path, line, side)) = self.pending_reveal.take() {
            self.reveal(&path, line, side, cx);
        }
        cx.notify();
    }

    fn editor_line(&self) -> Option<DiffLinePosition> {
        self.files.iter().find_map(|file| {
            let anchor = self
                .blocks
                .get(&file.meta.path)?
                .first_line(BlockSource::Editor)?;
            file.has_line(anchor)
                .then(|| position(&file.meta.path, anchor))
        })
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

    pub fn set_comments_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.comments_enabled = enabled;
        cx.notify();
    }

    pub fn set_viewed(&mut self, path: &str, viewed: bool, cx: &mut Context<Self>) {
        let Some(&ix) = self.index.get(path) else {
            return;
        };
        if self.files[ix].viewed != viewed {
            self.files[ix].viewed = viewed;
            self.state
                .update(cx, |state, cx| state.set_file_collapsed(path, viewed, cx));
        }
        cx.notify();
    }

    pub fn set_viewed_saving(&mut self, path: &str, saving: bool, cx: &mut Context<Self>) {
        if saving {
            self.saving.insert(path.to_string());
        } else {
            self.saving.remove(path);
        }
        cx.notify();
    }

    pub fn mode(&self) -> DiffMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        if mode != self.mode {
            self.mode = mode;
            self.state
                .update(cx, |state, cx| state.set_mode(mode.kit(), cx));
            cx.notify();
        }
    }

    pub fn toggle_mode(&mut self, cx: &mut Context<Self>) {
        self.set_mode(self.mode.toggled(), cx);
    }

    pub fn set_blocks(
        &mut self,
        path: &str,
        source: BlockSource,
        blocks: Vec<InlineBlock>,
        cx: &mut Context<Self>,
    ) {
        self.blocks
            .entry(path.to_string())
            .or_default()
            .set(source, blocks);
        cx.notify();
    }

    pub fn scroll_to_file(&mut self, path: &str, cx: &mut Context<Self>) {
        self.state
            .update(cx, |state, cx| state.scroll_to_file(path, cx));
    }

    pub fn reveal(&mut self, path: &str, line: u32, side: Side, cx: &mut Context<Self>) {
        if !self.ready {
            self.pending_reveal = Some((path.to_string(), line, side));
            return;
        }
        let Some(file) = self.index.get(path).map(|ix| &self.files[*ix]) else {
            return;
        };
        if !file.has_line((line, side)) {
            self.scroll_to_file(path, cx);
            return;
        }
        let context = (line.saturating_sub(REVEAL_CONTEXT)..line)
            .find(|above| file.has_line((*above, side)))
            .unwrap_or(line);
        let scroll = position(path, (context, side));
        self.state
            .update(cx, |state, cx| state.scroll_to_line(scroll, cx));
        self.start_flash((path.to_string(), (line, side)), cx);
    }

    fn start_flash(&mut self, target: (String, (u32, Side)), cx: &mut Context<Self>) {
        self.flash = Some(target.clone());
        self.flash_task = Some(cx.spawn(async move |this, cx| {
            for (visible, millis) in FLASH_STEPS {
                let target = target.clone();
                if this
                    .update(cx, |this, cx| {
                        this.flash = visible.then_some(target);
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(millis))
                    .await;
            }
            _ = this.update(cx, |this, cx| {
                this.flash = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn on_selection_ended(&mut self, range: &DiffLineRange, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.pointer_selection) || !self.comments_enabled {
            return;
        }
        let Some(file) = self
            .index
            .get(range.path().as_ref())
            .map(|ix| &self.files[*ix])
        else {
            return;
        };
        let gutter = gutter_range(range, |anchor| file.line_pos(anchor));
        cx.emit(DiffViewEvent::GutterClicked {
            path: gutter.path,
            line: gutter.line,
            side: gutter.side,
            start: gutter.start,
            crossed_hunks: gutter.crossed_hunks,
        });
    }
}

fn same_documents(old: &[FileDiff], new: &[FileDiff]) -> bool {
    old.len() == new.len()
        && old
            .iter()
            .zip(new)
            .all(|(old, new)| old.meta.path == new.meta.path && old.fingerprint == new.fingerprint)
}
