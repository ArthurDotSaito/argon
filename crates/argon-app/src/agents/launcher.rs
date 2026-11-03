use std::future::Future;
use std::path::{Path, PathBuf};

use argon_agents::{AgentSessionHandle, AgentSpec, SessionContext, WorktreeManager, start_session};
use argon_core::{PrKey, RenderedPrompt};
use futures::channel::mpsc::UnboundedSender;
use tokio::sync::watch;

use super::sink::SessionStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchStage {
    PreparingWorktree,
    StartingAgent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    Cancelled,
    Failed(String),
}

pub struct LaunchRequest {
    pub spec: AgentSpec,
    pub session_id: String,
    pub pr: PrKey,
    pub repo: String,
    pub number: u64,
    pub template_id: String,
    pub prompt: RenderedPrompt,
    pub allowed_files: Vec<String>,
    pub model: String,
    pub store: SessionStore,
    pub worktrees: WorktreeManager,
}

pub struct Launched {
    pub worktree: PathBuf,
    pub handle: AgentSessionHandle,
}

async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    if cancel.wait_for(|cancelled| *cancelled).await.is_err() {
        std::future::pending::<()>().await;
    }
}

async fn unless_cancelled<T>(
    cancel: &mut watch::Receiver<bool>,
    step: impl Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        biased;
        () = cancelled(cancel) => None,
        value = step => Some(value),
    }
}

pub async fn discard_worktree(worktrees: &WorktreeManager, path: &Path) {
    if let Err(error) = worktrees.discard(path).await {
        tracing::warn!(%error, path = %path.display(), "failed to remove worktree after aborted launch");
    }
}

pub async fn launch(
    request: LaunchRequest,
    progress: UnboundedSender<LaunchStage>,
    mut cancel: watch::Receiver<bool>,
) -> Result<Launched, LaunchError> {
    let worktrees = request.worktrees.clone();
    let planned = worktrees
        .planned_path(
            &request.pr.host,
            &request.repo,
            request.number,
            &request.session_id,
        )
        .map_err(|error| LaunchError::Failed(error.to_string()))?;
    _ = progress.unbounded_send(LaunchStage::PreparingWorktree);
    let prepare = worktrees.prepare(
        &request.pr.host,
        &request.repo,
        request.number,
        &request.session_id,
    );
    let worktree = match unless_cancelled(&mut cancel, prepare).await {
        None => {
            discard_worktree(&worktrees, &planned).await;
            return Err(LaunchError::Cancelled);
        }
        Some(Err(error)) => {
            discard_worktree(&worktrees, &planned).await;
            return Err(LaunchError::Failed(error.to_string()));
        }
        Some(Ok(path)) => path,
    };
    _ = progress.unbounded_send(LaunchStage::StartingAgent);
    let started = unless_cancelled(&mut cancel, start(&request, worktree.clone())).await;
    let outcome = match started {
        None => Err(LaunchError::Cancelled),
        Some(Err(error)) => Err(LaunchError::Failed(error)),
        Some(Ok(handle)) if *cancel.borrow() => {
            handle.shutdown().await;
            Err(LaunchError::Cancelled)
        }
        Some(Ok(handle)) => match handle.prompt(request.prompt.text.clone()) {
            Ok(()) => return Ok(Launched { worktree, handle }),
            Err(error) => {
                handle.shutdown().await;
                Err(LaunchError::Failed(error.to_string()))
            }
        },
    };
    discard_worktree(&worktrees, &worktree).await;
    outcome
}

async fn start(request: &LaunchRequest, cwd: PathBuf) -> Result<AgentSessionHandle, String> {
    let store = request.store.clone();
    let writer = tokio::task::spawn_blocking(move || store.writer())
        .await
        .map_err(|error| error.to_string())??;
    let ctx = SessionContext {
        id: request.session_id.clone(),
        pr: request.pr.clone(),
        template_id: request.template_id.clone(),
        nonce: request.prompt.nonce.clone(),
        allowed_files: request.allowed_files.clone(),
        cwd,
        model: request.model.clone(),
    };
    start_session(&request.spec, ctx, writer)
        .await
        .map(|(_, handle)| handle)
        .map_err(|error| error.to_string())
}
