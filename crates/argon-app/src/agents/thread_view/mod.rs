mod items;
mod tools;

use std::collections::HashSet;

use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AppContext as _, Context, Div, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, div, px,
};

use super::badges::{severity_counts, status_chip};
use super::composer::Composer;
use super::panel_labels as labels;
use super::session_actions::{SessionControls, controls};
use super::thread_model::ThreadModel;
use crate::bottom::duration_label;
use crate::inbox::InboxModel;

const FOLLOW_SLACK: f32 = 48.;

pub enum ThreadViewEvent {
    Back,
}

pub struct ThreadView {
    model: Entity<ThreadModel>,
    composer: Option<Entity<Composer>>,
    expanded: HashSet<SharedString>,
    scroll: ScrollHandle,
    follow: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ThreadViewEvent> for ThreadView {}

impl ThreadView {
    pub fn new(model: Entity<ThreadModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = model
            .read(cx)
            .is_live()
            .then(|| cx.new(|cx| Composer::new(model.clone(), window, cx)));
        let subscriptions = vec![cx.observe(&model, |this, _, cx| {
            this.follow |= this.at_bottom();
            cx.notify();
        })];
        Self {
            model,
            composer,
            expanded: HashSet::new(),
            scroll: ScrollHandle::new(),
            follow: true,
            _subscriptions: subscriptions,
        }
    }

    fn at_bottom(&self) -> bool {
        let max = self.scroll.max_offset().y;
        let offset = -self.scroll.offset().y;
        max - offset <= px(FOLLOW_SLACK)
    }

    pub(super) fn is_expanded(&self, key: &str) -> bool {
        self.expanded.contains(key)
    }

    pub(super) fn toggle(&mut self, key: SharedString, cx: &mut Context<Self>) {
        self.follow = false;
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
        cx.notify();
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        let model = self.model.read(cx);
        let info = model.info();
        let pr = InboxModel::global(cx).and_then(|inbox| inbox.read(cx).pr(&info.pr).cloned());
        let title: SharedString = pr
            .as_ref()
            .map_or(labels::UNKNOWN_PR.into(), |pr| pr.title.clone().into());
        let reference = pr.as_ref().map_or_else(
            || info.number.map(|n| format!("#{n}")).unwrap_or_default(),
            |pr| format!("{} #{}", pr.repo, pr.number),
        );
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
                    .child(
                        Button::new("agent-thread-back")
                            .icon(Icon::new(IconName::ArrowLeft))
                            .label(labels::BACK)
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(ThreadViewEvent::Back))),
                    )
                    .child(div().flex_1())
                    .child(status_chip(status, cx))
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
        if self.follow {
            self.scroll.scroll_to_bottom();
        }
        let header = self.header(cx);
        let items = self.render_items(cx);
        let footer = self.footer(cx);
        v_flex()
            .size_full()
            .child(header)
            .child(
                v_flex()
                    .id("agent-thread-scroll")
                    .flex_1()
                    .min_h_0()
                    .track_scroll(&self.scroll)
                    .overflow_y_scroll()
                    .on_scroll(cx.listener(|this, _, _, _| this.follow = false))
                    .p_2()
                    .gap_2()
                    .children(items),
            )
            .child(footer)
    }
}
