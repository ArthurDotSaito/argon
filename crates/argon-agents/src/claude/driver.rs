use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use argon_core::PermissionMode;
use serde_json::Value;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::protocol::{ClaudeLine, allow, deny, interrupt, parse_line, user_message};
use crate::process::KillOnDropChild;
use crate::registry::AgentSpec;
use crate::session::{
    AgentEvent, Command, DriverFuture, DriverRun, PermissionAnswer, PermissionAsk,
    PermissionDecision, PermissionReply, Ready, Result, STDERR_LINES, SessionError, Shared,
    Started, StderrTail, decide_permission, explain_failure, session_grant_applies, start_tracked,
    terminate, with_stderr,
};
use crate::util::{Resolved, read_line_lossy, resolve_command};

#[path = "driver_turns.rs"]
mod turns;

use turns::{Close, Turns};

const DEFAULT_MODEL: &str = "default";
const STREAM_ARGS: [&str; 13] = [
    "-p",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--permission-prompt-tool",
    "stdio",
    "--permission-mode",
    "default",
    "--setting-sources",
    "user",
    "--strict-mcp-config",
];
const SESSION_ID_FLAG: &str = "--session-id";
const RESUME_FLAG: &str = "--resume";
const MODEL_FLAG: &str = "--model";
const UUID_BYTES: usize = 16;
const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;
const CANCEL_GRACE: Duration = Duration::from_secs(10);
const STDERR_GRACE: Duration = Duration::from_secs(2);
const ALLOW_ONCE: &str = "allow_once";
const ALLOW_ALWAYS: &str = "allow_always";
const DENIED: &str = "Negado no Argon.";
const CANCELLED: &str = "Cancelado no Argon.";
const END_TURN: &str = "end_turn";
const CANCELLED_REASON: &str = "cancelled";
const SESSION_CLOSED: &str = "a sessão do Claude foi encerrada";
const BUSY_TURN: &str = "já existe um turno em andamento";

type Answer = (String, PermissionAnswer);
type Tagged = (u64, Output);

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

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(index, c)| match index {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

fn new_uuid() -> Result<String> {
    let mut bytes = [0u8; UUID_BYTES];
    getrandom::fill(&mut bytes)?;
    Ok(format_uuid(bytes))
}

fn auto_allowed(
    ask: &PermissionAsk,
    tool_name: &str,
    cwd: &Path,
    mode: PermissionMode,
    always: &HashSet<String>,
) -> bool {
    match decide_permission(ask, cwd, mode) {
        PermissionDecision::Allow { .. } => true,
        PermissionDecision::Ask(_) => always.contains(tool_name) && session_grant_applies(ask, cwd),
    }
}

fn exit_message(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("processo do Claude saiu (código {code})"),
        None => "processo do Claude saiu".to_owned(),
    }
}

fn reply_to(answers: &mpsc::UnboundedSender<Answer>, request_id: String) -> PermissionReply {
    let answers = answers.clone();
    Box::new(move |answer| {
        answers
            .send((request_id, answer))
            .map_err(|_| SESSION_CLOSED.to_owned())
    })
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

enum Output {
    Line(String),
    Closed,
    WriteFailed(String),
}

async fn write_line(stdin: &mut ChildStdin, line: &str) -> std::io::Result<()> {
    stdin.write_all(line.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await
}

fn spawn_writer(
    mut stdin: ChildStdin,
    generation: u64,
    outputs: mpsc::UnboundedSender<Tagged>,
) -> mpsc::UnboundedSender<String> {
    let (outbox, mut pending) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        while let Some(line) = pending.recv().await {
            if let Err(error) = write_line(&mut stdin, &line).await {
                let _ = outputs.send((generation, Output::WriteFailed(error.to_string())));
                return;
            }
        }
    });
    outbox
}

fn spawn_reader(stdout: ChildStdout, generation: u64, outputs: mpsc::UnboundedSender<Tagged>) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        while let Ok(Some(line)) = read_line_lossy(&mut reader, MAX_LINE_BYTES).await {
            if outputs.send((generation, Output::Line(line))).is_err() {
                return;
            }
        }
        let _ = outputs.send((generation, Output::Closed));
    });
}

struct Claude {
    child: KillOnDropChild,
    outbox: mpsc::UnboundedSender<String>,
    stderr: Option<JoinHandle<()>>,
    generation: u64,
}

