use argon_core::{SessionKind, resolve_template};
use gpui_kit::component::WindowExt as _;
use gpui_kit::{Context, SharedString, Window};

use super::{Gathering, Load, Phase, StartDialog, app_services, fresh_session_id};
use crate::agents::context::{plan_context, summary_context};
use crate::agents::labels;
use crate::agents::launcher::{LaunchPrompt, LaunchRequest, LaunchStage};
use crate::agents::launches::{LaunchState, SessionMeta};

pub(super) enum Status {
    Busy(SharedString),
    Notice(SharedString),
    Error(SharedString),
}

impl StartDialog {
    pub(super) fn clear_failure(&mut self) {
        if matches!(self.phase, Phase::Failed(_) | Phase::Cancelled) {
            self.phase = Phase::Idle;
        }
    }

    fn ci_pending(&self) -> bool {
        self.include_ci && matches!(self.ci, Some(Load::Loading) | None)
    }

    fn no_models(&self) -> Option<&'static str> {
        self.selected_option()
            .filter(|option| option.spec.models.is_empty())
            .map(|option| option.spec.label)
    }

    pub(super) fn can_start(&self) -> bool {
        !self.is_launching()
            && self
                .selected_option()
                .is_some_and(|option| option.state.is_selectable())
            && self.model.is_some()
            && !self.is_rendering()
            && self
                .preview
                .as_ref()
                .is_some_and(|preview| preview.rendered.is_ok())
            && !self.ci_pending()
            && self.session_id.is_ok()
    }

    pub(super) fn status(&self) -> Option<Status> {
        match &self.phase {
            Phase::Launching(LaunchStage::PreparingWorktree) => {
                return Some(Status::Busy(labels::PREPARING_WORKTREE.into()));
            }
            Phase::Launching(LaunchStage::StartingAgent) => {
                return Some(Status::Busy(labels::STARTING_AGENT.into()));
            }
            Phase::Cancelling => return Some(Status::Busy(labels::CANCELLING.into())),
            Phase::Cancelled => return Some(Status::Notice(labels::START_CANCELLED.into())),
            Phase::Failed(error) => return Some(Status::Error(error.clone())),
            Phase::Idle => {}
        }
        if let Err(error) = &self.session_id {
            return Some(Status::Error(error.clone()));
        }
        match &self.context {
            Load::Loading => return Some(Status::Busy(labels::BUILDING_CONTEXT.into())),
            Load::Failed(error) => return Some(Status::Error(error.clone())),
            Load::Ready(_) => {}
        }
        if self.ci_pending() {
            return Some(Status::Busy(labels::LOADING_CI.into()));
        }
        if let Some(agent) = self.no_models() {
            return Some(Status::Error(labels::no_models(agent).into()));
        }
        if self.selected_option().is_none() && self.agents.ready().is_some() {
            return Some(Status::Busy(labels::PICK_AGENT.into()));
        }
        None
    }

    fn request(&self, cx: &Context<Self>) -> Option<(LaunchRequest, SessionMeta)> {
        let option = self.selected_option()?;
        let prompt = self.preview.as_ref()?.rendered.as_ref().ok()?;
        let prompt = match self.context.ready()? {
            Gathering::Review(gathered) => LaunchPrompt::Review {
                prompt: prompt.clone(),
                allowed_files: gathered
                    .files
                    .iter()
                    .map(|file| file.path.clone())
                    .collect(),
            },
            Gathering::Summary(gathering) => LaunchPrompt::Summary {
                template: Box::new(
                    resolve_template(self.current_template()?, &self.all_templates).ok()?,
                ),
                context: Box::new(summary_context(
                    &gathering.detail,
                    gathering.overview.as_ref(),
                    self.refresh_project,
                )),
                related: gathering.related.clone(),
            },
            Gathering::Plan(detail) => LaunchPrompt::Plan {
                template: Box::new(
                    resolve_template(self.current_template()?, &self.all_templates).ok()?,
                ),
                context: Box::new(plan_context(
                    detail,
                    labels::PLAN_BASE_PENDING,
                    labels::PLAN_SHA_PENDING,
                )),
            },
        };
        let sessions = Self::sessions(cx)?;
        let sessions = sessions.read(cx);
        let request = LaunchRequest {
            spec: option.spec.clone(),
            session_id: self.session_id.clone().ok()?,
            pr: self.subject.key(),
            repo: self.subject.repo().to_owned(),
            number: self.subject.number(),
            template_id: self.template.clone()?,
            prompt,
            model: self.model.clone()?,
            permission_mode: self.permission_mode,
            store: sessions.store(),
            worktrees: sessions.worktrees(),
        };
        let meta = SessionMeta {
            agent: option.spec.kind,
            agent_label: option.spec.label,
            model_label: self.model_label().unwrap_or_default(),
        };
        Some((request, meta))
    }

    pub(super) fn start(&mut self, cx: &mut Context<Self>) {
        if !self.can_start() {
            return;
        }
        let (Some(services), Some(sessions), Some((request, meta))) =
            (app_services(cx), Self::sessions(cx), self.request(cx))
        else {
            self.phase = Phase::Failed(labels::SERVICES_UNAVAILABLE.into());
            cx.notify();
            return;
        };
        self.phase = Phase::Launching(LaunchStage::PreparingWorktree);
        sessions.update(cx, |sessions, cx| {
            sessions.start_launch(request, meta, &services, cx)
        });
        cx.notify();
    }

    pub(super) fn cancel_start(&mut self, cx: &mut Context<Self>) {
        let (Some(sessions), Ok(id)) = (Self::sessions(cx), self.session_id.clone()) else {
            return;
        };
        sessions.update(cx, |sessions, cx| sessions.cancel_launch(&id, cx));
    }

    pub(super) fn on_sessions_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_listed_models(window, cx);
        self.sync_usage_revision(cx);
        if !self.is_launching() {
            return;
        }
        let (Some(sessions), Ok(id)) = (Self::sessions(cx), self.session_id.as_ref()) else {
            return;
        };
        let Some(state) = sessions.read(cx).launch_state(id).cloned() else {
            return;
        };
        match state {
            LaunchState::Running(stage) => self.phase = Phase::Launching(stage),
            LaunchState::Cancelling => self.phase = Phase::Cancelling,
            LaunchState::Started => {
                window.close_dialog(cx);
                return;
            }
            LaunchState::Cancelled => {
                self.phase = Phase::Cancelled;
                self.session_id = fresh_session_id();
            }
            LaunchState::Failed(error) => {
                self.phase = Phase::Failed(labels::launch_failed(self.kind(), &error).into());
                self.session_id = fresh_session_id();
            }
        }
        if self.kind() == SessionKind::Summary
            && matches!(self.phase, Phase::Cancelled | Phase::Failed(_))
        {
            self.rerender(cx);
            return;
        }
        cx.notify();
    }
}
