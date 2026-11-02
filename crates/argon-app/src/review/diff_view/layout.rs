use argon_core::{Side, anchor};
use gpui_kit::{ListOffset, px};

use super::items::{BlockAnchor, FileShape, Item, flatten_file, locate, row_item};
use super::{DiffMode, DiffView, InlineBlock, ScrollTarget};

impl DiffView {
    pub(super) fn file_items(&self, ix: usize) -> Vec<Item> {
        let file = &self.files[ix];
        let blocks = self.blocks.get(&file.meta.path);
        let anchors: Vec<BlockAnchor> = blocks
            .map(|blocks| blocks.anchored.iter().map(InlineBlock::layout).collect())
            .unwrap_or_default();
        flatten_file(
            &FileShape {
                file: ix,
                collapsed: file.collapsed,
                diff: file.body.diff().map(|diff| diff.as_ref()),
                blocks: &anchors,
                outdated: blocks.map_or(0, |blocks| blocks.outdated.len()),
            },
            self.mode,
        )
    }

    pub(super) fn rebuild_all(&mut self) {
        self.dirty.clear();
        self.items.clear();
        self.ranges.clear();
        for ix in 0..self.files.len() {
            let start = self.items.len();
            let items = self.file_items(ix);
            self.items.extend(items);
            self.ranges.push(start..self.items.len());
        }
        self.list.reset(self.items.len());
    }

    pub(super) fn flush_dirty(&mut self) {
        if self.dirty.is_empty() {
            return;
        }
        let dirty = std::mem::take(&mut self.dirty);
        let offset = self.list.logical_scroll_top().offset_in_item;
        let keep = self.top_anchor().filter(|(file, _)| dirty.contains(file));
        for ix in dirty.into_iter().rev() {
            let old = self.ranges[ix].clone();
            let items = self.file_items(ix);
            let count = items.len();
            self.items.splice(old.clone(), items);
            self.list.splice(old, count);
        }
        self.ranges.clear();
        let mut start = 0;
        for (ix, item) in self.items.iter().enumerate() {
            if let Item::FileEnd { .. } = item {
                self.ranges.push(start..ix + 1);
                start = ix + 1;
            }
        }
        match keep {
            Some((file, Some(anchor))) => match self.anchor_item(file, anchor) {
                Some(item_ix) => self.list.scroll_to(ListOffset {
                    item_ix,
                    offset_in_item: offset,
                }),
                None => self.scroll_to_item(self.ranges[file].start),
            },
            Some((file, None)) => self.scroll_to_item(self.ranges[file].start),
            None => {}
        }
    }

    pub(super) fn apply_scroll(&mut self, target: ScrollTarget) {
        match target {
            ScrollTarget::File(file) => self.scroll_to_item(self.ranges[file].start),
            ScrollTarget::Anchor {
                file,
                anchor,
                context,
            } => self.scroll_to_anchor(file, anchor, context),
        }
    }

    fn anchor_item(&self, file: usize, anchor: (u32, Side)) -> Option<usize> {
        let range = self.ranges.get(file)?.clone();
        let pos = locate(self.files[file].body.diff()?, self.mode, anchor)?;
        row_item(&self.items[range.clone()], pos).map(|offset| range.start + offset)
    }

    pub(super) fn scroll_to_item(&self, ix: usize) {
        self.list.scroll_to(ListOffset {
            item_ix: ix,
            offset_in_item: px(0.),
        });
    }

    fn scroll_to_anchor(&self, file: usize, anchor: (u32, Side), context: usize) {
        let range = self.ranges[file].clone();
        match self.anchor_item(file, anchor) {
            Some(ix) => self.scroll_to_item(ix.saturating_sub(context).max(range.start)),
            None => self.scroll_to_item(range.start),
        }
    }

    pub(super) fn top_anchor(&self) -> Option<(usize, Option<(u32, Side)>)> {
        let top = self.list.logical_scroll_top().item_ix;
        let file = self.items.get(top)?.file();
        let row = self.items[..=top]
            .iter()
            .rev()
            .take_while(|item| item.file() == file)
            .find_map(|item| match *item {
                Item::Row { hunk, row, .. } => Some((hunk, row)),
                _ => None,
            });
        let Some((hunk, row)) = row else {
            return Some((file, None));
        };
        let diff = self.files[file].body.diff()?;
        let anchor = match self.mode {
            DiffMode::Inline => Some(anchor(&diff.hunks[hunk].lines[row])),
            DiffMode::Split => {
                let pair = &diff.split[hunk][row];
                pair.right.as_ref().or(pair.left.as_ref()).map(anchor)
            }
        };
        Some((file, anchor))
    }
}
