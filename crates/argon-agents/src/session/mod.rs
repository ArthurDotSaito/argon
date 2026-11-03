mod acp;
mod driver;
mod events;
mod failure;
mod permissions;
mod record;
mod setup;
mod stderr;
mod turn;
mod writer;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use argon_core::PrKey;
use futures::Stream;
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::registry::AgentSpec;

pub use driver::Snapshot;
pub use events::{AgentEvent, PendingPermission, PermissionOption, SavedFinding, ToolStatus};
pub use permissions::{PermissionDecision, PermissionPrompt, decide as decide_permission};
pub use turn::{TurnOutcome, finish_turn};
pub use writer::SessionWriter;

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
    #[error("o agente precisa de login: {hint}")]
    NeedsLogin { hint: String },
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
    #[error("já existe um turno ou troca de modelo em andamento")]
    Busy,
    #[error("pedido de permissão `{0}` não está pendente")]
    UnknownPermission(String),
    #[error("a opção `{option_id}` não foi oferecida no pedido `{request_id}`")]
    InvalidPermissionOption {
        request_id: String,
        option_id: String,
    },
    #[error("falha ao gravar a sessão: {0}")]
    Store(String),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionOptions {
    pub startup_timeout: Duration,
    pub turn_timeout: Duration,
    pub set_model_timeout: Duration,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            startup_timeout: Duration::from_secs(180),
            turn_timeout: Duration::from_secs(30 * 60),
            set_model_timeout: Duration::from_secs(15),
        }
    }
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
    SetModel {
        model: String,
        reply: oneshot::Sender<Result<()>>,
    },
    Shutdown,
}

#[derive(Clone)]
pub struct AgentSessionHandle {
    shared: Arc<driver::Shared>,
    commands: mpsc::UnboundedSender<Command>,
    done: watch::Receiver<bool>,
}

fn event_stream(
    receiver: broadcast::Receiver<(u64, AgentEvent)>,
    last_seq: u64,
) -> impl Stream<Item = (u64, AgentEvent)> + Send + 'static {
    futures::stream::unfold((receiver, last_seq), |(mut receiver, last)| async move {
        match receiver.recv().await {
            Ok((seq, event)) => Some(((seq, event), (receiver, seq))),
            Err(broadcast::error::RecvError::Lagged(_)) => Some((
                (last, AgentEvent::Resync { last_seq: last }),
                (receiver, last),
            )),
            Err(broadcast::error::RecvError::Closed) => None,
        }
    })
}

impl AgentSessionHandle {
    fn send(&self, command: Command) -> Result<()> {
        self.commands
            .send(command)
            .map_err(|_| SessionError::Closed)
    }

    pub fn prompt(&self, text: impl Into<String>) -> Result<()> {
        if !self.shared.try_acquire() {
            return Err(SessionError::Busy);
        }
        self.send(Command::Prompt(text.into()))
            .inspect_err(|_| self.shared.release())
    }

    pub fn cancel(&self) -> Result<()> {
        self.send(Command::Cancel)
    }

    pub fn respond_permission(&self, request_id: &str, option_id: &str) -> Result<()> {
        self.shared.respond(request_id, option_id)
    }

    pub async fn set_model(&self, model: &str) -> Result<()> {
        if !self.shared.try_acquire() {
            return Err(SessionError::Busy);
        }
        let (reply, response) = oneshot::channel();
        if let Err(error) = self.send(Command::SetModel {
            model: model.to_owned(),
            reply,
        }) {
            self.shared.release();
            return Err(error);
        }
        response.await.map_err(|_| SessionError::Closed)?
    }

    pub fn subscribe(
        &self,
    ) -> (
        Snapshot,
        impl Stream<Item = (u64, AgentEvent)> + Send + 'static,
    ) {
        let (snapshot, receiver) = self.shared.subscribe();
        let last = snapshot.last_seq;
        (snapshot, event_stream(receiver, last))
    }

    pub async fn events_since(&self, seq: u64) -> Result<Vec<(u64, AgentEvent)>> {
        self.shared
            .writer()
            .events_since(self.shared.id(), seq)
            .await
            .map_err(SessionError::Store)
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
    writer: SessionWriter,
) -> Result<(SessionHandle, AgentSessionHandle)> {
    start_session_with(spec, ctx, writer, SessionOptions::default()).await
}

pub async fn start_session_with(
    spec: &AgentSpec,
    ctx: SessionContext,
    writer: SessionWriter,
    options: SessionOptions,
) -> Result<(SessionHandle, AgentSessionHandle)> {
    acp::launch(spec, ctx, writer, None, options).await
}

pub async fn resume_session(
    spec: &AgentSpec,
    acp_session_id: &str,
    ctx: SessionContext,
    writer: SessionWriter,
) -> Result<(SessionHandle, AgentSessionHandle)> {
    if !spec.supports_load {
        return Err(SessionError::LoadUnsupported);
    }
    acp::launch(
        spec,
        ctx,
        writer,
        Some(acp_session_id.to_owned()),
        SessionOptions::default(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;

    use super::*;

    #[tokio::test]
    async fn lagging_subscriber_gets_resync_with_last_seen_seq() {
        let (sender, receiver) = broadcast::channel(2);
        let mut stream = Box::pin(event_stream(receiver, 4));
        sender.send((5, AgentEvent::Error("a".into()))).unwrap();
        assert_eq!(stream.next().await.unwrap().0, 5);
        for seq in 6..=9 {
            sender.send((seq, AgentEvent::Error("b".into()))).unwrap();
        }
        assert_eq!(
            stream.next().await.unwrap(),
            (5, AgentEvent::Resync { last_seq: 5 })
        );
        assert_eq!(stream.next().await.unwrap().0, 8);
        drop(sender);
        assert_eq!(stream.next().await.unwrap().0, 9);
        assert!(stream.next().await.is_none());
    }
}