impl Claude {
    fn spawn(
        spec: &AgentSpec,
        cwd: &Path,
        args: Vec<String>,
        tail: Arc<StderrTail>,
        generation: u64,
        outputs: &mpsc::UnboundedSender<Tagged>,
    ) -> Result<Claude> {
        let spec = AgentSpec {
            args,
            ..spec.clone()
        };
        let Started {
            child,
            stdin,
            stdout,
            stderr,
        } = start_tracked(&spec, cwd, tail)?;
        spawn_reader(stdout, generation, outputs.clone());
        Ok(Claude {
            child,
            outbox: spawn_writer(stdin, generation, outputs.clone()),
            stderr,
            generation,
        })
    }

    async fn stop(self) -> Option<i32> {
        let Claude {
            mut child,
            outbox,
            stderr,
            ..
        } = self;
        drop(outbox);
        terminate(&mut child).await;
        let code = child.wait().await.ok().and_then(|status| status.code());
        if let Some(stderr) = stderr {
            let _ = tokio::time::timeout(STDERR_GRACE, stderr).await;
        }
        code
    }
}

struct Ask {
    tool_name: String,
    input: Value,
}

enum Wake {
    Command(Option<Command>),
    Output(Tagged),
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
    transcript: bool,
    launch_model: String,
    model: String,
    mode: PermissionMode,
    claude: Option<Claude>,
    generation: u64,
    outputs: mpsc::UnboundedSender<Tagged>,
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

struct Inbox {
    commands: mpsc::UnboundedReceiver<Command>,
    outputs: mpsc::UnboundedReceiver<Tagged>,
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
                Some(output) = inbox.outputs.recv() => Wake::Output(output),
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
                    self.change_model(model, reply).await;
                }
                Wake::Command(Some(Command::SetPermissionMode { mode, reply })) => {
                    self.mode = mode;
                    self.always.clear();
                    self.shared.set_permission_mode(mode);
                    let _ = reply.send(Ok(()));
                }
                Wake::Output((generation, output)) if self.is_current(generation) => {
                    self.on_output(output).await;
                }
                Wake::Output(_) => {}
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
        self.claude
            .as_ref()
            .is_some_and(|claude| claude.generation == generation)
    }

    fn send(&self, line: String) {
        if let Some(claude) = &self.claude {
            let _ = claude.outbox.send(line);
        }
    }

    fn ensure_process(&mut self) -> Result<()> {
        if self.claude.is_some() {
            return Ok(());
        }
        let start = if self.transcript {
            Start::Resume(&self.id)
        } else {
            Start::New(&self.id)
        };
        let args = launch_args(&self.spec.args, start, &self.launch_model);
        self.tail.clear();
        self.generation += 1;
        self.claude = Some(Claude::spawn(
            &self.spec,
            &self.cwd,
            args,
            Arc::clone(&self.tail),
            self.generation,
            &self.outputs,
        )?);
        Ok(())
    }

    fn process_exited(&mut self) -> bool {
        self.claude
            .as_mut()
            .is_some_and(|claude| !matches!(claude.child.try_wait(), Ok(None)))
    }

    async fn stop_process(&mut self) -> Option<i32> {
        self.asks.clear();
        match self.claude.take() {
            Some(claude) => claude.stop().await,
            None => None,
        }
    }

    async fn prompt(&mut self, text: String, paused: bool) {
        if self.turns.in_turn() {
            self.shared.emit(AgentEvent::Error(BUSY_TURN.to_owned()));
            return;
        }
        if self.process_exited() {
            self.on_exit(None).await;
        }
        self.shared.begin_turn(&text);
        if let Err(error) = self.ensure_process() {
            let message = format!("não consegui iniciar o Claude: {error}");
            self.shared.fail(with_stderr(message, &self.shared));
            return;
        }
        self.turns.start();
        self.remaining = self.turn_timeout;
        self.running_since = (!paused).then(Instant::now);
        self.send(user_message(&text));
    }

    fn cancel(&mut self) {
        if self.turns.cancel() {
            self.cancel_deadline = Some(Instant::now() + CANCEL_GRACE);
            self.interrupts += 1;
            self.send(interrupt(&format!("argon-interrupt-{}", self.interrupts)));
        }
        self.shared.cancel_pending();
    }

    async fn expire_cancel(&mut self) {
        self.cancel_deadline = None;
        let close = self.turns.expire();
        if close.is_some() {
            self.stop_process().await;
        }
        self.close_turn(close, &[]).await;
    }

    async fn change_model(&mut self, model: String, reply: oneshot::Sender<Result<()>>) {
        if self.turns.in_turn() {
            let _ = reply.send(Err(SessionError::Busy));
            return;
        }
        self.stop_process().await;
        self.launch_model = model.clone();
        self.shared.set_model(&model);
        self.model = model;
        self.shared.release();
        let _ = reply.send(Ok(()));
    }

    async fn on_output(&mut self, output: Output) {
        match output {
            Output::Line(line) => self.on_line(&line).await,
            Output::Closed => self.on_exit(None).await,
            Output::WriteFailed(error) => self.on_exit(Some(error)).await,
        }
    }

    async fn on_exit(&mut self, write_error: Option<String>) {
        let mut message = exit_message(self.stop_process().await);
        if let Some(error) = write_error {
            message = format!("{message}; falha ao escrever no stdin: {error}");
        }
        if !self.turns.in_turn() {
            tracing::info!(%message, "claude exited between turns; the next prompt resumes it");
            return;
        }
        let tail = self.shared.stderr_tail(STDERR_LINES);
        let close = self.turns.abort(message);
        self.close_turn(close, &tail).await;
    }

    async fn on_line(&mut self, line: &str) {
        match parse_line(line) {
            ClaudeLine::Init { session_id, model } => self.on_init(session_id, model),
            ClaudeLine::TurnEnd { stop_reason, error } => {
                let close = self.turns.finish(&stop_reason, error);
                self.close_turn(close, &[]).await;
            }
            ClaudeLine::Permission { request_id, .. } if self.turns.is_cancelled() => {
                self.send(deny(&request_id, CANCELLED));
            }
            ClaudeLine::Permission {
                request_id,
                tool_name,
                ask,
                input,
            } => self.ask(request_id, tool_name, ask, input),
            ClaudeLine::Events(events) => self.shared.emit_all(events),
            ClaudeLine::ApiError(text) => self.turns.api_error(text),
            ClaudeLine::Ignore => {}
        }
    }

    fn on_init(&mut self, session_id: String, model: String) {
        self.transcript = true;
        if !session_id.is_empty() && session_id != self.id {
            self.follow(session_id);
        }
        if !model.is_empty() && model != self.model {
            self.shared.set_model(&model);
            self.model = model;
        }
    }

    fn follow(&mut self, session_id: String) {
        if !is_uuid(&session_id) {
            tracing::warn!(
                current = %self.id,
                reported = %session_id,
                "claude reported a session id that is not a UUID; keeping the current one"
            );
            return;
        }
        tracing::info!(
            previous = %self.id,
            reported = %session_id,
            "claude reported a new session id; following it"
        );
        self.shared.set_agent_session_id(&session_id);
        self.id = session_id;
    }

    fn ask(&mut self, request_id: String, tool_name: String, ask: PermissionAsk, input: Value) {
        if auto_allowed(&ask, &tool_name, &self.cwd, self.mode, &self.always) {
            self.send(allow(&request_id, &input));
            return;
        }
        self.asks
            .insert(request_id.clone(), Ask { tool_name, input });
        self.shared
            .on_permission(ask, reply_to(&self.answers, request_id));
    }

    fn on_answer(&mut self, (request_id, answer): Answer) {
        let Some(ask) = self.asks.remove(&request_id) else {
            return;
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
        self.send(line);
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

    async fn close_turn(&mut self, close: Option<Close>, tail: &[String]) {
        let Some(close) = close else {
            return;
        };
        self.running_since = None;
        self.cancel_deadline = None;
        match close {
            Close::Completed => self.shared.end_turn(END_TURN).await,
            Close::Cancelled => self.shared.end_turn(CANCELLED_REASON).await,
            Close::Failed(message) => self.shared.fail(explain_failure(&self.spec, message, tail)),
        }
    }
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

fn open(
    run: &DriverRun,
    answers: mpsc::UnboundedSender<Answer>,
    outputs: mpsc::UnboundedSender<Tagged>,
) -> Result<Session> {
    check_command(&run.spec)?;
    let id = match &run.load {
        Some(id) => id.clone(),
        None => new_uuid()?,
    };
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
        id,
        transcript: run.load.is_some(),
        launch_model: model.clone(),
        model,
        mode: run.ctx.permission_mode,
        claude: None,
        generation: 0,
        outputs,
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
        let (outputs, output) = mpsc::unbounded_channel();
        let mut session = match open(&run, answers, outputs) {
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
            permission_mode: None,
        };
        if run.ready.send(Ok(ready)).is_err() {
            return Ok(false);
        }
        let inbox = Inbox {
            commands: run.commands,
            outputs: output,
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
