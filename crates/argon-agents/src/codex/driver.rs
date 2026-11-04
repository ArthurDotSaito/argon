use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use argon_core::PermissionMode;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use super::protocol::{
    CodexEvent, CodexRequest, Decoder, approval_response, default_model, initialize_params,
    interrupt_params, message_turn, parse_models, resume_params, thread_id, thread_params, turn_id,
    turn_params, with_mode, with_untrusted_projects,
};
use crate::jsonrpc::{Incoming, RpcClient};
use crate::process::KillOnDropChild;
use crate::registry::{AgentSpec, ModelInfo};
use crate::session::{
    ALLOW_ONCE, AgentEvent, Command, DriverFuture, DriverRun, PermissionAnswer, PermissionAsk,
    PermissionReply, Ready, Result, SessionError, Shared, StderrTail, start_agent, terminate,
    with_stderr,
};
use crate::util::{Resolved, resolve_command};

#[path = "driver_state.rs"]
mod state;

use state::{
    Close, ModelPages, Turns, auto_allowed, check_model, exit_message, grants_session, is_archived,
    project_roots, trust_keys,
};

const APP_SERVER_ARG: &str = "app-server";
const GIT_MARKER: &str = ".git";
const CANCEL_GRACE: Duration = Duration::from_secs(10);
const UNSUPPORTED_REQUEST: i64 = -32601;
const END_TURN: &str = "end_turn";
const CANCELLED_REASON: &str = "cancelled";
const SESSION_CLOSED: &str = "a sessão do Codex foi encerrada";
const BUSY_TURN: &str = "já existe um turno em andamento";
const MISSING_THREAD: &str = "o Codex não devolveu o id da conversa";
const MISSING_TURN: &str = "o Codex não devolveu o id do turno";

type Answer = (Route, PermissionAnswer);
type Tagged = (u64, Feed);

enum Feed {
    Message(Incoming),
    Closed,
}

struct Route {
    generation: u64,
    id: Value,
    method: String,
    params: Value,
    kind: String,
}

enum Wake {
    Command(Option<Command>),
    Feed(Tagged),
    Answer(Answer),
    Awaiting(bool),
    TurnTimeout,
    CancelExpired,
}

enum Open<'a> {
    Start,
    Resume(&'a str),
}

struct ThreadSetup<'a> {
    cwd: &'a Path,
    model: Option<&'a str>,
    mode: PermissionMode,
    untrusted: &'a [String],
}

