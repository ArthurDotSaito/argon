use super::{Shared, State};
use crate::session::events::{AgentEvent, PendingPermission};
use crate::session::permissions::{
    PermissionAnswer, PermissionAsk, PermissionDecision, PermissionReply, decide,
};
use crate::session::{Result, SessionError};

pub(super) struct Pending {
    pub reply: PermissionReply,
    pub permission: PendingPermission,
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
    pub fn on_permission(&self, ask: PermissionAsk, reply: PermissionReply) {
        let mut state = self.lock();
        match decide(&ask, &self.config.cwd, self.config.full_access) {
            PermissionDecision::Allow { option_id } => {
                if let Err(error) = reply(PermissionAnswer::Selected(option_id)) {
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
                state.pending.push(Pending { reply, permission });
                self.emit_locked(&mut state, event);
                self.sync_awaiting(&mut state);
            }
        }
    }

    pub fn known_kind(&self, tool_call_id: &str) -> Option<String> {
        self.lock().tool_kinds.get(tool_call_id).cloned()
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
        let result = (pending.reply)(PermissionAnswer::Selected(option_id.to_owned()))
            .map_err(SessionError::Acp);
        self.sync_awaiting(&mut state);
        result
    }

    pub(super) fn cancel_pending_locked(&self, state: &mut State) {
        for pending in state.pending.drain(..) {
            let _ = (pending.reply)(PermissionAnswer::Cancelled);
        }
        self.sync_awaiting(state);
    }

    pub fn cancel_pending(&self) {
        let mut state = self.lock();
        self.cancel_pending_locked(&mut state);
    }
}
