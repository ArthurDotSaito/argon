use std::collections::HashMap;

use argon_core::{DiffLine, LineKind, Side};

use crate::review::file_diff::ParsedDiff;

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    FileHeader {
        file: usize,
    },
    Notice {
        file: usize,
    },
    OutdatedHeader {
        file: usize,
    },
    Outdated {
        file: usize,
        block: usize,
    },
    Floating {
        file: usize,
        block: usize,
    },
    HunkHeader {
        file: usize,
        hunk: usize,
    },
    Row {
        file: usize,
        hunk: usize,
        row: usize,
        marked: bool,
    },
    Block {
        file: usize,
        block: usize,
        side: Side,
    },
    FileEnd {
        file: usize,
    },
}

impl Item {
    pub fn file(self) -> usize {
        match self {
            Self::FileHeader { file }
            | Self::Notice { file }
            | Self::OutdatedHeader { file }
            | Self::Outdated { file, .. }
            | Self::Floating { file, .. }
            | Self::HunkHeader { file, .. }
            | Self::Row { file, .. }
            | Self::Block { file, .. }
            | Self::FileEnd { file } => file,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockAnchor {
    pub anchor: (u32, Side),
    pub start: Option<(u32, Side)>,
}

pub struct FileShape<'a> {
    pub file: usize,
    pub collapsed: bool,
    pub diff: Option<&'a ParsedDiff>,
    pub blocks: &'a [BlockAnchor],
    pub outdated: usize,
}

pub type RowPos = (usize, usize);

fn line_matches(line: &DiffLine, (number, side): (u32, Side)) -> bool {
    match side {
        Side::Left => line.kind != LineKind::Added && line.old_no == Some(number),
        Side::Right => line.kind != LineKind::Removed && line.new_no == Some(number),
    }
}

pub fn locate(diff: &ParsedDiff, mode: DiffMode, anchor: (u32, Side)) -> Option<RowPos> {
    match mode {
        DiffMode::Inline => diff.hunks.iter().enumerate().find_map(|(hunk_ix, hunk)| {
            hunk.lines
                .iter()
                .position(|line| line_matches(line, anchor))
                .map(|row| (hunk_ix, row))
        }),
        DiffMode::Split => diff.split.iter().enumerate().find_map(|(hunk_ix, rows)| {
            rows.iter()
                .position(|row| {
                    let cell = match anchor.1 {
                        Side::Left => row.left.as_ref(),
                        Side::Right => row.right.as_ref(),
                    };
                    cell.is_some_and(|line| line_matches(line, anchor))
                })
                .map(|row| (hunk_ix, row))
        }),
    }
}

fn row_count(diff: &ParsedDiff, mode: DiffMode, hunk: usize) -> usize {
    match mode {
        DiffMode::Inline => diff.hunks[hunk].lines.len(),
        DiffMode::Split => diff.split[hunk].len(),
    }
}

pub fn flatten_file(shape: &FileShape<'_>, mode: DiffMode) -> Vec<Item> {
    let file = shape.file;
    let mut items = vec![Item::FileHeader { file }];
    if shape.collapsed {
        items.push(Item::FileEnd { file });
        return items;
    }
    if shape.outdated > 0 {
        items.push(Item::OutdatedHeader { file });
        items.extend((0..shape.outdated).map(|block| Item::Outdated { file, block }));
    }
    let Some(diff) = shape.diff else {
        items.extend((0..shape.blocks.len()).map(|block| Item::Floating { file, block }));
        items.push(Item::Notice { file });
        items.push(Item::FileEnd { file });
        return items;
    };
    let mut after: HashMap<RowPos, Vec<usize>> = HashMap::new();
    let mut marked: Vec<(RowPos, RowPos)> = Vec::new();
    for (block, spec) in shape.blocks.iter().enumerate() {
        let Some(end) = locate(diff, mode, spec.anchor) else {
            items.push(Item::Floating { file, block });
            continue;
        };
        after.entry(end).or_default().push(block);
        if let Some(start) = spec.start.and_then(|start| locate(diff, mode, start)) {
            marked.push((start.min(end), start.max(end)));
        }
    }
    for hunk in 0..diff.hunks.len() {
        items.push(Item::HunkHeader { file, hunk });
        for row in 0..row_count(diff, mode, hunk) {
            let pos = (hunk, row);
            let in_range = marked
                .iter()
                .any(|(start, end)| *start <= pos && pos <= *end);
            items.push(Item::Row {
                file,
                hunk,
                row,
                marked: in_range,
            });
            if let Some(blocks) = after.get(&pos) {
                items.extend(blocks.iter().map(|block| Item::Block {
                    file,
                    block: *block,
                    side: shape.blocks[*block].anchor.1,
                }));
            }
        }
    }
    items.push(Item::FileEnd { file });
    items
}

pub fn row_item(items: &[Item], pos: RowPos) -> Option<usize> {
    items
        .iter()
        .position(|item| matches!(item, Item::Row { hunk, row, .. } if (*hunk, *row) == pos))
}

#[cfg(test)]
#[path = "items_tests.rs"]
mod tests;
