mod card;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use argon_core::Side;
use argon_github::ReviewThread;
use gpui_kit::{Context, Entity};

use super::diff_view::{BlockRender, InlineBlock};
pub use card::CardKind;

#[derive(Default)]
pub struct ThreadUi {
    expanded: HashSet<String>,
}

impl ThreadUi {
    pub fn is_expanded(&self, id: &str) -> bool {
        self.expanded.contains(id)
    }

    pub fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_string());
        }
        cx.notify();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Anchored {
        anchor: (u32, Side),
        start: Option<(u32, Side)>,
    },
    Outdated,
    File,
}

pub fn place(thread: &ReviewThread) -> Placement {
    match thread.line {
        _ if thread.outdated => Placement::Outdated,
        Some(line) => Placement::Anchored {
            anchor: (line, thread.side),
            start: thread
                .start_line
                .map(|start| (start, thread.start_side.unwrap_or(thread.side)))
                .filter(|start| *start != (line, thread.side)),
        },
        None => Placement::File,
    }
}

pub fn file_threads(
    threads: &[ReviewThread],
    ui: &Entity<ThreadUi>,
) -> HashMap<String, Vec<InlineBlock>> {
    let mut lines: HashMap<String, Vec<InlineBlock>> = HashMap::new();
    let mut files: HashMap<String, Vec<InlineBlock>> = HashMap::new();
    let mut outdated: HashMap<String, Vec<InlineBlock>> = HashMap::new();
    for thread in threads {
        let placement = place(thread);
        let kind = match placement {
            Placement::Anchored { .. } => CardKind::Line,
            Placement::Outdated => CardKind::Outdated,
            Placement::File => CardKind::File,
        };
        let thread = Rc::new(thread.clone());
        let key = format!("thread-{}", thread.id);
        let render_thread = thread.clone();
        let render_ui = ui.clone();
        let render: BlockRender = Rc::new(move |window, cx| {
            card::thread_card(&render_thread, &render_ui, kind, window, cx)
        });
        let path = thread.path.clone();
        match placement {
            Placement::Anchored { anchor, start } => lines
                .entry(path)
                .or_default()
                .push(InlineBlock::line(anchor, key, render).with_start(start)),
            Placement::Outdated => outdated
                .entry(path)
                .or_default()
                .push(InlineBlock::file(key, render)),
            Placement::File => files
                .entry(path)
                .or_default()
                .push(InlineBlock::file(key, render)),
        }
    }
    let mut out: HashMap<String, Vec<InlineBlock>> = HashMap::new();
    for (path, blocks) in outdated {
        let header = InlineBlock::file(
            format!("thread-outdated-{path}"),
            Rc::new(|_, cx| card::outdated_header(cx)),
        );
        let entry = out.entry(path).or_default();
        entry.push(header);
        entry.extend(blocks);
    }
    for (path, blocks) in files.into_iter().chain(lines) {
        out.entry(path).or_default().extend(blocks);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(line: Option<u32>, start_line: Option<u32>, outdated: bool) -> ReviewThread {
        ReviewThread {
            id: "t1".to_string(),
            resolved: false,
            outdated,
            path: "src/a.ts".to_string(),
            line,
            start_line,
            original_line: Some(7),
            original_start_line: None,
            side: Side::Right,
            start_side: None,
            comments: Vec::new(),
            truncated_comments: false,
        }
    }

    #[test]
    fn current_thread_anchors_on_its_line_and_side() {
        assert_eq!(
            place(&thread(Some(12), None, false)),
            Placement::Anchored {
                anchor: (12, Side::Right),
                start: None
            }
        );
    }

    #[test]
    fn multi_line_thread_keeps_its_start() {
        let mut multi = thread(Some(12), Some(9), false);
        multi.start_side = Some(Side::Left);
        assert_eq!(
            place(&multi),
            Placement::Anchored {
                anchor: (12, Side::Right),
                start: Some((9, Side::Left))
            }
        );
    }

    #[test]
    fn start_equal_to_line_is_dropped() {
        assert_eq!(
            place(&thread(Some(12), Some(12), false)),
            Placement::Anchored {
                anchor: (12, Side::Right),
                start: None
            }
        );
    }

    #[test]
    fn outdated_threads_go_to_the_outdated_section() {
        assert_eq!(place(&thread(Some(12), None, true)), Placement::Outdated);
        assert_eq!(place(&thread(None, None, true)), Placement::Outdated);
    }

    #[test]
    fn current_threads_without_line_are_file_comments() {
        assert_eq!(place(&thread(None, None, false)), Placement::File);
    }
}
