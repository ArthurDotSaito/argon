use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::v1::schema::{RequestPermissionRequest, SessionNotification};
use agent_client_protocol::{Client, ConnectTo, Lines};
use futures::StreamExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};

use super::acp_permissions::handle_request;
use super::bootstrap::{DriverFuture, DriverRun};
use super::events::map_update;
use super::setup::run_connection;
use super::stderr::StderrTail;
use super::{Result, SessionError};
use crate::process::{KillOnDropChild, spawn_in_job};
use crate::registry::AgentSpec;
use crate::util::{Resolved, resolve_command};

const EXIT_GRACE: Duration = Duration::from_secs(2);

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

fn start(
    spec: &AgentSpec,
    cwd: &Path,
    tail: Arc<StderrTail>,
) -> Result<(KillOnDropChild, ChildStdin, ChildStdout)> {
    let mut child = spawn(spec, cwd)?;
    let stdin = child.stdin.take().ok_or(SessionError::Closed)?;
    let stdout = child.stdout.take().ok_or(SessionError::Closed)?;
    if let Some(stderr) = child.stderr.take() {
        drain_stderr(stderr, tail);
    }
    Ok((child, stdin, stdout))
}

pub(super) fn run(run: DriverRun) -> DriverFuture {
    Box::pin(async move {
        let (child, stdin, stdout) = match start(&run.spec, &run.ctx.cwd, Arc::clone(&run.stderr)) {
            Ok(started) => started,
            Err(error) => {
                let _ = run.ready.send(Err(error));
                return Ok(false);
            }
        };
        let outcome = connect(transport(stdin, stdout), run).await;
        terminate(child).await;
        outcome
    })
}

async fn connect(
    transport: impl ConnectTo<Client> + 'static,
    run: DriverRun,
) -> std::result::Result<bool, String> {
    let on_update = Arc::clone(&run.shared);
    let on_permission = Arc::clone(&run.shared);
    Client
        .v1()
        .name("argon")
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                let update = serde_json::to_value(&notification.update).unwrap_or_default();
                on_update.emit_all(map_update(&update));
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _cx| {
                handle_request(&on_permission, &request, responder);
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, move |cx| run_connection(cx, run))
        .await
        .map_err(|error| error.to_string())
}
