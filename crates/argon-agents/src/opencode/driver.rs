use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::{Stream, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::protocol::{
    OpencodeEvent, create_session_body, decode, model_body, parse_models, permission_body,
    prompt_body,
};
use super::server::{OpencodeServer, start};
use crate::http::LocalApi;
use crate::registry::{AgentSpec, ModelInfo};
use crate::session::{
    AgentEvent, Command, DriverFuture, DriverRun, PermissionAnswer, PermissionAsk, PermissionReply,
    Ready, Result, SessionError, Shared, StderrTail, session_grant_applies,
};

#[path = "driver_state.rs"]
mod state;

use state::{
    Close, Family, Reconnect, Route, Turns, check_model, created_session, decision, event_session,
    grants_session, initial_model, is_active, is_not_found, is_path_safe, model_ref, needs_restart,
    same_directory, session_path, within, without_child_plans,
};

const EVENTS_PATH: &str = "/api/event";
const SESSIONS_PATH: &str = "/api/session";
const ACTIVE_PATH: &str = "/api/session/active";
const MODELS_PATH: &str = "/api/model";
const CANCEL_GRACE: Duration = Duration::from_secs(10);
const RESUBSCRIBE_DELAY: Duration = Duration::from_secs(1);
const SUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(10);
const ALLOW_ONCE: &str = "allow_once";
const REJECT: &str = "reject_once";
const END_TURN: &str = "end_turn";
const CANCELLED_REASON: &str = "cancelled";
const SESSION_CLOSED: &str = "a sessão do OpenCode foi encerrada";
const SESSION_NOT_FOUND: &str = "sessão do OpenCode não encontrada";
const CONNECTION_LOST: &str = "conexão com o OpenCode perdida";
const DROPPED_MID_TURN: &str = "a conexão com o OpenCode caiu durante o turno";
const BUSY_TURN: &str = "já existe um turno em andamento";
const SERVER_STOPPED: &str = "o servidor do OpenCode está parado";
const UNANSWERABLE: &str =
    "o OpenCode pediu uma permissão com identificador inválido; não foi possível respondê-la";

type Answer = (Route, PermissionAnswer);
type Tagged = (u64, Feed);

enum Feed {
    Event(Value),
    Ended,
}

enum Wake {
    Command(Option<Command>),
    Feed(Tagged),
    Answer(Answer),
    Awaiting(bool),
    TurnTimeout,
    CancelExpired,
    Resubscribe,
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

fn spawn_feed(
    stream: impl Stream<Item = Value> + Send + 'static,
    epoch: u64,
    feed: mpsc::UnboundedSender<Tagged>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut stream = std::pin::pin!(stream);
        while let Some(event) = stream.next().await {
            if feed.send((epoch, Feed::Event(event))).is_err() {
                return;
            }
        }
        let _ = feed.send((epoch, Feed::Ended));
    })
}

async fn subscribe(
    api: &LocalApi,
    epoch: u64,
    feed: &mpsc::UnboundedSender<Tagged>,
) -> std::result::Result<JoinHandle<()>, String> {
    let stream = within(SUBSCRIBE_TIMEOUT, api.events(EVENTS_PATH)).await?;
    Ok(spawn_feed(stream, epoch, feed.clone()))
}

