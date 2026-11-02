mod parse;
mod split;

use crate::model::Side;

pub use parse::{DiffError, parse_patch};
pub use split::{SplitRow, split_rows};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
    pub no_newline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    pub header: String,
    pub lines: Vec<DiffLine>,
}

pub fn anchor(line: &DiffLine) -> (u32, Side) {
    match (line.kind, line.old_no, line.new_no) {
        (LineKind::Removed, Some(old_no), _) => (old_no, Side::Left),
        (_, _, Some(new_no)) => (new_no, Side::Right),
        (_, Some(old_no), None) => (old_no, Side::Left),
        (_, None, None) => (0, Side::Right),
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn anchor_removed_uses_old_line_on_left() {
        let removed = line(LineKind::Removed, Some(7), None);
        assert_eq!(anchor(&removed), (7, Side::Left));
    }

    #[test]
    fn anchor_added_uses_new_line_on_right() {
        let added = line(LineKind::Added, None, Some(9));
        assert_eq!(anchor(&added), (9, Side::Right));
    }

    #[test]
    fn anchor_context_uses_new_line_on_right() {
        let context = line(LineKind::Context, Some(4), Some(6));
        assert_eq!(anchor(&context), (6, Side::Right));
    }
}
