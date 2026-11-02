use std::collections::BTreeMap;
use std::rc::Rc;

use argon_core::Side;
use gpui_kit::{AnyElement, App, SharedString, Window};

pub type BlockRender = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "drafts and the inline comment editor are produced in Task 16"
    )
)]
pub enum BlockSource {
    Threads,
    Drafts,
    Editor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockTarget {
    Line {
        anchor: (u32, Side),
        start: Option<(u32, Side)>,
    },
    File,
}

#[derive(Clone)]
pub struct InlineBlock {
    pub target: BlockTarget,
    pub key: SharedString,
    pub render: BlockRender,
}

impl InlineBlock {
    pub fn line(anchor: (u32, Side), key: impl Into<SharedString>, render: BlockRender) -> Self {
        Self {
            target: BlockTarget::Line {
                anchor,
                start: None,
            },
            key: key.into(),
            render,
        }
    }

    pub fn file(key: impl Into<SharedString>, render: BlockRender) -> Self {
        Self {
            target: BlockTarget::File,
            key: key.into(),
            render,
        }
    }

    pub fn with_start(mut self, start: Option<(u32, Side)>) -> Self {
        if let BlockTarget::Line {
            anchor,
            start: slot,
        } = &mut self.target
        {
            *slot = start.filter(|start| start != anchor);
        }
        self
    }
}

#[derive(Default, Clone)]
pub struct BlockSet {
    sources: BTreeMap<BlockSource, Vec<InlineBlock>>,
}

impl BlockSet {
    pub fn set(&mut self, source: BlockSource, blocks: Vec<InlineBlock>) {
        if blocks.is_empty() {
            self.sources.remove(&source);
        } else {
            self.sources.insert(source, blocks);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &InlineBlock> {
        self.sources.values().flatten()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    pub key: SharedString,
    pub line: Option<(u32, Side)>,
    pub start: Option<(u32, Side)>,
}

pub fn place(set: &BlockSet, has_line: impl Fn((u32, Side)) -> bool) -> Vec<Placed> {
    set.iter()
        .map(|block| match block.target {
            BlockTarget::Line { anchor, start } if has_line(anchor) => Placed {
                key: block.key.clone(),
                line: Some(anchor),
                start: start.filter(|start| has_line(*start)),
            },
            BlockTarget::Line { .. } | BlockTarget::File => Placed {
                key: block.key.clone(),
                line: None,
                start: None,
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use gpui_kit::{IntoElement as _, div};

    use super::*;

    fn render() -> BlockRender {
        Rc::new(|_, _| div().into_any_element())
    }

    fn keys(set: &BlockSet) -> Vec<String> {
        set.iter().map(|block| block.key.to_string()).collect()
    }

    #[test]
    fn replacing_one_source_keeps_the_others() {
        let mut set = BlockSet::default();
        set.set(
            BlockSource::Editor,
            vec![InlineBlock::line((3, Side::Right), "editor", render())],
        );
        set.set(
            BlockSource::Threads,
            vec![InlineBlock::line((1, Side::Right), "t1", render())],
        );
        set.set(
            BlockSource::Drafts,
            vec![InlineBlock::line((2, Side::Left), "d1", render())],
        );
        set.set(
            BlockSource::Threads,
            vec![InlineBlock::file("t2", render())],
        );
        assert_eq!(keys(&set), vec!["t2", "d1", "editor"]);
        set.set(BlockSource::Drafts, Vec::new());
        assert_eq!(keys(&set), vec!["t2", "editor"]);
    }

    #[test]
    fn start_equal_to_anchor_is_dropped() {
        let block =
            InlineBlock::line((4, Side::Right), "k", render()).with_start(Some((4, Side::Right)));
        assert_eq!(
            block.target,
            BlockTarget::Line {
                anchor: (4, Side::Right),
                start: None
            }
        );
    }

    #[test]
    fn lines_outside_the_diff_move_to_the_file_area() {
        let mut set = BlockSet::default();
        set.set(
            BlockSource::Threads,
            vec![
                InlineBlock::line((10, Side::Right), "in", render())
                    .with_start(Some((8, Side::Right))),
                InlineBlock::line((99, Side::Right), "out", render()),
                InlineBlock::file("file", render()),
            ],
        );
        let placed = place(&set, |(line, _)| (8..=10).contains(&line));
        assert_eq!(placed[0].line, Some((10, Side::Right)));
        assert_eq!(placed[0].start, Some((8, Side::Right)));
        assert_eq!(placed[1].line, None);
        assert_eq!(placed[2].line, None);
    }
}
