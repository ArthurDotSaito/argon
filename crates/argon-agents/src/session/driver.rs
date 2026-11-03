use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use agent_client_protocol::v1::Responder;
use agent_client_protocol::v1::schema::{
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome,
};
use argon_store::SessionStatus;
use serde_json::Value;
use tokio::sync::broadcast;

use super::events::{AgentEvent, map_update};
use super::permissions::{PermissionDecision, decide};
use super::sink::StoreSink;
use super::turn::{TurnBuffer, finish_turn};
use super::{Result, SessionError};

type PermissionResponder = Responder<RequestPermissionResponse>;

#[derive(Default)]
struct State {
    active: bool,
    in_turn: bool,
    status: Option<SessionStatus>,
    buffer: TurnBuffer,
    pending: HashMap<String, PermissionResponder>,
    tool_kinds: HashMap<String, String>,
    next_permission: u64,
}

pub(super) struct Shared {
    id: String,
    cwd: PathBuf,
    nonce: String,
    allowed_files: Vec<String>,
    store: Arc<dyn StoreSink>,
    events: broadcast::Sender<AgentEvent>,
    state: Mutex<State>,
}

fn selected(option_id: &str) -> RequestPermissionResponse {
    RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
        SelectedPermissionOutcome::new(option_id.to_owned()),
    ))
}

fn cancelled() -> RequestPermissionResponse {
    RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)
}

impl Shared {
    pub fn new(
        id: String,
        cwd: PathBuf,
        nonce: String,
        allowed_files: Vec<String>,
        store: Arc<dyn StoreSink>,
        events: broadcast::Sender<AgentEvent>,
    ) -> Shared {
        Shared {
            id,
            cwd,
            nonce,
            allowed_files,
            store,
            events,
            state: Mutex::new(State::default()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
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
        self.store.append_event(&self.id, &event);
        let _ = self.events.send(event);
    }

    fn status_locked(&self, state: &mut State, status: SessionStatus) {
        if state.status != Some(status) {
            state.status = Some(status);
            self.store.set_status(&self.id, status);
        }
    }

    pub fn activate(&self, status: SessionStatus) {
        let mut state = self.lock();
        state.active = true;
        self.status_locked(&mut state, status);
    }

    pub fn emit(&self, event: AgentEvent) {
        let mut state = self.lock();
        self.emit_locked(&mut state, event);
    }

    pub fn store(&self) -> &Arc<dyn StoreSink> {
        &self.store
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn in_turn(&self) -> bool {
        self.lock().in_turn
    }

    pub fn on_update(&self, update: &Value) {
        let mut state = self.lock();
        for event in map_update(update) {
            self.emit_locked(&mut state, event);
        }
    }

    pub fn on_permission(
        &self,
        request: &RequestPermissionRequest,
        responder: PermissionResponder,
    ) {
        let value = serde_json::to_value(request).unwrap_or(Value::Null);
        let mut state = self.lock();
        let tool_call_id = value
            .pointer("/toolCall/toolCallId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let known_kind = state.tool_kinds.get(tool_call_id).cloned();
        match decide(&value, known_kind.as_deref(), &self.cwd) {
            PermissionDecision::Allow { option_id } => {
                if let Err(error) = responder.respond(selected(&option_id)) {
                    tracing::warn!(%error, "failed to auto-allow permission");
                }
            }
            PermissionDecision::Ask(prompt) => {
                state.next_permission += 1;
                let request_id = format!("perm-{}", state.next_permission);
                state.pending.insert(request_id.clone(), responder);
                self.emit_locked(
                    &mut state,
                    AgentEvent::PermissionRequest {
                        request_id,
                        title: prompt.title,
                        kind: prompt.kind,
                        options: prompt.options,
                    },
                );
                self.status_locked(&mut state, SessionStatus::AwaitingUser);
            }
        }
    }

    pub fn respond(&self, request_id: &str, option_id: &str) -> Result<()> {
        let mut state = self.lock();
        let responder = state
            .pending
            .remove(request_id)
            .ok_or_else(|| SessionError::UnknownPermission(request_id.to_owned()))?;
        responder
            .respond(selected(option_id))
            .map_err(|error| SessionError::Acp(error.to_string()))?;
        if state.pending.is_empty() && state.in_turn {
            self.status_locked(&mut state, SessionStatus::Running);
        }
        Ok(())
    }

    pub fn cancel_pending(&self) {
        let mut state = self.lock();
        for (_, responder) in state.pending.drain() {
            let _ = responder.respond(cancelled());
        }
        if state.in_turn {
            self.status_locked(&mut state, SessionStatus::Running);
        }
    }

    pub fn begin_turn(&self, text: &str) {
        let mut state = self.lock();
        state.in_turn = true;
        state.buffer.take();
        self.emit_locked(&mut state, AgentEvent::UserMessage(text.to_owned()));
        self.status_locked(&mut state, SessionStatus::Running);
    }

    pub fn end_turn(&self, stop_reason: &str) {
        let mut state = self.lock();
        for (_, responder) in state.pending.drain() {
            let _ = responder.respond(cancelled());
        }
        let text = state.buffer.take();
        let outcome = finish_turn(&text, stop_reason, &self.nonce, &self.allowed_files);
        if !outcome.findings.is_empty() {
            self.store.save_findings(&self.id, &outcome.findings);
        }
        for event in outcome.events {
            self.emit_locked(&mut state, event);
        }
        state.in_turn = false;
        self.status_locked(&mut state, SessionStatus::Completed);
    }

    pub fn fail(&self, message: String) {
        let mut state = self.lock();
        for (_, responder) in state.pending.drain() {
            let _ = responder.respond(cancelled());
        }
        state.in_turn = false;
        state.buffer.take();
        self.emit_locked(&mut state, AgentEvent::Error(message));
        self.status_locked(&mut state, SessionStatus::Failed);
    }
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
