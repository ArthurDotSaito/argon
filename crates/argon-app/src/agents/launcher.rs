use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Instant;

use argon_agents::{
    AgentSessionHandle, AgentSpec, PreparedWorktree, SessionContext, WorktreeError,
    WorktreeManager, start_session,
};
use argon_core::{
    PermissionMode, PlanContext, PrKey, PromptTemplate, RenderedPrompt, SessionKind,
    render_plan_prompt,
};
use futures::channel::mpsc::UnboundedSender;
use tokio::sync::watch;

use super::labels;
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

pub enum LaunchPrompt {
    Review {
        prompt: RenderedPrompt,
        allowed_files: Vec<String>,
    },
    Plan {
        template: Box<PromptTemplate>,
        context: Box<PlanContext>,
    },
}

pub struct LaunchRequest {
    pub spec: AgentSpec,
    pub session_id: String,
    pub pr: PrKey,
    pub repo: String,
    pub number: u64,
    pub template_id: String,
    pub prompt: LaunchPrompt,
    pub model: String,
    pub permission_mode: PermissionMode,
    pub store: SessionStore,
    pub worktrees: WorktreeManager,
}

impl LaunchRequest {
    pub fn kind(&self) -> SessionKind {
        match self.prompt {
            LaunchPrompt::Review { .. } => SessionKind::Review,
            LaunchPrompt::Plan { .. } => SessionKind::Plan,
        }
    }

    fn planned_path(&self) -> Result<PathBuf, WorktreeError> {
        let (host, repo, number, session) =
            (&self.pr.host, &self.repo, self.number, &self.session_id);
        match self.prompt {
            LaunchPrompt::Review { .. } => self.worktrees.planned_path(host, repo, number, session),
            LaunchPrompt::Plan { .. } => self
                .worktrees
                .planned_default_path(host, repo, number, session),
        }
    }
}

pub struct Launched {
    pub worktree: PathBuf,
    pub head: Option<String>,
    pub handle: AgentSessionHandle,
}

