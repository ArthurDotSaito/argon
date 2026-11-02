mod labels;
mod rerun;
mod runs;

use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use argon_github::{PrDetail, RunStatus, WorkflowRun};
use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, px,
};

use super::conversation::CONTENT_MAX_WIDTH;
use crate::bottom::{JobLogModel, LogTarget, is_running};
use crate::services::{AppServices, Services};

const TICK: Duration = Duration::from_secs(1);
const POLL_INTERVAL: Duration = Duration::from_secs(15);
const VISIBLE_WINDOW: Duration = Duration::from_secs(2);
const RERUN_REFRESH_DELAY: Duration = Duration::from_secs(3);

enum Rerun {
    Idle,
    Running,
    Failed(SharedString),
}

pub struct ActionsTab {
    detail: Option<Rc<PrDetail>>,
    head: Option<String>,
    runs: Option<Rc<Vec<WorkflowRun>>>,
    error: Option<SharedString>,
    refreshing: bool,
    epoch: u64,
    loaded_at: Option<Instant>,
    painted_at: Option<Instant>,
    expanded: HashMap<u64, bool>,
    active: bool,
    auto_opened: Option<String>,
    rerun: Rerun,
    scroll: ScrollHandle,
    load_task: Option<Task<()>>,
    rerun_task: Option<Task<()>>,
    _ticker: Task<()>,
    _log_subscription: Option<Subscription>,
}

