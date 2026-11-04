use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use super::protocol::{ClaudeLine, allow, deny, interrupt, parse_line, user_message};
use crate::process::KillOnDropChild;
use crate::registry::AgentSpec;
use crate::session::{
    AgentEvent, Command, DriverFuture, DriverRun, PermissionAnswer, PermissionAsk, PermissionReply,
    Ready, Result, SessionError, Shared, StderrTail, start_agent, terminate,
};

#[path = "driver_turns.rs"]
mod turns;

use turns::{Close, Turns};

const DEFAULT_MODEL: &str = "default";
const STREAM_ARGS: [&str; 8] = [
    "-p",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--permission-prompt-tool",
    "stdio",
];
const SESSION_ID_FLAG: &str = "--session-id";
const RESUME_FLAG: &str = "--resume";
const MODEL_FLAG: &str = "--model";
const UUID_BYTES: usize = 16;
const CANCEL_GRACE: Duration = Duration::from_secs(10);
const ALLOW_ONCE: &str = "allow_once";
const ALLOW_ALWAYS: &str = "allow_always";
const DENIED: &str = "Negado no Argon.";
const CANCELLED: &str = "Cancelado no Argon.";
const END_TURN: &str = "end_turn";
const CANCELLED_REASON: &str = "cancelled";
const UNEXPECTED_EXIT: &str = "o Claude encerrou inesperadamente";
const BUSY_TURN: &str = "já existe um turno em andamento";

