mod acp;
mod driver;
mod events;
mod permissions;
mod setup;
mod sink;
mod turn;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use argon_core::PrKey;
use futures::Stream;
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::registry::AgentSpec;

pub use events::{AgentEvent, PermissionOption, ToolStatus};
pub use permissions::{PermissionDecision, PermissionPrompt, decide as decide_permission};
pub use sink::StoreSink;
pub use turn::{TurnOutcome, finish_turn};

const SHUTDOWN_WAIT: Duration = Duration::from_secs(10);
const SESSION_ID_BYTES: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("agente `{0}` não encontrado no PATH")]
    CommandNotFound(String),
    #[error("falha ao iniciar o agente: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("o agente não respondeu ao handshake ACP em {0:?}")]
    StartupTimeout(Duration),
    #[error("erro ACP: {0}")]
    Acp(String),
    #[error("o agente não suporta retomar sessões (session/load)")]
    LoadUnsupported,
    #[error("o modelo `{model}` não está disponível neste agente")]
    ModelUnavailable { model: String },
    #[error("não consegui trocar o modelo para `{model}`: {reason}")]
    SetModel { model: String, reason: String },
    #[error("este agente não roda via ACP")]
    NotAcp,
    #[error("pedido de permissão `{0}` não está pendente")]
    UnknownPermission(String),
    #[error("a sessão do agente foi encerrada")]
    Closed,
    #[error("falha ao gerar id de sessão: {0}")]
    Id(#[from] getrandom::Error),
}

pub type Result<T> = std::result::Result<T, SessionError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionContext {
    pub id: String,
    pub pr: PrKey,
    pub template_id: String,
    pub nonce: String,
    pub allowed_files: Vec<String>,
    pub cwd: PathBuf,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHandle {
    pub id: String,
    pub acp_session_id: String,
}

pub fn new_session_id() -> Result<String> {
    let mut bytes = [0u8; SESSION_ID_BYTES];
    getrandom::fill(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[derive(Debug)]
pub(crate) enum Command {
    Prompt(String),
    Cancel,
    RespondPermission {
        request_id: String,
        option_id: String,
        reply: oneshot::Sender<Result<()>>,
    },
    SetModel {
        model: String,
        reply: oneshot::Sender<Result<()>>,
    },
    Shutdown,
}

#[derive(Clone)]
pub struct AgentSessionHandle {
    commands: mpsc::UnboundedSender<Command>,
    events: broadcast::Sender<AgentEvent>,
    done: watch::Receiver<bool>,
}

impl AgentSessionHandle {
    fn send(&self, command: Command) -> Result<()> {
        self.commands
            .send(command)
            .map_err(|_| SessionError::Closed)
    }

    pub fn prompt(&self, text: impl Into<String>) -> Result<()> {
        self.send(Command::Prompt(text.into()))
    }

    pub fn cancel(&self) -> Result<()> {
        self.send(Command::Cancel)
    }

    pub async fn respond_permission(&self, request_id: &str, option_id: &str) -> Result<()> {
        let (reply, response) = oneshot::channel();
        self.send(Command::RespondPermission {
            request_id: request_id.to_owned(),
            option_id: option_id.to_owned(),
            reply,
        })?;
        response.await.map_err(|_| SessionError::Closed)?
    }

    pub async fn set_model(&self, model: &str) -> Result<()> {
        let (reply, response) = oneshot::channel();
        self.send(Command::SetModel {
            model: model.to_owned(),
            reply,
        })?;
        response.await.map_err(|_| SessionError::Closed)?
    }

    pub fn events(&self) -> impl Stream<Item = AgentEvent> + Send + 'static {
        futures::stream::unfold(self.events.subscribe(), |mut receiver| async move {
            loop {
                match receiver.recv().await {
                    Ok(event) => return Some((event, receiver)),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        })
    }

    pub fn is_finished(&self) -> bool {
        *self.done.borrow()
    }

    pub async fn shutdown(&self) {
        let _ = self.send(Command::Shutdown);
        let mut done = self.done.clone();
        let _ = tokio::time::timeout(SHUTDOWN_WAIT, done.wait_for(|finished| *finished)).await;
    }
}

pub async fn start_session(
    spec: &AgentSpec,
    ctx: SessionContext,
    store: Arc<dyn StoreSink>,
) -> Result<(SessionHandle, AgentSessionHandle)> {
    acp::launch(spec, ctx, store, None).await
}

pub async fn resume_session(
    spec: &AgentSpec,
    acp_session_id: &str,
    ctx: SessionContext,
    store: Arc<dyn StoreSink>,
) -> Result<(SessionHandle, AgentSessionHandle)> {
    if !spec.supports_load {
        return Err(SessionError::LoadUnsupported);
    }
    acp::launch(spec, ctx, store, Some(acp_session_id.to_owned())).await
}
