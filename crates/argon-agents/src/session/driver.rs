use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use argon_core::{FindingsParse, PermissionMode, PrKey, SessionKind, SummaryOutput};
use argon_store::SessionStatus;
use tokio::sync::{broadcast, watch};

use super::events::{AgentEvent, PendingPermission, SavedFinding};
use super::record::Recorder;
use super::stderr::StderrTail;
use super::turn::{TurnBuffer, TurnOutcome, finish_turn};
use super::writer::{Op, SessionWriter, SummaryRecord};
use crate::registry::ModelInfo;
use crate::util::lock;

pub(super) const ENDED_BEFORE_ACTIVATION: &str = "o agente encerrou antes de abrir a sessão";
const MISSING_BASE_SHA: &str = "a sessão de plano não tem o commit base";
const MISSING_SUMMARY_SHA: &str = "a sessão de resumo não tem o commit do worktree";

#[path = "driver_permissions.rs"]
mod permission_flow;

use permission_flow::Pending;
#[cfg(test)]
use permission_flow::validate_option;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub status: SessionStatus,
    pub model: String,
    pub models: Vec<ModelInfo>,
    pub pending_permissions: Vec<PendingPermission>,
    pub permission_mode: PermissionMode,
    pub last_seq: u64,
    pub open_record: Option<(u64, AgentEvent)>,
}

struct State {
    active: bool,
    early_failure: Option<String>,
    in_turn: bool,
    status: SessionStatus,
    model: String,
    models: Vec<ModelInfo>,
    recorder: Recorder,
    buffer: TurnBuffer,
    pending: Vec<Pending>,
    tool_kinds: HashMap<String, String>,
    next_permission: u64,
    permission_mode: PermissionMode,
    output_recorded: bool,
}

pub(super) struct SharedConfig {
    pub id: String,
    pub subject: PrKey,
    pub kind: SessionKind,
    pub base_sha: Option<String>,
    pub denial_ends_turn: bool,
    pub cwd: PathBuf,
    pub read_roots: Vec<PathBuf>,
    pub repo: String,
    pub number: u64,
    pub refresh_project: bool,
    pub nonce: String,
    pub allowed_files: Vec<String>,
    pub writer: SessionWriter,
    pub stderr: Arc<StderrTail>,
}

pub(crate) struct Shared {
    config: SharedConfig,
    events: broadcast::Sender<(u64, AgentEvent)>,
    busy: AtomicBool,
    awaiting: watch::Sender<bool>,
    state: Mutex<State>,
}

