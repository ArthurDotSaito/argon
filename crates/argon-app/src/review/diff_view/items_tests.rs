use argon_core::Hunk;

use super::*;

fn line(kind: LineKind, old_no: Option<u32>, new_no: Option<u32>) -> DiffLine {
    DiffLine {
        kind,
        old_no,
        new_no,
        text: String::new(),
        no_newline: false,
    }
}

fn sample() -> ParsedDiff {
    ParsedDiff::new(vec![Hunk {
        old_start: 1,
        old_len: 3,
        new_start: 1,
        new_len: 3,
        header: String::new(),
        lines: vec![
            line(LineKind::Context, Some(1), Some(1)),
            line(LineKind::Removed, Some(2), None),
            line(LineKind::Removed, Some(3), None),
            line(LineKind::Added, None, Some(2)),
            line(LineKind::Context, Some(4), Some(3)),
        ],
    }])
}

fn shape<'a>(diff: &'a ParsedDiff, blocks: &'a [BlockAnchor]) -> FileShape<'a> {
    FileShape {
        file: 0,
        collapsed: false,
        diff: Some(diff),
        blocks,
        outdated: 0,
    }
}

fn block(line: u32, side: Side) -> BlockAnchor {
    BlockAnchor {
        anchor: (line, side),
        start: None,
    }
}

#[test]
fn locate_inline_uses_side_specific_numbers() {
    let diff = sample();
    assert_eq!(
        locate(&diff, DiffMode::Inline, (2, Side::Left)),
        Some((0, 1))
    );
    assert_eq!(
        locate(&diff, DiffMode::Inline, (2, Side::Right)),
        Some((0, 3))
    );
    assert_eq!(
        locate(&diff, DiffMode::Inline, (4, Side::Left)),
        Some((0, 4))
    );
    assert_eq!(locate(&diff, DiffMode::Inline, (9, Side::Right)), None);
}

#[test]
fn locate_split_pairs_rows() {
    let diff = sample();
    assert_eq!(
        locate(&diff, DiffMode::Split, (2, Side::Right)),
        Some((0, 1))
    );
    assert_eq!(
        locate(&diff, DiffMode::Split, (3, Side::Left)),
        Some((0, 2))
    );
    assert_eq!(
        locate(&diff, DiffMode::Split, (3, Side::Right)),
        Some((0, 3))
    );
}

#[test]
fn inline_flatten_places_block_after_anchor() {
    let diff = sample();
    let blocks = [block(2, Side::Right)];
    let items = flatten_file(&shape(&diff, &blocks), DiffMode::Inline);
    assert_eq!(items.len(), 1 + 1 + 5 + 1 + 1);
    assert_eq!(items[0], Item::FileHeader { file: 0 });
    assert_eq!(items[1], Item::HunkHeader { file: 0, hunk: 0 });
    assert_eq!(
        items[6],
        Item::Block {
            file: 0,
            block: 0,
            side: Side::Right
        }
    );
    assert_eq!(items.last(), Some(&Item::FileEnd { file: 0 }));
}

#[test]
fn split_flatten_has_one_item_per_split_row() {
    let diff = sample();
    let items = flatten_file(&shape(&diff, &[]), DiffMode::Split);
    let rows = items
        .iter()
        .filter(|item| matches!(item, Item::Row { .. }))
        .count();
    assert_eq!(rows, 4);
}

#[test]
fn unlocated_blocks_float_to_the_top() {
    let diff = sample();
    let blocks = [block(42, Side::Right)];
    let items = flatten_file(&shape(&diff, &blocks), DiffMode::Inline);
    assert_eq!(items[1], Item::Floating { file: 0, block: 0 });
}

#[test]
fn ranges_mark_rows_between_start_and_anchor() {
    let diff = sample();
    let blocks = [BlockAnchor {
        anchor: (3, Side::Right),
        start: Some((1, Side::Right)),
    }];
    let items = flatten_file(&shape(&diff, &blocks), DiffMode::Inline);
    let marked: Vec<usize> = items
        .iter()
        .filter_map(|item| match item {
            Item::Row {
                row, marked: true, ..
            } => Some(*row),
            _ => None,
        })
        .collect();
    assert_eq!(marked, vec![0, 1, 2, 3, 4]);
}

#[test]
fn collapsed_file_keeps_only_header() {
    let diff = sample();
    let mut collapsed = shape(&diff, &[]);
    collapsed.collapsed = true;
    let items = flatten_file(&collapsed, DiffMode::Inline);
    assert_eq!(
        items,
        vec![Item::FileHeader { file: 0 }, Item::FileEnd { file: 0 }]
    );
}

#[test]
fn outdated_section_comes_before_hunks() {
    let diff = sample();
    let mut with_outdated = shape(&diff, &[]);
    with_outdated.outdated = 2;
    let items = flatten_file(&with_outdated, DiffMode::Inline);
    assert_eq!(items[1], Item::OutdatedHeader { file: 0 });
    assert_eq!(items[2], Item::Outdated { file: 0, block: 0 });
    assert_eq!(items[3], Item::Outdated { file: 0, block: 1 });
    assert_eq!(items[4], Item::HunkHeader { file: 0, hunk: 0 });
}

#[test]
fn missing_diff_renders_notice() {
    let blocks = [block(1, Side::Right)];
    let items = flatten_file(
        &FileShape {
            file: 3,
            collapsed: false,
            diff: None,
            blocks: &blocks,
            outdated: 0,
        },
        DiffMode::Split,
    );
    assert_eq!(
        items,
        vec![
            Item::FileHeader { file: 3 },
            Item::Floating { file: 3, block: 0 },
            Item::Notice { file: 3 },
            Item::FileEnd { file: 3 },
        ]
    );
}

#[test]
fn row_item_finds_position() {
    let diff = sample();
    let items = flatten_file(&shape(&diff, &[]), DiffMode::Inline);
    assert_eq!(row_item(&items, (0, 2)), Some(4));
}
