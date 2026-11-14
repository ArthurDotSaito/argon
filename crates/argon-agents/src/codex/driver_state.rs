use std::collections::HashSet;
use std::path::{Path, PathBuf};

use argon_core::{PermissionMode, SessionKind};
use serde_json::{Value, json};

use crate::codex::protocol::next_cursor;
use crate::registry::ModelInfo;
use crate::session::{
    ALLOW_ALWAYS, PermissionAnswer, PermissionAsk, PermissionDecision, Result, SessionError,
    decide_permission, session_grant_applies,
};

const END_TURN: &str = "end_turn";
const CANCELLED: &str = "cancelled";
const ARCHIVED_MARKER: &str = "archived";
const MAX_MODELS: usize = 500;
const MAX_MODEL_PAGES: usize = 50;
const SANDBOX_READY: &str = "ready";
const GITDIR_PREFIX: &str = "gitdir:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Close {
    Completed,
    Cancelled,
    Failed(String),
}

#[derive(Debug)]
struct Turn {
    id: String,
    cancelled: bool,
}

#[derive(Debug, Default)]
pub(super) struct Turns {
    current: Option<Turn>,
}

impl Turns {
    pub fn in_turn(&self) -> bool {
        self.current.is_some()
    }

    pub fn is_cancelled(&self) -> bool {
        self.current.as_ref().is_some_and(|turn| turn.cancelled)
    }

    pub fn current_id(&self) -> Option<&str> {
        self.current.as_ref().map(|turn| turn.id.as_str())
    }

    pub fn start(&mut self, id: String) {
        self.current = Some(Turn {
            id,
            cancelled: false,
        });
    }

    pub fn owns(&self, turn_id: Option<&str>) -> bool {
        match (&self.current, turn_id) {
            (Some(turn), Some(id)) => turn.id == id,
            (Some(_), None) => true,
            (None, _) => false,
        }
    }

    pub fn accepts_request(&self, turn_id: Option<&str>) -> bool {
        self.owns(turn_id) && !self.is_cancelled()
    }

    pub fn cancel(&mut self) -> bool {
        match &mut self.current {
            Some(turn) if !turn.cancelled => {
                turn.cancelled = true;
                true
            }
            _ => false,
        }
    }

    pub fn finish(
        &mut self,
        turn_id: &str,
        stop_reason: &str,
        error: Option<String>,
    ) -> Option<Close> {
        if !self.owns(Some(turn_id)) {
            return None;
        }
        let turn = self.current.take()?;
        Some(if turn.cancelled || stop_reason == CANCELLED {
            Close::Cancelled
        } else if stop_reason == END_TURN {
            Close::Completed
        } else {
            Close::Failed(error.unwrap_or_else(|| stop_reason.to_owned()))
        })
    }

    pub fn expire(&mut self) -> Option<Close> {
        if !self.is_cancelled() {
            return None;
        }
        self.current = None;
        Some(Close::Cancelled)
    }
}

pub(super) fn auto_allowed(
    ask: &PermissionAsk,
    cwd: &Path,
    mode: PermissionMode,
    kind: SessionKind,
    grants: &HashSet<String>,
) -> bool {
    match decide_permission(ask, cwd, mode, kind) {
        PermissionDecision::Allow { .. } => true,
        PermissionDecision::Deny { .. } => false,
        PermissionDecision::Ask(_) => grants.contains(&ask.kind) && session_grant_applies(ask, cwd),
    }
}

pub(super) fn grants_session(answer: &PermissionAnswer) -> bool {
    matches!(answer, PermissionAnswer::Selected(option) if option == ALLOW_ALWAYS)
}

pub(super) fn check_model(models: &[ModelInfo], model: &str) -> Result<()> {
    if models.is_empty() || models.iter().any(|known| known.id == model) {
        Ok(())
    } else {
        Err(SessionError::ModelUnavailable {
            model: model.to_owned(),
        })
    }
}

pub(super) fn is_archived(error: &str) -> bool {
    error.to_lowercase().contains(ARCHIVED_MARKER)
}

pub(super) fn exit_message(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("o Codex encerrou: o processo saiu (código {code})"),
        None => "o Codex encerrou: a conexão foi fechada".to_owned(),
    }
}

#[derive(Debug, Default)]
pub(super) struct ModelPages {
    items: Vec<Value>,
    cursors: HashSet<String>,
    pages: usize,
}

impl ModelPages {
    pub fn add(&mut self, page: &Value) -> Option<String> {
        self.pages += 1;
        let room = MAX_MODELS.saturating_sub(self.items.len());
        let items = page
            .get("data")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        self.items.extend(items.iter().take(room).cloned());
        let cursor = next_cursor(page)?;
        let more = self.items.len() < MAX_MODELS
            && self.pages < MAX_MODEL_PAGES
            && self.cursors.insert(cursor.clone());
        more.then_some(cursor)
    }

