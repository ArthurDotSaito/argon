use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::v1::schema::{RequestPermissionRequest, SessionNotification};
use agent_client_protocol::{Client, ConnectTo, Lines};
use argon_store::{NewAgentSession, SessionStatus};
use futures::StreamExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use super::driver::{Shared, SharedConfig};
use super::failure::{STDERR_LINES, classify, with_stderr};
use super::setup::{Connection, Ready, run_connection};
use super::stderr::StderrTail;
use super::writer::{Op, SessionWriter};
use super::{
    AgentSessionHandle, Result, SessionContext, SessionError, SessionHandle, SessionOptions,
};
use crate::process::{KillOnDropChild, TreeKiller, spawn_in_job};
use crate::registry::AgentSpec;
use crate::util::{Resolved, resolve_command};
use crate::{AgentEvent, Transport};

const EXIT_GRACE: Duration = Duration::from_secs(2);
const EVENT_CAPACITY: usize = 4096;

fn spawn(spec: &AgentSpec, cwd: &Path) -> Result<KillOnDropChild> {
    let program = match resolve_command(&spec.command) {
        Resolved::Path(path) => path,
        Resolved::Missing => {
            return Err(SessionError::CommandNotFound(
                spec.command.display().to_string(),
            ));
        }
        Resolved::Failed(message) => return Err(SessionError::Acp(message)),
    };
    let mut command = tokio::process::Command::new(program);
    command
        .args(&spec.args)
        .envs(&spec.env)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(spawn_in_job(&mut command)?)
}

fn transport(stdin: ChildStdin, stdout: ChildStdout) -> impl ConnectTo<Client> {
    let outgoing = futures::sink::unfold(stdin, |mut stdin, line: String| async move {
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok::<_, std::io::Error>(stdin)
    });
    let incoming =
        futures::stream::unfold(Some(BufReader::new(stdout).lines()), |lines| async move {
            let mut lines = lines?;
            match lines.next_line().await {
                Ok(Some(line)) => Some((Ok(line), Some(lines))),
                Ok(None) => None,
                Err(error) => Some((Err(error), None)),
            }
        });
    Lines::new(Box::pin(outgoing), incoming.boxed())
}

fn drain_stderr(stderr: ChildStderr, tail: Arc<StderrTail>) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(target: "argon_agents::agent_stderr", "{line}");
            tail.push(line);
        }
    });
}

async fn terminate(mut child: KillOnDropChild) {
    let _ = tokio::time::timeout(EXIT_GRACE, child.wait()).await;
    if let Err(error) = child.kill_tree() {
        tracing::debug!(%error, "agent process tree already gone");
    }
    let _ = child.wait().await;
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

struct Aborter {
    killer: TreeKiller,
    abort: Option<oneshot::Sender<()>>,
}

impl Aborter {
    fn abort(&mut self) {
        if let Err(error) = self.killer.kill() {
            tracing::debug!(%error, "failed to terminate agent process tree");
        }
        if let Some(abort) = self.abort.take() {
            let _ = abort.send(());
        }
    }
}

fn startup_error(tail: &StderrTail, spec: &AgentSpec) -> SessionError {
    let stderr = tail.last(STDERR_LINES).join("\n");
    if stderr.is_empty() {
        SessionError::Acp("o agente encerrou antes de abrir a sessão".to_owned())
    } else {
        classify(spec, stderr)
    }
}

pub(super) async fn launch(
    spec: &AgentSpec,
    ctx: SessionContext,
    writer: SessionWriter,
    load: Option<String>,
    options: SessionOptions,
) -> Result<(SessionHandle, AgentSessionHandle)> {
    if spec.transport != Transport::Acp {
        return Err(SessionError::NotAcp);
    }
    let mut child = spawn(spec, &ctx.cwd)?;
    let stdin = child.stdin.take().ok_or(SessionError::Closed)?;
    let stdout = child.stdout.take().ok_or(SessionError::Closed)?;
    let tail = Arc::new(StderrTail::default());
    if let Some(stderr) = child.stderr.take() {
        drain_stderr(stderr, Arc::clone(&tail));
    }
    let (abort_tx, abort_rx) = oneshot::channel::<()>();
    let mut aborter = Aborter {
        killer: child.killer(),
        abort: Some(abort_tx),
    };

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
        events,
    ));
    let (commands, receiver) = mpsc::unbounded_channel();
    let (done_tx, done) = watch::channel(false);
    let (ready_tx, ready_rx) = oneshot::channel::<Result<Ready>>();

    let connection = connect(
        transport(stdin, stdout),
        Connection {
            shared: Arc::clone(&shared),
            spec: spec.clone(),
            ctx: ctx.clone(),
            load: load.clone(),
            options,
            commands: receiver,
            ready: ready_tx,
        },
    );
    let supervisor = Arc::clone(&shared);
    let supervisor_writer = writer.clone();
    tokio::spawn(async move {
        let outcome = tokio::select! {
            outcome = connection => Some(outcome),
            Ok(()) = abort_rx => None,
        };
        match outcome {
            Some(Ok(true)) if supervisor.in_turn() => {
                supervisor.fail("a sessão foi encerrada durante o turno".to_owned());
            }
            Some(Err(error)) => {
                let message = format!("o agente encerrou inesperadamente: {error}");
                supervisor.fail(with_stderr(message, &supervisor));
            }
            _ => supervisor.close(),
        }
        terminate(child).await;
        supervisor_writer.push(Op::Unregister {
            session: supervisor.id().to_owned(),
        });
        let _ = done_tx.send(true);
    });

    let ready = match tokio::time::timeout(options.startup_timeout, ready_rx).await {
        Ok(Ok(Ok(ready))) => ready,
        Ok(Ok(Err(error))) => return Err(error),
        Ok(Err(_)) => return Err(startup_error(&tail, spec)),
        Err(_) => {
            aborter.abort();
            return Err(SessionError::StartupTimeout(options.startup_timeout));
        }
    };

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
                acp_session_id: Some(ready.acp_session_id.clone()),
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
            aborter.abort();
            return Err(SessionError::Store(error));
        }
    };
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
    if !ready.models.is_empty() {
        shared.emit(AgentEvent::ModelsAvailable(ready.models));
    }
    for warning in ready.warnings {
        shared.emit(AgentEvent::Error(warning));
    }
    Ok((
        SessionHandle {
            id: ctx.id,
            acp_session_id: ready.acp_session_id,
        },
        AgentSessionHandle {
            shared,
            commands,
            done,
        },
    ))
}

async fn connect(
    transport: impl ConnectTo<Client> + 'static,
    connection: Connection,
) -> std::result::Result<bool, String> {
    let on_update = Arc::clone(&connection.shared);
    let on_permission = Arc::clone(&connection.shared);
    Client
        .v1()
        .name("argon")
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                let update = serde_json::to_value(&notification.update).unwrap_or_default();
                on_update.on_update(&update);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _cx| {
                on_permission.on_permission(&request, responder);
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, move |cx| run_connection(cx, connection))
        .await
        .map_err(|error| error.to_string())
}
