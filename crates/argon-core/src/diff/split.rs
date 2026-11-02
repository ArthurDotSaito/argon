use super::{DiffLine, Hunk, LineKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitRow {
    pub left: Option<DiffLine>,
    pub right: Option<DiffLine>,
}

pub fn split_rows(hunk: &Hunk) -> Vec<SplitRow> {
    let lines = &hunk.lines;
    let mut rows = Vec::with_capacity(lines.len());
    let mut index = 0;
    while index < lines.len() {
        if lines[index].kind == LineKind::Context {
            rows.push(SplitRow {
                left: Some(lines[index].clone()),
                right: Some(lines[index].clone()),
            });
            index += 1;
            continue;
        }
        let removed_end = run_end(lines, index, LineKind::Removed);
        let added_end = run_end(lines, removed_end, LineKind::Added);
        let removed = &lines[index..removed_end];
        let added = &lines[removed_end..added_end];
        for pair in 0..removed.len().max(added.len()) {
            rows.push(SplitRow {
                left: removed.get(pair).cloned(),
                right: added.get(pair).cloned(),
            });
        }
        index = added_end;
    }
    rows
}

fn run_end(lines: &[DiffLine], start: usize, kind: LineKind) -> usize {
    lines[start..]
        .iter()
        .position(|line| line.kind != kind)
        .map_or(lines.len(), |offset| start + offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunk(spec: &[(LineKind, &str)]) -> Hunk {
        let lines = spec
            .iter()
            .enumerate()
            .map(|(position, (kind, text))| {
                let number = Some(position as u32 + 1);
                DiffLine {
                    kind: *kind,
                    old_no: if *kind == LineKind::Added {
                        None
                    } else {
                        number
                    },
                    new_no: if *kind == LineKind::Removed {
                        None
                    } else {
                        number
                    },
                    text: (*text).to_string(),
                    no_newline: false,
                }
            })
            .collect();
        Hunk {
            old_start: 1,
            old_len: 0,
            new_start: 1,
            new_len: 0,
            header: String::new(),
            lines,
        }
    }

    fn texts(rows: &[SplitRow]) -> Vec<(Option<&str>, Option<&str>)> {
        rows.iter()
            .map(|row| {
                (
                    row.left.as_ref().map(|line| line.text.as_str()),
                    row.right.as_ref().map(|line| line.text.as_str()),
                )
            })
            .collect()
    }

    #[test]
    fn context_is_paired_with_itself() {
        let rows = split_rows(&hunk(&[(LineKind::Context, "a")]));
        assert_eq!(texts(&rows), vec![(Some("a"), Some("a"))]);
        assert_eq!(rows[0].left, rows[0].right);
    }

    #[test]
    fn equal_blocks_pair_in_order() {
        let rows = split_rows(&hunk(&[
            (LineKind::Removed, "r1"),
            (LineKind::Removed, "r2"),
            (LineKind::Added, "a1"),
            (LineKind::Added, "a2"),
        ]));
        assert_eq!(
            texts(&rows),
            vec![(Some("r1"), Some("a1")), (Some("r2"), Some("a2"))]
        );
    }

    #[test]
    fn more_removed_than_added_leaves_right_empty() {
        let rows = split_rows(&hunk(&[
            (LineKind::Removed, "r1"),
            (LineKind::Removed, "r2"),
            (LineKind::Removed, "r3"),
            (LineKind::Added, "a1"),
        ]));
        assert_eq!(
            texts(&rows),
            vec![
                (Some("r1"), Some("a1")),
                (Some("r2"), None),
                (Some("r3"), None)
            ]
        );
    }

    #[test]
    fn more_added_than_removed_leaves_left_empty() {
        let rows = split_rows(&hunk(&[
            (LineKind::Removed, "r1"),
            (LineKind::Added, "a1"),
            (LineKind::Added, "a2"),
            (LineKind::Added, "a3"),
        ]));
        assert_eq!(
            texts(&rows),
            vec![
                (Some("r1"), Some("a1")),
                (None, Some("a2")),
                (None, Some("a3"))
            ]
        );
    }

    #[test]
    fn lone_blocks_and_context_keep_order() {
        let rows = split_rows(&hunk(&[
            (LineKind::Context, "c1"),
            (LineKind::Added, "a1"),
            (LineKind::Context, "c2"),
            (LineKind::Removed, "r1"),
            (LineKind::Context, "c3"),
        ]));
        assert_eq!(
            texts(&rows),
            vec![
                (Some("c1"), Some("c1")),
                (None, Some("a1")),
                (Some("c2"), Some("c2")),
                (Some("r1"), None),
                (Some("c3"), Some("c3")),
            ]
        );
    }

    #[test]
    fn empty_hunk_has_no_rows() {
        assert!(split_rows(&hunk(&[])).is_empty());
    }
}
