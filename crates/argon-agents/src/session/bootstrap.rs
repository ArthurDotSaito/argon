use std::sync::Arc;

use argon_core::PermissionMode;
use argon_store::{NewAgentSession, SessionStatus};
use futures::future::BoxFuture;
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use super::driver::{Shared, SharedConfig};
use super::failure::{STDERR_LINES, classify, transport_error, with_stderr};
use super::stderr::StderrTail;
use super::writer::{Op, SessionWriter};
use super::{
    AgentSessionHandle, Command, Result, SessionContext, SessionError, SessionHandle,
    SessionOptions,
};
use crate::AgentEvent;
use crate::registry::{AgentSpec, ModelInfo};

const EVENT_CAPACITY: usize = 4096;

pub(crate) struct Ready {
    pub agent_session_id: String,
    pub model: String,
    pub models: Vec<ModelInfo>,
    pub warnings: Vec<String>,
    pub permission_mode: Option<PermissionMode>,
}

pub(crate) struct DriverRun {
    pub shared: Arc<Shared>,
    pub spec: AgentSpec,
    pub ctx: SessionContext,
    pub load: Option<String>,
    pub options: SessionOptions,
    pub commands: mpsc::UnboundedReceiver<Command>,
    pub ready: oneshot::Sender<Result<Ready>>,
    pub stderr: Arc<StderrTail>,
}

pub(crate) type DriverFuture = BoxFuture<'static, std::result::Result<bool, String>>;

struct AbortOnDrop(Option<oneshot::Sender<()>>);

impl AbortOnDrop {
    fn disarm(mut self) {
        self.0.take();
    }
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(abort) = self.0.take() {
            let _ = abort.send(());
        }
    }
}

fn forward_writer_errors(writer: &SessionWriter, shared: &Arc<Shared>) {
    let (errors, mut received) = mpsc::unbounded_channel();
    writer.push(Op::Register {
        session: shared.id().to_owned(),
        errors,
    });
    let shared = Arc::clone(shared);
    tokio::spawn(async move {
        while let Some(message) = received.recv().await {
            shared.emit_transient(AgentEvent::Error(message));
        }
    });
}

fn startup_error(tail: &StderrTail, spec: &AgentSpec) -> SessionError {
    let stderr = tail.last(STDERR_LINES).join("\n");
    if stderr.is_empty() {
        transport_error(spec, "o agente encerrou antes de abrir a sessão".to_owned())
    } else {
        classify(spec, stderr)
    }
}

fn supervise(
    driver: DriverFuture,
    shared: Arc<Shared>,
    writer: SessionWriter,
    abort: oneshot::Receiver<()>,
    done: watch::Sender<bool>,
) {
    tokio::spawn(async move {
        let outcome = tokio::select! {
            outcome = driver => Some(outcome),
            Ok(()) = abort => None,
        };
        match outcome {
            Some(Ok(true)) if shared.in_turn() => {
                shared.fail("a sessão foi encerrada durante o turno".to_owned());
            }
            Some(Err(error)) => {
                let message = format!("o agente encerrou inesperadamente: {error}");
                shared.fail(with_stderr(message, &shared));
            }
            _ => shared.close(),
        }
        writer.push(Op::Unregister {
            session: shared.id().to_owned(),
        });
        let _ = done.send(true);
    });
}