    pub fn result(&self) -> Value {
        json!({"data": self.items})
    }
}

pub(super) type Sandbox = std::result::Result<(), String>;

pub(super) fn readiness(result: std::result::Result<Value, String>) -> Sandbox {
    let result = result?;
    match result.get("status").and_then(Value::as_str) {
        Some(SANDBOX_READY) => Ok(()),
        Some(status) => Err(status.to_owned()),
        None => Err(result.to_string()),
    }
}

pub(super) fn full_access_blocked(mode: PermissionMode, sandbox: &Sandbox) -> Option<&str> {
    match (mode, sandbox) {
        (PermissionMode::FullAccess, Err(status)) => Some(status),
        _ => None,
    }
}

pub(super) fn effective_mode(requested: PermissionMode, sandbox: &Sandbox) -> PermissionMode {
    if full_access_blocked(requested, sandbox).is_some() {
        PermissionMode::Supervised
    } else {
        requested
    }
}

pub(super) fn sandbox_warning(requested: PermissionMode, sandbox: &Sandbox) -> Option<String> {
    full_access_blocked(requested, sandbox).map(|status| {
        format!(
            "a sandbox do Codex no Windows não está pronta ({status}); a sessão abriu em Supervisionado — Acesso total fica indisponível"
        )
    })
}

pub(super) fn mode_refusal(mode: PermissionMode, sandbox: &Sandbox) -> Option<SessionError> {
    full_access_blocked(mode, sandbox).map(|status| SessionError::SetPermissionMode {
        mode: mode.as_str().to_owned(),
        reason: format!(
            "a sandbox do Codex no Windows não está pronta ({status}); Acesso total fica indisponível nesta sessão"
        ),
    })
}

pub(super) const LEFT_FULL_ACCESS: &str = "o turno do Codex foi interrompido para sair do Acesso total; mande outra mensagem para continuar no modo mais restrito";

fn breadth(mode: PermissionMode) -> u8 {
    match mode {
        PermissionMode::Supervised => 0,
        PermissionMode::AcceptEdits => 1,
        PermissionMode::FullAccess => 2,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModeChange {
    ApplyNow(PermissionMode),
    AtTurnEnd(PermissionMode),
    InterruptTurn(PermissionMode),
}

#[derive(Debug, Default)]
pub(super) struct ModeSwitch {
    pending: Option<PermissionMode>,
}

impl ModeSwitch {
    pub fn request(
        &mut self,
        mode: PermissionMode,
        current: PermissionMode,
        in_turn: bool,
    ) -> ModeChange {
        if in_turn && breadth(mode) > breadth(current) {
            self.pending = Some(mode);
            return ModeChange::AtTurnEnd(mode);
        }
        self.pending = None;
        if in_turn && current == PermissionMode::FullAccess && mode != current {
            ModeChange::InterruptTurn(mode)
        } else {
            ModeChange::ApplyNow(mode)
        }
    }

    pub fn take(&mut self) -> Option<PermissionMode> {
        self.pending.take()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ModelChoice {
    pub shown: String,
    pub sent: Option<String>,
}

pub(super) fn pick_model(
    chosen: Option<&str>,
    reported: Option<&str>,
    default: Option<&str>,
) -> ModelChoice {
    let pick = |model: &str, sent: bool| ModelChoice {
        shown: model.to_owned(),
        sent: sent.then(|| model.to_owned()),
    };
    match (chosen, reported, default) {
        (Some(model), _, _) => pick(model, true),
        (None, Some(model), _) => pick(model, false),
        (None, None, Some(model)) => pick(model, true),
        (None, None, None) => ModelChoice {
            shown: String::new(),
            sent: None,
        },
    }
}

pub(super) fn gitdir_target(gitfile: &str, root: &Path) -> Option<PathBuf> {
    let target = gitfile.lines().next()?.strip_prefix(GITDIR_PREFIX)?.trim();
    if target.is_empty() {
        return None;
    }
    Some(root.join(target))
}

pub(super) fn common_dir(gitdir: &Path, commondir: &str) -> Option<PathBuf> {
    let target = commondir.trim();
    (!target.is_empty()).then(|| gitdir.join(target))
}

pub(super) fn project_roots(cwd: &Path, is_root: impl Fn(&Path) -> bool) -> Vec<&Path> {
    let mut roots = Vec::new();
    for dir in cwd.ancestors() {
        roots.push(dir);
        if is_root(dir) {
            break;
        }
    }
    roots
}

pub(super) fn trust_keys(roots: &[PathBuf]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for root in roots {
        let text = root.display().to_string();
        let lower = cfg!(windows).then(|| text.to_lowercase());
        for key in std::iter::once(text).chain(lower) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    keys
}
