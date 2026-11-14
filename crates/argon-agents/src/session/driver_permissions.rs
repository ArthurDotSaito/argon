use argon_core::PermissionMode;

use super::{Shared, State};
use crate::session::events::{AgentEvent, PendingPermission};
use crate::session::permissions::{
    PermissionAnswer, PermissionAsk, PermissionDecision, PermissionReply, decide, prompt_title,
};
use crate::session::writer::Op;
use crate::session::{Result, SessionError};

const PLAN_DENIAL_NOTICE: &str = "Edição negada: Agent Plan é somente leitura.";
const TURN_ENDS_HINT: &str = " Mande outra mensagem para continuar.";

pub(super) struct Pending {
    pub reply: PermissionReply,
    pub ask: PermissionAsk,
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
        match decide(
            &ask,
            &self.config.cwd,
            state.permission_mode,
            self.config.kind,
        ) {
            PermissionDecision::Allow { option_id } => {
                if let Err(error) = reply(PermissionAnswer::Selected(option_id)) {
                    tracing::warn!(%error, "failed to auto-allow permission");
                }
            }
            PermissionDecision::Deny { option_id } => {
                if let Err(error) = reply(PermissionAnswer::Selected(option_id)) {
                    tracing::warn!(%error, "failed to auto-deny permission");
                }
                self.announce_denial(&mut state);
            }
            PermissionDecision::Ask(prompt) => {
                state.next_permission += 1;
                let permission = PendingPermission {
                    request_id: format!("perm-{}", state.next_permission),
                    title: prompt_title(&prompt.title, &prompt.kind),
                    kind: prompt.kind,
                    options: prompt.options,
                };
                let event = AgentEvent::permission(&permission);
                state.pending.push(Pending {
                    reply,
                    ask,
                    permission,
                });
                self.emit_locked(&mut state, event);
                self.sync_awaiting(&mut state);
            }
        }
    }

    fn announce_denial(&self, state: &mut State) {
        let notice = if self.config.denial_ends_turn {
            format!("{PLAN_DENIAL_NOTICE}{TURN_ENDS_HINT}")
        } else {
            PLAN_DENIAL_NOTICE.to_owned()
        };
        self.emit_locked(state, AgentEvent::Error(notice));
    }

    pub fn set_permission_mode(&self, mode: PermissionMode) {
        let mut state = self.lock();
        let changed = std::mem::replace(&mut state.permission_mode, mode) != mode;
        self.config.writer.push(Op::PermissionMode {
            session: self.config.id.clone(),
            mode,
        });
        if changed {
            self.emit_locked(&mut state, AgentEvent::PermissionModeChanged(mode));
        }
        for pending in std::mem::take(&mut state.pending) {
            match decide(&pending.ask, &self.config.cwd, mode, self.config.kind) {
                PermissionDecision::Allow { option_id } => {
                    if let Err(error) = (pending.reply)(PermissionAnswer::Selected(option_id)) {
                        tracing::warn!(%error, "failed to auto-allow pending permission");
                    }
                    let event = AgentEvent::permission_resolved(&pending.permission);
                    self.emit_locked(&mut state, event);
                }
                PermissionDecision::Deny { option_id } => {
                    if let Err(error) = (pending.reply)(PermissionAnswer::Selected(option_id)) {
                        tracing::warn!(%error, "failed to auto-deny pending permission");
                    }
                    let event = AgentEvent::permission_resolved(&pending.permission);
                    self.emit_locked(&mut state, event);
                    self.announce_denial(&mut state);
                }
                PermissionDecision::Ask(_) => state.pending.push(pending),
            }
        }
        self.sync_awaiting(&mut state);
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
            .map_err(SessionError::Agent);
        let event = AgentEvent::permission_resolved(&pending.permission);
        self.emit_locked(&mut state, event);
        self.sync_awaiting(&mut state);
        result
    }

    pub(super) fn cancel_pending_locked(&self, state: &mut State) {
        for pending in std::mem::take(&mut state.pending) {
            let _ = (pending.reply)(PermissionAnswer::Cancelled);
            let event = AgentEvent::permission_resolved(&pending.permission);
            self.emit_locked(state, event);
        }
        self.sync_awaiting(state);
    }

    pub fn cancel_pending(&self) {
        let mut state = self.lock();
        self.cancel_pending_locked(&mut state);
    }
}