type Answer = (String, PermissionAnswer);
type Flow = std::result::Result<(), String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Start<'a> {
    New(&'a str),
    Resume(&'a str),
}

fn launch_args(base: &[String], start: Start<'_>, model: &str) -> Vec<String> {
    let mut args = base.to_vec();
    args.extend(STREAM_ARGS.map(str::to_owned));
    let (flag, id) = match start {
        Start::New(id) => (SESSION_ID_FLAG, id),
        Start::Resume(id) => (RESUME_FLAG, id),
    };
    args.extend([flag.to_owned(), id.to_owned()]);
    if !model.is_empty() && model != DEFAULT_MODEL {
        args.extend([MODEL_FLAG.to_owned(), model.to_owned()]);
    }
    args
}

fn format_uuid(mut bytes: [u8; UUID_BYTES]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn new_uuid() -> Result<String> {
    let mut bytes = [0u8; UUID_BYTES];
    getrandom::fill(&mut bytes)?;
    Ok(format_uuid(bytes))
}

fn reply_to(answers: &mpsc::UnboundedSender<Answer>, request_id: String) -> PermissionReply {
    let answers = answers.clone();
    Box::new(move |answer| {
        answers
            .send((request_id, answer))
            .map_err(|_| UNEXPECTED_EXIT.to_owned())
    })
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

struct Claude {
    child: KillOnDropChild,
    stdin: Option<ChildStdin>,
    lines: Lines<BufReader<ChildStdout>>,
}

impl Claude {
    fn spawn(
        spec: &AgentSpec,
        cwd: &Path,
        args: Vec<String>,
        tail: Arc<StderrTail>,
    ) -> Result<Claude> {
        let spec = AgentSpec {
            args,
            ..spec.clone()
        };
        let (child, stdin, stdout) = start_agent(&spec, cwd, tail)?;
        Ok(Claude {
            child,
            stdin: Some(stdin),
            lines: BufReader::new(stdout).lines(),
        })
    }

    async fn send(&mut self, line: &str) -> Flow {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| UNEXPECTED_EXIT.to_owned())?;
        let written = async {
            stdin.write_all(line.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await
        };
        written
            .await
            .map_err(|error| format!("{UNEXPECTED_EXIT}: {error}"))
    }

    async fn shutdown(&mut self) {
        self.stdin = None;
        terminate(&mut self.child).await;
    }
}

struct Ask {
    tool_name: String,
    input: Value,
}

enum Wake {
    Command(Option<Command>),
    Line(std::io::Result<Option<String>>),
    Answer(Answer),
    Awaiting(bool),
    TurnTimeout,
    CancelExpired,
}

struct Session {
    shared: Arc<Shared>,
    spec: AgentSpec,
    cwd: PathBuf,
    tail: Arc<StderrTail>,
    id: String,
    has_history: bool,
    model: String,
    claude: Claude,
    turns: Turns,
    asks: HashMap<String, Ask>,
    always: HashSet<String>,
    answers: mpsc::UnboundedSender<Answer>,
    interrupts: u64,
    cancel_deadline: Option<Instant>,
    turn_timeout: Duration,
    remaining: Duration,
    running_since: Option<Instant>,
}

impl Session {
    async fn drive(
        &mut self,
        mut commands: mpsc::UnboundedReceiver<Command>,
        mut answered: mpsc::UnboundedReceiver<Answer>,
    ) -> std::result::Result<bool, String> {
        let mut awaiting = self.shared.awaiting();
        loop {
            let deadline = self.running_since.map(|since| since + self.remaining);
            let cancel_deadline = self.cancel_deadline;
            let in_turn = self.turns.in_turn();
            let wake = tokio::select! {
                command = commands.recv() => Wake::Command(command),
                line = self.claude.lines.next_line() => Wake::Line(line),
                Some(answer) = answered.recv() => Wake::Answer(answer),
                changed = awaiting.changed(), if in_turn => Wake::Awaiting(changed.is_ok()),
                () = sleep_until(deadline) => Wake::TurnTimeout,
                () = sleep_until(cancel_deadline) => Wake::CancelExpired,
            };
            match wake {
                Wake::Command(None | Some(Command::Shutdown)) => return Ok(true),
                Wake::Command(Some(Command::Prompt(text))) => {
                    let paused = *awaiting.borrow();
                    self.prompt(text, paused).await?;
                }
                Wake::Command(Some(Command::Cancel)) => self.cancel().await?,
                Wake::Command(Some(Command::SetModel { model, reply })) => {
                    self.change_model(model, reply).await?;
                }
                Wake::Command(Some(Command::SetPermissionMode { mode, reply })) => {
                    self.shared.set_permission_mode(mode);
                    let _ = reply.send(Ok(()));
                }
                Wake::Line(Ok(Some(line))) => self.on_line(&line).await?,
                Wake::Line(_) => return Err(UNEXPECTED_EXIT.to_owned()),
                Wake::Answer(answer) => self.on_answer(answer).await?,
                Wake::Awaiting(true) => self.on_awaiting(*awaiting.borrow_and_update()),
                Wake::Awaiting(false) => {}
                Wake::TurnTimeout => {
                    self.shared.fail(format!(
                        "o turno passou de {} minutos e foi encerrado",
                        self.turn_timeout.as_secs() / 60
                    ));
                    return Ok(true);
                }
                Wake::CancelExpired => {
                    let close = self.turns.expire();
                    self.close_turn(close).await;
                }
            }
        }
    }

    async fn prompt(&mut self, text: String, paused: bool) -> Flow {
        if self.turns.in_turn() {
            self.shared.emit(AgentEvent::Error(BUSY_TURN.to_owned()));
            return Ok(());
        }
        self.shared.begin_turn(&text);
        self.turns.start();
        self.has_history = true;
        self.remaining = self.turn_timeout;
        self.running_since = (!paused).then(Instant::now);
        self.claude.send(&user_message(&text)).await
    }

    async fn cancel(&mut self) -> Flow {
        if self.turns.cancel() {
            self.cancel_deadline = Some(Instant::now() + CANCEL_GRACE);
            self.interrupts += 1;
            let request_id = format!("argon-interrupt-{}", self.interrupts);
            self.claude.send(&interrupt(&request_id)).await?;
        }
        self.shared.cancel_pending();
        Ok(())
    }

    async fn change_model(&mut self, model: String, reply: oneshot::Sender<Result<()>>) -> Flow {
        if self.turns.in_turn() {
            let _ = reply.send(Err(SessionError::Busy));
            return Ok(());
        }
        self.claude.shutdown().await;
        match self.respawn(&model) {
            Ok(()) => {
                self.shared.set_model(&model);
                self.model = model;
                self.shared.release();
                let _ = reply.send(Ok(()));
                Ok(())
            }
            Err(error) => {
                self.shared.release();
                let reason = error.to_string();
                let message = format!("não consegui reiniciar o Claude com `{model}`: {reason}");
                let _ = reply.send(Err(SessionError::SetModel { model, reason }));
                Err(message)
            }
        }
    }

    fn respawn(&mut self, model: &str) -> Result<()> {
        let start = if self.has_history {
            Start::Resume(&self.id)
        } else {
            Start::New(&self.id)
        };
        let args = launch_args(&self.spec.args, start, model);
        self.claude = Claude::spawn(&self.spec, &self.cwd, args, Arc::clone(&self.tail))?;
        self.turns = Turns::default();
        self.asks.clear();
        self.cancel_deadline = None;
        Ok(())
    }

    async fn on_line(&mut self, line: &str) -> Flow {
        match parse_line(line) {
            ClaudeLine::Init { model, .. } => {
                if !model.is_empty() && model != self.model {
                    self.shared.set_model(&model);
                    self.model = model;
                }
            }
            ClaudeLine::TurnEnd { stop_reason, error } => {
                let close = self.turns.finish(&stop_reason, error);
                self.close_turn(close).await;
            }
            ClaudeLine::Permission { request_id, .. } if self.turns.is_stale() => {
                return self.claude.send(&deny(&request_id, CANCELLED)).await;
            }
            ClaudeLine::Permission {
                request_id,
                tool_name,
                ask,
                input,
            } => return self.ask(request_id, tool_name, ask, input).await,
            ClaudeLine::Events(events) if !self.turns.is_stale() => self.shared.emit_all(events),
            ClaudeLine::ApiError(text) => self.turns.api_error(text),
            ClaudeLine::Events(_) | ClaudeLine::Ignore => {}
        }
        Ok(())
    }

    async fn ask(
        &mut self,
        request_id: String,
        tool_name: String,
        ask: PermissionAsk,
        input: Value,
    ) -> Flow {
        if self.always.contains(&tool_name) {
            return self.claude.send(&allow(&request_id, &input)).await;
        }
        self.asks
            .insert(request_id.clone(), Ask { tool_name, input });
        self.shared
            .on_permission(ask, reply_to(&self.answers, request_id));
        Ok(())
    }

    async fn on_answer(&mut self, (request_id, answer): Answer) -> Flow {
        let Some(ask) = self.asks.remove(&request_id) else {
            return Ok(());
        };
        let line = match answer {
            PermissionAnswer::Selected(option) if option == ALLOW_ALWAYS => {
                let line = allow(&request_id, &ask.input);
                self.always.insert(ask.tool_name);
                line
            }
            PermissionAnswer::Selected(option) if option == ALLOW_ONCE => {
                allow(&request_id, &ask.input)
            }
            PermissionAnswer::Selected(_) => deny(&request_id, DENIED),
            PermissionAnswer::Cancelled => deny(&request_id, CANCELLED),
        };
        self.claude.send(&line).await
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

fn open(run: &DriverRun, answers: mpsc::UnboundedSender<Answer>) -> Result<Session> {
    let id = match &run.load {
        Some(id) => id.clone(),
        None => new_uuid()?,
    };
    let start = if run.load.is_some() {
        Start::Resume(&id)
    } else {
        Start::New(&id)
    };
    let args = launch_args(&run.spec.args, start, &run.ctx.model);
    let claude = Claude::spawn(&run.spec, &run.ctx.cwd, args, Arc::clone(&run.stderr))?;
    let model = if run.ctx.model.is_empty() {
        DEFAULT_MODEL.to_owned()
    } else {
        run.ctx.model.clone()
    };
    Ok(Session {
        shared: Arc::clone(&run.shared),
        spec: run.spec.clone(),
        cwd: run.ctx.cwd.clone(),
        tail: Arc::clone(&run.stderr),
        has_history: run.load.is_some(),
        id,
        model,
        claude,
        turns: Turns::default(),
        asks: HashMap::new(),
        always: HashSet::new(),
        answers,
        interrupts: 0,
        cancel_deadline: None,
        turn_timeout: run.options.turn_timeout,
        remaining: run.options.turn_timeout,
        running_since: None,
    })
}

pub(crate) fn run(run: DriverRun) -> DriverFuture {
    Box::pin(async move {
        let (answers, answered) = mpsc::unbounded_channel();
        let mut session = match open(&run, answers) {
            Ok(session) => session,
            Err(error) => {
                let _ = run.ready.send(Err(error));
                return Ok(false);
            }
        };
        let ready = Ready {
            agent_session_id: session.id.clone(),
            model: session.model.clone(),
            models: run.spec.models.clone(),
            warnings: Vec::new(),
        };
        if run.ready.send(Ok(ready)).is_err() {
            session.claude.shutdown().await;
            return Ok(false);
        }
        let outcome = session.drive(run.commands, answered).await;
        session.claude.shutdown().await;
        outcome
    })
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
