use argon_core::{AgentKind, PrKey};
use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::Context;
use tokio::sync::watch;

use super::launcher::{
    LaunchError, LaunchRequest, LaunchStage, Launched, discard_worktree, launch,
};
use super::sessions::{ActiveSession, AgentSessions};
use crate::services::{ServiceError, Services};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchState {
    Running(LaunchStage),
    Cancelling,
    Started,
    Cancelled,
    Failed(String),
}

pub struct SessionMeta {
    pub agent: AgentKind,
    pub agent_label: &'static str,
    pub model_label: String,
}

struct Identity {
    session_id: String,
    pr: PrKey,
    number: u64,
    model: String,
    meta: SessionMeta,
}

pub struct PendingLaunch {
    cancel: watch::Sender<bool>,
    state: LaunchState,
}

impl AgentSessions {
    pub fn pending_launches(&self) -> Vec<String> {
        self.launches()
            .iter()
            .filter(|(_, launch)| {
                matches!(
                    launch.state,
                    LaunchState::Running(_) | LaunchState::Cancelling
                )
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn launch_state(&self, session_id: &str) -> Option<&LaunchState> {
        self.launches().get(session_id).map(|launch| &launch.state)
    }

    fn set_launch_state(&mut self, session_id: &str, state: LaunchState, cx: &mut Context<Self>) {
        if let Some(launch) = self.launches_mut().get_mut(session_id) {
            launch.state = state;
            cx.notify();
        }
    }

    pub fn start_launch(
        &mut self,
        request: LaunchRequest,
        meta: SessionMeta,
        services: &Services,
        cx: &mut Context<Self>,
    ) {
        let identity = Identity {
            session_id: request.session_id.clone(),
            pr: request.pr.clone(),
            number: request.number,
            model: request.model.clone(),
            meta,
        };
        let id = identity.session_id.clone();
        let (cancel, cancelled) = watch::channel(false);
        let (progress, mut stages) = mpsc::unbounded();
        self.launches_mut().insert(
            id.clone(),
            PendingLaunch {
                cancel,
                state: LaunchState::Running(LaunchStage::PreparingWorktree),
            },
        );
        cx.notify();
        let job = services.spawn_runtime(launch(request, progress, cancelled));
        let progress_id = id.clone();
        cx.spawn(async move |this, cx| {
            while let Some(stage) = stages.next().await {
                _ = this.update(cx, |this, cx| {
                    if matches!(
                        this.launch_state(&progress_id),
                        Some(LaunchState::Running(_))
                    ) {
                        this.set_launch_state(&progress_id, LaunchState::Running(stage), cx);
                    }
                });
            }
        })
        .detach();
        let services = services.clone();
        cx.spawn(async move |this, cx| {
            let result = job.await;
            _ = this.update(cx, |this, cx| {
                this.finish_launch(identity, result, &services, cx)
            });
        })
        .detach();
    }

    pub fn cancel_launch(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let Some(launch) = self.launches_mut().get_mut(session_id) else {
            return;
        };
        if matches!(launch.state, LaunchState::Running(_)) {
            _ = launch.cancel.send(true);
            launch.state = LaunchState::Cancelling;
            cx.notify();
        }
    }

    fn finish_launch(
        &mut self,
        identity: Identity,
        result: Result<Result<Launched, LaunchError>, ServiceError>,
        services: &Services,
        cx: &mut Context<Self>,
    ) {
        let id = identity.session_id.clone();
        let cancelling = matches!(self.launch_state(&id), Some(LaunchState::Cancelling));
        let state = match result {
            Ok(Ok(launched)) if cancelling => {
                let worktrees = self.worktrees();
                drop(services.spawn_runtime(async move {
                    launched.handle.shutdown().await;
                    discard_worktree(&worktrees, &launched.worktree).await;
                }));
                LaunchState::Cancelled
            }
            Ok(Ok(launched)) => {
                self.add(
                    ActiveSession {
                        session_id: identity.session_id,
                        pr: identity.pr,
                        number: Some(identity.number),
                        agent: identity.meta.agent,
                        agent_label: identity.meta.agent_label,
                        model: identity.model,
                        model_label: identity.meta.model_label,
                        worktree: launched.worktree,
                        handle: launched.handle,
                        head_oid: launched.head,
                        created_at: chrono::Utc::now(),
                    },
                    cx,
                );
                LaunchState::Started
            }
            Ok(Err(LaunchError::Cancelled)) => LaunchState::Cancelled,
            Ok(Err(LaunchError::Failed(error))) => LaunchState::Failed(error),
            Err(error) => LaunchState::Failed(error.to_string()),
        };
        self.set_launch_state(&id, state, cx);
    }
}