struct Session {
    shared: Arc<Shared>,
    spec: AgentSpec,
    stderr: Arc<StderrTail>,
    server: Option<OpencodeServer>,
    epoch: u64,
    verify_idle: bool,
    cwd: PathBuf,
    family: Family,
    models: Vec<ModelInfo>,
    model: String,
    pending_model: Option<String>,
    turns: Turns,
    grants: HashSet<String>,
    answers: mpsc::UnboundedSender<Answer>,
    feed: mpsc::UnboundedSender<Tagged>,
    reader: Option<JoinHandle<()>>,
    reconnect: Reconnect,
    resubscribe_at: Option<Instant>,
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
            let resubscribe_at = self.resubscribe_at;
            let in_turn = self.turns.in_turn();
            let wake = tokio::select! {
                command = inbox.commands.recv() => Wake::Command(command),
                Some(feed) = inbox.feed.recv() => Wake::Feed(feed),
                Some(answer) = inbox.answers.recv() => Wake::Answer(answer),
                changed = awaiting.changed(), if in_turn => Wake::Awaiting(changed.is_ok()),
                () = sleep_until(deadline) => Wake::TurnTimeout,
                () = sleep_until(cancel_deadline) => Wake::CancelExpired,
                () = sleep_until(resubscribe_at) => Wake::Resubscribe,
            };
            match wake {
                Wake::Command(None | Some(Command::Shutdown)) => return Ok(true),
                Wake::Command(Some(Command::Prompt(text))) => {
                    let paused = *awaiting.borrow();
                    self.prompt(text, paused).await;
                }
                Wake::Command(Some(Command::Cancel)) => self.cancel().await,
                Wake::Command(Some(Command::SetModel { model, reply })) => {
                    self.change_model(model, reply);
                }
                Wake::Command(Some(Command::SetPermissionMode { mode, reply })) => {
                    self.grants.clear();
                    self.shared.set_permission_mode(mode);
                    let _ = reply.send(Ok(()));
                }
                Wake::Feed((epoch, _)) if epoch != self.epoch => {}
                Wake::Feed((_, Feed::Event(event))) => self.on_event(&event).await,
                Wake::Feed((_, Feed::Ended)) => {
                    self.reader = None;
                    self.resubscribe_at = Some(Instant::now() + RESUBSCRIBE_DELAY);
                }
                Wake::Answer(answer) => self.on_answer(answer).await,
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
                Wake::Resubscribe => self.resubscribe().await?,
            }
        }
    }

    fn root(&self) -> &str {
        self.family.root()
    }

    fn api(&self) -> std::result::Result<&LocalApi, String> {
        self.server
            .as_ref()
            .map(|server| &server.api)
            .ok_or_else(|| SERVER_STOPPED.to_owned())
    }

    async fn post(&self, path: &str, body: &Value) -> std::result::Result<Value, String> {
        self.api()?.post(path, body).await
    }

    async fn get(&self, path: &str) -> std::result::Result<Value, String> {
        self.api()?.get(path).await
    }

    async fn must_restart(&self) -> bool {
        let active = self.get(ACTIVE_PATH).await;
        if let Err(error) = &active {
            tracing::warn!(%error, "failed to check active OpenCode sessions");
        }
        needs_restart(active.as_ref().ok(), &self.family)
    }

    async fn stop_server(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.abort();
        }
        self.resubscribe_at = None;
        self.epoch += 1;
        if let Some(server) = self.server.take() {
            server.stop().await;
        }
    }

    async fn ensure_server(&mut self) -> std::result::Result<(), String> {
        let verify = std::mem::take(&mut self.verify_idle);
        if verify && self.server.is_some() && self.must_restart().await {
            self.stop_server().await;
        }
        if self.server.is_some() {
            return Ok(());
        }
        let server = start(&self.spec, &self.cwd, Arc::clone(&self.stderr)).await?;
        self.epoch += 1;
        let root = self.root().to_owned();
        let reopened = match open_session(&server.api, &self.cwd, Some(&root)).await {
            Ok(_) => subscribe(&server.api, self.epoch, &self.feed).await,
            Err(error) => Err(error),
        };
        match reopened {
            Ok(reader) => {
                self.server = Some(server);
                self.reader = Some(reader);
                self.reconnect.succeeded();
                self.family.reset();
                self.pending_model = initial_model(&self.model);
                Ok(())
            }
            Err(error) => {
                server.stop().await;
                Err(error)
            }
        }
    }

    async fn prompt(&mut self, text: String, paused: bool) {
        if self.turns.in_turn() {
            self.shared.emit(AgentEvent::Error(BUSY_TURN.to_owned()));
            return;
        }
        self.shared.begin_turn(&text);
        if let Err(error) = self.ensure_server().await {
            self.shared
                .fail(format!("não consegui reiniciar o OpenCode: {error}"));
            return;
        }
        if let Err(error) = self.apply_model().await {
            self.shared.fail(error);
            return;
        }
        let path = session_path(self.root(), "prompt");
        if let Err(error) = self.post(&path, &prompt_body(&text)).await {
            self.shared
                .fail(format!("não consegui enviar o prompt ao OpenCode: {error}"));
            return;
        }
        self.turns.start();
        self.remaining = self.turn_timeout;
        self.running_since = (!paused).then(Instant::now);
    }

    async fn apply_model(&mut self) -> std::result::Result<(), String> {
        let Some(model) = self.pending_model.take() else {
            return Ok(());
        };
        let applied = match model_ref(&model) {
            Ok((provider_id, model_id)) => {
                let path = session_path(self.root(), "model");
                self.post(&path, &model_body(provider_id, model_id, None))
                    .await
                    .map(|_| ())
                    .map_err(|error| {
                        format!("não consegui trocar o modelo para `{model}`: {error}")
                    })
            }
            Err(error) => Err(error),
        };
        if applied.is_err() {
            self.pending_model = Some(model);
        }
        applied
    }

    async fn interrupt(&self) {
        let sessions: Vec<String> = std::iter::once(self.root())
            .chain(self.family.children())
            .map(str::to_owned)
            .collect();
        for id in sessions {
            match self.post(&session_path(&id, "interrupt"), &json!({})).await {
                Ok(_) => {}
                Err(error) if is_not_found(&error) => {}
                Err(error) => {
                    tracing::warn!(session = %id, %error, "failed to interrupt OpenCode session");
                    self.shared.emit(AgentEvent::Error(format!(
                        "não consegui interromper o OpenCode: {error}"
                    )));
                }
            }
        }
    }

    async fn cancel(&mut self) {
        if self.turns.cancel() {
            self.cancel_deadline = Some(Instant::now() + CANCEL_GRACE);
            self.interrupt().await;
        }
        self.shared.cancel_pending();
    }

    async fn expire_cancel(&mut self) {
        self.cancel_deadline = None;
        let close = self.turns.expire();
        if close.is_some() && self.must_restart().await {
            self.stop_server().await;
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
            self.pending_model = Some(model.clone());
            self.model = model;
        }
        self.shared.release();
        let _ = reply.send(result);
    }

    async fn on_event(&mut self, event: &Value) {
        let family = &self.family;
        let decoded = decode(event, family.root(), &|id| family.contains(id));
        match decoded {
            OpencodeEvent::SessionCreated { id, .. } => {
                self.family.insert(id);
            }
            OpencodeEvent::Events(events) => {
                let from_child = event_session(event).is_some_and(|id| id != self.root());
                self.shared
                    .emit_all(without_child_plans(events, from_child));
            }
            OpencodeEvent::TurnEnd { stop_reason, error } => {
                let close = self.turns.finish(&stop_reason, error);
                self.close_turn(close).await;
            }
            OpencodeEvent::Permission {
                id,
                session_id,
                ask,
            } => {
                let route = Route {
                    session_id,
                    request_id: id,
                    kind: ask.kind.clone(),
                };
                if route.reply_path().is_none() {
                    tracing::warn!(request = %route.request_id, "unsafe OpenCode permission route");
                    self.shared.emit(AgentEvent::Error(UNANSWERABLE.to_owned()));
                } else if self.turns.is_cancelled() {
                    self.reply(&route, REJECT).await;
                } else {
                    self.ask(route, ask).await;
                }
            }
            OpencodeEvent::Ignore => {}
        }
    }

    async fn ask(&mut self, route: Route, ask: PermissionAsk) {
        if self.grants.contains(&ask.kind) && session_grant_applies(&ask, &self.cwd) {
            self.reply(&route, ALLOW_ONCE).await;
            return;
        }
        self.shared
            .on_permission(ask, reply_to(&self.answers, route));
    }

    async fn on_answer(&mut self, (route, answer): Answer) {
        if grants_session(&answer) {
            self.grants.insert(route.kind.clone());
        }
        self.reply(&route, decision(&answer)).await;
    }

    async fn reply(&self, route: &Route, option: &str) {
        let (Some(path), Ok(api)) = (route.reply_path(), self.api()) else {
            return;
        };
        match api.post(&path, &permission_body(option)).await {
            Ok(_) => {}
            Err(error) if is_not_found(&error) => {}
            Err(error) => self.shared.emit(AgentEvent::Error(format!(
                "não consegui responder a permissão do OpenCode: {error}"
            ))),
        }
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

    async fn resubscribe(&mut self) -> std::result::Result<(), String> {
        self.resubscribe_at = None;
        let subscribed = match self.api() {
            Ok(api) => subscribe(api, self.epoch, &self.feed).await,
            Err(error) => Err(error),
        };
        match subscribed {
            Ok(reader) => {
                self.reader = Some(reader);
                self.reconnect.succeeded();
                if self.turns.in_turn() && !self.still_running().await {
                    self.turns.clear();
                    self.running_since = None;
                    self.cancel_deadline = None;
                    self.shared.fail(DROPPED_MID_TURN.to_owned());
                }
                Ok(())
            }
            Err(error) if self.reconnect.failed() => Err(format!("{CONNECTION_LOST}: {error}")),
            Err(error) => {
                tracing::warn!(%error, "failed to resubscribe to OpenCode events");
                self.resubscribe_at = Some(Instant::now() + RESUBSCRIBE_DELAY);
                Ok(())
            }
        }
    }

    async fn still_running(&self) -> bool {
        match self.get(ACTIVE_PATH).await {
            Ok(body) => is_active(&body, self.root()),
            Err(error) => {
                tracing::warn!(%error, "failed to check active OpenCode sessions");
                false
            }
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
            Close::Cancelled => {
                self.verify_idle = true;
                self.shared.end_turn(CANCELLED_REASON).await;
            }
            Close::Failed(message) => self.shared.fail(message),
        }
    }

    async fn shutdown(mut self) {
        self.stop_server().await;
    }
}

