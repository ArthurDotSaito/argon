use argon_github::{Job, RunStatus, WorkflowRun};
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    ClickEvent, Context, Div, InteractiveElement as _, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::ActionsTab;
use super::labels;
use crate::bottom::{duration_label, is_running, run_status_icon};

const ROW_HEIGHT: f32 = 30.;
const JOB_INDENT: f32 = 30.;

impl ActionsTab {
    pub(super) fn run_block(
        &self,
        ix: usize,
        run: &WorkflowRun,
        now: DateTime<Utc>,
        selected_job: Option<u64>,
        cx: &mut Context<Self>,
    ) -> Div {
        let expanded = self.is_expanded(run);
        let header = self.run_header(ix, run, expanded, now, cx);
        let theme = cx.theme();
        let warning = run.jobs_error.as_ref().map(|error| {
            h_flex()
                .pl(px(JOB_INDENT))
                .pr_3()
                .py_1()
                .gap_1p5()
                .items_center()
                .text_xs()
                .text_color(theme.warning)
                .child(Icon::new(IconName::TriangleAlert).xsmall())
                .child(div().min_w_0().truncate().child(error.clone()))
        });
        let jobs = expanded.then(|| {
            v_flex().children(
                run.jobs
                    .iter()
                    .enumerate()
                    .map(|(job_ix, job)| {
                        let selected = selected_job == Some(job.id);
                        self.job_row(ix, job_ix, job, selected, now, cx)
                    })
                    .collect::<Vec<_>>(),
            )
        });
        let theme = cx.theme();
        v_flex()
            .w_full()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden()
            .child(header)
            .children(warning)
            .children(jobs)
    }

    fn run_header(
        &self,
        ix: usize,
        run: &WorkflowRun,
        expanded: bool,
        now: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let chevron = if expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };
        let duration = run_duration(run, now).unwrap_or_else(|| labels::NO_DURATION.to_string());
        let run_id = run.id;
        h_flex()
            .id(("actions-run", ix))
            .h(px(ROW_HEIGHT))
            .px_2()
            .gap_2()
            .items_center()
            .bg(theme.secondary.opacity(0.4))
            .cursor_pointer()
            .hover(|style| style.bg(theme.secondary))
            .child(
                Icon::new(chevron)
                    .xsmall()
                    .text_color(theme.muted_foreground),
            )
            .child(run_status_icon(run.status, cx))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(run.name.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(labels::attempt(run.run_attempt)),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(duration),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_run(run_id, cx)))
    }

    fn job_row(
        &self,
        run_ix: usize,
        job_ix: usize,
        job: &Job,
        selected: bool,
        now: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let row = h_flex()
            .id(("actions-job", run_ix * 1000 + job_ix))
            .h(px(ROW_HEIGHT - 4.))
            .pl(px(JOB_INDENT))
            .pr_3()
            .gap_2()
            .items_center()
            .border_t_1()
            .border_color(theme.border.opacity(0.6))
            .cursor_pointer()
            .hover(|style| style.bg(theme.muted.opacity(0.6)));
        let row = if selected {
            row.bg(theme.list_active)
                .border_l_2()
                .border_color(theme.primary)
        } else {
            row
        };
        row.child(run_status_icon(job.status, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(theme.foreground)
                    .child(job.name.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(job_duration(job, now)),
            )
            .on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| this.open_job(run_ix, job_ix, cx)),
            )
    }
}

fn job_duration(job: &Job, now: DateTime<Utc>) -> String {
    match (job.started_at, job.completed_at) {
        (Some(start), Some(end)) => duration_label(end - start),
        (Some(start), None) if is_running(job.status) => {
            labels::running_for(&duration_label(now - start))
        }
        (None, _) if job.status == RunStatus::Queued => labels::QUEUED.to_string(),
        _ => labels::NO_DURATION.to_string(),
    }
}

fn run_duration(run: &WorkflowRun, now: DateTime<Utc>) -> Option<String> {
    let start = run.jobs.iter().filter_map(|job| job.started_at).min()?;
    let running = run.jobs.iter().any(|job| is_running(job.status));
    let end = if running {
        now
    } else {
        run.jobs.iter().filter_map(|job| job.completed_at).max()?
    };
    let label = duration_label(end - start);
    Some(if running {
        labels::running_for(&label)
    } else {
        label
    })
}