pub(super) async fn launch(
    spec: &AgentSpec,
    ctx: SessionContext,
    writer: SessionWriter,
    load: Option<String>,
    options: SessionOptions,
    run: fn(DriverRun) -> DriverFuture,
) -> Result<(SessionHandle, AgentSessionHandle)> {
    let tail = Arc::new(StderrTail::default());
    let (events, _) = broadcast::channel(EVENT_CAPACITY);
    let shared = Arc::new(Shared::new(
        SharedConfig {
            id: ctx.id.clone(),
            cwd: ctx.cwd.clone(),
            nonce: ctx.nonce.clone(),
            allowed_files: ctx.allowed_files.clone(),
            writer: writer.clone(),
            stderr: Arc::clone(&tail),
        },
        ctx.permission_mode,
        events,
    ));
    let (commands, receiver) = mpsc::unbounded_channel();
    let (done_tx, done) = watch::channel(false);
    let (ready_tx, ready_rx) = oneshot::channel::<Result<Ready>>();
    let (abort_tx, abort_rx) = oneshot::channel::<()>();
    let aborter = AbortOnDrop(Some(abort_tx));

    let driver = run(DriverRun {
        shared: Arc::clone(&shared),
        spec: spec.clone(),
        ctx: ctx.clone(),
        load: load.clone(),
        options,
        commands: receiver,
        ready: ready_tx,
        stderr: Arc::clone(&tail),
    });
    supervise(
        driver,
        Arc::clone(&shared),
        writer.clone(),
        abort_rx,
        done_tx,
    );

    let ready = match tokio::time::timeout(options.startup_timeout, ready_rx).await {
        Ok(Ok(Ok(ready))) => ready,
        Ok(Ok(Err(error))) => return Err(error),
        Ok(Err(_)) => return Err(startup_error(&tail, spec)),
        Err(_) => return Err(SessionError::StartupTimeout(options.startup_timeout)),
    };
    let permission_mode = ready.permission_mode.unwrap_or(ctx.permission_mode);
    shared.start_in_mode(permission_mode);

    let last_seq = match &load {
        Some(_) => writer.last_seq(&ctx.id).await,
        None => writer
            .create_session(NewAgentSession {
                id: ctx.id.clone(),
                pr: ctx.pr.clone(),
                agent: spec.kind,
                model: ready.model.clone(),
                template: ctx.template_id.clone(),
                worktree_path: Some(ctx.cwd.clone()),
                acp_session_id: Some(ready.agent_session_id.clone()),
                transport: spec.transport.as_str().to_owned(),
                permission_mode: permission_mode.as_str().to_owned(),
                nonce: ctx.nonce.clone(),
                allowed_files: ctx.allowed_files.clone(),
            })
            .await
            .map(|()| 0),
    };
    let last_seq = match last_seq {
        Ok(seq) => seq,
        Err(error) => {
            tracing::error!(%error, "failed to persist agent session");
            return Err(SessionError::Store(error));
        }
    };
    aborter.disarm();
    forward_writer_errors(&writer, &shared);
    let status = if load.is_some() {
        SessionStatus::Completed
    } else {
        SessionStatus::Running
    };
    shared.activate(status, last_seq, ready.model.clone(), ready.models.clone());
    if load.is_some() && !ready.model.is_empty() {
        shared.set_model(&ready.model);
    }
    if load.is_some() && permission_mode != ctx.permission_mode {
        shared.set_permission_mode(permission_mode);
    }
    if !ready.models.is_empty() {
        shared.emit(AgentEvent::ModelsAvailable(ready.models));
    }
    for warning in ready.warnings {
        shared.emit(AgentEvent::Error(warning));
    }
    Ok((
        SessionHandle {
            id: ctx.id,
            agent_session_id: ready.agent_session_id,
        },
        AgentSessionHandle {
            shared,
            commands,
            done,
        },
    ))
}

#[cfg(test)]
mod tests {
    use argon_core::{AgentKind, Config};

    use super::*;
    use crate::Transport;
    use crate::registry::agent_spec;

    #[test]
    fn startup_error_names_acp_only_for_the_acp_transport() {
        let tail = StderrTail::default();
        let mut spec = agent_spec(AgentKind::ClaudeCode, &Config::default());
        spec.transport = Transport::Acp;
        assert!(matches!(startup_error(&tail, &spec), SessionError::Acp(_)));
        spec.transport = Transport::ClaudeStream;
        assert!(matches!(
            startup_error(&tail, &spec),
            SessionError::Agent(text) if text == "o agente encerrou antes de abrir a sessão"
        ));
        tail.push("boom".to_owned());
        assert!(matches!(
            startup_error(&tail, &spec),
            SessionError::Agent(text) if text == "boom"
        ));
    }
}
