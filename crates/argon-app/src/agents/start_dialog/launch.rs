use argon_core::{AgentKind, PrKey};
use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::component::WindowExt as _;
use gpui_kit::{Context, SharedString, Window};

use super::{Load, Phase, StartDialog, app_services, fresh_session_id};
use crate::agents::labels;
use crate::agents::launcher::{LaunchRequest, LaunchStage, launch};
use crate::agents::sessions::ActiveSession;

struct Pending {
    session_id: String,
    pr: PrKey,
    number: u64,
    agent: AgentKind,
    agent_label: &'static str,
    model: String,
    model_label: String,
}

pub(super) enum Status {
    Busy(&'static str),
    Error(SharedString),
}

impl StartDialog {
    pub(super) fn clear_failure(&mut self) {
        if matches!(self.phase, Phase::Failed(_)) {
            self.phase = Phase::Idle;
        }
    }

    fn ci_pending(&self) -> bool {
        self.include_ci && matches!(self.ci, Some(Load::Loading) | None)
    }

    pub(super) fn can_start(&self) -> bool {
        !self.is_launching()
            && self
                .selected_option()
                .is_some_and(|option| option.state.is_selectable())
            && self.model.is_some()
            && matches!(self.rendered, Some(Ok(_)))
            && !self.ci_pending()
            && self.session_id.is_ok()
    }

    pub(super) fn status(&self) -> Option<Status> {
        match &self.phase {
            Phase::Launching(LaunchStage::PreparingWorktree) => {
                return Some(Status::Busy(labels::PREPARING_WORKTREE));
            }
            Phase::Launching(LaunchStage::StartingAgent) => {
                return Some(Status::Busy(labels::STARTING_AGENT));
            }
            Phase::Failed(error) => return Some(Status::Error(error.clone())),
            Phase::Idle => {}
        }
        if let Err(error) = &self.session_id {
            return Some(Status::Error(error.clone()));
        }
        match &self.context {
            Load::Loading => return Some(Status::Busy(labels::BUILDING_CONTEXT)),
            Load::Failed(error) => return Some(Status::Error(error.clone())),
            Load::Ready(_) => {}
        }
        if self.ci_pending() {
            return Some(Status::Busy(labels::LOADING_CI));
        }
        if self.selected_option().is_none() && self.agents.ready().is_some() {
            return Some(Status::Busy(labels::PICK_AGENT));
        }
        None
    }

    fn request(&self, cx: &Context<Self>) -> Option<LaunchRequest> {
        let option = self.selected_option()?;
        let Some(Ok(prompt)) = &self.rendered else {
            return None;
        };
        let gathered = self.context.ready()?;
        let sessions = Self::sessions(cx)?;
        let sessions = sessions.read(cx);
        Some(LaunchRequest {
            spec: option.spec.clone(),
            session_id: self.session_id.clone().ok()?,
            pr: self.pr.key(),
            repo: self.pr.repo.clone(),
            number: self.pr.number,
            template_id: self.template.clone()?,
            prompt: prompt.clone(),
            allowed_files: gathered
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect(),
            model: self.model.clone()?,
            store: sessions.store(),
            worktrees: sessions.worktrees(),
        })
    }

    pub(super) fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_start() {
            return;
        }
        let (Some(services), Some(request)) = (app_services(cx), self.request(cx)) else {
            self.phase = Phase::Failed(labels::SERVICES_UNAVAILABLE.into());
            cx.notify();
            return;
        };
        let Some(option) = self.selected_option() else {
            return;
        };
        let pending = Pending {
            session_id: request.session_id.clone(),
            pr: request.pr.clone(),
            number: request.number,
            agent: option.spec.kind,
            agent_label: option.spec.label,
            model: request.model.clone(),
            model_label: self.model_label().unwrap_or_default(),
        };
        self.phase = Phase::Launching(LaunchStage::PreparingWorktree);
        cx.notify();
        let (progress, mut stages) = mpsc::unbounded();
        let job = services.spawn_runtime(launch(request, progress));
        self.tasks.push(cx.spawn(async move |this, cx| {
            while let Some(stage) = stages.next().await {
                _ = this.update(cx, |this, cx| {
                    if this.is_launching() {
                        this.phase = Phase::Launching(stage);
                        cx.notify();
                    }
                });
            }
        }));
        self.tasks.push(cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            _ = this.update_in(cx, |this, window, cx| {
                match result
                    .map_err(|error| error.to_string())
                    .and_then(|inner| inner)
                {
                    Ok(launched) => {
                        let session = ActiveSession {
                            session_id: pending.session_id,
                            pr: pending.pr,
                            number: pending.number,
                            agent: pending.agent,
                            agent_label: pending.agent_label,
                            model: pending.model,
                            model_label: pending.model_label,
                            worktree: launched.worktree,
                            handle: launched.handle,
                        };
                        if let Some(sessions) = Self::sessions(cx) {
                            sessions.update(cx, |sessions, cx| sessions.add(session, cx));
                        }
                        window.close_dialog(cx);
                    }
                    Err(error) => {
                        this.phase = Phase::Failed(labels::launch_failed(&error).into());
                        this.session_id = fresh_session_id();
                        cx.notify();
                    }
                }
            });
        }));
    }
}
