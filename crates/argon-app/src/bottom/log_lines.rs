use std::ops::Range;

const TAB_WIDTH: usize = 8;
const BOM: char = '\u{feff}';
const ESC: char = '\u{1b}';
const BEL: char = '\u{7}';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Plain,
    Error,
    Warning,
    Notice,
    Command,
    Debug,
    GroupStart,
    GroupEnd,
}

const MARKERS: [(&str, LineKind); 8] = [
    ("##[error]", LineKind::Error),
    ("##[warning]", LineKind::Warning),
    ("##[notice]", LineKind::Notice),
    ("##[command]", LineKind::Command),
    ("##[debug]", LineKind::Debug),
    ("##[group]", LineKind::GroupStart),
    ("##[endgroup]", LineKind::GroupEnd),
    ("[command]", LineKind::Command),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub text: String,
    pub kind: LineKind,
    pub group: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogDoc {
    pub lines: Vec<LogLine>,
    pub first_error: Option<usize>,
}

pub fn parse_log(text: &str) -> LogDoc {
    let text = text.strip_prefix(BOM).unwrap_or(text);
    let text = text.strip_suffix('\n').unwrap_or(text);
    if text.is_empty() {
        return LogDoc::default();
    }
    let mut lines = Vec::new();
    let mut first_error = None;
    let mut open_group = None;
    for raw in text.split('\n') {
        let cleaned = clean_line(raw);
        let (kind, rest) = classify(&cleaned);
        let ix = lines.len();
        let group = match kind {
            LineKind::GroupStart => {
                open_group = Some(ix);
                None
            }
            LineKind::GroupEnd => open_group.take(),
            _ => open_group,
        };
        if kind == LineKind::Error && first_error.is_none() {
            first_error = Some(ix);
        }
        lines.push(LogLine {
            text: rest.to_string(),
            kind,
            group,
        });
    }
    LogDoc { lines, first_error }
}

pub fn classify(line: &str) -> (LineKind, &str) {
    MARKERS
        .iter()
        .find_map(|(marker, kind)| line.strip_prefix(marker).map(|rest| (*kind, rest)))
        .unwrap_or((LineKind::Plain, line))
}

pub fn clean_line(raw: &str) -> String {
    let line = raw.trim_end_matches('\r');
    let line = last_carriage_segment(line);
    let line = strip_timestamp(line);
    expand_tabs(&strip_ansi(line))
}

fn last_carriage_segment(line: &str) -> &str {
    line.rsplit('\r')
        .find(|segment| !segment.is_empty())
        .unwrap_or("")
}

pub fn strip_timestamp(line: &str) -> &str {
    let bytes = line.as_bytes();
    let digits = |range: Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|slice| slice.iter().all(u8::is_ascii_digit))
    };
    let at = |ix: usize, byte: u8| bytes.get(ix) == Some(&byte);
    let date_time = digits(0..4)
        && at(4, b'-')
        && digits(5..7)
        && at(7, b'-')
        && digits(8..10)
        && at(10, b'T')
        && digits(11..13)
        && at(13, b':')
        && digits(14..16)
        && at(16, b':')
        && digits(17..19);
    if !date_time {
        return line;
    }
    let mut end = 19;
    if at(end, b'.') {
        let fraction = bytes[end + 1..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if fraction == 0 {
            return line;
        }
        end += 1 + fraction;
    }
    if !at(end, b'Z') {
        return line;
    }
    end += 1;
    if at(end, b' ') {
        end += 1;
    } else if end != bytes.len() {
        return line;
    }
    &line[end..]
}

pub fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != ESC {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == BEL {
                        break;
                    }
                    if c == ESC && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len() + TAB_WIDTH);
    let mut column = 0;
    for c in line.chars() {
        if c == '\t' {
            let spaces = TAB_WIDTH - column % TAB_WIDTH;
            out.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else {
            out.push(c);
            column += 1;
        }
    }
    out
}

