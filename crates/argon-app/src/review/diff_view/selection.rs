use argon_core::Side;
use gpui_kit::Context;

use super::items::{RowPos, locate};
use super::{DiffView, DiffViewEvent, FLASH_DURATION};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub file: usize,
    pub start: (u32, Side),
    pub end: (u32, Side),
}

impl Selection {
    pub fn contains(&self, file: usize, anchor: (u32, Side)) -> bool {
        if self.file != file {
            return false;
        }
        if anchor == self.start || anchor == self.end {
            return true;
        }
        let (number, side) = anchor;
        side == self.start.1
            && side == self.end.1
            && self.start.0.min(self.end.0) <= number
            && number <= self.start.0.max(self.end.0)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Drag {
    file: usize,
    anchor: (u32, Side),
}

#[derive(Clone, Copy, Debug)]
pub struct Flash {
    pub file: usize,
    pub anchor: (u32, Side),
    pub seq: u64,
}

impl DiffView {
    pub(super) fn gutter_down(
        &mut self,
        file: usize,
        anchor: (u32, Side),
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        let previous = self.selection.filter(|selection| selection.file == file);
        match previous {
            Some(previous) if shift => {
                self.drag = None;
                self.select(file, previous.start, anchor, cx);
            }
            _ => self.drag = Some(Drag { file, anchor }),
        }
    }

    pub(super) fn gutter_up(&mut self, file: usize, anchor: (u32, Side), cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if drag.file == file {
            self.select(file, drag.anchor, anchor, cx);
        } else {
            self.select(file, anchor, anchor, cx);
        }
    }

    fn select(
        &mut self,
        file: usize,
        first: (u32, Side),
        second: (u32, Side),
        cx: &mut Context<Self>,
    ) {
        let (start, end) = self.ordered(file, first, second);
        self.selection = Some(Selection { file, start, end });
        let Some(path) = self.files.get(file).map(|file| file.meta.path.clone()) else {
            return;
        };
        cx.emit(DiffViewEvent::GutterClicked {
            path,
            line: end.0,
            side: end.1,
            start: (start != end).then_some(start),
        });
        cx.notify();
    }

    fn ordered(
        &self,
        file: usize,
        first: (u32, Side),
        second: (u32, Side),
    ) -> ((u32, Side), (u32, Side)) {
        let position = |anchor| -> Option<RowPos> {
            let diff = self.files.get(file)?.body.diff()?;
            locate(diff, self.mode, anchor)
        };
        match (position(first), position(second)) {
            (Some(a), Some(b)) if b < a => (second, first),
            _ => (first, second),
        }
    }

    pub(super) fn start_flash(&mut self, file: usize, anchor: (u32, Side), cx: &mut Context<Self>) {
        self.flash_seq += 1;
        let seq = self.flash_seq;
        self.flash = Some(Flash { file, anchor, seq });
        self.flash_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FLASH_DURATION).await;
            _ = this.update(cx, |this, cx| {
                if this.flash.is_some_and(|flash| flash.seq == seq) {
                    this.flash = None;
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn is_selected(&self, file: usize, anchor: (u32, Side)) -> bool {
        self.selection
            .is_some_and(|selection| selection.contains(file, anchor))
    }

    pub(super) fn flash_seq(&self, file: usize, anchor: (u32, Side)) -> Option<u64> {
        self.flash
            .filter(|flash| flash.file == file && flash.anchor == anchor)
            .map(|flash| flash.seq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_contains_both_ends_and_same_side_lines_between() {
        let selection = Selection {
            file: 1,
            start: (10, Side::Right),
            end: (14, Side::Right),
        };
        assert!(selection.contains(1, (10, Side::Right)));
        assert!(selection.contains(1, (12, Side::Right)));
        assert!(!selection.contains(1, (12, Side::Left)));
        assert!(!selection.contains(1, (15, Side::Right)));
        assert!(!selection.contains(0, (12, Side::Right)));
    }

    #[test]
    fn cross_side_selection_only_contains_its_ends() {
        let selection = Selection {
            file: 0,
            start: (3, Side::Left),
            end: (5, Side::Right),
        };
        assert!(selection.contains(0, (3, Side::Left)));
        assert!(selection.contains(0, (5, Side::Right)));
        assert!(!selection.contains(0, (4, Side::Right)));
    }
}
