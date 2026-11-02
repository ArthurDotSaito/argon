use std::sync::Arc;

use argon_github::RunStatus;
use gpui_kit::{App, Context, Entity, EventEmitter, Global, SharedString, Task};

use super::labels;
use super::log_lines::{LogDoc, parse_log};
use super::run_status::is_running;
use crate::services::AppServices;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogTarget {
    pub host: String,
    pub repo: String,
    pub workflow: String,
    pub job: String,
    pub job_id: u64,
    pub status: RunStatus,
    pub html_url: String,
}

pub enum LogLoad {
    Loading,
    Failed(SharedString),
    Ready { doc: Arc<LogDoc>, truncated: bool },
}

pub enum JobLogEvent {
    Reveal,
}

pub struct JobLogHandle(pub Entity<JobLogModel>);

impl Global for JobLogHandle {}

pub struct JobLogModel {
    target: Option<LogTarget>,
    load: LogLoad,
    epoch: u64,
    task: Option<Task<()>>,
}

impl EventEmitter<JobLogEvent> for JobLogModel {}

impl JobLogModel {
    pub fn new() -> Self {
        Self {
            target: None,
            load: LogLoad::Loading,
            epoch: 0,
            task: None,
        }
    }

    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<JobLogHandle>()
            .map(|handle| handle.0.clone())
    }

    pub fn target(&self) -> Option<&LogTarget> {
        self.target.as_ref()
    }

    pub fn load(&self) -> &LogLoad {
        &self.load
    }

    pub fn open(&mut self, target: LogTarget, cx: &mut Context<Self>) {
        let unchanged = self.target.as_ref() == Some(&target)
            && !matches!(self.load, LogLoad::Failed(_))
            && !is_running(target.status);
        self.target = Some(target);
        if !unchanged {
            self.fetch(cx);
        }
        cx.emit(JobLogEvent::Reveal);
        cx.notify();
    }

    pub fn retry(&mut self, cx: &mut Context<Self>) {
        if self.target.is_some() {
            self.fetch(cx);
            cx.notify();
        }
    }

    pub fn update_status(&mut self, job_id: u64, status: RunStatus, cx: &mut Context<Self>) {
        if let Some(target) = self
            .target
            .as_mut()
            .filter(|target| target.job_id == job_id)
            && target.status != status
        {
            target.status = status;
            cx.notify();
        }
    }

    fn fetch(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.target.clone() else {
            return;
        };
        self.epoch += 1;
        let epoch = self.epoch;
        self.load = LogLoad::Loading;
        let Some(services) = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone())
        else {
            self.load = LogLoad::Failed(labels::load_failed(labels::SERVICES_UNAVAILABLE).into());
            return;
        };
        let repo = target.repo.clone();
        let job_id = target.job_id;
        let task = services.spawn_github(cx, target.host, move |client| async move {
            let log = client.job_log(&repo, job_id).await?;
            Ok((parse_log(&log.text), log.truncated))
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| {
                if epoch != this.epoch {
                    return;
                }
                this.load = match result {
                    Ok((doc, truncated)) => LogLoad::Ready {
                        doc: Arc::new(doc),
                        truncated,
                    },
                    Err(error) => {
                        tracing::warn!(%error, job_id, "failed to load job log");
                        LogLoad::Failed(labels::load_failed(&error.to_string()).into())
                    }
                };
                cx.notify();
            });
        }));
    }
}