pub fn find_matches(haystack: &str, needle: &str) -> Vec<Range<usize>> {
    let needle = needle.as_bytes();
    let bytes = haystack.as_bytes();
    let mut matches = Vec::new();
    if needle.is_empty() || needle.len() > bytes.len() {
        return matches;
    }
    let mut start = 0;
    while start + needle.len() <= bytes.len() {
        let end = start + needle.len();
        if haystack.is_char_boundary(start) && bytes[start..end].eq_ignore_ascii_case(needle) {
            matches.push(start..end);
            start = end;
        } else {
            start += 1;
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_timestamp_with_fraction_is_removed() {
        assert_eq!(
            strip_timestamp("2026-11-02T12:34:56.1234567Z Run cargo test"),
            "Run cargo test"
        );
    }

    #[test]
    fn iso_timestamp_without_fraction_is_removed() {
        assert_eq!(strip_timestamp("2026-11-02T12:34:56Z hello"), "hello");
        assert_eq!(strip_timestamp("2026-11-02T12:34:56.5Z"), "");
    }

    #[test]
    fn text_that_only_looks_like_a_timestamp_is_kept() {
        for line in [
            "2026-11-02 12:34:56Z hello",
            "2026-11-02T12:34:56 hello",
            "2026-11-02T12:34:56.Z hello",
            "2026-11-02T12:34:56Zhello",
            "build 2026-11-02T12:34:56Z",
            "",
        ] {
            assert_eq!(strip_timestamp(line), line, "{line:?}");
        }
    }

    #[test]
    fn ansi_color_and_osc_sequences_are_removed() {
        assert_eq!(
            strip_ansi("\u{1b}[36;1mcargo test\u{1b}[0m done"),
            "cargo test done"
        );
        assert_eq!(
            strip_ansi("\u{1b}]8;;https://x.y\u{7}link\u{1b}]8;;\u{1b}\\ end"),
            "link end"
        );
        assert_eq!(strip_ansi("\u{1b}[2Kplain"), "plain");
        assert_eq!(strip_ansi("ünïcode ✓"), "ünïcode ✓");
    }

    #[test]
    fn workflow_markers_are_classified_and_stripped() {
        let cases = [
            (
                "##[error]Process completed",
                LineKind::Error,
                "Process completed",
            ),
            ("##[warning]Deprecated", LineKind::Warning, "Deprecated"),
            ("##[notice]Heads up", LineKind::Notice, "Heads up"),
            ("##[command]cargo test", LineKind::Command, "cargo test"),
            (
                "[command]/usr/bin/git log",
                LineKind::Command,
                "/usr/bin/git log",
            ),
            ("##[debug]state", LineKind::Debug, "state"),
            ("##[group]Run tests", LineKind::GroupStart, "Run tests"),
            ("##[endgroup]", LineKind::GroupEnd, ""),
            ("test result: ok", LineKind::Plain, "test result: ok"),
            (
                "  ##[error]indented",
                LineKind::Plain,
                "  ##[error]indented",
            ),
        ];
        for (line, kind, rest) in cases {
            assert_eq!(classify(line), (kind, rest), "{line:?}");
        }
    }

    #[test]
    fn clean_line_handles_crlf_progress_tabs_and_colors() {
        assert_eq!(
            clean_line("2026-11-02T12:34:56.1Z \u{1b}[31m##[error]boom\u{1b}[0m\r"),
            "##[error]boom"
        );
        assert_eq!(clean_line("10%\r50%\r100%\r"), "100%");
        assert_eq!(clean_line("a\tb"), "a       b");
    }

    #[test]
    fn parse_tracks_groups_and_the_first_error() {
        let log = "\u{feff}2026-11-02T12:00:00.0000001Z ##[group]Run tests\n\
                   2026-11-02T12:00:01.0000001Z running 3 tests\n\
                   2026-11-02T12:00:02.0000001Z ##[endgroup]\n\
                   2026-11-02T12:00:03.0000001Z ##[warning]slow\n\
                   2026-11-02T12:00:04.0000001Z ##[error]first failure\n\
                   2026-11-02T12:00:05.0000001Z ##[error]second failure\n";
        let doc = parse_log(log);
        let summary: Vec<(&str, LineKind, Option<usize>)> = doc
            .lines
            .iter()
            .map(|line| (line.text.as_str(), line.kind, line.group))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("Run tests", LineKind::GroupStart, None),
                ("running 3 tests", LineKind::Plain, Some(0)),
                ("", LineKind::GroupEnd, Some(0)),
                ("slow", LineKind::Warning, None),
                ("first failure", LineKind::Error, None),
                ("second failure", LineKind::Error, None),
            ]
        );
        assert_eq!(doc.first_error, Some(4));
    }

    #[test]
    fn empty_log_has_no_lines_and_log_without_errors_has_no_first_error() {
        assert_eq!(parse_log(""), LogDoc::default());
        let doc = parse_log("one\ntwo");
        assert_eq!(doc.lines.len(), 2);
        assert_eq!(doc.first_error, None);
    }

    #[test]
    fn matches_are_case_insensitive_and_non_overlapping() {
        assert_eq!(
            find_matches("Error: error ERROR", "error"),
            vec![0..5, 7..12, 13..18]
        );
        assert_eq!(find_matches("aaaa", "aa"), vec![0..2, 2..4]);
        assert!(find_matches("abc", "").is_empty());
        assert!(find_matches("ab", "abc").is_empty());
    }

    #[test]
    fn matches_respect_utf8_boundaries() {
        let text = "café Café";
        let found = find_matches(text, "café");
        assert_eq!(found.len(), 2);
        for range in found {
            assert_eq!(text[range].to_lowercase(), "café");
        }
    }
}
