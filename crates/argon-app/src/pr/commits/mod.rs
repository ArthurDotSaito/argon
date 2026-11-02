mod grouping;
mod labels;
mod row;

use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use argon_github::PrDetail;
use chrono::{Local, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    ClipboardItem, Context, Div, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollHandle, StatefulInteractiveElement as _, Styled as _, Task,
    Window, div, px,
};

use self::grouping::{DayGroup, day_label, group_by_day, local_day};
use super::conversation::CONTENT_MAX_WIDTH;
use row::{CommitRow, commit_row};

const COPIED_FEEDBACK: Duration = Duration::from_millis(1500);

pub enum CommitsEvent {
    OpenCommit { oid: String, headline: String },
}

pub struct CommitsTab {
    detail: Option<Rc<PrDetail>>,
    groups: Vec<DayGroup>,
    expanded: HashSet<String>,
    copied: Option<String>,
    copied_reset: Option<Task<()>>,
    scroll: ScrollHandle,
}

impl EventEmitter<CommitsEvent> for CommitsTab {}

impl CommitsTab {
    pub fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            detail: None,
            groups: Vec::new(),
            expanded: HashSet::new(),
            copied: None,
            copied_reset: None,
            scroll: ScrollHandle::new(),
        }
    }

    pub fn set_detail(&mut self, detail: Rc<PrDetail>, cx: &mut Context<Self>) {
        let days: Vec<_> = detail.commits.iter().map(local_day).collect();
        self.groups = group_by_day(&days);
        self.detail = Some(detail);
        cx.notify();
    }

    pub fn count(&self) -> Option<usize> {
        self.detail.as_ref().map(|detail| detail.commits.len())
    }

    fn open_commit(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(commit) = self.detail.as_ref().and_then(|d| d.commits.get(ix)) else {
            return;
        };
        cx.emit(CommitsEvent::OpenCommit {
            oid: commit.oid.clone(),
            headline: commit.headline.clone(),
        });
    }

    fn toggle_message(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(oid) = self
            .detail
            .as_ref()
            .and_then(|d| d.commits.get(ix))
            .map(|commit| commit.oid.clone())
        else {
            return;
        };
        if !self.expanded.remove(&oid) {
            self.expanded.insert(oid);
        }
        cx.notify();
    }

    fn copy_sha(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(oid) = self
            .detail
            .as_ref()
            .and_then(|d| d.commits.get(ix))
            .map(|commit| commit.oid.clone())
        else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(oid.clone()));
        self.copied = Some(oid);
        self.copied_reset = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FEEDBACK).await;
            _ = this.update(cx, |this, cx| {
                this.copied = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn day_header(&self, group: &DayGroup, today: chrono::NaiveDate, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        h_flex()
            .w_full()
            .px_1()
            .gap_1p5()
            .items_center()
            .text_xs()
            .font_semibold()
            .text_color(theme.muted_foreground)
            .child(Icon::new(IconName::GitCommitHorizontal).xsmall())
            .child(day_label(group.day, today))
    }
}

impl Render for CommitsTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scroller = div()
            .id("pr-commits-scroll")
            .size_full()
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .vertical_scrollbar(&self.scroll);
        let Some(detail) = self.detail.clone() else {
            return scroller;
        };
        if detail.commits.is_empty() {
            return scroller.child(
                div()
                    .p_4()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(labels::EMPTY),
            );
        }
        let now = Utc::now();
        let today = now.with_timezone(&Local).date_naive();
        let sections: Vec<Div> = self
            .groups
            .iter()
            .map(|group| {
                let rows: Vec<_> = group
                    .commits
                    .clone()
                    .map(|ix| {
                        let commit = &detail.commits[ix];
                        let state = CommitRow {
                            ix,
                            expanded: self.expanded.contains(&commit.oid),
                            copied: self.copied.as_deref() == Some(commit.oid.as_str()),
                        };
                        commit_row(commit, state, now, cx)
                    })
                    .collect();
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .child(self.day_header(group, today, cx))
                    .child(
                        v_flex()
                            .w_full()
                            .overflow_hidden()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .children(rows),
                    )
            })
            .collect();
        scroller.child(
            v_flex()
                .w_full()
                .min_w_0()
                .max_w(px(CONTENT_MAX_WIDTH))
                .mx_auto()
                .px_4()
                .py_4()
                .gap_4()
                .text_color(cx.theme().foreground)
                .children(sections),
        )
    }
}
