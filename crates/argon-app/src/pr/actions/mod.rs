mod labels;
mod rerun;
mod runs;
mod view;

use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use argon_github::{GithubError, PrDetail, RunStatus, WorkflowRun};
use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, Context, ScrollHandle, SharedString, Subscription, Task, Window};

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
    auto_opened: Option<String>,
    rerun: Rerun,
    scroll: ScrollHandle,
    load_task: Option<Task<()>>,
    rerun_task: Option<Task<()>>,
    ticker: Option<Task<()>>,
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
            auto_opened: None,
            rerun: Rerun::Idle,
            scroll: ScrollHandle::new(),
            load_task: None,
            rerun_task: None,
            ticker: None,
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
            self.sync_ticker(cx);
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
        self.sync_ticker(cx);
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
        self.sync_ticker(cx);
    }

    fn sync_ticker(&mut self, cx: &mut Context<Self>) {
        if !(self.active && self.has_running()) {
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
        if !(self.active && self.has_running()) {
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

    fn confirm_rerun(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

fn app_services(cx: &App) -> Option<Services> {
    cx.try_global::<AppServices>()
        .map(|global| global.0.clone())
}
