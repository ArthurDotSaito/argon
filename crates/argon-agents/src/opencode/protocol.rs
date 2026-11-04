use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::registry::ModelInfo;
use crate::session::{AgentEvent, PermissionAsk, ToolStatus, native_options_for};
use crate::util::json_text;

const OUTPUT_LIMIT: usize = 8000;
const SUBJECT_LIMIT: usize = 120;
const SUBJECT_KEYS: [&str; 6] = ["filePath", "path", "pattern", "command", "url", "query"];
const COMMAND_KEY: &str = "command";
const TODO_WRITE: &str = "todowrite";
const PATH_KINDS: [&str; 2] = ["read", "edit"];
const COMPACT_TITLE_KINDS: [&str; 3] = ["read", "search", "edit"];
const COMPACT_TITLE_LIMIT: usize = 300;
const FULL_TITLE_LIMIT: usize = 2000;
const TITLE_SEPARATOR: &str = " · ";
const TITLE_ELLIPSIS: &str = " …";
const ROOT_ONLY_PREFIX: &str = "session.execution.";
const PERMISSION_ID_KEYS: [&str; 3] = ["id", "requestID", "requestId"];
const PERMISSION_ACTION_KEYS: [&str; 3] = ["permission", "action", "type"];
const PERMISSION_RESOURCE_KEYS: [&str; 3] = ["patterns", "resources", "pattern"];
const CREATED_ID_KEYS: [&str; 2] = ["sessionID", "id"];
const DEFAULT_PERMISSION_ACTION: &str = "permission";
const EXECUTION_FAILED_MESSAGE: &str = "falha no OpenCode";
const ALLOW_ONCE: &str = "allow_once";
const ALLOW_ALWAYS: &str = "allow_always";

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OpencodeEvent {
    Events(Vec<AgentEvent>),
    Permission {
        id: String,
        session_id: String,
        ask: PermissionAsk,
    },
    TurnEnd {
        stop_reason: String,
        error: Option<String>,
    },
    SessionCreated {
        id: String,
        parent_id: String,
    },
    Ignore,
}

pub(crate) fn decode(
    event: &Value,
    root_id: &str,
    is_ours: &dyn Fn(&str) -> bool,
) -> OpencodeEvent {
    let Some(data) = event.get("data") else {
        return OpencodeEvent::Ignore;
    };
    let kind = event.get("type").and_then(Value::as_str);
    if kind == Some("session.created") {
        return session_created(data, is_ours);
    }
    let Some(session_id) = json_text(data, "sessionID").filter(|id| is_ours(id)) else {
        return OpencodeEvent::Ignore;
    };
    let is_execution = kind.is_some_and(|kind| kind.starts_with(ROOT_ONLY_PREFIX));
    if is_execution && session_id != root_id {
        return OpencodeEvent::Ignore;
    }
    match kind {
        Some("session.text.delta") => chunk(data, AgentEvent::MessageChunk),
        Some("session.reasoning.delta") => chunk(data, AgentEvent::ThoughtChunk),
        Some("session.tool.input.started") => tool_started(data),
        Some("session.tool.called") => tool_called(data),
        Some("session.tool.success") => tool_success(data),
        Some("session.tool.failed") => tool_failed(data),
        Some("session.execution.succeeded") => turn_end("end_turn", None),
        Some("session.execution.failed") => execution_failed(data),
        Some("session.execution.interrupted") => turn_end("cancelled", None),
        Some("permission.asked") => permission_asked(data, session_id),
        _ => OpencodeEvent::Ignore,
    }
}

pub(crate) fn kind_for(name: &str) -> &'static str {
    match name.to_ascii_lowercase().as_str() {
        "read" | "list" => "read",
        "glob" | "grep" => "search",
        "edit" | "write" | "patch" | "multiedit" => "edit",
        "bash" | "shell" => "execute",
        "webfetch" | "websearch" | "codesearch" => "fetch",
        _ => "other",
    }
}

pub(crate) const MODELS_PATH: &str = "/api/model";

pub(crate) fn parse_models(body: &Value) -> Vec<ModelInfo> {
    let mut models = BTreeMap::new();
    let items = body
        .get("data")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for item in items {
        if item.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let (Some(provider_id), Some(model_id)) = (
            json_text(item, "providerID").filter(|id| !id.is_empty()),
            json_text(item, "id").filter(|id| !id.is_empty()),
        ) else {
            continue;
        };
        let id = format!("{provider_id}/{model_id}");
        let label = json_text(item, "name")
            .filter(|name| !name.is_empty())
            .map(|name| format!("{name} · {}", provider_label(&provider_id)))
            .unwrap_or_else(|| id.clone());
        models
            .entry((provider_id != PLAN_PROVIDER, id.clone()))
            .or_insert_with(|| ModelInfo { id, label });
    }
    models.into_values().collect()
}