async fn open_session(
    api: &LocalApi,
    cwd: &Path,
    load: Option<&str>,
) -> std::result::Result<String, String> {
    if let Some(id) = load {
        if !is_path_safe(id) {
            return Err(SESSION_NOT_FOUND.to_owned());
        }
        return match api.get(&format!("{SESSIONS_PATH}/{id}")).await {
            Ok(_) => Ok(id.to_owned()),
            Err(error) if is_not_found(&error) => Err(SESSION_NOT_FOUND.to_owned()),
            Err(error) => Err(format!("{SESSION_NOT_FOUND} ({error})")),
        };
    }
    let directory = cwd.display().to_string();
    let body = api
        .post(SESSIONS_PATH, &create_session_body(&directory))
        .await
        .map_err(|error| format!("não consegui criar a sessão do OpenCode: {error}"))?;
    let (id, reported) =
        created_session(&body).ok_or("o OpenCode não devolveu o id da sessão criada")?;
    match reported {
        Some(reported) if same_directory(&reported, cwd) => Ok(id),
        reported => Err(format!(
            "o OpenCode abriu a sessão em outra pasta ({}) em vez de {directory}",
            reported.unwrap_or_default()
        )),
    }
}

async fn list_models(api: &LocalApi) -> (Vec<ModelInfo>, Vec<String>) {
    match api.get(MODELS_PATH).await {
        Ok(body) => (parse_models(&body), Vec::new()),
        Err(error) => (
            Vec::new(),
            vec![format!(
                "não consegui listar os modelos do OpenCode: {error}"
            )],
        ),
    }
}

