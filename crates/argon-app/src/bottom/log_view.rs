use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

use super::log_lines::{LineKind, LogDoc, find_matches};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub line: usize,
    pub range: Range<usize>,
}

pub struct LogView {
    doc: Rc<LogDoc>,
    collapsed: HashSet<usize>,
    rows: Vec<usize>,
    widest: Option<usize>,
    matches: Vec<Match>,
    current: Option<usize>,
}

impl LogView {
    pub fn new(doc: Rc<LogDoc>) -> Self {
        let mut view = Self {
            doc,
            collapsed: HashSet::new(),
            rows: Vec::new(),
            widest: None,
            matches: Vec::new(),
            current: None,
        };
        view.rebuild_rows();
        view
    }

    pub fn doc(&self) -> &Rc<LogDoc> {
        &self.doc
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn widest_row(&self) -> Option<usize> {
        self.widest
    }

    pub fn line_at(&self, row: usize) -> Option<usize> {
        self.rows.get(row).copied()
    }

    pub fn is_collapsed(&self, line: usize) -> bool {
        self.collapsed.contains(&line)
    }

    pub fn toggle_group(&mut self, line: usize) {
        if !self.collapsed.remove(&line) {
            self.collapsed.insert(line);
        }
        self.rebuild_rows();
    }

    pub fn first_error_row(&mut self) -> Option<usize> {
        let line = self.doc.first_error?;
        self.reveal_line(line)
    }

    pub fn search(&mut self, query: &str) {
        self.matches = if query.is_empty() {
            Vec::new()
        } else {
            self.doc
                .lines
                .iter()
                .enumerate()
                .filter(|(_, line)| line.kind != LineKind::GroupEnd)
                .flat_map(|(ix, line)| {
                    find_matches(&line.text, query)
                        .into_iter()
                        .map(move |range| Match { line: ix, range })
                })
                .collect()
        };
        self.current = None;
    }

    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    pub fn current_match(&self) -> Option<usize> {
        self.current
    }

    pub fn current_match_ref(&self) -> Option<&Match> {
        self.matches.get(self.current?)
    }

    pub fn matches_in(&self, line: usize) -> &[Match] {
        let start = self.matches.partition_point(|found| found.line < line);
        let end = self.matches.partition_point(|found| found.line <= line);
        &self.matches[start..end]
    }

    pub fn step_match(&mut self, forward: bool, from_row: usize) -> Option<usize> {
        let count = self.matches.len();
        if count == 0 {
            return None;
        }
        let next = match self.current {
            Some(current) if forward => (current + 1) % count,
            Some(current) => (current + count - 1) % count,
            None => {
                let from_line = self.line_at(from_row).unwrap_or(0);
                let after = self.matches.partition_point(|found| found.line < from_line);
                if forward {
                    after % count
                } else {
                    (after + count - 1) % count
                }
            }
        };
        self.current = Some(next);
        let line = self.matches[next].line;
        self.reveal_line(line)
    }

    fn reveal_line(&mut self, line: usize) -> Option<usize> {
        if let Some(group) = self.doc.lines.get(line)?.group
            && self.collapsed.remove(&group)
        {
            self.rebuild_rows();
        }
        self.rows.binary_search(&line).ok()
    }

    fn rebuild_rows(&mut self) {
        self.rows = self
            .doc
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                line.kind != LineKind::GroupEnd
                    && !line
                        .group
                        .is_some_and(|group| self.collapsed.contains(&group))
            })
            .map(|(ix, _)| ix)
            .collect();
        self.widest = self
            .rows
            .iter()
            .enumerate()
            .max_by_key(|(_, line)| self.doc.lines[**line].text.len())
            .map(|(row, _)| row);
    }
}

#[cfg(test)]
mod tests {
    use super::super::log_lines::parse_log;
    use super::*;

    const LOG: &str = "##[group]Setup\nsetup line\n##[endgroup]\nerror in tests\n##[group]Tests\nfirst error\n##[error]boom\n##[endgroup]\nlast";

    fn view() -> LogView {
        LogView::new(Rc::new(parse_log(LOG)))
    }

    fn visible_lines(view: &LogView) -> Vec<usize> {
        (0..view.row_count())
            .filter_map(|row| view.line_at(row))
            .collect()
    }

    #[test]
    fn group_end_markers_are_hidden_and_collapsed_groups_drop_their_lines() {
        let mut view = view();
        assert_eq!(visible_lines(&view), vec![0, 1, 3, 4, 5, 6, 8]);
        view.toggle_group(4);
        assert!(view.is_collapsed(4));
        assert_eq!(visible_lines(&view), vec![0, 1, 3, 4, 8]);
        view.toggle_group(4);
        assert_eq!(visible_lines(&view), vec![0, 1, 3, 4, 5, 6, 8]);
    }

    #[test]
    fn first_error_expands_its_group_and_returns_the_row() {
        let mut view = view();
        view.toggle_group(4);
        assert_eq!(view.first_error_row(), Some(5));
        assert!(!view.is_collapsed(4));
    }

    #[test]
    fn search_steps_forward_and_backward_with_wrap() {
        let mut view = view();
        view.search("ERROR");
        assert_eq!(view.match_count(), 2);
        assert_eq!(view.matches_in(3).len(), 1);
        assert_eq!(view.step_match(true, 0), Some(2));
        assert_eq!(view.step_match(true, 0), Some(4));
        assert_eq!(view.step_match(true, 0), Some(2));
        assert_eq!(view.step_match(false, 0), Some(4));
    }

    #[test]
    fn first_step_starts_from_the_given_row() {
        let mut view = view();
        view.search("error");
        assert_eq!(view.step_match(true, 4), Some(4));
        view.search("error");
        assert_eq!(view.step_match(false, 4), Some(2));
    }

    #[test]
    fn empty_query_clears_matches() {
        let mut view = view();
        view.search("error");
        view.search("");
        assert_eq!(view.match_count(), 0);
        assert_eq!(view.step_match(true, 0), None);
    }
}