impl Shared {
    pub(super) fn new(
        config: SharedConfig,
        permission_mode: PermissionMode,
        events: broadcast::Sender<(u64, AgentEvent)>,
    ) -> Shared {
        Shared {
            config,
            events,
            busy: AtomicBool::new(false),
            awaiting: watch::channel(false).0,
            state: Mutex::new(State {
                active: false,
                early_failure: None,
                in_turn: false,
                status: SessionStatus::Running,
                model: String::new(),
                models: Vec::new(),
                recorder: Recorder::default(),
                buffer: TurnBuffer::default(),
                pending: Vec::new(),
                tool_kinds: HashMap::new(),
                next_permission: 0,
                permission_mode,
                output_recorded: false,
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_tests(
        id: &str,
        cwd: PathBuf,
        writer: SessionWriter,
        stderr: Arc<StderrTail>,
    ) -> Shared {
        Shared::new(
            SharedConfig {
                id: id.to_owned(),
                subject: PrKey::new("github.com", "PR_1"),
                kind: SessionKind::Review,
                base_sha: None,
                denial_ends_turn: false,
                cwd,
                read_roots: Vec::new(),
                repo: String::new(),
                number: 0,
                refresh_project: false,
                nonce: String::new(),
                allowed_files: Vec::new(),
                writer,
                stderr,
            },
            PermissionMode::Supervised,
            broadcast::channel(64).0,
        )
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    pub fn id(&self) -> &str {
        &self.config.id
    }

    pub fn kind(&self) -> SessionKind {
        self.config.kind
    }

    #[cfg(test)]
    pub(super) fn read_roots(&self) -> &[PathBuf] {
        &self.config.read_roots
    }

    pub fn writer(&self) -> &SessionWriter {
        &self.config.writer
    }

    pub fn stderr_tail(&self, lines: usize) -> Vec<String> {
        self.config.stderr.last(lines)
    }

    pub fn awaiting(&self) -> watch::Receiver<bool> {
        self.awaiting.subscribe()
    }

    pub fn try_acquire(&self) -> bool {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn release(&self) {
        self.busy.store(false, Ordering::Release);
    }

    fn persist(&self, records: Vec<(u64, AgentEvent)>) {
        for (seq, event) in records {
            match serde_json::to_string(&event) {
                Ok(data) => self.config.writer.push(Op::Event {
                    session: self.config.id.clone(),
                    seq,
                    data,
                }),
                Err(error) => tracing::error!(%error, "failed to serialize agent event"),
            }
        }
    }

    fn emit_locked(&self, state: &mut State, event: AgentEvent) {
        if !state.active {
            return;
        }
        if let AgentEvent::Error(message) = &event {
            tracing::warn!(session = %self.config.id, %message, "agent session error");
        }
        state.buffer.observe(&event);
        if let AgentEvent::ToolCall { id, kind, .. } = &event {
            state.tool_kinds.insert(id.clone(), kind.clone());
        }
        let recorded = state.recorder.record(&event);
        self.persist(recorded.closed);
        let _ = self.events.send((recorded.seq, event));
    }

    fn close_record(&self, state: &mut State) {
        let closed: Vec<_> = state.recorder.close().into_iter().collect();
        self.persist(closed);
    }

    fn status_locked(&self, state: &mut State, status: SessionStatus) {
        if state.status != status {
            state.status = status;
            self.config.writer.push(Op::Status {
                session: self.config.id.clone(),
                status,
            });
        }
    }

    fn sync_awaiting(&self, state: &mut State) {
        let awaiting = !state.pending.is_empty();
        self.awaiting.send_if_modified(|current| {
            let changed = *current != awaiting;
            *current = awaiting;
            changed
        });
        if state.in_turn {
            let status = if awaiting {
                SessionStatus::AwaitingUser
            } else {
                SessionStatus::Running
            };
            self.status_locked(state, status);
        }
    }

    pub fn activate(
        &self,
        status: SessionStatus,
        last_seq: u64,
        model: String,
        models: Vec<ModelInfo>,
    ) {
        let mut state = self.lock();
        state.active = true;
        state.recorder = Recorder::starting_after(last_seq);
        state.model = model;
        state.models = models;
        state.status = status;
        self.config.writer.push(Op::Status {
            session: self.config.id.clone(),
            status,
        });
        if let Some(message) = state.early_failure.take() {
            self.fail_locked(&mut state, message);
            drop(state);
            self.release();
        }
    }

    pub(super) fn start_in_mode(&self, mode: PermissionMode) {
        self.lock().permission_mode = mode;
    }

    pub fn emit(&self, event: AgentEvent) {
        let mut state = self.lock();
        self.emit_locked(&mut state, event);
    }

    pub fn emit_transient(&self, event: AgentEvent) {
        let state = self.lock();
        let _ = self.events.send((state.recorder.last_seq(), event));
    }

    pub fn subscribe(&self) -> (Snapshot, broadcast::Receiver<(u64, AgentEvent)>) {
        let state = self.lock();
        let snapshot = Snapshot {
            status: state.status,
            model: state.model.clone(),
            models: state.models.clone(),
            pending_permissions: state.pending.iter().map(|p| p.permission.clone()).collect(),
            permission_mode: state.permission_mode,
            last_seq: state.recorder.last_closed(),
            open_record: state.recorder.open_record(),
        };
        (snapshot, self.events.subscribe())
    }

    pub fn set_model(&self, model: &str) {
        let mut state = self.lock();
        state.model = model.to_owned();
        self.config.writer.push(Op::Model {
            session: self.config.id.clone(),
            model: model.to_owned(),
        });
    }

    pub fn set_agent_session_id(&self, id: &str) {
        self.config.writer.push(Op::AgentSessionId {
            session: self.config.id.clone(),
            id: id.to_owned(),
        });
    }

    pub fn in_turn(&self) -> bool {
        self.lock().in_turn
    }

    pub fn status(&self) -> SessionStatus {
        self.lock().status
    }

    pub fn emit_all(&self, events: Vec<AgentEvent>) {
        let mut state = self.lock();
        for event in events {
            self.emit_locked(&mut state, event);
        }
    }

    pub fn begin_turn(&self, text: &str) {
        let mut state = self.lock();
        state.in_turn = true;
        state.buffer.take();
        self.emit_locked(&mut state, AgentEvent::UserMessage(text.to_owned()));
        self.status_locked(&mut state, SessionStatus::Running);
    }

    pub(super) fn mark_output_recorded(&self) {
        self.lock().output_recorded = true;
    }

    pub async fn end_turn(&self, stop_reason: &str) {
        let (outcome, output_recorded) = {
            let mut state = self.lock();
            self.cancel_pending_locked(&mut state);
            let text = state.buffer.take();
            self.emit_locked(
                &mut state,
                AgentEvent::TurnEnded {
                    stop_reason: stop_reason.to_owned(),
                },
            );
            let outcome = finish_turn(
                &text,
                stop_reason,
                &self.config.nonce,
                &self.config.allowed_files,
                self.config.kind,
            );
            (outcome, state.output_recorded)
        };
        match outcome {
            TurnOutcome::NotFinished => {}
            TurnOutcome::Unreadable(_) if self.config.kind.is_read_only() && output_recorded => {}
            TurnOutcome::Unreadable(message) => self.emit(AgentEvent::Error(message)),
            TurnOutcome::Findings(parsed) => self.save_findings(parsed).await,
            TurnOutcome::Plan(markdown) => self.record_plan(markdown).await,
            TurnOutcome::Summary(output) => self.record_summary(output).await,
        }
        let mut state = self.lock();
        state.in_turn = false;
        self.close_record(&mut state);
        self.status_locked(&mut state, SessionStatus::Completed);
        drop(state);
        self.release();
    }

    async fn save_findings(&self, parsed: FindingsParse) {
        let saved = if parsed.findings.is_empty() {
            Ok(Vec::new())
        } else {
            self.config
                .writer
                .save_findings(&self.config.id, parsed.findings)
                .await
        };
        match saved {
            Ok(stored) => self.emit(AgentEvent::Findings {
                findings: stored
                    .into_iter()
                    .map(|stored| SavedFinding {
                        id: stored.id,
                        finding: stored.finding,
                    })
                    .collect(),
                skipped: parsed.skipped,
            }),
            Err(error) => {
                tracing::error!(%error, "failed to save findings");
                self.emit(AgentEvent::Error(format!(
                    "não consegui gravar os achados: {error}"
                )));
            }
        }
    }

    async fn record_plan(&self, markdown: String) {
        let recorded = match self.config.base_sha.as_deref() {
            Some(base_sha) => {
                self.config
                    .writer
                    .record_plan(&self.config.id, &self.config.subject, &markdown, base_sha)
                    .await
            }
            None => Err(MISSING_BASE_SHA.to_owned()),
        };
        match recorded {
            Ok(draft) => {
                self.mark_output_recorded();
                self.emit(AgentEvent::PlanDraft {
                    draft_id: draft.id,
                    markdown,
                });
            }
            Err(error) => {
                tracing::error!(session = %self.config.id, %error, "failed to save plan draft");
                self.emit(AgentEvent::Error(format!(
                    "não consegui gravar o plano: {error}"
                )));
            }
        }
    }

    async fn record_summary(&self, output: SummaryOutput) {
        let SummaryOutput {
            summary,
            project,
            arch,
            touched,
        } = output;
        let project =
            project.filter(|_| self.config.refresh_project && !self.config.repo.is_empty());
        let project_updated = project.is_some();
        let recorded = match self.config.base_sha.as_deref() {
            Some(head_sha) => {
                self.config
                    .writer
                    .record_summary(SummaryRecord {
                        session: self.config.id.clone(),
                        subject: self.config.subject.clone(),
                        repo: self.config.repo.clone(),
                        number: self.config.number,
                        body: summary.clone(),
                        head_sha: head_sha.to_owned(),
                        touched,
                        project,
                        arch,
                    })
                    .await
            }
            None => Err(MISSING_SUMMARY_SHA.to_owned()),
        };
        match recorded {
            Ok(stored) => {
                self.mark_output_recorded();
                self.emit(AgentEvent::Summary {
                    id: stored.id,
                    body: summary,
                    project_updated,
                });
            }
            Err(error) => {
                tracing::error!(session = %self.config.id, %error, "failed to save pull request summary");
                self.emit(AgentEvent::Error(format!(
                    "não consegui gravar o resumo: {error}"
                )));
            }
        }
    }

    fn fail_locked(&self, state: &mut State, message: String) {
        self.cancel_pending_locked(state);
        state.in_turn = false;
        state.buffer.take();
        self.emit_locked(state, AgentEvent::Error(message));
        self.close_record(state);
        self.status_locked(state, SessionStatus::Failed);
    }

    pub fn fail(&self, message: String) {
        let mut state = self.lock();
        if state.active {
            self.fail_locked(&mut state, message);
        } else {
            state.early_failure = Some(message);
        }
        drop(state);
        self.release();
    }

    pub fn close(&self) {
        let mut state = self.lock();
        if state.active {
            self.close_record(&mut state);
        } else {
            state
                .early_failure
                .get_or_insert_with(|| ENDED_BEFORE_ACTIVATION.to_owned());
        }
    }
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
