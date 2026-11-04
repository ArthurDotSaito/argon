use std::collections::HashSet;
use std::future::Future;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::opencode::protocol::split_model;
use crate::registry::ModelInfo;
use crate::session::{AgentEvent, PermissionAnswer, Result, SessionError};
use crate::util::json_text;

const END_TURN: &str = "end_turn";
const CANCELLED: &str = "cancelled";
const ALLOW_ALWAYS: &str = "allow_always";
const REJECT: &str = "reject_once";
const NOT_FOUND_PREFIX: &str = "HTTP 404";
const MAX_RESUBSCRIBE_FAILURES: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Close {
    Completed,
    Cancelled,
    Failed(String),
}

#[derive(Debug, Default)]
pub(super) struct Turns {
    current: Option<bool>,
}

impl Turns {
    pub fn in_turn(&self) -> bool {
        self.current.is_some()
    }

    pub fn is_cancelled(&self) -> bool {
        self.current == Some(true)
    }

    pub fn start(&mut self) {
        self.current = Some(false);
    }

    pub fn clear(&mut self) {
        self.current = None;
    }

    pub fn cancel(&mut self) -> bool {
        match &mut self.current {
            Some(cancelled) if !*cancelled => {
                *cancelled = true;
                true
            }
            _ => false,
        }
    }

    pub fn finish(&mut self, stop_reason: &str, error: Option<String>) -> Option<Close> {
        let cancelled = self.current.take()?;
        Some(if cancelled || stop_reason == CANCELLED {
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

pub(super) struct Family {
    root: String,
    members: HashSet<String>,
}

impl Family {
    pub fn new(root: String) -> Family {
        Family {
            members: HashSet::from([root.clone()]),
            root,
        }
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    pub fn contains(&self, id: &str) -> bool {
        self.members.contains(id)
    }

    pub fn insert(&mut self, id: String) -> bool {
        is_path_safe(&id) && self.members.insert(id)
    }

    pub fn reset(&mut self) {
        self.members = HashSet::from([self.root.clone()]);
    }

    pub fn members(&self) -> impl Iterator<Item = &str> {
        self.members.iter().map(String::as_str)
    }

    pub fn children(&self) -> impl Iterator<Item = &str> {
        self.members
            .iter()
            .map(String::as_str)
            .filter(|id| *id != self.root)
    }
}

pub(super) fn is_path_safe(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub(super) fn session_path(id: &str, action: &str) -> String {
    format!("/api/session/{id}/{action}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Route {
    pub session_id: String,
    pub request_id: String,
    pub kind: String,
}

impl Route {
    pub fn reply_path(&self) -> Option<String> {
        (is_path_safe(&self.session_id) && is_path_safe(&self.request_id)).then(|| {
            format!(
                "/api/session/{}/permission/{}/reply",
                self.session_id, self.request_id
            )
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Intake {
    Unanswerable,
    Refuse,
    Ask,
}

pub(super) fn permission_intake(route: &Route, turns: &Turns) -> Intake {
    if route.reply_path().is_none() {
        Intake::Unanswerable
    } else if turns.in_turn() && !turns.is_cancelled() {
        Intake::Ask
    } else {
        Intake::Refuse
    }
}

pub(super) fn decision(answer: &PermissionAnswer) -> &str {
    match answer {
        PermissionAnswer::Selected(option) => option,
        PermissionAnswer::Cancelled => REJECT,
    }
}

pub(super) fn grants_session(answer: &PermissionAnswer) -> bool {
    matches!(answer, PermissionAnswer::Selected(option) if option == ALLOW_ALWAYS)
}

pub(super) fn is_not_found(error: &str) -> bool {
    error.starts_with(NOT_FOUND_PREFIX)
}

pub(super) fn initial_model(model: &str) -> Option<String> {
    (!model.is_empty()).then(|| model.to_owned())
}

pub(super) fn model_ref(model: &str) -> std::result::Result<(&str, &str), String> {
    split_model(model).ok_or_else(|| format!("modelo `{model}` inválido; use provedor/modelo"))
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

pub(super) fn event_session(event: &Value) -> Option<String> {
    event
        .get("data")
        .and_then(|data| json_text(data, "sessionID"))
}

pub(super) fn without_child_plans(events: Vec<AgentEvent>, from_child: bool) -> Vec<AgentEvent> {
    if !from_child {
        return events;
    }
    events
        .into_iter()
        .filter(|event| !matches!(event, AgentEvent::Plan(_)))
        .collect()
}

pub(super) fn is_active(body: &Value, id: &str) -> bool {
    body.get("data").and_then(|data| data.get(id)).is_some()
}

pub(super) fn needs_restart(active: Option<&Value>, family: &Family) -> bool {
    match active {
        Some(body) => family.members().any(|id| is_active(body, id)),
        None => true,
    }
}

pub(super) async fn within<T>(
    limit: Duration,
    work: impl Future<Output = std::result::Result<T, String>>,
) -> std::result::Result<T, String> {
    match tokio::time::timeout(limit, work).await {
        Ok(result) => result,
        Err(_) => Err(format!("sem resposta em {}s", limit.as_secs())),
    }
}

pub(super) fn created_session(body: &Value) -> Option<(String, Option<String>)> {
    let data = body.get("data")?;
    let id = json_text(data, "id").filter(|id| is_path_safe(id))?;
    let directory = data
        .get("location")
        .and_then(|location| json_text(location, "directory"));
    Some((id, directory))
}

fn comparable(path: &str) -> String {
    let text = path.replace('\\', "/");
    let text = text.trim_end_matches('/');
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.to_owned()
    }
}

pub(super) fn same_directory(reported: &str, cwd: &Path) -> bool {
    match (dunce::canonicalize(reported), dunce::canonicalize(cwd)) {
        (Ok(reported), Ok(cwd)) => {
            comparable(&reported.display().to_string()) == comparable(&cwd.display().to_string())
        }
        _ => comparable(reported) == comparable(&cwd.display().to_string()),
    }
}

#[derive(Debug, Default)]
pub(super) struct Reconnect {
    failures: u32,
}

impl Reconnect {
    pub fn failed(&mut self) -> bool {
        self.failures += 1;
        self.failures >= MAX_RESUBSCRIBE_FAILURES
    }

    pub fn succeeded(&mut self) {
        self.failures = 0;
    }
}
