use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::v1::schema::{RequestPermissionRequest, SessionNotification};
use agent_client_protocol::{Client, Lines};
use argon_store::NewAgentSession;
use futures::StreamExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use super::driver::Shared;
use super::setup::{Ready, run_connection};
use super::sink::StoreSink;
use super::{AgentSessionHandle, Result, SessionContext, SessionError, SessionHandle};
use crate::process::{KillOnDropChild, spawn_in_job};
use crate::registry::AgentSpec;
use crate::{AgentEvent, Transport};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(180);
const EXIT_GRACE: Duration = Duration::from_secs(2);
const EVENT_CAPACITY: usize = 4096;
const STDERR_TAIL_LINES: usize = 40;

type StderrTail = Arc<Mutex<VecDeque<String>>>;

fn resolve(command: &Path) -> Result<PathBuf> {
    if command.components().count() > 1 || command.is_absolute() {
        return Ok(command.to_path_buf());
    }
    which::which(command).map_err(|_| SessionError::CommandNotFound(command.display().to_string()))
}

fn spawn(spec: &AgentSpec, cwd: &Path) -> Result<KillOnDropChild> {
    let program = resolve(&spec.command)?;
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

fn transport(
    stdin: ChildStdin,
    stdout: ChildStdout,
) -> impl agent_client_protocol::ConnectTo<Client> {
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

fn drain_stderr(stderr: ChildStderr, tail: StderrTail) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(target: "argon_agents::agent_stderr", "{line}");
            let mut tail = match tail.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if tail.len() == STDERR_TAIL_LINES {
                tail.pop_front();
            }
            tail.push_back(line);
        }
    });
}

fn stderr_summary(tail: &StderrTail) -> String {
    let tail = match tail.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    tail.iter()
        .rev()
        .take(5)
        .rev()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

async fn terminate(mut child: KillOnDropChild) {
    let _ = tokio::time::timeout(EXIT_GRACE, child.wait()).await;
    if let Err(error) = child.kill_tree() {
        tracing::debug!(%error, "agent process tree already gone");
    }
    let _ = child.wait().await;
}

pub(super) async fn launch(
    spec: &AgentSpec,
    ctx: SessionContext,
    store: Arc<dyn StoreSink>,
    load: Option<String>,
) -> Result<(SessionHandle, AgentSessionHandle)> {
    if spec.transport != Transport::Acp {
        return Err(SessionError::NotAcp);
    }
    let mut child = spawn(spec, &ctx.cwd)?;
    let stdin = child.stdin.take().ok_or(SessionError::Closed)?;
    let stdout = child.stdout.take().ok_or(SessionError::Closed)?;
    let tail: StderrTail = Arc::default();
    if let Some(stderr) = child.stderr.take() {
        drain_stderr(stderr, Arc::clone(&tail));
    }

    let (events, _) = broadcast::channel::<AgentEvent>(EVENT_CAPACITY);
    let (commands, receiver) = mpsc::unbounded_channel();
    let (done_tx, done) = watch::channel(false);
    let (ready_tx, ready_rx) = oneshot::channel::<Result<Ready>>();
    let shared = Arc::new(Shared::new(
        ctx.id.clone(),
        ctx.cwd.clone(),
        ctx.nonce.clone(),
        ctx.allowed_files.clone(),
        store,
        events.clone(),
    ));

    let connection = connect(
        transport(stdin, stdout),
        Arc::clone(&shared),
        spec.clone(),
        ctx.clone(),
        load.clone(),
        receiver,
        ready_tx,
    );
    let supervisor_shared = Arc::clone(&shared);
    let supervisor_tail = Arc::clone(&tail);
    tokio::spawn(async move {
        let outcome = connection.await;
        let stderr = stderr_summary(&supervisor_tail);
        match outcome {
            Ok(true) => {
                if supervisor_shared.in_turn() {
                    supervisor_shared.fail("a sessão foi encerrada durante o turno".to_owned());
                }
            }
            Ok(false) => {}
            Err(error) => {
                let detail = if stderr.is_empty() {
                    error
                } else {
                    format!("{error}\n{stderr}")
                };
                supervisor_shared.fail(format!("o agente encerrou inesperadamente: {detail}"));
            }
        }
        terminate(child).await;
        let _ = done_tx.send(true);
    });

    let ready = match tokio::time::timeout(STARTUP_TIMEOUT, ready_rx).await {
        Ok(Ok(Ok(ready))) => ready,
        Ok(Ok(Err(error))) => return Err(error),
        Ok(Err(_)) => {
            let stderr = stderr_summary(&tail);
            return Err(SessionError::Acp(if stderr.is_empty() {
                "o agente encerrou antes de abrir a sessão".to_owned()
            } else {
                stderr
            }));
        }
        Err(_) => {
            let _ = commands.send(super::Command::Shutdown);
            return Err(SessionError::StartupTimeout(STARTUP_TIMEOUT));
        }
    };

    let handle = AgentSessionHandle {
        commands,
        events,
        done,
    };
    if load.is_none() {
        shared.store().create_session(&NewAgentSession {
            id: ctx.id.clone(),
            pr: ctx.pr.clone(),
            agent: spec.kind,
            model: ready.model.clone(),
            template: ctx.template_id.clone(),
            worktree_path: Some(ctx.cwd.clone()),
            acp_session_id: Some(ready.acp_session_id.clone()),
            nonce: ctx.nonce.clone(),
            allowed_files: ctx.allowed_files.clone(),
        });
    } else if !ready.model.is_empty() {
        shared.store().set_model(&ctx.id, &ready.model);
    }
    shared.activate(argon_store::SessionStatus::Completed);
    if !ready.models.is_empty() {
        shared.emit(AgentEvent::ModelsAvailable(ready.models.clone()));
    }
    for warning in ready.warnings {
        shared.emit(AgentEvent::Error(warning));
    }
    Ok((
        SessionHandle {
            id: ctx.id,
            acp_session_id: ready.acp_session_id,
        },
        handle,
    ))
}

async fn connect(
    transport: impl agent_client_protocol::ConnectTo<Client> + 'static,
    shared: Arc<Shared>,
    spec: AgentSpec,
    ctx: SessionContext,
    load: Option<String>,
    commands: mpsc::UnboundedReceiver<super::Command>,
    ready: oneshot::Sender<Result<Ready>>,
) -> std::result::Result<bool, String> {
    let on_update = Arc::clone(&shared);
    let on_permission = Arc::clone(&shared);
    let foreground = Arc::clone(&shared);
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
        .connect_with(transport, move |cx| {
            run_connection(cx, foreground, spec, ctx, load, commands, ready)
        })
        .await
        .map_err(|error| error.to_string())
}
