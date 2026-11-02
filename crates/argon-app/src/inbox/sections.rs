use argon_core::{DateBucket, ViewSet, date_bucket};
use chrono::{DateTime, Local};
use gpui_kit::SharedString;

use super::InboxView;
use super::items::InboxItem;
use super::labels;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InboxRow {
    Header { title: SharedString, count: usize },
    Pr { position: usize },
}

#[derive(Default)]
pub struct Sections {
    pub rows: Vec<InboxRow>,
    pub visible: Vec<usize>,
    pub row_of: Vec<usize>,
}

impl Sections {
    pub fn build(
        items: &[InboxItem],
        matches: Vec<usize>,
        view: InboxView,
        now: DateTime<Local>,
    ) -> Sections {
        let mut sections = Sections::default();
        let rest = if view == InboxView::Inbox {
            let (needs_you, rest): (Vec<usize>, Vec<usize>) = matches
                .into_iter()
                .partition(|ix| items[*ix].views.contains(ViewSet::NEEDS_YOU));
            sections.push(labels::NEEDS_YOU_SECTION, &needs_you);
            rest
        } else {
            matches
        };
        for (bucket, group) in by_bucket(items, &rest, now) {
            sections.push(labels::bucket_title(bucket), &group);
        }
        sections
    }

    fn push(&mut self, title: &'static str, group: &[usize]) {
        if group.is_empty() {
            return;
        }
        self.rows.push(InboxRow::Header {
            title: title.into(),
            count: group.len(),
        });
        for ix in group {
            self.row_of.push(self.rows.len());
            self.rows.push(InboxRow::Pr {
                position: self.visible.len(),
            });
            self.visible.push(*ix);
        }
    }
}

fn by_bucket(
    items: &[InboxItem],
    indexes: &[usize],
    now: DateTime<Local>,
) -> Vec<(DateBucket, Vec<usize>)> {
    let mut groups: Vec<(DateBucket, Vec<usize>)> = Vec::new();
    for &ix in indexes {
        let bucket = date_bucket(now, items[ix].pr.updated_at.with_timezone(&Local));
        match groups.last_mut() {
            Some((last, group)) if *last == bucket => group.push(ix),
            _ => groups.push((bucket, vec![ix])),
        }
    }
    groups
}
