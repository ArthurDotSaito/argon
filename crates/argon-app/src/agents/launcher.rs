use std::path::PathBuf;

use argon_agents::{AgentSessionHandle, AgentSpec, SessionContext, WorktreeManager, start_session};
use argon_core::{PrKey, RenderedPrompt};
use futures::channel::mpsc::UnboundedSender;

use super::sink::SessionStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchStage {
    PreparingWorktree,
    StartingAgent,
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

pub async fn launch(
    request: LaunchRequest,
    progress: UnboundedSender<LaunchStage>,
) -> Result<Launched, String> {
    _ = progress.unbounded_send(LaunchStage::PreparingWorktree);
    let worktree = request
        .worktrees
        .prepare(
            &request.pr.host,
            &request.repo,
            request.number,
            &request.session_id,
        )
        .await
        .map_err(|error| error.to_string())?;
    _ = progress.unbounded_send(LaunchStage::StartingAgent);
    match start(&request, worktree.clone()).await {
        Ok(handle) => Ok(Launched { worktree, handle }),
        Err(error) => {
            if let Err(cleanup) = request.worktrees.remove(&worktree).await {
                tracing::warn!(error = %cleanup, path = %worktree.display(), "failed to remove worktree after launch error");
            }
            Err(error)
        }
    }
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
    let (_, handle) = start_session(&request.spec, ctx, writer)
        .await
        .map_err(|error| error.to_string())?;
    if let Err(error) = handle.prompt(request.prompt.text.clone()) {
        handle.shutdown().await;
        return Err(error.to_string());
    }
    Ok(handle)
}