const PLAN_PROVIDER: &str = "opencode-go";

fn provider_label(provider_id: &str) -> &str {
    match provider_id {
        PLAN_PROVIDER => "OpenCode Go",
        "opencode" => "OpenCode Zen",
        other => other,
    }
}

pub(crate) fn split_model(id: &str) -> Option<(&str, &str)> {
    id.split_once('/')
        .filter(|(provider_id, model_id)| !provider_id.is_empty() && !model_id.is_empty())
}

pub(crate) fn create_session_body(directory: &str) -> Value {
    json!({"location": {"directory": directory}})
}

pub(crate) fn prompt_body(text: &str) -> Value {
    json!({"text": text})
}

pub(crate) fn model_body(provider_id: &str, model_id: &str, variant: Option<&str>) -> Value {
    let mut model = json!({"providerID": provider_id, "id": model_id});
    if let Some(variant) = variant.filter(|variant| !variant.is_empty()) {
        model["variant"] = json!(variant);
    }
    json!({"model": model})
}

pub(crate) fn permission_body(answer: &str) -> Value {
    match answer {
        ALLOW_ONCE | ALLOW_ALWAYS => json!({"decision": "once"}),
        _ => json!({"decision": "reject"}),
    }
}

fn session_created(data: &Value, is_ours: &dyn Fn(&str) -> bool) -> OpencodeEvent {
    let parent_id = json_text(data, "parentID").filter(|parent| is_ours(parent));
    match (first_text(data, &CREATED_ID_KEYS), parent_id) {
        (Some(id), Some(parent_id)) => OpencodeEvent::SessionCreated { id, parent_id },
        _ => OpencodeEvent::Ignore,
    }
}

fn chunk(data: &Value, build: fn(String) -> AgentEvent) -> OpencodeEvent {
    match json_text(data, "delta") {
        Some(delta) => OpencodeEvent::Events(vec![build(delta)]),
        None => OpencodeEvent::Ignore,
    }
}

fn single(event: AgentEvent) -> OpencodeEvent {
    OpencodeEvent::Events(vec![event])
}

fn tool_id(data: &Value) -> Option<String> {
    json_text(data, "id").filter(|id| !id.is_empty())
}

fn tool_started(data: &Value) -> OpencodeEvent {
    let Some(id) = tool_id(data) else {
        return OpencodeEvent::Ignore;
    };
    let name = json_text(data, "name").unwrap_or_default();
    single(AgentEvent::ToolCall {
        id,
        kind: kind_for(&name).to_owned(),
        title: sanitize(&name, SUBJECT_LIMIT),
        status: ToolStatus::Pending,
    })
}

fn tool_called(data: &Value) -> OpencodeEvent {
    let name = json_text(data, "name").unwrap_or_default();
    let input = data.get("input").unwrap_or(&Value::Null);
    if name.eq_ignore_ascii_case(TODO_WRITE) {
        return single(AgentEvent::Plan(todo_items(input)));
    }
    let Some(id) = tool_id(data) else {
        return OpencodeEvent::Ignore;
    };
    single(AgentEvent::ToolCallUpdate {
        id,
        status: Some(ToolStatus::InProgress),
        output: None,
        title: Some(tool_title(&name, input)),
    })
}

fn todo_items(input: &Value) -> Vec<String> {
    input
        .get("todos")
        .and_then(Value::as_array)
        .map(|todos| {
            todos
                .iter()
                .filter_map(|todo| json_text(todo, "content"))
                .collect()
        })
        .unwrap_or_default()
}

fn tool_title(name: &str, input: &Value) -> String {
    let name = sanitize(name, SUBJECT_LIMIT);
    let subject = SUBJECT_KEYS.iter().find_map(|key| {
        json_text(input, key)
            .filter(|text| !text.is_empty())
            .map(|text| (*key, text))
    });
    match subject {
        Some((COMMAND_KEY, text)) => format!("{name}: {}", sanitize(&text, SUBJECT_LIMIT)),
        Some((_, text)) => format!("{name} {}", sanitize(&text, SUBJECT_LIMIT)),
        None => name,
    }
}

fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn sanitize(text: &str, limit: usize) -> String {
    text.chars()
        .take(limit)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn tool_success(data: &Value) -> OpencodeEvent {
    let Some(id) = tool_id(data) else {
        return OpencodeEvent::Ignore;
    };
    let text = data
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|item| json_text(item, "text"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    single(AgentEvent::ToolCallUpdate {
        id,
        status: Some(ToolStatus::Completed),
        output: Some(truncate(&text, OUTPUT_LIMIT)).filter(|text| !text.is_empty()),
        title: None,
    })
}

fn tool_failed(data: &Value) -> OpencodeEvent {
    let Some(id) = tool_id(data) else {
        return OpencodeEvent::Ignore;
    };
    let message = data
        .get("error")
        .and_then(|error| json_text(error, "message"))
        .map(|message| truncate(&message, OUTPUT_LIMIT))
        .filter(|message| !message.is_empty());
    single(AgentEvent::ToolCallUpdate {
        id,
        status: Some(ToolStatus::Failed),
        output: message,
        title: None,
    })
}

fn turn_end(stop_reason: &str, error: Option<String>) -> OpencodeEvent {
    OpencodeEvent::TurnEnd {
        stop_reason: stop_reason.to_owned(),
        error,
    }
}

fn execution_failed(data: &Value) -> OpencodeEvent {
    let error = data.get("error");
    let message = error
        .and_then(|error| json_text(error, "message").or_else(|| json_text(error, "type")))
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| EXECUTION_FAILED_MESSAGE.to_owned());
    turn_end("error", Some(message))
}

fn first_text(data: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| json_text(data, key).filter(|text| !text.is_empty()))
}

fn resources(data: &Value) -> Vec<Value> {
    PERMISSION_RESOURCE_KEYS
        .iter()
        .filter_map(|key| match data.get(key)? {
            Value::Array(items) => Some(items.clone()),
            single @ Value::String(_) => Some(vec![single.clone()]),
            _ => None,
        })
        .find(|found| !found.is_empty())
        .unwrap_or_default()
}

fn resource_text(resource: &Value) -> String {
    match resource {
        Value::String(text) => sanitize(text, usize::MAX),
        other => sanitize(&other.to_string(), usize::MAX),
    }
}

fn trusted_paths(resources: &[Value]) -> Vec<String> {
    resources
        .iter()
        .map(|resource| resource.as_str().filter(|text| !text.trim().is_empty()))
        .collect::<Option<Vec<_>>>()
        .map(|paths| paths.into_iter().map(str::to_owned).collect())
        .unwrap_or_default()
}

fn permission_tool_call_id(data: &Value) -> Option<String> {
    ["tool", "source"]
        .iter()
        .find_map(|key| data.get(key).and_then(|owner| json_text(owner, "callID")))
}

fn title_limit(kind: &str) -> usize {
    if COMPACT_TITLE_KINDS.contains(&kind) {
        COMPACT_TITLE_LIMIT
    } else {
        FULL_TITLE_LIMIT
    }
}

fn permission_title(action: &str, resources: &[String], kind: &str) -> String {
    let limit = title_limit(kind);
    let separator_length = TITLE_SEPARATOR.chars().count();
    let mut title = sanitize(action, SUBJECT_LIMIT);
    let mut used = 0;
    for (index, resource) in resources.iter().enumerate() {
        let available = limit.saturating_sub(used + separator_length);
        if available == 0 {
            title.push_str(&format!("{TITLE_ELLIPSIS} (+{})", resources.len() - index));
            return title;
        }
        title.push_str(TITLE_SEPARATOR);
        let length = resource.chars().count();
        if length > available {
            title.push_str(&truncate(resource, available));
            title.push_str(TITLE_ELLIPSIS);
            let hidden = resources.len() - index - 1;
            if hidden > 0 {
                title.push_str(&format!(" (+{hidden})"));
            }
            return title;
        }
        title.push_str(resource);
        used += length + separator_length;
    }
    title
}

fn permission_asked(data: &Value, session_id: String) -> OpencodeEvent {
    let Some(id) = first_text(data, &PERMISSION_ID_KEYS) else {
        return OpencodeEvent::Ignore;
    };
    let action = first_text(data, &PERMISSION_ACTION_KEYS)
        .unwrap_or_else(|| DEFAULT_PERMISSION_ACTION.to_owned());
    let kind = kind_for(&action);
    let resources = resources(data);
    let texts: Vec<String> = resources.iter().map(resource_text).collect();
    OpencodeEvent::Permission {
        id,
        session_id,
        ask: PermissionAsk {
            tool_call_id: permission_tool_call_id(data),
            kind: kind.to_owned(),
            title: permission_title(&action, &texts, kind),
            paths: if PATH_KINDS.contains(&kind) {
                trusted_paths(&resources)
            } else {
                Vec::new()
            },
            options: native_options_for(kind),
        },
    }
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
