use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::registry::ModelInfo;
use crate::session::{AgentEvent, PermissionAsk, ToolStatus, native_options_for};
use crate::util::json_text;

const OUTPUT_LIMIT: usize = 8000;
const SUBJECT_LIMIT: usize = 120;
const SUBJECT_KEYS: [&str; 6] = ["filePath", "path", "pattern", "command", "url", "query"];
const COMMAND_KEY: &str = "command";
const LINE_BREAKS: [char; 2] = ['\r', '\n'];
const TODO_WRITE: &str = "todowrite";
const PATH_KINDS: [&str; 3] = ["read", "search", "edit"];
const PERMISSION_ID_KEYS: [&str; 3] = ["id", "requestID", "requestId"];
const PERMISSION_ACTION_KEYS: [&str; 3] = ["permission", "action", "type"];
const PERMISSION_RESOURCE_KEYS: [&str; 3] = ["patterns", "resources", "pattern"];
const DEFAULT_PERMISSION_ACTION: &str = "permission";
const EXECUTION_FAILED_MESSAGE: &str = "falha no OpenCode";
const ALLOW_ONCE: &str = "allow_once";
const ALLOW_ALWAYS: &str = "allow_always";

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OpencodeEvent {
    Events(Vec<AgentEvent>),
    Permission {
        id: String,
        ask: PermissionAsk,
    },
    TurnEnd {
        stop_reason: String,
        error: Option<String>,
    },
    Ignore,
}

pub(crate) fn decode(event: &Value, session_id: &str) -> OpencodeEvent {
    let Some(data) = event.get("data") else {
        return OpencodeEvent::Ignore;
    };
    if json_text(data, "sessionID").as_deref() != Some(session_id) {
        return OpencodeEvent::Ignore;
    }
    match event.get("type").and_then(Value::as_str) {
        Some("session.text.delta") => chunk(data, AgentEvent::MessageChunk),
        Some("session.reasoning.delta") => chunk(data, AgentEvent::ThoughtChunk),
        Some("session.tool.input.started") => tool_started(data),
        Some("session.tool.called") => tool_called(data),
        Some("session.tool.success") => tool_success(data),
        Some("session.tool.failed") => tool_failed(data),
        Some("session.execution.succeeded") => turn_end("end_turn", None),
        Some("session.execution.failed") => execution_failed(data),
        Some("session.execution.interrupted") => turn_end("cancelled", None),
        Some("permission.asked") => permission_asked(data),
        _ => OpencodeEvent::Ignore,
    }
}

pub(crate) fn kind_for(name: &str) -> &'static str {
    match name.to_ascii_lowercase().as_str() {
        "read" | "list" => "read",
        "glob" | "grep" | "codesearch" => "search",
        "edit" | "write" | "patch" | "multiedit" => "edit",
        "bash" | "shell" => "execute",
        "webfetch" | "websearch" => "fetch",
        _ => "other",
    }
}

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
        let (Some(provider_id), Some(model_id)) =
            (json_text(item, "providerID"), json_text(item, "id"))
        else {
            continue;
        };
        let id = format!("{provider_id}/{model_id}");
        let label = json_text(item, "name")
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| id.clone());
        models
            .entry(id.clone())
            .or_insert_with(|| ModelInfo { id, label });
    }
    models.into_values().collect()
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
    if let Some(variant) = variant {
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

fn chunk(data: &Value, build: fn(String) -> AgentEvent) -> OpencodeEvent {
    match json_text(data, "delta") {
        Some(delta) => OpencodeEvent::Events(vec![build(delta)]),
        None => OpencodeEvent::Ignore,
    }
}

fn single(event: AgentEvent) -> OpencodeEvent {
    OpencodeEvent::Events(vec![event])
}

fn tool_started(data: &Value) -> OpencodeEvent {
    let name = json_text(data, "name").unwrap_or_default();
    single(AgentEvent::ToolCall {
        id: json_text(data, "id").unwrap_or_default(),
        kind: kind_for(&name).to_owned(),
        title: name,
        status: ToolStatus::Pending,
    })
}

fn tool_called(data: &Value) -> OpencodeEvent {
    let name = json_text(data, "name").unwrap_or_default();
    let input = data.get("input").unwrap_or(&Value::Null);
    if name.eq_ignore_ascii_case(TODO_WRITE) {
        return single(AgentEvent::Plan(todo_items(input)));
    }
    single(AgentEvent::ToolCallUpdate {
        id: json_text(data, "id").unwrap_or_default(),
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
    let subject = SUBJECT_KEYS.iter().find_map(|key| {
        json_text(input, key)
            .filter(|text| !text.is_empty())
            .map(|text| (*key, text))
    });
    let title = match subject {
        Some((COMMAND_KEY, text)) => format!("{name}: {}", truncate(&text, SUBJECT_LIMIT)),
        Some((_, text)) => format!("{name} {}", truncate(&text, SUBJECT_LIMIT)),
        None => name.to_owned(),
    };
    title.replace(LINE_BREAKS, " ")
}

fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn tool_success(data: &Value) -> OpencodeEvent {
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
        id: json_text(data, "id").unwrap_or_default(),
        status: Some(ToolStatus::Completed),
        output: Some(truncate(&text, OUTPUT_LIMIT)).filter(|text| !text.is_empty()),
        title: None,
    })
}

fn tool_failed(data: &Value) -> OpencodeEvent {
    let message = data
        .get("error")
        .and_then(|error| json_text(error, "message"))
        .map(|message| truncate(&message, OUTPUT_LIMIT))
        .filter(|message| !message.is_empty());
    single(AgentEvent::ToolCallUpdate {
        id: json_text(data, "id").unwrap_or_default(),
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

fn resources(data: &Value) -> Vec<String> {
    PERMISSION_RESOURCE_KEYS
        .iter()
        .filter_map(|key| match data.get(key)? {
            Value::String(text) => Some(vec![text.clone()]),
            Value::Array(items) => Some(
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect(),
            ),
            _ => None,
        })
        .find(|found| !found.is_empty())
        .unwrap_or_default()
}

fn permission_tool_call_id(data: &Value) -> Option<String> {
    ["tool", "source"]
        .iter()
        .find_map(|key| data.get(key).and_then(|owner| json_text(owner, "callID")))
}

fn permission_title(action: &str, resources: &[String]) -> String {
    match resources.first() {
        Some(resource) => format!("{action} {}", truncate(resource, SUBJECT_LIMIT)),
        None => action.to_owned(),
    }
    .replace(LINE_BREAKS, " ")
}

fn permission_asked(data: &Value) -> OpencodeEvent {
    let Some(id) = first_text(data, &PERMISSION_ID_KEYS) else {
        return OpencodeEvent::Ignore;
    };
    let action = first_text(data, &PERMISSION_ACTION_KEYS)
        .unwrap_or_else(|| DEFAULT_PERMISSION_ACTION.to_owned());
    let kind = kind_for(&action);
    let resources = resources(data);
    OpencodeEvent::Permission {
        id,
        ask: PermissionAsk {
            tool_call_id: permission_tool_call_id(data),
            kind: kind.to_owned(),
            title: permission_title(&action, &resources),
            paths: if PATH_KINDS.contains(&kind) {
                resources
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
