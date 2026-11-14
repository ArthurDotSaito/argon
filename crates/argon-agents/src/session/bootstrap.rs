use std::sync::Arc;

use argon_core::{PermissionMode, SessionKind};
use argon_store::{NewAgentSession, SessionStatus};
use futures::future::BoxFuture;
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use super::driver::{ENDED_BEFORE_ACTIVATION, Shared, SharedConfig};
use super::failure::{STDERR_LINES, classify, transport_error, with_stderr};
use super::stderr::StderrTail;
use super::writer::{Op, SessionWriter};
use super::{
    AgentSessionHandle, Command, Result, SessionContext, SessionError, SessionHandle,
    SessionOptions,
};
use crate::AgentEvent;
use crate::Transport;
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
        transport_error(spec, ENDED_BEFORE_ACTIVATION.to_owned())
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

async fn restore_plan_state(writer: &SessionWriter, shared: &Shared, ctx: &SessionContext) {
    if ctx.kind != SessionKind::Plan {
        return;
    }
    match writer.has_plan(&ctx.id).await {
        Ok(true) => shared.mark_plan_recorded(),
        Ok(false) => {}
        Err(error) => tracing::warn!(session = %ctx.id, %error, "failed to read plan drafts"),
    }
}

fn pinned_mode(ctx: SessionContext) -> SessionContext {
    match ctx.kind {
        SessionKind::Plan => SessionContext {
            permission_mode: PermissionMode::Supervised,
            ..ctx
        },
        SessionKind::Review => ctx,
    }
}

