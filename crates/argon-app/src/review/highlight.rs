use std::collections::HashMap;
use std::ops::Range;

use argon_core::{LineKind, Side};
use gpui_kit::HighlightStyle;
use gpui_kit::base::input::{HighlightStyleResolver, Rope};
use gpui_kit::component::highlighter::SyntaxHighlighter;

use super::file_diff::ParsedDiff;

const MAX_SIDE_BYTES: usize = 1 << 20;

pub type Spans = Vec<(Range<usize>, HighlightStyle)>;

#[derive(Debug, Default)]
pub struct FileHighlights {
    pub old: HashMap<u32, Spans>,
    pub new: HashMap<u32, Spans>,
}

impl FileHighlights {
    pub fn spans(&self, side: Side, line: u32) -> Option<&Spans> {
        match side {
            Side::Left => self.old.get(&line),
            Side::Right => self.new.get(&line),
        }
    }
}

pub fn language_for(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let (_, extension) = name.rsplit_once('.')?;
    let language = match extension.to_ascii_lowercase().as_str() {
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "rs" => "rust",
        "go" => "go",
        "py" | "pyi" => "python",
        "cs" => "csharp",
        "json" | "jsonc" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "md" | "markdown" => "markdown",
        _ => return None,
    };
    Some(language)
}

pub struct SideText {
    pub text: String,
    pub lines: Vec<(u32, Range<usize>)>,
}

pub fn side_text(diff: &ParsedDiff, side: Side) -> SideText {
    let mut text = String::new();
    let mut lines = Vec::new();
    for line in diff.hunks.iter().flat_map(|hunk| hunk.lines.iter()) {
        let number = match (side, line.kind) {
            (Side::Left, LineKind::Added) | (Side::Right, LineKind::Removed) => None,
            (Side::Left, _) => line.old_no,
            (Side::Right, _) => line.new_no,
        };
        let Some(number) = number else {
            continue;
        };
        let start = text.len();
        text.push_str(&line.text);
        lines.push((number, start..text.len()));
        text.push('\n');
    }
    SideText { text, lines }
}

pub fn spans_by_line(
    lines: &[(u32, Range<usize>)],
    styles: &[(Range<usize>, HighlightStyle)],
) -> HashMap<u32, Spans> {
    let mut out: HashMap<u32, Spans> = HashMap::new();
    let mut style_ix = 0;
    for (number, line) in lines {
        while style_ix < styles.len() && styles[style_ix].0.end <= line.start {
            style_ix += 1;
        }
        let mut cursor = style_ix;
        while let Some((range, style)) = styles.get(cursor) {
            if range.start >= line.end {
                break;
            }
            let start = range.start.max(line.start);
            let end = range.end.min(line.end);
            if start < end && *style != HighlightStyle::default() {
                out.entry(*number)
                    .or_default()
                    .push((start - line.start..end - line.start, *style));
            }
            cursor += 1;
        }
    }
    out
}

pub fn highlight_file(
    language: &str,
    diff: &ParsedDiff,
    theme: &dyn HighlightStyleResolver,
) -> FileHighlights {
    FileHighlights {
        old: highlight_side(language, &side_text(diff, Side::Left), theme),
        new: highlight_side(language, &side_text(diff, Side::Right), theme),
    }
}

fn highlight_side(
    language: &str,
    side: &SideText,
    theme: &dyn HighlightStyleResolver,
) -> HashMap<u32, Spans> {
    if side.text.is_empty() || side.text.len() > MAX_SIDE_BYTES {
        return HashMap::new();
    }
    let mut highlighter = SyntaxHighlighter::new(language);
    highlighter.update(None, &Rope::from_str(&side.text), None);
    let styles = highlighter.styles(&(0..side.text.len()), theme);
    spans_by_line(&side.lines, &styles)
}

#[cfg(test)]
mod tests {
    use argon_core::{DiffLine, Hunk};
    use gpui_kit::FontWeight;

    use super::*;

    fn bold() -> HighlightStyle {
        HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            ..HighlightStyle::default()
        }
    }

    fn line(kind: LineKind, old_no: Option<u32>, new_no: Option<u32>, text: &str) -> DiffLine {
        DiffLine {
            kind,
            old_no,
            new_no,
            text: text.to_string(),
            no_newline: false,
        }
    }

    #[test]
    fn language_is_picked_by_extension() {
        assert_eq!(language_for("src/app/page.tsx"), Some("tsx"));
        assert_eq!(language_for("src/Api/Program.CS"), Some("csharp"));
        assert_eq!(language_for("lib.rs"), Some("rust"));
        assert_eq!(language_for(".github/workflows/ci.yml"), Some("yaml"));
        assert_eq!(language_for("Makefile"), None);
        assert_eq!(language_for("assets/logo.png"), None);
    }

    #[test]
    fn side_text_rebuilds_each_side_separately() {
        let diff = ParsedDiff::new(vec![Hunk {
            old_start: 1,
            old_len: 2,
            new_start: 1,
            new_len: 2,
            header: String::new(),
            lines: vec![
                line(LineKind::Context, Some(1), Some(1), "a"),
                line(LineKind::Removed, Some(2), None, "old"),
                line(LineKind::Added, None, Some(2), "new"),
            ],
        }]);
        let left = side_text(&diff, Side::Left);
        assert_eq!(left.text, "a\nold\n");
        assert_eq!(left.lines, vec![(1, 0..1), (2, 2..5)]);
        let right = side_text(&diff, Side::Right);
        assert_eq!(right.text, "a\nnew\n");
        assert_eq!(right.lines, vec![(1, 0..1), (2, 2..5)]);
    }

    #[test]
    fn spans_are_split_per_line_and_rebased() {
        let lines = vec![(10, 0..5), (11, 6..12)];
        let styles = vec![
            (0..2, bold()),
            (2..4, HighlightStyle::default()),
            (3..9, bold()),
            (11..12, bold()),
        ];
        let spans = spans_by_line(&lines, &styles);
        assert_eq!(spans[&10], vec![(0..2, bold()), (3..5, bold())]);
        assert_eq!(spans[&11], vec![(0..3, bold()), (5..6, bold())]);
    }

    #[test]
    fn lines_without_styles_have_no_entry() {
        let spans = spans_by_line(&[(1, 0..3), (2, 4..6)], &[(4..6, bold())]);
        assert!(!spans.contains_key(&1));
        assert_eq!(spans[&2], vec![(0..2, bold())]);
    }
}
