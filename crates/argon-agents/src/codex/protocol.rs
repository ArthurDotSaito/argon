use std::collections::HashMap;
use std::path::Path;

use argon_core::PermissionMode;
use serde_json::{Value, json};

use crate::registry::ModelInfo;
use crate::session::{AgentEvent, PermissionAnswer, PermissionAsk, ToolStatus, native_options_for};
use crate::util::json_text;

const OUTPUT_LIMIT: usize = 8000;
const COMPACT_TITLE_LIMIT: usize = 300;
const FULL_TITLE_LIMIT: usize = 2000;
const REASON_LIMIT: usize = 300;
const TITLE_SEPARATOR: &str = " · ";
const TITLE_ELLIPSIS: &str = " …";
const REASON_SEPARATOR: &str = " — ";
const TURN_FAILED_MESSAGE: &str = "falha no Codex";
const ALLOW_ONCE: &str = "allow_once";
const ALLOW_ALWAYS: &str = "allow_always";

const COMMAND_APPROVAL_METHODS: [&str; 2] = [
    "item/commandExecution/requestApproval",
    "execCommandApproval",
];
const FILE_APPROVAL_METHODS: [&str; 2] = ["item/fileChange/requestApproval", "applyPatchApproval"];
const PERMISSIONS_APPROVAL_METHOD: &str = "item/permissions/requestApproval";
const USER_INPUT_METHODS: [&str; 2] = ["item/tool/requestUserInput", "requestUserInput"];

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CodexEvent {
    Events(Vec<AgentEvent>),
    TurnStarted {
        turn_id: String,
    },
    TurnEnd {
        turn_id: String,
        stop_reason: String,
        error: Option<String>,
    },
    Ignore,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CodexRequest {
    Ask(PermissionAsk),
    Reply(Value),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReasoningKind {
    Summary,
    Text,
}

#[derive(Debug, Default)]
pub(crate) struct Decoder {
    reasoning: HashMap<String, ReasoningKind>,
    paths: HashMap<String, Vec<String>>,
    commands: HashMap<String, String>,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn notification(&mut self, method: &str, params: &Value) -> CodexEvent {
        match method {
            "turn/started" => turn_started(params),
            "item/agentMessage/delta" => chunk(params, AgentEvent::MessageChunk),
            "item/reasoning/summaryTextDelta" => self.reasoning(params, ReasoningKind::Summary),
            "item/reasoning/textDelta" => self.reasoning(params, ReasoningKind::Text),
            "item/started" => self.item_started(params),
            "item/completed" => self.item_completed(params),
            "turn/plan/updated" => plan(params),
            "turn/completed" => turn_completed(params),
            _ => CodexEvent::Ignore,
        }
    }

    pub fn request(&mut self, method: &str, params: &Value) -> CodexRequest {
        if COMMAND_APPROVAL_METHODS.contains(&method) {
            self.command_approval(params)
        } else if FILE_APPROVAL_METHODS.contains(&method) {
            self.file_approval(params)
        } else if method == PERMISSIONS_APPROVAL_METHOD {
            permissions_approval(params)
        } else if USER_INPUT_METHODS.contains(&method) {
            CodexRequest::Reply(json!({"answers": []}))
        } else {
            CodexRequest::Reply(json!({}))
        }
    }

    fn reasoning(&mut self, params: &Value, kind: ReasoningKind) -> CodexEvent {
        let Some(delta) = text_field(params, "delta") else {
            return CodexEvent::Ignore;
        };
        let item_id = json_text(params, "itemId").unwrap_or_default();
        if *self.reasoning.entry(item_id).or_insert(kind) != kind {
            return CodexEvent::Ignore;
        }
        single(AgentEvent::ThoughtChunk(delta))
    }

    fn item_started(&mut self, params: &Value) -> CodexEvent {
        let Some((item, kind, id)) = item_parts(params) else {
            return CodexEvent::Ignore;
        };
        match kind.as_str() {
            "commandExecution" => {
                let command = command_text(item.get("command")).unwrap_or_default();
                let title = format!("Bash: {}", sanitize(&command, FULL_TITLE_LIMIT));
                self.commands.insert(id.clone(), command);
                tool_call(id, "execute", title)
            }
            "fileChange" => {
                let title = list_title("Edit", &display_paths(item), COMPACT_TITLE_LIMIT);
                self.paths.insert(id.clone(), change_paths(item));
                tool_call(id, "edit", title)
            }
            "mcpToolCall" => {
                let server = json_text(item, "server").unwrap_or_default();
                let tool = json_text(item, "tool").unwrap_or_default();
                let title = sanitize(&format!("{server}.{tool}"), FULL_TITLE_LIMIT);
                tool_call(id, "other", title)
            }
            "webSearch" => {
                let query = text_field(item, "query").unwrap_or_default();
                let title = format!("Web search {}", sanitize(&query, FULL_TITLE_LIMIT));
                tool_call(id, "fetch", title.trim_end().to_owned())
            }
            _ => CodexEvent::Ignore,
        }
    }

    fn item_completed(&mut self, params: &Value) -> CodexEvent {
        let Some((item, kind, id)) = item_parts(params) else {
            return CodexEvent::Ignore;
        };
        match kind.as_str() {
            "commandExecution" => {
                self.commands.remove(&id);
                let output = text_field(item, "aggregatedOutput");
                tool_update(id, final_status(item), output)
            }
            "fileChange" => {
                self.paths.remove(&id);
                tool_update(id, final_status(item), None)
            }
            "mcpToolCall" => tool_update(id, final_status(item), mcp_output(item)),
            "webSearch" => tool_update(id, ToolStatus::Completed, None),
            "reasoning" => {
                self.reasoning.remove(&id);
                CodexEvent::Ignore
            }
            _ => CodexEvent::Ignore,
        }
    }

    fn command_approval(&self, params: &Value) -> CodexRequest {
        let item_id = approval_item_id(params);
        let command = command_text(params.get("command")).or_else(|| {
            item_id
                .as_ref()
                .and_then(|id| self.commands.get(id).cloned())
        });
        let title = match command {
            Some(command) => format!("Bash: {}", sanitize(&command, FULL_TITLE_LIMIT)),
            None => "Bash".to_owned(),
        };
        ask(item_id, "execute", with_reason(title, params), Vec::new())
    }

    fn file_approval(&self, params: &Value) -> CodexRequest {
        let item_id = approval_item_id(params);
        let paths = item_id
            .as_ref()
            .and_then(|id| self.paths.get(id).cloned())
            .unwrap_or_default();
        let title = list_title("Edit", &paths, COMPACT_TITLE_LIMIT);
        ask(item_id, "edit", with_reason(title, params), paths)
    }
}

pub(crate) fn approval_response(method: &str, params: &Value, answer: &PermissionAnswer) -> Value {
    if COMMAND_APPROVAL_METHODS.contains(&method) || FILE_APPROVAL_METHODS.contains(&method) {
        json!({"decision": decision(answer)})
    } else if method == PERMISSIONS_APPROVAL_METHOD {
        let permissions = if is_allowed(answer) {
            params.get("permissions").cloned().unwrap_or(json!({}))
        } else {
            json!({})
        };
        json!({"permissions": permissions, "scope": "turn"})
    } else {
        json!({})
    }
}

pub(crate) fn initialize_params(version: &str) -> Value {
    json!({
        "clientInfo": {"name": "argon", "title": "Argon", "version": version},
        "capabilities": {"experimentalApi": true, "requestAttestation": false},
    })
}

pub(crate) fn thread_params(cwd: &Path, model: Option<&str>, mode: PermissionMode) -> Value {
    let (approval_policy, sandbox) = match mode {
        PermissionMode::Supervised | PermissionMode::AcceptEdits => ("on-request", "read-only"),
        PermissionMode::FullAccess => ("never", "workspace-write"),
    };
    let mut params = json!({
        "cwd": cwd.to_string_lossy(),
        "approvalPolicy": approval_policy,
        "sandbox": sandbox,
    });
    set_optional(&mut params, "model", model);
    params
}

pub(crate) fn resume_params(
    thread_id: &str,
    cwd: &Path,
    model: Option<&str>,
    mode: PermissionMode,
) -> Value {
    let mut params = thread_params(cwd, model, mode);
    params["threadId"] = json!(thread_id);
    params
}

pub(crate) fn turn_params(
    thread_id: &str,
    text: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> Value {
    let mut params = json!({
        "threadId": thread_id,
        "input": [{"type": "text", "text": text}],
    });
    set_optional(&mut params, "model", model);
    set_optional(&mut params, "effort", effort);
    params
}

pub(crate) fn interrupt_params(thread_id: &str, turn_id: &str) -> Value {
    json!({"threadId": thread_id, "turnId": turn_id})
}

pub(crate) fn parse_models(result: &Value) -> Vec<ModelInfo> {
    model_items(result)
        .filter(|item| item.get("hidden").and_then(Value::as_bool) != Some(true))
        .filter_map(|item| {
            let id = text_field(item, "id")?;
            let label = text_field(item, "displayName").unwrap_or_else(|| id.clone());
            Some(ModelInfo { id, label })
        })
        .collect()
}

pub(crate) fn default_model(result: &Value) -> Option<String> {
    model_items(result)
        .find(|item| item.get("isDefault").and_then(Value::as_bool) == Some(true))
        .and_then(|item| text_field(item, "id"))
}

pub(crate) fn next_cursor(result: &Value) -> Option<String> {
    text_field(result, "nextCursor")
}

fn model_items(result: &Value) -> impl Iterator<Item = &Value> {
    result
        .get("data")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
}

fn set_optional(params: &mut Value, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        params[key] = json!(value);
    }
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    json_text(value, key).filter(|text| !text.is_empty())
}

fn single(event: AgentEvent) -> CodexEvent {
    CodexEvent::Events(vec![event])
}

fn chunk(params: &Value, build: fn(String) -> AgentEvent) -> CodexEvent {
    match text_field(params, "delta") {
        Some(delta) => single(build(delta)),
        None => CodexEvent::Ignore,
    }
}

fn turn_started(params: &Value) -> CodexEvent {
    match params.get("turn").and_then(|turn| text_field(turn, "id")) {
        Some(turn_id) => CodexEvent::TurnStarted { turn_id },
        None => CodexEvent::Ignore,
    }
}

fn turn_completed(params: &Value) -> CodexEvent {
    let Some(turn) = params.get("turn") else {
        return CodexEvent::Ignore;
    };
    let (Some(turn_id), Some(status)) = (text_field(turn, "id"), text_field(turn, "status")) else {
        return CodexEvent::Ignore;
    };
    let (stop_reason, error) = match status.as_str() {
        "completed" => ("end_turn", None),
        "interrupted" | "cancelled" => ("cancelled", None),
        "failed" => {
            let message = turn
                .get("error")
                .and_then(|error| text_field(error, "message"))
                .unwrap_or_else(|| TURN_FAILED_MESSAGE.to_owned());
            ("error", Some(message))
        }
        _ => ("error", Some(status)),
    };
    CodexEvent::TurnEnd {
        turn_id,
        stop_reason: stop_reason.to_owned(),
        error,
    }
}

fn plan(params: &Value) -> CodexEvent {
    let Some(steps) = params.get("plan").and_then(Value::as_array) else {
        return CodexEvent::Ignore;
    };
    single(AgentEvent::Plan(
        steps
            .iter()
            .filter_map(|step| text_field(step, "step"))
            .collect(),
    ))
}

fn item_parts(params: &Value) -> Option<(&Value, String, String)> {
    let item = params.get("item")?;
    Some((item, text_field(item, "type")?, text_field(item, "id")?))
}

fn tool_call(id: String, kind: &str, title: String) -> CodexEvent {
    single(AgentEvent::ToolCall {
        id,
        title,
        kind: kind.to_owned(),
        status: ToolStatus::InProgress,
    })
}

fn tool_update(id: String, status: ToolStatus, output: Option<String>) -> CodexEvent {
    single(AgentEvent::ToolCallUpdate {
        id,
        status: Some(status),
        output: output
            .map(|text| truncate(&text, OUTPUT_LIMIT))
            .filter(|text| !text.is_empty()),
        title: None,
    })
}

fn final_status(item: &Value) -> ToolStatus {
    match item.get("status").and_then(Value::as_str) {
        Some("failed" | "declined") => ToolStatus::Failed,
        _ => ToolStatus::Completed,
    }
}

fn mcp_output(item: &Value) -> Option<String> {
    match item.get("result")? {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

fn command_text(command: Option<&Value>) -> Option<String> {
    let text = match command? {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" "),
        _ => return None,
    };
    Some(text).filter(|text| !text.trim().is_empty())
}

fn changes(item: &Value) -> &[Value] {
    item.get("changes")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn display_paths(item: &Value) -> Vec<String> {
    changes(item)
        .iter()
        .filter_map(|change| text_field(change, "path"))
        .collect()
}

fn change_paths(item: &Value) -> Vec<String> {
    let paths = display_paths(item);
    if paths.len() == changes(item).len() && paths.iter().all(|path| !path.trim().is_empty()) {
        paths
    } else {
        Vec::new()
    }
}

fn approval_item_id(params: &Value) -> Option<String> {
    text_field(params, "itemId").or_else(|| text_field(params, "callId"))
}

fn ask(
    tool_call_id: Option<String>,
    kind: &str,
    title: String,
    paths: Vec<String>,
) -> CodexRequest {
    CodexRequest::Ask(PermissionAsk {
        tool_call_id,
        kind: kind.to_owned(),
        title,
        paths,
        options: native_options_for(kind),
    })
}

fn permissions_approval(params: &Value) -> CodexRequest {
    let title = match params.get("permissions") {
        Some(permissions) if !permissions.is_null() => format!(
            "Permissões: {}",
            truncated(&permissions.to_string(), FULL_TITLE_LIMIT)
        ),
        _ => "Permissões".to_owned(),
    };
    ask(
        approval_item_id(params),
        "other",
        with_reason(title, params),
        Vec::new(),
    )
}

fn with_reason(title: String, params: &Value) -> String {
    match text_field(params, "reason").filter(|reason| !reason.trim().is_empty()) {
        Some(reason) => format!(
            "{title}{REASON_SEPARATOR}{}",
            sanitize(&reason, REASON_LIMIT)
        ),
        None => title,
    }
}

fn is_allowed(answer: &PermissionAnswer) -> bool {
    matches!(answer, PermissionAnswer::Selected(id) if id == ALLOW_ONCE || id == ALLOW_ALWAYS)
}

fn decision(answer: &PermissionAnswer) -> &'static str {
    match answer {
        PermissionAnswer::Cancelled => "cancel",
        answer if is_allowed(answer) => "accept",
        _ => "decline",
    }
}

fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn truncated(text: &str, limit: usize) -> String {
    let mut shown = sanitize(text, limit);
    if text.chars().count() > limit {
        shown.push_str(TITLE_ELLIPSIS);
    }
    shown
}

fn sanitize(text: &str, limit: usize) -> String {
    text.chars()
        .take(limit)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn list_title(prefix: &str, items: &[String], limit: usize) -> String {
    let mut title = prefix.to_owned();
    let mut used = 0;
    for (index, item) in items.iter().enumerate() {
        let separator = if index == 0 { " " } else { TITLE_SEPARATOR };
        let separator_length = separator.chars().count();
        let available = limit.saturating_sub(used + separator_length);
        if available == 0 {
            title.push_str(&format!("{TITLE_ELLIPSIS} (+{})", items.len() - index));
            return title;
        }
        title.push_str(separator);
        let text = sanitize(item, usize::MAX);
        let length = text.chars().count();
        if length > available {
            title.push_str(&truncate(&text, available));
            title.push_str(TITLE_ELLIPSIS);
            let hidden = items.len() - index - 1;
            if hidden > 0 {
                title.push_str(&format!(" (+{hidden})"));
            }
            return title;
        }
        title.push_str(&text);
        used += separator_length + length;
    }
    title
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
