use std::collections::VecDeque;
use std::sync::Mutex;

use crate::redact::redact_line;
use crate::util::lock;

const CAPACITY: usize = 200;
#[derive(Default)]
pub(crate) struct StderrTail {
    lines: Mutex<VecDeque<String>>,
}

impl StderrTail {
    pub fn push(&self, line: String) {
        let mut lines = lock(&self.lines);
        if lines.len() == CAPACITY {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    pub fn clear(&self) {
        lock(&self.lines).clear();
    }

    pub fn last(&self, count: usize) -> Vec<String> {
        let lines = lock(&self.lines);
        let skip = lines.len().saturating_sub(count);
        lines
            .iter()
            .skip(skip)
            .map(|line| redact_line(line))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_only_last_lines() {
        let tail = StderrTail::default();
        for index in 0..250 {
            tail.push(format!("line {index}"));
        }
        let last = tail.last(20);
        assert_eq!(last.len(), 20);
        assert_eq!(last[0], "line 230");
        assert_eq!(last[19], "line 249");
    }

    #[test]
    fn clear_drops_earlier_lines() {
        let tail = StderrTail::default();
        tail.push("old".to_owned());
        tail.clear();
        tail.push("new".to_owned());
        assert_eq!(tail.last(20), ["new"]);
    }
}
