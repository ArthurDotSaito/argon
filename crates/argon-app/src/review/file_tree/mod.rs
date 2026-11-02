mod model;
mod render;

use std::collections::HashSet;

use argon_github::FileStatus;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, Subscription, UniformListScrollHandle, Window,
};

pub use model::file_order;
use model::{TreeRow, rows};

use super::labels;

pub enum FileTreeEvent {
    Open(String),
    ToggleViewed { path: String, viewed: bool },
}

pub struct TreeFile {
    pub path: String,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
    pub viewed: bool,
}

pub struct FileTree {
    files: Vec<TreeFile>,
    collapsed: HashSet<String>,
    query: String,
    rows: Vec<TreeRow>,
    active: Option<String>,
    saving: HashSet<String>,
    viewed_enabled: bool,
    filter: Entity<InputState>,
    scroll: UniformListScrollHandle,
    _subscription: Subscription,
}

impl EventEmitter<FileTreeEvent> for FileTree {}

impl FileTree {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter =
            cx.new(|cx| InputState::new(window, cx).placeholder(labels::FILTER_PLACEHOLDER));
        let subscription = cx.subscribe(&filter, |this: &mut Self, filter, event, cx| {
            if let InputEvent::Change = event {
                this.query = filter.read(cx).value().to_string();
                this.refresh_rows();
                cx.notify();
            }
        });
        Self {
            files: Vec::new(),
            collapsed: HashSet::new(),
            query: String::new(),
            rows: Vec::new(),
            active: None,
            saving: HashSet::new(),
            viewed_enabled: true,
            filter,
            scroll: UniformListScrollHandle::new(),
            _subscription: subscription,
        }
    }

    pub fn set_files(
        &mut self,
        files: Vec<TreeFile>,
        viewed_enabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.files = files;
        self.viewed_enabled = viewed_enabled;
        self.active = None;
        self.saving.clear();
        self.refresh_rows();
        cx.notify();
    }

    pub fn set_viewed(&mut self, path: &str, viewed: bool, cx: &mut Context<Self>) {
        if let Some(file) = self.files.iter_mut().find(|file| file.path == path) {
            file.viewed = viewed;
            cx.notify();
        }
    }

    pub fn set_saving(&mut self, path: &str, saving: bool, cx: &mut Context<Self>) {
        if saving {
            self.saving.insert(path.to_string());
        } else {
            self.saving.remove(path);
        }
        cx.notify();
    }

    pub fn set_active(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        if self.active != path {
            self.active = path;
            cx.notify();
        }
    }

    pub fn viewed_count(&self) -> usize {
        self.files.iter().filter(|file| file.viewed).count()
    }

    fn toggle_dir(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_string());
        }
        self.refresh_rows();
        cx.notify();
    }

    fn refresh_rows(&mut self) {
        let paths: Vec<&str> = self.files.iter().map(|file| file.path.as_str()).collect();
        self.rows = rows(&paths, &self.collapsed, &self.query);
    }
}
