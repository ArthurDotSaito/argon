use thiserror::Error;

use super::{DiffLine, Hunk, LineKind};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DiffError {
    #[error("invalid hunk header at line {line}")]
    InvalidHeader { line: usize },
    #[error(
        "hunk {hunk_index} does not match its header at line {line}: expected -{expected_old} +{expected_new} lines, got -{got_old} +{got_new}"
    )]
    CountMismatch {
        hunk_index: usize,
        line: usize,
        expected_old: u32,
        expected_new: u32,
        got_old: u32,
        got_new: u32,
    },
}

struct Cursor {
    old_no: u32,
    new_no: u32,
    old_left: u32,
    new_left: u32,
}

impl Cursor {
    fn closed() -> Self {
        Self {
            old_no: 0,
            new_no: 0,
            old_left: 0,
            new_left: 0,
        }
    }

    fn for_hunk(hunk: &Hunk) -> Self {
        Self {
            old_no: hunk.old_start,
            new_no: hunk.new_start,
            old_left: hunk.old_len,
            new_left: hunk.new_len,
        }
    }

    fn is_open(&self) -> bool {
        self.old_left > 0 || self.new_left > 0
    }

    fn take(&mut self, kind: LineKind) -> Option<(Option<u32>, Option<u32>)> {
        let wants_old = kind != LineKind::Added;
        let wants_new = kind != LineKind::Removed;
        if (wants_old && self.old_left == 0) || (wants_new && self.new_left == 0) {
            return None;
        }
        let old_no = wants_old.then_some(self.old_no);
        let new_no = wants_new.then_some(self.new_no);
        if wants_old {
            self.old_no += 1;
            self.old_left -= 1;
        }
        if wants_new {
            self.new_no += 1;
            self.new_left -= 1;
        }
        Some((old_no, new_no))
    }
}

pub fn parse_patch(patch: &str) -> Result<Vec<Hunk>, DiffError> {
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut cursor = Cursor::closed();
    let mut line_no = 0;
    for (index, raw) in patch.split('\n').enumerate() {
        line_no = index + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.starts_with('\\') {
            mark_no_newline(&mut hunks);
            continue;
        }
        match hunks.last_mut() {
            Some(hunk) if cursor.is_open() => match read_line(line, &mut cursor) {
                Some(parsed) => hunk.lines.push(parsed),
                None => return Err(count_mismatch(&hunks, &cursor, line_no, line)),
            },
            _ if line.starts_with("@@") => {
                let hunk = parse_header(line).ok_or(DiffError::InvalidHeader { line: line_no })?;
                cursor = Cursor::for_hunk(&hunk);
                hunks.push(hunk);
            }
            Some(_) if is_surplus_line(line) => {
                return Err(count_mismatch(&hunks, &cursor, line_no, line));
            }
            _ => {}
        }
    }
    if cursor.is_open() {
        return Err(count_mismatch(&hunks, &cursor, line_no, ""));
    }
    Ok(hunks)
}

fn count_mismatch(hunks: &[Hunk], cursor: &Cursor, line_no: usize, line: &str) -> DiffError {
    let hunk_index = hunks.len().saturating_sub(1);
    let (expected_old, expected_new, mut got_old, mut got_new) =
        hunks.last().map_or((0, 0, 0, 0), |hunk| {
            (
                hunk.old_len,
                hunk.new_len,
                hunk.old_len - cursor.old_left,
                hunk.new_len - cursor.new_left,
            )
        });
    match line.as_bytes().first() {
        Some(b' ') => {
            got_old += 1;
            got_new += 1;
        }
        Some(b'-') => got_old += 1,
        Some(b'+') => got_new += 1,
        _ => {}
    }
    DiffError::CountMismatch {
        hunk_index,
        line: line_no,
        expected_old,
        expected_new,
        got_old,
        got_new,
    }
}

fn is_surplus_line(line: &str) -> bool {
    line.starts_with(' ')
        || (line.starts_with('+') && !line.starts_with("+++ "))
        || (line.starts_with('-') && !line.starts_with("--- "))
}

