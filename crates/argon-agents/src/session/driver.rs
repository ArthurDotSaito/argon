use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use argon_store::SessionStatus;
use serde_json::Value;
use tokio::sync::{broadcast, watch};

use super::events::{AgentEvent, PendingPermission, SavedFinding, map_update};
use super::record::Recorder;
use super::stderr::StderrTail;
use super::turn::{TurnBuffer, TurnOutcome, finish_turn};
use super::writer::{Op, SessionWriter};
use crate::registry::ModelInfo;
use crate::util::lock;

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
    pub last_seq: u64,
    pub open_record: Option<(u64, AgentEvent)>,
}

struct State {
    active: bool,
    in_turn: bool,
    status: SessionStatus,
    model: String,
    models: Vec<ModelInfo>,
    recorder: Recorder,
    buffer: TurnBuffer,
    pending: Vec<Pending>,
    tool_kinds: HashMap<String, String>,
    next_permission: u64,
}

pub(super) struct SharedConfig {
    pub id: String,
    pub cwd: PathBuf,
    pub nonce: String,
    pub allowed_files: Vec<String>,
    pub writer: SessionWriter,
    pub stderr: Arc<StderrTail>,
    pub full_access: bool,
}

pub(super) struct Shared {
    config: SharedConfig,
    events: broadcast::Sender<(u64, AgentEvent)>,
    busy: AtomicBool,
    awaiting: watch::Sender<bool>,
    state: Mutex<State>,
}

impl Shared {
    pub fn new(config: SharedConfig, events: broadcast::Sender<(u64, AgentEvent)>) -> Shared {
        Shared {
            config,
            events,
            busy: AtomicBool::new(false),
            awaiting: watch::channel(false).0,
            state: Mutex::new(State {
                active: false,
                in_turn: false,
                status: SessionStatus::Running,
                model: String::new(),
                models: Vec::new(),
                recorder: Recorder::default(),
                buffer: TurnBuffer::default(),
                pending: Vec::new(),
                tool_kinds: HashMap::new(),
                next_permission: 0,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    pub fn id(&self) -> &str {
        &self.config.id
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

    pub fn in_turn(&self) -> bool {
        self.lock().in_turn
    }

    pub fn status(&self) -> SessionStatus {
        self.lock().status
    }

    pub fn on_update(&self, update: &Value) {
        let mut state = self.lock();
        for event in map_update(update) {
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

    pub async fn end_turn(&self, stop_reason: &str) {
        let outcome = {
            let mut state = self.lock();
            self.cancel_pending_locked(&mut state);
            let text = state.buffer.take();
            self.emit_locked(
                &mut state,
                AgentEvent::TurnEnded {
                    stop_reason: stop_reason.to_owned(),
                },
            );
            finish_turn(
                &text,
                stop_reason,
                &self.config.nonce,
                &self.config.allowed_files,
            )
        };
        match outcome {
            TurnOutcome::NotFinished => {}
            TurnOutcome::Unreadable(message) => self.emit(AgentEvent::Error(message)),
            TurnOutcome::Findings(parsed) => {
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
        }
        let mut state = self.lock();
        state.in_turn = false;
        self.close_record(&mut state);
        self.status_locked(&mut state, SessionStatus::Completed);
        drop(state);
        self.release();
    }

    pub fn fail(&self, message: String) {
        let mut state = self.lock();
        self.cancel_pending_locked(&mut state);
        state.in_turn = false;
        state.buffer.take();
        self.emit_locked(&mut state, AgentEvent::Error(message));
        self.close_record(&mut state);
        self.status_locked(&mut state, SessionStatus::Failed);
        drop(state);
        self.release();
    }

    pub fn close(&self) {
        let mut state = self.lock();
        self.close_record(&mut state);
    }
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