struct Opened {
    id: String,
    models: Vec<ModelInfo>,
    warnings: Vec<String>,
    reader: JoinHandle<()>,
}

async fn connect(
    api: &LocalApi,
    cwd: &Path,
    load: Option<&str>,
    feed: &mpsc::UnboundedSender<Tagged>,
) -> std::result::Result<Opened, String> {
    let id = open_session(api, cwd, load).await?;
    let reader = subscribe(api, 0, feed)
        .await
        .map_err(|error| format!("não consegui assinar os eventos do OpenCode: {error}"))?;
    let (models, warnings) = list_models(api).await;
    Ok(Opened {
        id,
        models,
        warnings,
        reader,
    })
}

pub(crate) fn run(run: DriverRun) -> DriverFuture {
    Box::pin(async move {
        let server = match start(&run.spec, &run.ctx.cwd, Arc::clone(&run.stderr)).await {
            Ok(server) => server,
            Err(error) => {
                let _ = run.ready.send(Err(SessionError::Agent(error)));
                return Ok(false);
            }
        };
        let (feed, fed) = mpsc::unbounded_channel();
        let opened = match connect(&server.api, &run.ctx.cwd, run.load.as_deref(), &feed).await {
            Ok(opened) => opened,
            Err(error) => {
                let _ = run.ready.send(Err(SessionError::Agent(error)));
                server.stop().await;
                return Ok(false);
            }
        };
        let ready = Ready {
            agent_session_id: opened.id.clone(),
            model: run.ctx.model.clone(),
            models: opened.models.clone(),
            warnings: opened.warnings,
        };
        if run.ready.send(Ok(ready)).is_err() {
            opened.reader.abort();
            server.stop().await;
            return Ok(false);
        }
        let (answers, answered) = mpsc::unbounded_channel();
        let mut session = Session {
            shared: Arc::clone(&run.shared),
            spec: run.spec.clone(),
            stderr: Arc::clone(&run.stderr),
            server: Some(server),
            epoch: 0,
            verify_idle: false,
            cwd: run.ctx.cwd.clone(),
            family: Family::new(opened.id),
            models: opened.models,
            model: run.ctx.model.clone(),
            pending_model: initial_model(&run.ctx.model),
            turns: Turns::default(),
            grants: HashSet::new(),
            answers,
            feed,
            reader: Some(opened.reader),
            reconnect: Reconnect::default(),
            resubscribe_at: None,
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
        session.shutdown().await;
        outcome
    })
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
