mod labels;
mod rerun;
mod runs;
mod view;

use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use argon_core::{ChecksState, PrState};
use argon_github::{GithubError, PrDetail, RunStatus, WorkflowRun};
use gpui_kit::component::WindowExt as _;
use gpui_kit::{
    App, AppContext as _, Context, ScrollHandle, SharedString, Subscription, Task, Window,
};

use super::readiness::error_line;
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

pub struct RerunTarget {
    pub run_id: u64,
    pub name: String,
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
    opened: bool,
    auto_opened: Option<String>,
    rerun: Rerun,
    scroll: ScrollHandle,
    load_task: Option<Task<()>>,
    rerun_task: Option<Task<()>>,
    ticker: Option<Task<()>>,
    error_job: Option<u64>,
    error_line: Option<SharedString>,
    error_task: Option<Task<()>>,
    overview: bool,
    _log_subscription: Option<Subscription>,
}

impl ActionsTab {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
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
            opened: false,
            auto_opened: None,
            rerun: Rerun::Idle,
            scroll: ScrollHandle::new(),
            load_task: None,
            rerun_task: None,
            ticker: None,
            error_job: None,
            error_line: None,
            error_task: None,
            overview: false,
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
            self.error_job = None;
            self.error_line = None;
            self.error_task = None;
            self.sync_ticker(cx);
        }
        self.overview = wants_overview(&detail);
        self.opened |= self.overview;
        self.detail = Some(detail);
        if self.opened {
            self.reload(cx);
        } else {
            cx.notify();
        }
    }

    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        if active && !self.opened {
            self.opened = true;
            self.reload(cx);
        } else if active {
            self.auto_open_failed_job(cx);
        }
        self.sync_ticker(cx);
        cx.notify();
    }

    pub fn count(&self) -> Option<usize> {
        self.runs.as_ref().map(|runs| runs.len())
    }

    pub fn runs(&self) -> Option<Rc<Vec<WorkflowRun>>> {
        self.runs.clone()
    }

    pub fn failed_count(&self) -> usize {
        self.failed_runs().len()
    }

    pub fn error_line(&self) -> Option<SharedString> {
        self.error_line.clone()
    }

    pub fn mark_overview_visible(&mut self) {
        if self.overview {
            self.painted_at = Some(Instant::now());
        }
    }

    pub fn reopen_failed_log(&mut self, cx: &mut Context<Self>) {
        self.auto_opened = None;
        self.auto_open_failed_job(cx);
    }

    fn watching(&self) -> bool {
        (self.active || self.overview) && self.has_running()
    }

    fn load_error_line(&mut self, cx: &mut Context<Self>) {
        let failed_job = self.runs.as_ref().and_then(|runs| {
            runs.iter()
                .filter(|run| run.status == RunStatus::Failure)
                .flat_map(|run| &run.jobs)
                .find(|job| job.status == RunStatus::Failure)
                .map(|job| job.id)
        });
        if self.error_job == failed_job {
            return;
        }
        self.error_job = failed_job;
        self.error_line = None;
        self.error_task = None;
        let (Some(job_id), Some(detail), Some(services)) =
            (failed_job, self.detail.clone(), app_services(cx))
        else {
            return;
        };
        let repo = detail.pr.repo.clone();
        let task = services.spawn_github(cx, detail.pr.host.clone(), move |client| async move {
            client.job_log(&repo, job_id).await
        });
        self.error_task = Some(cx.spawn(async move |this, cx| {
            let log = match task.await {
                Ok(log) => log,
                Err(error) => {
                    tracing::warn!(%error, job_id, "failed to load the failed job log");
                    _ = this.update(cx, |this, _| {
                        if this.error_job == Some(job_id) {
                            this.error_job = None;
                        }
                    });
                    return;
                }
            };
            let line = cx
                .background_spawn(async move { error_line(&log.text) })
                .await;
            _ = this.update(cx, |this, cx| {
                if this.error_job == Some(job_id) {
                    this.error_line = line.map(Into::into);
                    cx.notify();
                }
            });
        }));
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
        self.load_error_line(cx);
        self.auto_open_failed_job(cx);
        self.sync_ticker(cx);
    }

    fn sync_ticker(&mut self, cx: &mut Context<Self>) {
        if !self.watching() {
            self.ticker = None;
            return;
        }
        if self.ticker.is_some() {
            return;
        }
        self.ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let keep = this.update(cx, |this, cx| this.tick(cx)).unwrap_or(false);
                if !keep {
                    break;
                }
            }
        }));
    }

    fn tick(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.watching() {
            return false;
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
        true
    }

    pub fn has_running(&self) -> bool {
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
        let failed = runs.iter().find_map(|run| {
            let job = run
                .jobs
                .iter()
                .find(|job| job.status == RunStatus::Failure)?;
            Some((run.id, job.id))
        });
        if let Some((run_id, job_id)) = failed
            && self.open_job(run_id, job_id, cx)
        {
            self.auto_opened = self.head.clone();
        }
    }

    fn open_job(&mut self, run_id: u64, job_id: u64, cx: &mut Context<Self>) -> bool {
        let (Some(detail), Some(runs)) = (self.detail.as_ref(), self.runs.as_ref()) else {
            return false;
        };
        let Some(run) = runs.iter().find(|run| run.id == run_id) else {
            return false;
        };
        let Some(job) = run.jobs.iter().find(|job| job.id == job_id) else {
            return false;
        };
        let Some(model) = JobLogModel::global(cx) else {
            return false;
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
        model.update(cx, |model, cx| model.open(target, cx));
        true
    }

    fn can_rerun(&self, window: &mut Window, cx: &mut App) -> bool {
        !matches!(self.rerun, Rerun::Running) && !window.has_active_dialog(cx)
    }

    pub fn confirm_rerun(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_rerun(window, cx) {
            return;
        }
        let targets: Vec<RerunTarget> = self
            .failed_runs()
            .into_iter()
            .map(|run| RerunTarget {
                run_id: run.id,
                name: run.name.clone(),
            })
            .collect();
        if targets.is_empty() {
            return;
        }
        rerun::open(cx.weak_entity(), targets, window, cx);
        cx.notify();
    }

    pub fn rerun_failed(&mut self, run_ids: Vec<u64>, cx: &mut Context<Self>) {
        let Some(detail) = self.detail.clone() else {
            return;
        };
        if run_ids.is_empty() || matches!(self.rerun, Rerun::Running) {
            return;
        }
        let Some(services) = app_services(cx) else {
            self.rerun = Rerun::Failed(labels::rerun_failed(labels::SERVICES_UNAVAILABLE).into());
            cx.notify();
            return;
        };
        self.rerun = Rerun::Running;
        let repo = detail.pr.repo.clone();
        let task = services.spawn_github(cx, detail.pr.host.clone(), move |client| async move {
            let mut done = Vec::new();
            for id in run_ids {
                if let Err(error) = client.rerun_failed(&repo, id).await {
                    return Ok::<_, GithubError>((done, Some(error.to_string())));
                }
                done.push(id);
            }
            Ok((done, None))
        });
        self.rerun_task = Some(cx.spawn(async move |this, cx| {
            let (done, error) = match task.await {
                Ok(outcome) => outcome,
                Err(error) => (Vec::new(), Some(error.to_string())),
            };
            _ = this.update(cx, |this, cx| {
                tracing::info!(runs = done.len(), "failed jobs re-run requested");
                this.mark_queued(&done, cx);
                this.rerun = match &error {
                    Some(error) => {
                        tracing::warn!(%error, "failed to re-run failed jobs");
                        Rerun::Failed(labels::rerun_failed(error).into())
                    }
                    None => Rerun::Idle,
                };
                cx.notify();
            });
            if !done.is_empty() {
                cx.background_executor().timer(RERUN_REFRESH_DELAY).await;
                _ = this.update(cx, |this, cx| this.reload(cx));
            }
        }));
        cx.notify();
    }

    fn mark_queued(&mut self, ids: &[u64], cx: &mut Context<Self>) {
        let Some(runs) = self.runs.as_ref().filter(|_| !ids.is_empty()) else {
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
        self.sync_ticker(cx);
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
}

fn wants_overview(detail: &PrDetail) -> bool {
    detail.pr.state == PrState::Open
        && matches!(
            detail.pr.checks,
            Some(ChecksState::Failure | ChecksState::Error | ChecksState::Pending)
        )
}

fn app_services(cx: &App) -> Option<Services> {
    cx.try_global::<AppServices>()
        .map(|global| global.0.clone())
}