fn reply_to(answers: &mpsc::UnboundedSender<Answer>, route: Route) -> PermissionReply {
    let answers = answers.clone();
    Box::new(move |answer| {
        answers
            .send((route, answer))
            .map_err(|_| SESSION_CLOSED.to_owned())
    })
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

fn non_empty(text: &str) -> Option<&str> {
    (!text.is_empty()).then_some(text)
}

fn check_command(spec: &AgentSpec) -> Result<()> {
    match resolve_command(&spec.command) {
        Resolved::Path(_) => Ok(()),
        Resolved::Missing => Err(SessionError::CommandNotFound(
            spec.command.display().to_string(),
        )),
        Resolved::Failed(message) => Err(SessionError::Agent(message)),
    }
}

fn untrusted_roots(cwd: &Path) -> Vec<String> {
    let canonical = dunce::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    trust_keys(&project_roots(&canonical, |dir| {
        dir.join(GIT_MARKER).exists()
    }))
}

fn forward(
    mut incoming: mpsc::UnboundedReceiver<Incoming>,
    generation: u64,
    feed: mpsc::UnboundedSender<Tagged>,
) {
    tokio::spawn(async move {
        while let Some(message) = incoming.recv().await {
            if feed.send((generation, Feed::Message(message))).is_err() {
                return;
            }
        }
        let _ = feed.send((generation, Feed::Closed));
    });
}

struct Codex {
    child: KillOnDropChild,
    rpc: RpcClient,
    generation: u64,
}

impl Codex {
    fn spawn(
        spec: &AgentSpec,
        cwd: &Path,
        tail: Arc<StderrTail>,
        generation: u64,
        feed: &mpsc::UnboundedSender<Tagged>,
    ) -> Result<Codex> {
        check_command(spec)?;
        let mut args = spec.args.clone();
        args.push(APP_SERVER_ARG.to_owned());
        let spec = AgentSpec {
            args,
            ..spec.clone()
        };
        let (child, stdin, stdout) = start_agent(&spec, cwd, tail)?;
        let (rpc, incoming) = RpcClient::new(stdin, stdout);
        forward(incoming, generation, feed.clone());
        Ok(Codex {
            child,
            rpc,
            generation,
        })
    }

    async fn stop(self) -> Option<i32> {
        let Codex { mut child, rpc, .. } = self;
        drop(rpc);
        terminate(&mut child).await;
        child.wait().await.ok().and_then(|status| status.code())
    }
}

async fn handshake(rpc: &RpcClient) -> std::result::Result<(), String> {
    rpc.call("initialize", initialize_params(env!("CARGO_PKG_VERSION")))
        .await?;
    rpc.notify("initialized", None)
}

async fn resume(rpc: &RpcClient, id: &str, params: Value) -> std::result::Result<(), String> {
    match rpc.call("thread/resume", params.clone()).await {
        Ok(_) => Ok(()),
        Err(error) if is_archived(&error) => {
            rpc.call("thread/unarchive", json!({"threadId": id}))
                .await?;
            rpc.call("thread/resume", params).await.map(|_| ())
        }
        Err(error) => Err(error),
    }
}

async fn open_thread(
    rpc: &RpcClient,
    open: Open<'_>,
    setup: &ThreadSetup<'_>,
) -> std::result::Result<String, String> {
    handshake(rpc).await?;
    match open {
        Open::Start => {
            let params = thread_params(setup.cwd, setup.model, setup.mode);
            let result = rpc
                .call(
                    "thread/start",
                    with_untrusted_projects(params, setup.untrusted),
                )
                .await?;
            thread_id(&result).ok_or_else(|| MISSING_THREAD.to_owned())
        }
        Open::Resume(id) => {
            let params = resume_params(id, setup.cwd, setup.model, setup.mode);
            resume(rpc, id, with_untrusted_projects(params, setup.untrusted)).await?;
            Ok(id.to_owned())
        }
    }
}

async fn list_models(rpc: &RpcClient) -> std::result::Result<Value, String> {
    let mut pages = ModelPages::default();
    let mut params = json!({});
    loop {
        let page = rpc.call("model/list", params).await?;
        match pages.add(&page) {
            Some(cursor) => params = json!({"cursor": cursor}),
            None => return Ok(pages.result()),
        }
    }
}

struct Session {
    shared: Arc<Shared>,
    spec: AgentSpec,
    cwd: PathBuf,
    tail: Arc<StderrTail>,
    untrusted: Vec<String>,
    thread: String,
    models: Vec<ModelInfo>,
    model: String,
    mode: PermissionMode,
    codex: Option<Codex>,
    generation: u64,
    decoder: Decoder,
    feed: mpsc::UnboundedSender<Tagged>,
    turns: Turns,
    grants: HashSet<String>,
    answers: mpsc::UnboundedSender<Answer>,
    cancel_deadline: Option<Instant>,
    turn_timeout: Duration,
    remaining: Duration,
    running_since: Option<Instant>,
}

struct Inbox {
    commands: mpsc::UnboundedReceiver<Command>,
    feed: mpsc::UnboundedReceiver<Tagged>,
    answers: mpsc::UnboundedReceiver<Answer>,
}

impl Session {
    async fn drive(&mut self, mut inbox: Inbox) -> std::result::Result<bool, String> {
        let mut awaiting = self.shared.awaiting();
        loop {
            let deadline = self.running_since.map(|since| since + self.remaining);
            let cancel_deadline = self.cancel_deadline;
            let in_turn = self.turns.in_turn();
            let wake = tokio::select! {
                command = inbox.commands.recv() => Wake::Command(command),
                Some(feed) = inbox.feed.recv() => Wake::Feed(feed),
                Some(answer) = inbox.answers.recv() => Wake::Answer(answer),
                changed = awaiting.changed(), if in_turn => Wake::Awaiting(changed.is_ok()),
                () = sleep_until(deadline) => Wake::TurnTimeout,
                () = sleep_until(cancel_deadline) => Wake::CancelExpired,
            };
            match wake {
                Wake::Command(None | Some(Command::Shutdown)) => return Ok(true),
                Wake::Command(Some(Command::Prompt(text))) => {
                    let paused = *awaiting.borrow();
                    self.prompt(text, paused).await;
                }
                Wake::Command(Some(Command::Cancel)) => self.cancel(),
                Wake::Command(Some(Command::SetModel { model, reply })) => {
                    self.change_model(model, reply);
                }
                Wake::Command(Some(Command::SetPermissionMode { mode, reply })) => {
                    self.mode = mode;
                    self.grants.clear();
                    self.shared.set_permission_mode(mode);
                    let _ = reply.send(Ok(()));
                }
                Wake::Feed((generation, feed)) if self.is_current(generation) => {
                    self.on_feed(feed).await?;
                }
                Wake::Feed(_) => {}
                Wake::Answer(answer) => self.on_answer(answer),
                Wake::Awaiting(true) => self.on_awaiting(*awaiting.borrow_and_update()),
                Wake::Awaiting(false) => {}
                Wake::TurnTimeout => {
                    self.shared.fail(format!(
                        "o turno passou de {} minutos e foi encerrado",
                        self.turn_timeout.as_secs() / 60
                    ));
                    return Ok(true);
                }
                Wake::CancelExpired => self.expire_cancel().await,
            }
        }
    }

    fn is_current(&self, generation: u64) -> bool {
        self.codex
            .as_ref()
            .is_some_and(|codex| codex.generation == generation)
    }

    fn rpc(&self) -> Option<&RpcClient> {
        self.codex.as_ref().map(|codex| &codex.rpc)
    }

    fn respond(&self, id: Value, result: Value) {
        if let Some(Err(error)) = self.rpc().map(|rpc| rpc.respond(id, result)) {
            tracing::debug!(%error, "failed to answer a Codex request");
        }
    }

    fn respond_unsupported(&self, id: Value, method: &str) {
        let message = format!("o Argon não suporta `{method}`");
        if let Some(Err(error)) = self
            .rpc()
            .map(|rpc| rpc.respond_error(id, UNSUPPORTED_REQUEST, &message))
        {
            tracing::debug!(%error, "failed to refuse a Codex request");
        }
    }

    async fn ensure_process(&mut self) -> std::result::Result<(), String> {
        if self.codex.is_some() {
            return Ok(());
        }
        self.generation += 1;
        let codex = Codex::spawn(
            &self.spec,
            &self.cwd,
            Arc::clone(&self.tail),
            self.generation,
            &self.feed,
        )
        .map_err(|error| error.to_string())?;
        let setup = ThreadSetup {
            cwd: &self.cwd,
            model: non_empty(&self.model),
            mode: self.mode,
            untrusted: &self.untrusted,
        };
        match open_thread(&codex.rpc, Open::Resume(&self.thread), &setup).await {
            Ok(_) => {
                self.codex = Some(codex);
                self.decoder = Decoder::new();
                Ok(())
            }
            Err(error) => {
                codex.stop().await;
                Err(error)
            }
        }
    }

    async fn stop_process(&mut self) -> Option<i32> {
        match self.codex.take() {
            Some(codex) => codex.stop().await,
            None => None,
        }
    }

    async fn prompt(&mut self, text: String, paused: bool) {
        if self.turns.in_turn() {
            self.shared.emit(AgentEvent::Error(BUSY_TURN.to_owned()));
            return;
        }
        self.shared.begin_turn(&text);
        if let Err(error) = self.ensure_process().await {
            let message = format!("não consegui reiniciar o Codex: {error}");
            self.shared.fail(with_stderr(message, &self.shared));
            return;
        }
        let params = with_mode(
            turn_params(&self.thread, &text, non_empty(&self.model), None),
            self.mode,
        );
        let started = match self.rpc() {
            Some(rpc) => rpc.call("turn/start", params).await,
            None => Err(SESSION_CLOSED.to_owned()),
        };
        match started.and_then(|result| turn_id(&result).ok_or_else(|| MISSING_TURN.to_owned())) {
            Ok(id) => {
                self.turns.start(id);
                self.remaining = self.turn_timeout;
                self.running_since = (!paused).then(Instant::now);
            }
            Err(error) => self
                .shared
                .fail(format!("não consegui iniciar o turno do Codex: {error}")),
        }
    }

    fn cancel(&mut self) {
        if self.turns.cancel() {
            self.cancel_deadline = Some(Instant::now() + CANCEL_GRACE);
            self.interrupt();
        }
        self.shared.cancel_pending();
    }

    fn interrupt(&self) {
        let (Some(rpc), Some(turn)) = (self.rpc(), self.turns.current_id()) else {
            return;
        };
        let rpc = rpc.clone();
        let params = interrupt_params(&self.thread, turn);
        tokio::spawn(async move {
            if let Err(error) = rpc.call("turn/interrupt", params).await {
                tracing::warn!(%error, "failed to interrupt the Codex turn");
            }
        });
    }

    async fn expire_cancel(&mut self) {
        self.cancel_deadline = None;
        let close = self.turns.expire();
        if close.is_some() {
            self.stop_process().await;
        }
        self.close_turn(close).await;
    }

    fn change_model(&mut self, model: String, reply: oneshot::Sender<Result<()>>) {
        let result = if self.turns.in_turn() {
            Err(SessionError::Busy)
        } else {
            check_model(&self.models, &model)
        };
        if result.is_ok() {
            self.shared.set_model(&model);
            self.model = model;
        }
        self.shared.release();
        let _ = reply.send(result);
    }

    async fn on_feed(&mut self, feed: Feed) -> std::result::Result<(), String> {
        match feed {
            Feed::Message(Incoming::Notification { method, params }) => {
                self.on_notification(&method, &params).await;
                Ok(())
            }
            Feed::Message(Incoming::Request { id, method, params }) => {
                self.on_request(id, method, params);
                Ok(())
            }
            Feed::Closed => Err(exit_message(self.stop_process().await)),
        }
    }

    async fn on_notification(&mut self, method: &str, params: &Value) {
        let event = self.decoder.notification(method, params);
        if !self.turns.owns(message_turn(params).as_deref()) {
            return;
        }
        match event {
            CodexEvent::Events(events) => self.shared.emit_all(events),
            CodexEvent::TurnEnd {
                turn_id,
                stop_reason,
                error,
            } => {
                let close = self.turns.finish(&turn_id, &stop_reason, error);
                self.close_turn(close).await;
            }
            CodexEvent::TurnStarted { .. } | CodexEvent::Ignore => {}
        }
    }

    fn on_request(&mut self, id: Value, method: String, params: Value) {
        match self.decoder.request(&method, &params) {
            CodexRequest::Reply(result) => self.respond(id, result),
            CodexRequest::Unsupported => self.respond_unsupported(id, &method),
            CodexRequest::Ask(_)
                if !self.turns.accepts_request(message_turn(&params).as_deref()) =>
            {
                let refusal = approval_response(&method, &params, &PermissionAnswer::Cancelled);
                self.respond(id, refusal);
            }
            CodexRequest::Ask(ask) => self.ask(id, method, params, ask),
        }
    }

    fn ask(&mut self, id: Value, method: String, params: Value, ask: PermissionAsk) {
        if auto_allowed(&ask, &self.cwd, self.mode, &self.grants) {
            let allowed = PermissionAnswer::Selected(ALLOW_ONCE.to_owned());
            self.respond(id, approval_response(&method, &params, &allowed));
            return;
        }
        let route = Route {
            generation: self.generation,
            id,
            method,
            params,
            kind: ask.kind.clone(),
        };
        self.shared
            .on_permission(ask, reply_to(&self.answers, route));
    }

    fn on_answer(&mut self, (route, answer): Answer) {
        if !self.is_current(route.generation) {
            return;
        }
        if grants_session(&answer) {
            self.grants.insert(route.kind);
        }
        self.respond(
            route.id,
            approval_response(&route.method, &route.params, &answer),
        );
    }

    fn on_awaiting(&mut self, awaiting: bool) {
        if awaiting {
            if let Some(since) = self.running_since.take() {
                self.remaining = self.remaining.saturating_sub(since.elapsed());
            }
        } else if self.running_since.is_none() {
            self.running_since = Some(Instant::now());
        }
    }

    async fn close_turn(&mut self, close: Option<Close>) {
        let Some(close) = close else {
            return;
        };
        self.running_since = None;
        self.cancel_deadline = None;
        match close {
            Close::Completed => self.shared.end_turn(END_TURN).await,
            Close::Cancelled => self.shared.end_turn(CANCELLED_REASON).await,
            Close::Failed(message) => self.shared.fail(message),
        }
    }
}

struct Opened {
    codex: Codex,
    thread: String,
    models: Vec<ModelInfo>,
    model: String,
    warnings: Vec<String>,
    untrusted: Vec<String>,
}

async fn connect(
    run: &DriverRun,
    feed: &mpsc::UnboundedSender<Tagged>,
) -> std::result::Result<Opened, SessionError> {
    let codex = Codex::spawn(&run.spec, &run.ctx.cwd, Arc::clone(&run.stderr), 1, feed)?;
    let untrusted = untrusted_roots(&run.ctx.cwd);
    let setup = ThreadSetup {
        cwd: &run.ctx.cwd,
        model: non_empty(&run.ctx.model),
        mode: run.ctx.permission_mode,
        untrusted: &untrusted,
    };
    let open = match &run.load {
        Some(id) => Open::Resume(id),
        None => Open::Start,
    };
    let thread = match open_thread(&codex.rpc, open, &setup).await {
        Ok(thread) => thread,
        Err(error) => {
            codex.stop().await;
            let message = format!("não consegui abrir a conversa do Codex: {error}");
            return Err(SessionError::Agent(with_stderr(message, &run.shared)));
        }
    };
    let (models, default, warnings) = match list_models(&codex.rpc).await {
        Ok(result) => (parse_models(&result), default_model(&result), Vec::new()),
        Err(error) => (
            Vec::new(),
            None,
            vec![format!("não consegui listar os modelos do Codex: {error}")],
        ),
    };
    let model = non_empty(&run.ctx.model)
        .map(str::to_owned)
        .or(default)
        .unwrap_or_default();
    Ok(Opened {
        codex,
        thread,
        models,
        model,
        warnings,
        untrusted,
    })
}

pub(crate) fn run(run: DriverRun) -> DriverFuture {
    Box::pin(async move {
        let (feed, fed) = mpsc::unbounded_channel();
        let opened = match connect(&run, &feed).await {
            Ok(opened) => opened,
            Err(error) => {
                let _ = run.ready.send(Err(error));
                return Ok(false);
            }
        };
        let ready = Ready {
            agent_session_id: opened.thread.clone(),
            model: opened.model.clone(),
            models: opened.models.clone(),
            warnings: opened.warnings,
        };
        if run.ready.send(Ok(ready)).is_err() {
            opened.codex.stop().await;
            return Ok(false);
        }
        let (answers, answered) = mpsc::unbounded_channel();
        let mut session = Session {
            shared: Arc::clone(&run.shared),
            spec: run.spec.clone(),
            cwd: run.ctx.cwd.clone(),
            tail: Arc::clone(&run.stderr),
            untrusted: opened.untrusted,
            thread: opened.thread,
            models: opened.models,
            model: opened.model,
            mode: run.ctx.permission_mode,
            generation: opened.codex.generation,
            codex: Some(opened.codex),
            decoder: Decoder::new(),
            feed,
            turns: Turns::default(),
            grants: HashSet::new(),
            answers,
            cancel_deadline: None,
            turn_timeout: run.options.turn_timeout,
            remaining: run.options.turn_timeout,
            running_since: None,
        };
        let inbox = Inbox {
            commands: run.commands,
            feed: fed,
            answers: answered,
        };
        let outcome = session.drive(inbox).await;
        session.stop_process().await;
        outcome
    })
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
