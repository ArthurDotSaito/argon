use std::collections::HashSet;
use std::sync::Arc;

use super::log_lines::{LineKind, LogDoc, SearchHits};

pub struct LogView {
    doc: Arc<LogDoc>,
    collapsed: HashSet<usize>,
    rows: Vec<usize>,
    widest: Option<usize>,
    query: String,
    hits: SearchHits,
    current: Option<usize>,
}

impl LogView {
    pub fn new(doc: Arc<LogDoc>) -> Self {
        let mut view = Self {
            doc,
            collapsed: HashSet::new(),
            rows: Vec::new(),
            widest: None,
            query: String::new(),
            hits: SearchHits::default(),
            current: None,
        };
        view.rebuild_rows();
        view
    }

    pub fn doc(&self) -> &Arc<LogDoc> {
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

    pub fn set_hits(&mut self, query: String, hits: SearchHits) {
        self.query = query;
        self.hits = hits;
        self.current = None;
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn hits(&self) -> &SearchHits {
        &self.hits
    }

    pub fn current_match(&self) -> Option<usize> {
        self.current
    }

    pub fn current_line(&self) -> Option<usize> {
        self.hits.lines.get(self.current?).copied()
    }

    pub fn is_hit(&self, line: usize) -> bool {
        self.hits.lines.binary_search(&line).is_ok()
    }

    pub fn step_match(&mut self, forward: bool, from_row: usize) -> Option<usize> {
        let lines = &self.hits.lines;
        let count = lines.len();
        if count == 0 {
            return None;
        }
        let next = match self.current {
            Some(current) if forward => (current + 1) % count,
            Some(current) => (current + count - 1) % count,
            None => {
                let from_line = self.line_at(from_row).unwrap_or(0);
                let after = lines.partition_point(|line| *line < from_line);
                if forward {
                    after % count
                } else {
                    (after + count - 1) % count
                }
            }
        };
        self.current = Some(next);
        let line = lines[next];
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
    use super::super::log_lines::{parse_log, search_lines};
    use super::*;

    const LOG: &str = "##[group]Setup\nsetup line\n##[endgroup]\nerror in tests\n##[group]Tests\nfirst error\n##[error]boom\n##[endgroup]\nlast";

    fn view() -> LogView {
        LogView::new(Arc::new(parse_log(LOG)))
    }

    fn search(view: &mut LogView, query: &str) {
        let hits = search_lines(view.doc(), query, 10_000);
        view.set_hits(query.to_string(), hits);
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
        search(&mut view, "ERROR");
        assert_eq!(view.hits().lines, vec![3, 5]);
        assert!(view.is_hit(3));
        assert!(!view.is_hit(4));
        assert_eq!(view.step_match(true, 0), Some(2));
        assert_eq!(view.current_line(), Some(3));
        assert_eq!(view.step_match(true, 0), Some(4));
        assert_eq!(view.step_match(true, 0), Some(2));
        assert_eq!(view.step_match(false, 0), Some(4));
    }

    #[test]
    fn stepping_into_a_collapsed_group_expands_it() {
        let mut view = view();
        view.toggle_group(4);
        search(&mut view, "first");
        assert_eq!(view.step_match(true, 0), Some(4));
        assert!(!view.is_collapsed(4));
    }

    #[test]
    fn first_step_starts_from_the_given_row() {
        let mut view = view();
        search(&mut view, "error");
        assert_eq!(view.step_match(true, 4), Some(4));
        search(&mut view, "error");
        assert_eq!(view.step_match(false, 4), Some(2));
    }

    #[test]
    fn empty_hits_have_no_steps() {
        let mut view = view();
        view.set_hits(String::new(), SearchHits::default());
        assert_eq!(view.step_match(true, 0), None);
        assert_eq!(view.current_line(), None);
    }
}
