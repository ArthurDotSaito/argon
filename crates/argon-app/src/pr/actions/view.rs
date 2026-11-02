use std::time::Instant;

use argon_github::{RunStatus, WorkflowRun};
use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use super::{ActionsTab, Rerun, labels};
use crate::bottom::JobLogModel;
use crate::pr::conversation::CONTENT_MAX_WIDTH;

impl ActionsTab {
    fn toolbar(&self, runs: &[WorkflowRun], window: &mut Window, cx: &mut Context<Self>) -> Div {
        let sha = self
            .head
            .as_deref()
            .map(|head| head.get(..7).unwrap_or(head))
            .unwrap_or_default();
        let has_failed = runs.iter().any(|run| run.status == RunStatus::Failure);
        let rerunning = matches!(self.rerun, Rerun::Running);
        let enabled = self.can_rerun(window, cx);
        let rerun = has_failed.then(|| {
            Button::new("actions-rerun")
                .icon(Icon::new(IconName::RotateCw))
                .label(if rerunning {
                    labels::RERUNNING
                } else {
                    labels::RERUN_FAILED
                })
                .small()
                .outline()
                .disabled(!enabled)
                .on_click(
                    cx.listener(|this, _: &ClickEvent, window, cx| this.confirm_rerun(window, cx)),
                )
        });
        let refresh: AnyElement = if self.refreshing {
            Spinner::new().small().into_any_element()
        } else {
            Button::new("actions-refresh")
                .icon(Icon::new(IconName::RefreshCw))
                .small()
                .ghost()
                .tooltip(labels::REFRESH)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reload(cx)))
                .into_any_element()
        };
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(labels::summary(runs.len(), sha)),
            )
            .child(div().flex_1())
            .child(refresh)
            .children(rerun)
            .child(
                Button::new("actions-open-github")
                    .icon(Icon::new(IconName::ExternalLink))
                    .label(labels::OPEN_ON_GITHUB)
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, _| this.open_checks())),
            )
    }

    fn banner(&self, cx: &App) -> Option<Div> {
        let message = match &self.rerun {
            Rerun::Failed(message) => Some(message.clone()),
            _ => self.error.clone(),
        }?;
        let theme = cx.theme();
        Some(
            h_flex()
                .w_full()
                .px_3()
                .py_1p5()
                .gap_2()
                .items_center()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.danger.opacity(0.5))
                .bg(theme.danger.opacity(0.08))
                .text_sm()
                .child(
                    Icon::new(IconName::CircleAlert)
                        .small()
                        .text_color(theme.danger),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.foreground)
                        .child(message),
                ),
        )
    }

    fn state_panel(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let base = v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .text_sm()
            .text_color(theme.muted_foreground);
        match &self.error {
            Some(message) => base
                .child(
                    Icon::new(IconName::CircleAlert)
                        .large()
                        .text_color(theme.danger),
                )
                .child(
                    div()
                        .max_w(px(560.))
                        .text_center()
                        .text_color(theme.foreground)
                        .child(message.clone()),
                )
                .child(
                    Button::new("actions-retry")
                        .icon(Icon::new(IconName::RotateCw))
                        .label(labels::RETRY)
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reload(cx))),
                ),
            None => base.child(Spinner::new()).child(labels::LOADING),
        }
    }

    fn empty_runs(&self, cx: &App) -> AnyElement {
        v_flex()
            .py_8()
            .items_center()
            .gap_2()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(Icon::new(IconName::Play).large())
            .child(labels::EMPTY)
            .into_any_element()
    }
}

impl Render for ActionsTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.painted_at = Some(Instant::now());
        let Some(runs) = self.runs.clone() else {
            return self.state_panel(cx).into_any_element();
        };
        let now = Utc::now();
        let selected_job = JobLogModel::global(cx)
            .and_then(|model| model.read(cx).target().map(|target| target.job_id));
        let toolbar = self.toolbar(&runs, window, cx);
        let banner = self.banner(cx);
        let body = if runs.is_empty() {
            self.empty_runs(cx)
        } else {
            v_flex()
                .w_full()
                .gap_2()
                .children(
                    runs.iter()
                        .map(|run| self.run_block(run, now, selected_job, cx))
                        .collect::<Vec<_>>(),
                )
                .into_any_element()
        };
        div()
            .id("pr-actions-scroll")
            .size_full()
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .vertical_scrollbar(&self.scroll)
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .max_w(px(CONTENT_MAX_WIDTH))
                    .mx_auto()
                    .px_4()
                    .py_4()
                    .gap_3()
                    .child(toolbar)
                    .children(banner)
                    .child(body),
            )
            .into_any_element()
    }
}