pub(super) async fn launch(
    spec: &AgentSpec,
    ctx: SessionContext,
    writer: SessionWriter,
    load: Option<String>,
    options: SessionOptions,
    run: fn(DriverRun) -> DriverFuture,
) -> Result<(SessionHandle, AgentSessionHandle)> {
    let ctx = pinned_mode(ctx);
    let tail = Arc::new(StderrTail::default());
    let (events, _) = broadcast::channel(EVENT_CAPACITY);
    let shared = Arc::new(Shared::new(
        SharedConfig {
            id: ctx.id.clone(),
            subject: ctx.pr.clone(),
            kind: ctx.kind,
            base_sha: ctx.base_sha.clone(),
            denial_ends_turn: spec.transport == Transport::OpencodeServe,
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
    forward_writer_errors(&writer, &shared);
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
                kind: ctx.kind,
                base_sha: ctx.base_sha.clone(),
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
    restore_plan_state(&writer, &shared, &ctx).await;
    aborter.disarm();
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
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use argon_core::{AgentKind, Config, PrKey};
    use argon_store::Store;

    use super::*;
    use crate::Transport;
    use crate::registry::agent_spec;

    const DRIVER_ERROR: &str = "boom";
    const NONCE: &str = "00112233445566778899aabbccddeeff";

    fn ready() -> Ready {
        Ready {
            agent_session_id: "agent".to_owned(),
            model: "sonnet".to_owned(),
            models: Vec::new(),
            warnings: Vec::new(),
            permission_mode: None,
        }
    }

    fn dies_after_ready(run: DriverRun) -> DriverFuture {
        Box::pin(async move {
            let _ = run.ready.send(Ok(ready()));
            Err(DRIVER_ERROR.to_owned())
        })
    }

    fn replies_with_the_prompt(mut run: DriverRun) -> DriverFuture {
        Box::pin(async move {
            let _ = run.ready.send(Ok(ready()));
            while let Some(command) = run.commands.recv().await {
                match command {
                    Command::Prompt(reply) => {
                        run.shared.begin_turn("planeje");
                        run.shared.emit(AgentEvent::MessageChunk(reply));
                        run.shared.end_turn("end_turn").await;
                    }
                    Command::Shutdown => break,
                    _ => {}
                }
            }
            Ok(false)
        })
    }

    fn temp_store(name: &str) -> (PathBuf, SessionWriter) {
        let dir =
            std::env::temp_dir().join(format!("argon-bootstrap-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let writer =
            SessionWriter::from_store(Store::open(&dir.join("argon.db")).unwrap()).unwrap();
        (dir, writer)
    }

    fn stream_spec() -> AgentSpec {
        let mut spec = agent_spec(AgentKind::ClaudeCode, &Config::default());
        spec.transport = Transport::ClaudeStream;
        spec
    }

    async fn reply_turn(
        agent: &AgentSessionHandle,
        writer: &SessionWriter,
        reply: &str,
    ) -> Vec<AgentEvent> {
        agent.prompt(reply).unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            while !agent.shared.try_acquire() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        agent.shared.release();
        let events: Vec<AgentEvent> = writer
            .events_since("s1", 0)
            .await
            .unwrap()
            .into_iter()
            .map(|(_, event)| event)
            .collect();
        let start = events
            .iter()
            .rposition(|event| matches!(event, AgentEvent::UserMessage(_)))
            .unwrap();
        events[start..].to_vec()
    }

    fn context(id: &str, cwd: PathBuf) -> SessionContext {
        SessionContext {
            id: id.to_owned(),
            pr: PrKey::new("github.com", "PR_1"),
            template_id: "review-padrao".to_owned(),
            nonce: NONCE.to_owned(),
            allowed_files: Vec::new(),
            cwd,
            model: String::new(),
            permission_mode: PermissionMode::Supervised,
            kind: SessionKind::Review,
            base_sha: None,
        }
    }

    fn plan_context(id: &str, cwd: PathBuf) -> SessionContext {
        SessionContext {
            pr: PrKey::new("github.com", "I_7"),
            template_id: "plan-padrao".to_owned(),
            kind: SessionKind::Plan,
            base_sha: Some("0123456789abcdef0123456789abcdef01234567".to_owned()),
            ..context(id, cwd)
        }
    }

    async fn launch_dying(
        name: &str,
        build: fn(&str, PathBuf) -> SessionContext,
    ) -> (PathBuf, SessionWriter, AgentSessionHandle) {
        let (dir, writer) = temp_store(name);
        let (_, agent) = launch(
            &stream_spec(),
            build("s1", dir.clone()),
            writer.clone(),
            None,
            SessionOptions::default(),
            dies_after_ready,
        )
        .await
        .unwrap();
        writer.flush().await;
        (dir, writer, agent)
    }

    #[tokio::test]
    async fn early_failure_releases_the_writer_error_forwarder() {
        let (dir, _writer, agent) = launch_dying("forwarder", context).await;

        for _ in 0..500 {
            if Arc::strong_count(&agent.shared) == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(Arc::strong_count(&agent.shared), 1);
        drop(agent);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn driver_dying_before_activation_fails_the_session() {
        let (dir, writer, agent) = launch_dying("activate", context).await;
        let db = dir.join("argon.db");

        assert!(agent.is_finished());
        assert_eq!(agent.status(), SessionStatus::Failed);
        let row = Store::open(&db).unwrap().session("s1").unwrap().unwrap();
        assert_eq!(row.status, SessionStatus::Failed);
        assert_eq!(row.kind, SessionKind::Review);
        assert_eq!(row.base_sha, None);
        let events = writer.events_since("s1", 0).await.unwrap();
        assert!(
            matches!(
                events.as_slice(),
                [(_, AgentEvent::Error(message))]
                    if message == &format!("o agente encerrou inesperadamente: {DRIVER_ERROR}")
            ),
            "{events:?}"
        );
        drop(writer);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn new_plan_session_persists_its_kind_and_base_sha() {
        let (dir, writer, agent) = launch_dying("plan-kind", plan_context).await;
        let expected = plan_context("s1", dir.clone());

        let row = Store::open(&dir.join("argon.db"))
            .unwrap()
            .session("s1")
            .unwrap()
            .unwrap();

        assert_eq!(row.kind, SessionKind::Plan);
        assert_eq!(row.base_sha, expected.base_sha);
        assert_eq!(row.pr, expected.pr);
        drop((writer, agent));
        let _ = std::fs::remove_dir_all(&dir);
    }

    async fn launch_replying(
        dir: &Path,
        writer: &SessionWriter,
        load: Option<String>,
    ) -> AgentSessionHandle {
        let (_, agent) = launch(
            &stream_spec(),
            plan_context("s1", dir.to_path_buf()),
            writer.clone(),
            load,
            SessionOptions::default(),
            replies_with_the_prompt,
        )
        .await
        .unwrap();
        agent
    }

    #[tokio::test]
    async fn plan_session_warns_only_until_it_has_a_draft_even_after_resume() {
        let (dir, writer) = temp_store("plan-follow-up");
        let missing =
            AgentEvent::Error("O agente não devolveu um plano no formato esperado.".into());
        let plan = format!("```argon-plan-{NONCE}\n## Plano\n```");

        let agent = launch_replying(&dir, &writer, None).await;
        let first = reply_turn(&agent, &writer, "Ainda lendo.").await;
        let second = reply_turn(&agent, &writer, &plan).await;
        let third = reply_turn(&agent, &writer, "Respondido sem plano novo.").await;
        agent.shutdown().await;

        assert_eq!(first.last(), Some(&missing));
        assert!(
            matches!(second.last(), Some(AgentEvent::PlanDraft { markdown, .. }) if markdown == "## Plano")
        );
        assert!(
            matches!(third.last(), Some(AgentEvent::TurnEnded { .. })),
            "{third:?}"
        );

        let resumed = launch_replying(&dir, &writer, Some("agent".into())).await;
        let after_resume = reply_turn(&resumed, &writer, "Outra pergunta respondida.").await;
        resumed.shutdown().await;

        assert!(
            matches!(after_resume.last(), Some(AgentEvent::TurnEnded { .. })),
            "{after_resume:?}"
        );
        drop(writer);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn plan_sessions_run_supervised_whatever_mode_was_configured() {
        for mode in [PermissionMode::AcceptEdits, PermissionMode::FullAccess] {
            let (dir, writer) = temp_store(&format!("plan-mode-{}", mode.as_str()));
            let configured = SessionContext {
                permission_mode: mode,
                ..plan_context("s1", dir.clone())
            };
            let (_, started) = launch(
                &stream_spec(),
                configured.clone(),
                writer.clone(),
                None,
                SessionOptions::default(),
                replies_with_the_prompt,
            )
            .await
            .unwrap();
            writer.flush().await;
            let row = Store::open(&dir.join("argon.db"))
                .unwrap()
                .session("s1")
                .unwrap()
                .unwrap();
            let started_mode = started.shared.subscribe().0.permission_mode;
            started.shutdown().await;

            let (_, resumed) = launch(
                &stream_spec(),
                configured,
                writer.clone(),
                Some("agent".into()),
                SessionOptions::default(),
                replies_with_the_prompt,
            )
            .await
            .unwrap();
            let resumed_mode = resumed.shared.subscribe().0.permission_mode;
            resumed.shutdown().await;

            assert_eq!(started_mode, PermissionMode::Supervised, "{mode:?}");
            assert_eq!(row.permission_mode, "supervised", "{mode:?}");
            assert_eq!(resumed_mode, PermissionMode::Supervised, "{mode:?}");
            drop(writer);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[tokio::test]
    async fn plan_sessions_refuse_to_leave_supervised() {
        let (dir, writer) = temp_store("plan-refuse");
        let agent = launch_replying(&dir, &writer, None).await;

        for mode in [PermissionMode::AcceptEdits, PermissionMode::FullAccess] {
            let refused = agent.set_permission_mode(mode).await;
            assert!(
                matches!(&refused, Err(SessionError::SetPermissionMode { mode: shown, .. })
                    if shown == mode.as_str()),
                "{mode:?}: {refused:?}"
            );
        }
        assert_eq!(
            agent.shared.subscribe().0.permission_mode,
            PermissionMode::Supervised
        );
        agent.shutdown().await;
        drop(writer);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn review_sessions_keep_the_configured_mode() {
        let (dir, writer) = temp_store("review-mode");
        let configured = SessionContext {
            permission_mode: PermissionMode::AcceptEdits,
            ..context("s1", dir.clone())
        };
        let (_, agent) = launch(
            &stream_spec(),
            configured,
            writer.clone(),
            None,
            SessionOptions::default(),
            replies_with_the_prompt,
        )
        .await
        .unwrap();
        let mode = agent.shared.subscribe().0.permission_mode;
        agent.shutdown().await;

        assert_eq!(mode, PermissionMode::AcceptEdits);
        drop(writer);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
