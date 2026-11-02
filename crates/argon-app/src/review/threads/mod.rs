mod card;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use argon_core::Side;
use argon_github::ReviewThread;
use gpui_kit::{Context, Entity};

use super::diff_view::InlineBlock;

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
}

pub fn place(thread: &ReviewThread) -> Placement {
    match thread.line {
        Some(line) if !thread.outdated => Placement::Anchored {
            anchor: (line, thread.side),
            start: thread
                .start_line
                .map(|start| (start, thread.start_side.unwrap_or(thread.side)))
                .filter(|start| *start != (line, thread.side)),
        },
        _ => Placement::Outdated,
    }
}

#[derive(Default)]
pub struct FileThreads {
    pub anchored: Vec<InlineBlock>,
    pub outdated: Vec<InlineBlock>,
}

pub fn file_threads(
    threads: &[ReviewThread],
    ui: &Entity<ThreadUi>,
) -> HashMap<String, FileThreads> {
    let mut out: HashMap<String, FileThreads> = HashMap::new();
    for thread in threads {
        let placement = place(thread);
        let outdated = placement == Placement::Outdated;
        let thread = Rc::new(thread.clone());
        let entry = out.entry(thread.path.clone()).or_default();
        let key = format!("thread-{}", thread.id);
        let render_thread = thread.clone();
        let render_ui = ui.clone();
        let render = Rc::new(
            move |window: &mut gpui_kit::Window, cx: &mut gpui_kit::App| {
                card::thread_card(&render_thread, &render_ui, outdated, window, cx)
            },
        );
        match placement {
            Placement::Anchored { anchor, start } => entry
                .anchored
                .push(InlineBlock::new(anchor, key, render).with_start(start)),
            Placement::Outdated => entry.outdated.push(InlineBlock::new(
                (thread.original_line.unwrap_or_default(), thread.side),
                key,
                render,
            )),
        }
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
    fn outdated_or_unanchored_threads_go_to_the_outdated_section() {
        assert_eq!(place(&thread(Some(12), None, true)), Placement::Outdated);
        assert_eq!(place(&thread(None, None, false)), Placement::Outdated);
    }
}
