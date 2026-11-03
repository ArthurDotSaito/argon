use agent_client_protocol::v1::Responder;
use agent_client_protocol::v1::schema::{
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome,
};
use serde_json::Value;

use super::{Shared, State};
use crate::session::events::{AgentEvent, PendingPermission};
use crate::session::permissions::{PermissionDecision, decide};
use crate::session::{Result, SessionError};

pub(super) type PermissionResponder = Responder<RequestPermissionResponse>;

pub(super) struct Pending {
    pub responder: PermissionResponder,
    pub permission: PendingPermission,
}

fn selected(option_id: &str) -> RequestPermissionResponse {
    RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
        SelectedPermissionOutcome::new(option_id.to_owned()),
    ))
}

fn cancelled() -> RequestPermissionResponse {
    RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)
}

pub(crate) fn validate_option(permission: &PendingPermission, option_id: &str) -> Result<()> {
    if permission
        .options
        .iter()
        .any(|option| option.id == option_id)
    {
        Ok(())
    } else {
        Err(SessionError::InvalidPermissionOption {
            request_id: permission.request_id.clone(),
            option_id: option_id.to_owned(),
        })
    }
}

impl Shared {
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
        match decide(
            &value,
            known_kind.as_deref(),
            &self.config.cwd,
            self.config.full_access,
        ) {
            PermissionDecision::Allow { option_id } => {
                if let Err(error) = responder.respond(selected(&option_id)) {
                    tracing::warn!(%error, "failed to auto-allow permission");
                }
            }
            PermissionDecision::Ask(prompt) => {
                state.next_permission += 1;
                let permission = PendingPermission {
                    request_id: format!("perm-{}", state.next_permission),
                    title: prompt.title,
                    kind: prompt.kind,
                    options: prompt.options,
                };
                let event = AgentEvent::permission(&permission);
                state.pending.push(Pending {
                    responder,
                    permission,
                });
                self.emit_locked(&mut state, event);
                self.sync_awaiting(&mut state);
            }
        }
    }

    pub fn respond(&self, request_id: &str, option_id: &str) -> Result<()> {
        let mut state = self.lock();
        let index = state
            .pending
            .iter()
            .position(|pending| pending.permission.request_id == request_id)
            .ok_or_else(|| SessionError::UnknownPermission(request_id.to_owned()))?;
        validate_option(&state.pending[index].permission, option_id)?;
        let pending = state.pending.remove(index);
        let result = pending
            .responder
            .respond(selected(option_id))
            .map_err(|error| SessionError::Acp(error.to_string()));
        self.sync_awaiting(&mut state);
        result
    }

    pub(super) fn cancel_pending_locked(&self, state: &mut State) {
        for pending in state.pending.drain(..) {
            let _ = pending.responder.respond(cancelled());
        }
        self.sync_awaiting(state);
    }

    pub fn cancel_pending(&self) {
        let mut state = self.lock();
        self.cancel_pending_locked(&mut state);
    }
}
