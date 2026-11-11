mod items;
mod tools;

use std::collections::HashSet;

use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AppContext as _, Context, Div, Entity, FollowMode, IntoElement, ListAlignment, ListState,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div, list, px,
};

use super::badges::{severity_counts, status_chip};
use super::composer::Composer;
use super::panel_labels as labels;
use super::session_actions::{SessionControls, controls};
use super::sessions_list::pr_labels;
use super::thread_model::ThreadModel;
use crate::bottom::duration_label;

const LIST_OVERDRAW: f32 = 600.;

pub struct ThreadView {
    model: Entity<ThreadModel>,
    composer: Option<Entity<Composer>>,
    expanded: HashSet<SharedString>,
    list: ListState,
    rows: usize,
    _subscriptions: Vec<Subscription>,
}

impl ThreadView {
    pub fn new(model: Entity<ThreadModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = model
            .read(cx)
            .is_live()
            .then(|| cx.new(|cx| Composer::new(model.clone(), window, cx)));
        let subscriptions = vec![cx.observe(&model, |this, _, cx| this.sync_rows(cx))];
        let list = ListState::new(0, ListAlignment::Top, px(LIST_OVERDRAW));
        list.set_follow_mode(FollowMode::Tail);
        let mut this = Self {
            model,
            composer,
            expanded: HashSet::new(),
            list,
            rows: 0,
            _subscriptions: subscriptions,
        };
        this.sync_rows(cx);
        this
    }

    fn sync_rows(&mut self, cx: &mut Context<Self>) {
        let rows = self.row_count(cx);
        if rows > self.rows {
            self.list.splice(self.rows..self.rows, rows - self.rows);
        } else if rows < self.rows {
            self.list.reset(rows);
        }
        self.rows = rows;
        self.list.remeasure_items(0..rows);
        cx.notify();
    }

    pub(super) fn is_expanded(&self, key: &str) -> bool {
        self.expanded.contains(key)
    }

    pub(super) fn toggle(&mut self, key: SharedString, ix: usize, cx: &mut Context<Self>) {
        self.list.pause_following_tail();
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
        self.list.remeasure_items(ix..ix + 1);
        cx.notify();
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        let model = self.model.read(cx);
        let info = model.info();
        let (title, reference) = pr_labels(&info.pr, info.number, cx);
        let meta = format!("{reference} · {} · {}", info.agent_label, info.model_label);
        let status = model.card_status();
        let session_controls = SessionControls {
            id: info.session_id.clone(),
            status,
            resume: model.resume_action(),
            worktree: info.worktree.clone(),
        };
        let notice = model.notice().cloned();
        let counts = model.counts();
        let elapsed = duration_label(model.elapsed(Utc::now()));
        let theme = cx.theme();
        v_flex()
            .flex_none()
            .px_2()
            .py_1p5()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(status_chip(status, cx))
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(elapsed),
                    ),
            )
            .child(div().text_sm().font_semibold().line_clamp(2).child(title))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(meta),
                    )
                    .children((counts.total() > 0).then(|| severity_counts(counts, cx))),
            )
            .children(notice.map(|notice| {
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .text_xs()
                    .text_color(theme.warning)
                    .child(Icon::new(IconName::TriangleAlert).xsmall())
                    .child(notice)
            }))
            .children(controls(&session_controls, "thread", cx))
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let model = self.model.read(cx);
        let theme = cx.theme();
        let error = model.action_error().cloned().map(|error| {
            h_flex()
                .px_2()
                .py_1()
                .gap_2()
                .items_center()
                .text_xs()
                .text_color(theme.danger)
                .child(div().flex_1().min_w_0().child(error))
                .child(
                    Button::new("agent-thread-dismiss-error")
                        .icon(Icon::new(IconName::X))
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.model.update(cx, |model, cx| model.dismiss_error(cx))
                        })),
                )
        });
        let body = match (&self.composer, model.is_live()) {
            (Some(composer), true) => composer.clone().into_any_element(),
            _ => div()
                .px_2()
                .py_1p5()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(labels::SESSION_ENDED)
                .into_any_element(),
        };
        v_flex()
            .flex_none()
            .border_t_1()
            .border_color(theme.border)
            .children(error)
            .child(body)
    }
}

impl Render for ThreadView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.header(cx);
        let footer = self.footer(cx);
        let view = cx.weak_entity();
        let rows = list(self.list.clone(), move |ix, _, cx| {
            view.update(cx, |this, cx| this.render_row(ix, cx))
                .unwrap_or_else(|_| div().into_any_element())
        })
        .size_full();
        v_flex()
            .size_full()
            .child(header)
            .child(div().flex_1().min_h_0().pt_2().child(rows))
            .child(footer)
    }
}