impl ActionsTab {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            }
        });
        let log_subscription =
            JobLogModel::global(cx).map(|model| cx.observe(&model, |_, _, cx| cx.notify()));
        Self {
            detail: None,
            head: None,
            runs: None,
            error: None,
            refreshing: false,
            epoch: 0,
            loaded_at: None,
            painted_at: None,
            expanded: HashMap::new(),
            active: false,
            auto_opened: None,
            rerun: Rerun::Idle,
            scroll: ScrollHandle::new(),
            load_task: None,
            rerun_task: None,
            _ticker: ticker,
            _log_subscription: log_subscription,
        }
    }

    pub fn set_detail(&mut self, detail: Rc<PrDetail>, cx: &mut Context<Self>) {
        if self.head.as_deref() != Some(detail.head_oid.as_str()) {
            self.head = Some(detail.head_oid.clone());
            self.runs = None;
            self.error = None;
            self.expanded.clear();
            self.rerun = Rerun::Idle;
        }
        self.detail = Some(detail);
        self.reload(cx);
    }

    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        if active {
            self.auto_open_failed_job(cx);
        }
        cx.notify();
    }

    pub fn count(&self) -> Option<usize> {
        self.runs.as_ref().map(|runs| runs.len())
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(detail) = self.detail.clone() else {
            return;
        };
        let Some(services) = app_services(cx) else {
            self.error = Some(labels::load_failed(labels::SERVICES_UNAVAILABLE).into());
            cx.notify();
            return;
        };
        self.epoch += 1;
        let epoch = self.epoch;
        self.refreshing = true;
        let repo = detail.pr.repo.clone();
        let head = detail.head_oid.clone();
        let task = services.spawn_github(cx, detail.pr.host.clone(), move |client| async move {
            client.workflow_runs(&repo, &head).await
        });
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| {
                if epoch != this.epoch {
                    return;
                }
                this.refreshing = false;
                this.loaded_at = Some(Instant::now());
                match result {
                    Ok(runs) => this.apply(runs, cx),
                    Err(error) => {
                        tracing::warn!(%error, "failed to load workflow runs");
                        this.error = Some(labels::load_failed(&error.to_string()).into());
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn apply(&mut self, runs: Vec<WorkflowRun>, cx: &mut Context<Self>) {
        self.error = None;
        if let Some(model) = JobLogModel::global(cx) {
            model.update(cx, |model, cx| {
                for job in runs.iter().flat_map(|run| &run.jobs) {
                    model.update_status(job.id, job.status, cx);
                }
            });
        }
        self.runs = Some(Rc::new(runs));
        self.auto_open_failed_job(cx);
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        if !self.active || !self.has_running() {
            return;
        }
        let visible = self
            .painted_at
            .is_some_and(|painted| painted.elapsed() <= VISIBLE_WINDOW);
        let stale = self
            .loaded_at
            .is_none_or(|loaded| loaded.elapsed() >= POLL_INTERVAL);
        if visible && stale && !self.refreshing {
            self.reload(cx);
        }
        cx.notify();
    }

    fn has_running(&self) -> bool {
        self.runs
            .as_ref()
            .is_some_and(|runs| runs.iter().any(|run| is_running(run.status)))
    }

    fn failed_runs(&self) -> Vec<&WorkflowRun> {
        self.runs
            .as_ref()
            .map(|runs| {
                runs.iter()
                    .filter(|run| run.status == RunStatus::Failure)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn is_expanded(&self, run: &WorkflowRun) -> bool {
        self.expanded
            .get(&run.id)
            .copied()
            .unwrap_or(run.status == RunStatus::Failure)
    }

    fn toggle_run(&mut self, run_id: u64, cx: &mut Context<Self>) {
        let expanded = self
            .runs
            .as_ref()
            .and_then(|runs| runs.iter().find(|run| run.id == run_id))
            .is_some_and(|run| self.is_expanded(run));
        self.expanded.insert(run_id, !expanded);
        cx.notify();
    }

    fn auto_open_failed_job(&mut self, cx: &mut Context<Self>) {
        if !self.active || self.auto_opened == self.head {
            return;
        }
        let Some(runs) = self.runs.clone() else {
            return;
        };
        self.auto_opened = self.head.clone();
        let failed = runs.iter().enumerate().find_map(|(run_ix, run)| {
            let job_ix = run
                .jobs
                .iter()
                .position(|job| job.status == RunStatus::Failure)?;
            Some((run_ix, job_ix))
        });
        if let Some((run_ix, job_ix)) = failed {
            self.open_job(run_ix, job_ix, cx);
        }
    }

    fn open_job(&mut self, run_ix: usize, job_ix: usize, cx: &mut Context<Self>) {
        let (Some(detail), Some(runs)) = (self.detail.as_ref(), self.runs.as_ref()) else {
            return;
        };
        let Some(run) = runs.get(run_ix) else {
            return;
        };
        let Some(job) = run.jobs.get(job_ix) else {
            return;
        };
        let target = LogTarget {
            host: detail.pr.host.clone(),
            repo: detail.pr.repo.clone(),
            workflow: run.name.clone(),
            job: job.name.clone(),
            job_id: job.id,
            status: job.status,
            html_url: job.html_url.clone(),
        };
        if let Some(model) = JobLogModel::global(cx) {
            model.update(cx, |model, cx| model.open(target, cx));
        }
    }

    fn confirm_rerun(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let workflows: Vec<String> = self
            .failed_runs()
            .into_iter()
            .map(|run| run.name.clone())
            .collect();
        if workflows.is_empty() || matches!(self.rerun, Rerun::Running) {
            return;
        }
        rerun::open(cx.weak_entity(), workflows, window, cx);
    }

    pub fn rerun_failed(&mut self, cx: &mut Context<Self>) {
        let Some(detail) = self.detail.clone() else {
            return;
        };
        let ids: Vec<u64> = self.failed_runs().iter().map(|run| run.id).collect();
        if ids.is_empty() || matches!(self.rerun, Rerun::Running) {
            return;
        }
        let Some(services) = app_services(cx) else {
            self.rerun = Rerun::Failed(labels::rerun_failed(labels::SERVICES_UNAVAILABLE).into());
            cx.notify();
            return;
        };
        self.rerun = Rerun::Running;
        let repo = detail.pr.repo.clone();
        let run_ids = ids.clone();
        let task = services.spawn_github(cx, detail.pr.host.clone(), move |client| async move {
            for id in run_ids {
                client.rerun_failed(&repo, id).await?;
            }
            Ok(())
        });
        self.rerun_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let succeeded = this
                .update(cx, |this, cx| {
                    let succeeded = match result {
                        Ok(()) => {
                            tracing::info!(runs = ids.len(), "failed jobs re-run requested");
                            this.rerun = Rerun::Idle;
                            this.mark_queued(&ids);
                            true
                        }
                        Err(error) => {
                            tracing::warn!(%error, "failed to re-run failed jobs");
                            this.rerun =
                                Rerun::Failed(labels::rerun_failed(&error.to_string()).into());
                            false
                        }
                    };
                    cx.notify();
                    succeeded
                })
                .unwrap_or(false);
            if succeeded {
                cx.background_executor().timer(RERUN_REFRESH_DELAY).await;
                _ = this.update(cx, |this, cx| this.reload(cx));
            }
        }));
        cx.notify();
    }

    fn mark_queued(&mut self, ids: &[u64]) {
        let Some(runs) = self.runs.as_ref() else {
            return;
        };
        let runs = runs
            .iter()
            .cloned()
            .map(|mut run| {
                if ids.contains(&run.id) {
                    run.status = RunStatus::Queued;
                    for job in &mut run.jobs {
                        if matches!(job.status, RunStatus::Failure | RunStatus::Cancelled) {
                            job.status = RunStatus::Queued;
                            job.started_at = None;
                            job.completed_at = None;
                        }
                    }
                }
                run
            })
            .collect();
        self.runs = Some(Rc::new(runs));
        self.loaded_at = Some(Instant::now());
    }

    fn open_checks(&self) {
        let Some(detail) = self.detail.as_ref() else {
            return;
        };
        let url = format!("{}/checks", detail.pr.url);
        if let Err(error) = open::that_detached(&url) {
            tracing::warn!(%error, url, "failed to open checks in browser");
        }
    }

    fn toolbar(&self, runs: &[WorkflowRun], cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let sha = self
            .head
            .as_deref()
            .map(|head| head.get(..7).unwrap_or(head))
            .unwrap_or_default();
        let has_failed = runs.iter().any(|run| run.status == RunStatus::Failure);
        let rerunning = matches!(self.rerun, Rerun::Running);
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
                .disabled(rerunning)
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
                    .text_color(theme.muted_foreground)
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
}

fn app_services(cx: &App) -> Option<Services> {
    cx.try_global::<AppServices>()
        .map(|global| global.0.clone())
}

impl Render for ActionsTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.painted_at = Some(Instant::now());
        let Some(runs) = self.runs.clone() else {
            return self.state_panel(cx).into_any_element();
        };
        let now = Utc::now();
        let selected_job = JobLogModel::global(cx)
            .and_then(|model| model.read(cx).target().map(|target| target.job_id));
        let toolbar = self.toolbar(&runs, cx);
        let banner = self.banner(cx);
        let body: AnyElement = if runs.is_empty() {
            v_flex()
                .py_8()
                .items_center()
                .gap_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(IconName::Play).large())
                .child(labels::EMPTY)
                .into_any_element()
        } else {
            v_flex()
                .w_full()
                .gap_2()
                .children(
                    runs.iter()
                        .enumerate()
                        .map(|(ix, run)| self.run_block(ix, run, now, selected_job, cx))
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