fn mark_no_newline(hunks: &mut [Hunk]) {
    if let Some(last) = hunks.last_mut().and_then(|hunk| hunk.lines.last_mut()) {
        last.no_newline = true;
    }
}

fn read_line(line: &str, cursor: &mut Cursor) -> Option<DiffLine> {
    let (kind, text) = match line.as_bytes().first() {
        None => (LineKind::Context, ""),
        Some(b' ') => (LineKind::Context, &line[1..]),
        Some(b'+') => (LineKind::Added, &line[1..]),
        Some(b'-') => (LineKind::Removed, &line[1..]),
        Some(_) => return None,
    };
    let (old_no, new_no) = cursor.take(kind)?;
    Some(DiffLine {
        kind,
        old_no,
        new_no,
        text: text.to_string(),
        no_newline: false,
    })
}

fn parse_header(line: &str) -> Option<Hunk> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, context) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let (old_start, old_len) = parse_range(old)?;
    let (new_start, new_len) = parse_range(new)?;
    old_start.checked_add(old_len)?;
    new_start.checked_add(new_len)?;
    Some(Hunk {
        old_start,
        old_len,
        new_start,
        new_len,
        header: context.trim().to_string(),
        lines: Vec::new(),
    })
}

fn parse_range(range: &str) -> Option<(u32, u32)> {
    match range.split_once(',') {
        Some((start, len)) => Some((start.parse().ok()?, len.parse().ok()?)),
        None => Some((range.parse().ok()?, 1)),
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;
    use std::time::Instant;

    use super::*;

    fn numbers(hunk: &Hunk) -> Vec<(LineKind, Option<u32>, Option<u32>)> {
        hunk.lines
            .iter()
            .map(|line| (line.kind, line.old_no, line.new_no))
            .collect()
    }

    #[test]
    fn parses_multiple_hunks_with_numbering() {
        let patch =
            "@@ -1,3 +1,3 @@ fn first\n a\n-b\n+B\n c\n@@ -10,2 +10,3 @@ fn second\n x\n+y\n z\n";
        let hunks = parse_patch(patch).unwrap();
        assert_eq!(hunks.len(), 2);
        let first = &hunks[0];
        assert_eq!(
            (
                first.old_start,
                first.old_len,
                first.new_start,
                first.new_len
            ),
            (1, 3, 1, 3)
        );
        assert_eq!(first.header, "fn first");
        assert_eq!(
            numbers(first),
            vec![
                (LineKind::Context, Some(1), Some(1)),
                (LineKind::Removed, Some(2), None),
                (LineKind::Added, None, Some(2)),
                (LineKind::Context, Some(3), Some(3)),
            ]
        );
        assert_eq!(first.lines[1].text, "b");
        assert_eq!(first.lines[2].text, "B");
        let second = &hunks[1];
        assert_eq!(second.header, "fn second");
        assert_eq!(
            numbers(second),
            vec![
                (LineKind::Context, Some(10), Some(10)),
                (LineKind::Added, None, Some(11)),
                (LineKind::Context, Some(11), Some(12)),
            ]
        );
    }

    #[test]
    fn parses_added_only_new_file() {
        let hunks = parse_patch("@@ -0,0 +1,3 @@\n+one\n+two\n+three").unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!((hunks[0].old_start, hunks[0].old_len), (0, 0));
        assert_eq!(
            numbers(&hunks[0]),
            vec![
                (LineKind::Added, None, Some(1)),
                (LineKind::Added, None, Some(2)),
                (LineKind::Added, None, Some(3)),
            ]
        );
        assert_eq!(hunks[0].header, "");
    }

    #[test]
    fn parses_removed_only() {
        let hunks = parse_patch("@@ -4,2 +3,0 @@\n-gone\n-away\n").unwrap();
        assert_eq!(
            numbers(&hunks[0]),
            vec![
                (LineKind::Removed, Some(4), None),
                (LineKind::Removed, Some(5), None),
            ]
        );
    }

    #[test]
    fn header_without_counts_defaults_to_one() {
        let hunks = parse_patch("@@ -1 +1 @@\n-a\n+b\n").unwrap();
        let hunk = &hunks[0];
        assert_eq!(
            (hunk.old_start, hunk.old_len, hunk.new_start, hunk.new_len),
            (1, 1, 1, 1)
        );
        assert_eq!(hunk.lines.len(), 2);
    }

    #[test]
    fn no_newline_marker_flags_previous_line() {
        let patch = "@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n";
        let lines = &parse_patch(patch).unwrap()[0].lines;
        assert_eq!(lines.len(), 3);
        assert!(!lines[0].no_newline);
        assert!(lines[1].no_newline);
        assert!(lines[2].no_newline);
    }

    #[test]
    fn crlf_is_stripped_from_lines_and_headers() {
        let patch = "@@ -1,2 +1,2 @@ ctx\r\n a\r\n-b\r\n+c\r\n";
        let hunk = &parse_patch(patch).unwrap()[0];
        assert_eq!(hunk.header, "ctx");
        let texts: Vec<_> = hunk.lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(texts, vec!["a", "b", "c"]);
    }

    #[test]
    fn empty_line_inside_hunk_is_empty_context() {
        let hunk = &parse_patch("@@ -1,3 +1,3 @@\n a\n\n-b\n+c\n").unwrap()[0];
        assert_eq!(hunk.lines[1].kind, LineKind::Context);
        assert_eq!(hunk.lines[1].text, "");
        assert_eq!(hunk.lines[1].new_no, Some(2));
    }

    #[test]
    fn git_diff_headers_with_rename_are_ignored() {
        let patch = "diff --git a/old.rs b/new.rs\nsimilarity index 90%\nrename from old.rs\nrename to new.rs\nindex 111..222 100644\n--- a/old.rs\n+++ b/new.rs\n@@ -1,2 +1,2 @@ fn main\n keep\n-old\n+new\ndiff --git a/other.rs b/other.rs\nindex 333..444 100644\n--- a/other.rs\n+++ b/other.rs\n@@ -5 +5 @@\n-x\n+y\n";
        let hunks = parse_patch(patch).unwrap();
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].lines.len(), 3);
        assert_eq!(hunks[1].old_start, 5);
        assert_eq!(hunks[1].lines.len(), 2);
    }

    #[test]
    fn rename_only_and_binary_patches_have_no_hunks() {
        let rename =
            "diff --git a/a.rs b/b.rs\nsimilarity index 100%\nrename from a.rs\nrename to b.rs\n";
        let binary = "diff --git a/x.png b/x.png\nnew file mode 100644\nindex 0000000..abc1234\nBinary files /dev/null and b/x.png differ\n";
        assert_eq!(parse_patch(rename), Ok(vec![]));
        assert_eq!(parse_patch(binary), Ok(vec![]));
        assert_eq!(parse_patch(""), Ok(vec![]));
    }

    #[test]
    fn too_few_lines_is_count_mismatch() {
        assert_eq!(
            parse_patch("@@ -1,3 +1,3 @@\n a\n-b\n+c"),
            Err(DiffError::CountMismatch {
                hunk_index: 0,
                line: 4,
                expected_old: 3,
                expected_new: 3,
                got_old: 2,
                got_new: 2,
            })
        );
    }

    #[test]
    fn too_many_lines_is_count_mismatch() {
        let patch = "@@ -1,1 +1,1 @@\n a\n@@ -9,1 +9,2 @@\n x\n+y\n+z\n";
        assert_eq!(
            parse_patch(patch),
            Err(DiffError::CountMismatch {
                hunk_index: 1,
                line: 6,
                expected_old: 1,
                expected_new: 2,
                got_old: 1,
                got_new: 3,
            })
        );
    }

    #[test]
    fn surplus_removed_line_is_count_mismatch() {
        assert_eq!(
            parse_patch("@@ -1,1 +1,1 @@\n a\n-b\n"),
            Err(DiffError::CountMismatch {
                hunk_index: 0,
                line: 3,
                expected_old: 1,
                expected_new: 1,
                got_old: 2,
                got_new: 1,
            })
        );
    }

    #[test]
    fn next_header_before_counts_are_met_is_count_mismatch() {
        let patch = "@@ -1,2 +1,2 @@\n a\n@@ -9,1 +9,1 @@\n x\n";
        assert_eq!(
            parse_patch(patch),
            Err(DiffError::CountMismatch {
                hunk_index: 0,
                line: 3,
                expected_old: 2,
                expected_new: 2,
                got_old: 1,
                got_new: 1,
            })
        );
    }

    #[test]
    fn count_mismatch_message_is_informative() {
        let message = parse_patch("@@ -1,3 +1,3 @@\n a\n")
            .unwrap_err()
            .to_string();
        assert!(message.contains("hunk 0"), "{message}");
        assert!(message.contains("expected -3 +3"), "{message}");
    }

    #[test]
    fn trailing_empty_context_line_is_kept() {
        for patch in [
            "@@ -1,2 +1,2 @@\n a\n",
            "@@ -1,2 +1,2 @@\n a\n\n",
            "@@ -1,2 +1,2 @@\n a\n ",
        ] {
            let hunk = &parse_patch(patch).unwrap()[0];
            assert_eq!(hunk.lines.len(), 2, "{patch:?}");
            assert_eq!(hunk.lines[1].kind, LineKind::Context);
            assert_eq!(hunk.lines[1].text, "");
            assert_eq!(hunk.lines[1].old_no, Some(2));
            assert_eq!(hunk.lines[1].new_no, Some(2));
        }
    }

    #[test]
    fn trailing_newline_after_closed_hunk_is_ignored() {
        let hunk = &parse_patch("@@ -1,1 +1,1 @@\n a\n").unwrap()[0];
        assert_eq!(hunk.lines.len(), 1);
    }

    #[test]
    fn dashes_and_pluses_inside_open_hunk_are_content() {
        let hunk = &parse_patch("@@ -1,1 +1,1 @@\n--- x\n+++ x\n").unwrap()[0];
        assert_eq!(hunk.lines[0].kind, LineKind::Removed);
        assert_eq!(hunk.lines[0].text, "-- x");
        assert_eq!(hunk.lines[1].kind, LineKind::Added);
        assert_eq!(hunk.lines[1].text, "++ x");
    }

    #[test]
    fn range_overflowing_u32_is_invalid_header() {
        assert_eq!(
            parse_patch("@@ -4294967295,2 +1 @@\n a\n b\n"),
            Err(DiffError::InvalidHeader { line: 1 })
        );
        assert_eq!(
            parse_patch("@@ -1 +4294967295,2 @@\n a\n b\n"),
            Err(DiffError::InvalidHeader { line: 1 })
        );
    }

    #[test]
    fn malformed_header_is_invalid_header() {
        assert_eq!(
            parse_patch("@@ -1,2 +x @@\n a\n"),
            Err(DiffError::InvalidHeader { line: 1 })
        );
        assert_eq!(
            parse_patch("@@ nonsense\n"),
            Err(DiffError::InvalidHeader { line: 1 })
        );
        assert_eq!(
            parse_patch("@@ -1 +1 @@\n-a\n+b\n@@ -1,x +1 @@\n"),
            Err(DiffError::InvalidHeader { line: 4 })
        );
    }

    #[test]
    #[ignore = "performance check, run in release"]
    fn parses_fifty_thousand_lines_under_fifty_ms() {
        let total = 50_000u32;
        let mut patch = String::with_capacity(total as usize * 32);
        writeln!(patch, "@@ -1,{total} +1,{total} @@ big").unwrap();
        for position in 0..total {
            writeln!(patch, " context line number {position}").unwrap();
        }
        let started = Instant::now();
        let hunks = parse_patch(&patch).unwrap();
        let elapsed = started.elapsed();
        println!("parse_patch 50k lines: {elapsed:?}");
        assert_eq!(hunks[0].lines.len(), total as usize);
        assert!(elapsed.as_millis() < 50, "took {elapsed:?}");
    }
}