struct Checkout {
    path: PathBuf,
    base_sha: Option<String>,
    prompt: RenderedPrompt,
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

pub fn plan_prompt(
    template: &PromptTemplate,
    context: &PlanContext,
    prepared: &PreparedWorktree,
) -> Result<RenderedPrompt, String> {
    let context = PlanContext {
        base: prepared.branch.clone(),
        base_sha: prepared.base_sha.clone(),
        ..context.clone()
    };
    render_plan_prompt(template, &context)
        .map_err(|error| labels::render_failed(&error.to_string()))
}

async fn checkout(request: &LaunchRequest) -> Result<Checkout, String> {
    let worktrees = &request.worktrees;
    let (host, repo, number, session) = (
        &request.pr.host,
        &request.repo,
        request.number,
        &request.session_id,
    );
    match &request.prompt {
        LaunchPrompt::Review { prompt, .. } => {
            let path = worktrees
                .prepare(host, repo, number, session)
                .await
                .map_err(|error| error.to_string())?;
            Ok(Checkout {
                path,
                base_sha: None,
                prompt: prompt.clone(),
            })
        }
        LaunchPrompt::Plan { template, context } => {
            let prepared = worktrees
                .prepare_default(host, repo, number, session)
                .await
                .map_err(|error| error.to_string())?;
            match plan_prompt(template, context, &prepared) {
                Ok(prompt) => Ok(Checkout {
                    path: prepared.path,
                    base_sha: Some(prepared.base_sha),
                    prompt,
                }),
                Err(error) => {
                    discard_worktree(worktrees, &prepared.path).await;
                    Err(error)
                }
            }
        }
    }
}

async fn session_head(worktrees: &WorktreeManager, checkout: &Checkout) -> Option<String> {
    if let Some(base_sha) = &checkout.base_sha {
        return Some(base_sha.clone());
    }
    match worktrees.head_oid(&checkout.path).await {
        Ok(head) => Some(head),
        Err(error) => {
            tracing::warn!(%error, "failed to read worktree head at session start");
            None
        }
    }
}

pub async fn launch(
    request: LaunchRequest,
    progress: UnboundedSender<LaunchStage>,
    mut cancel: watch::Receiver<bool>,
) -> Result<Launched, LaunchError> {
    let worktrees = request.worktrees.clone();
    let planned = request
        .planned_path()
        .map_err(|error| LaunchError::Failed(error.to_string()))?;
    _ = progress.unbounded_send(LaunchStage::PreparingWorktree);
    let prepare_started = Instant::now();
    let checkout = match unless_cancelled(&mut cancel, checkout(&request)).await {
        None => {
            discard_worktree(&worktrees, &planned).await;
            return Err(LaunchError::Cancelled);
        }
        Some(Err(error)) => {
            discard_worktree(&worktrees, &planned).await;
            return Err(LaunchError::Failed(error));
        }
        Some(Ok(checkout)) => checkout,
    };
    tracing::info!(
        elapsed_ms = prepare_started.elapsed().as_millis(),
        "launch stage finished: worktree"
    );
    let head = session_head(&worktrees, &checkout).await;
    _ = progress.unbounded_send(LaunchStage::StartingAgent);
    let start_started = Instant::now();
    let started = unless_cancelled(&mut cancel, start(&request, &checkout)).await;
    tracing::info!(
        agent = request.spec.label,
        elapsed_ms = start_started.elapsed().as_millis(),
        ok = matches!(started, Some(Ok(_))),
        "launch stage finished: agent session"
    );
    let outcome = match started {
        None => Err(LaunchError::Cancelled),
        Some(Err(error)) => Err(LaunchError::Failed(error)),
        Some(Ok(handle)) if *cancel.borrow() => {
            handle.shutdown().await;
            Err(LaunchError::Cancelled)
        }
        Some(Ok(handle)) => match handle.prompt(checkout.prompt.text.clone()) {
            Ok(()) => {
                return Ok(Launched {
                    worktree: checkout.path,
                    head,
                    handle,
                });
            }
            Err(error) => {
                handle.shutdown().await;
                Err(LaunchError::Failed(error.to_string()))
            }
        },
    };
    discard_worktree(&worktrees, &checkout.path).await;
    outcome
}

async fn start(request: &LaunchRequest, checkout: &Checkout) -> Result<AgentSessionHandle, String> {
    let store = request.store.clone();
    let writer = tokio::task::spawn_blocking(move || store.writer())
        .await
        .map_err(|error| error.to_string())??;
    let allowed_files = match &request.prompt {
        LaunchPrompt::Review { allowed_files, .. } => allowed_files.clone(),
        LaunchPrompt::Plan { .. } => Vec::new(),
    };
    let ctx = SessionContext {
        id: request.session_id.clone(),
        pr: request.pr.clone(),
        template_id: request.template_id.clone(),
        nonce: checkout.prompt.nonce.clone(),
        allowed_files,
        cwd: checkout.path.clone(),
        read_roots: Vec::new(),
        repo: request.repo.clone(),
        number: request.number,
        refresh_project: false,
        model: request.model.clone(),
        permission_mode: request.permission_mode,
        kind: request.kind(),
        base_sha: checkout.base_sha.clone(),
    };
    start_session(&request.spec, ctx, writer)
        .await
        .map(|(_, handle)| handle)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use argon_core::TemplateKind;

    use super::*;

    fn template(body: &str) -> PromptTemplate {
        PromptTemplate {
            id: "plano".into(),
            name: "Plano".into(),
            kind: TemplateKind::Plan,
            body: body.into(),
            ..PromptTemplate::default()
        }
    }

    fn context() -> PlanContext {
        PlanContext {
            issue_title: "Slot".into(),
            issue_number: 7,
            issue_url: "https://github.com/acme/api/issues/7".into(),
            issue_body: String::new(),
            repo: "acme/api".into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            milestone: None,
            comments: Vec::new(),
            base: "branch padrão".into(),
            base_sha: "definido ao iniciar".into(),
        }
    }

    #[test]
    fn plan_prompt_uses_the_prepared_branch_and_sha() {
        let prepared = PreparedWorktree {
            path: PathBuf::from("wt"),
            base_sha: "abc123".into(),
            branch: "develop".into(),
        };
        let prompt = plan_prompt(
            &template("#{{issue.number}} em {{base}} @ {{base_sha}} ({{nonce}})"),
            &context(),
            &prepared,
        )
        .unwrap();
        assert!(prompt.text.starts_with("#7 em develop @ abc123 ("));
        assert!(prompt.text.contains(&prompt.nonce));
    }

    #[test]
    fn plan_prompt_reports_a_broken_template() {
        let prepared = PreparedWorktree {
            path: PathBuf::from("wt"),
            base_sha: "abc123".into(),
            branch: "main".into(),
        };
        let error = plan_prompt(&template("{{ pr.title }}"), &context(), &prepared).unwrap_err();
        assert!(error.starts_with("Falha ao renderizar o prompt"));
    }
}
