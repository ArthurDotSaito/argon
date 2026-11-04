use serde_json::{Value, json};

use crate::session::{AgentEvent, PermissionAsk, ToolStatus, input_paths, native_options_for};
use crate::util::json_text;

const OUTPUT_LIMIT: usize = 8000;
const SUBJECT_LIMIT: usize = 120;
const SUBJECT_KEYS: [&str; 7] = [
    "file_path",
    "notebook_path",
    "path",
    "pattern",
    "command",
    "url",
    "query",
];
const COMMAND_KEY: &str = "command";
const LINE_BREAKS: [char; 2] = ['\r', '\n'];
const TODO_WRITE: &str = "TodoWrite";
const MAX_TURNS_SUBTYPE: &str = "error_max_turns";
const MAX_TURNS_MESSAGE: &str = "limite de turnos atingido";

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ClaudeLine {
    Events(Vec<AgentEvent>),
    Init {
        session_id: String,
        model: String,
    },
    Permission {
        request_id: String,
        tool_name: String,
        ask: PermissionAsk,
        input: Value,
    },
    ApiError(String),
    TurnEnd {
        stop_reason: String,
        error: Option<String>,
    },
    Ignore,
}

pub(crate) fn parse_line(line: &str) -> ClaudeLine {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return ClaudeLine::Ignore;
    };
    match value.get("type").and_then(Value::as_str) {
        Some("system") => parse_system(&value),
        Some("assistant") => parse_assistant(&value),
        Some("user") => parse_user(&value),
        Some("control_request") => parse_control_request(&value),
        Some("result") => parse_result(&value),
        _ => ClaudeLine::Ignore,
    }
}

pub(crate) fn kind_for(tool_name: &str) -> &'static str {
    match tool_name {
        "Read" | "LS" | "NotebookRead" => "read",
        "Grep" | "Glob" => "search",
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => "edit",
        "Bash" | "BashOutput" | "KillShell" => "execute",
        "WebFetch" | "WebSearch" => "fetch",
        _ => "other",
    }
}

pub(crate) fn user_message(text: &str) -> String {
    json!({"type": "user", "message": {"role": "user", "content": text}}).to_string()
}

pub(crate) fn allow(request_id: &str, input: &Value) -> String {
    control_response(
        request_id,
        json!({"behavior": "allow", "updatedInput": input}),
    )
}

pub(crate) fn deny(request_id: &str, message: &str) -> String {
    control_response(request_id, json!({"behavior": "deny", "message": message}))
}

pub(crate) fn interrupt(request_id: &str) -> String {
    json!({
        "type": "control_request",
        "request_id": request_id,
        "request": {"subtype": "interrupt"},
    })
    .to_string()
}

fn control_response(request_id: &str, payload: Value) -> String {
    json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": payload,
        },
    })
    .to_string()
}

fn block_type(block: &Value) -> Option<&str> {
    block.get("type").and_then(Value::as_str)
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool) == Some(true)
}

fn parse_system(value: &Value) -> ClaudeLine {
    if value.get("subtype").and_then(Value::as_str) != Some("init") {
        return ClaudeLine::Ignore;
    }
    ClaudeLine::Init {
        session_id: json_text(value, "session_id").unwrap_or_default(),
        model: json_text(value, "model").unwrap_or_default(),
    }
}

fn content_blocks(value: &Value) -> &[Value] {
    value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn is_subagent(value: &Value) -> bool {
    value
        .get("parent_tool_use_id")
        .is_some_and(Value::is_string)
}

fn events_or_ignore(events: Vec<AgentEvent>) -> ClaudeLine {
    if events.is_empty() {
        ClaudeLine::Ignore
    } else {
        ClaudeLine::Events(events)
    }
}

fn parse_assistant(value: &Value) -> ClaudeLine {
    if is_subagent(value) {
        return ClaudeLine::Ignore;
    }
    let blocks = content_blocks(value);
    if flag(value, "isApiErrorMessage") {
        let text = blocks
            .iter()
            .filter(|block| block_type(block) == Some("text"))
            .filter_map(|block| json_text(block, "text"))
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            return ClaudeLine::Ignore;
        }
        return ClaudeLine::ApiError(text);
    }
    events_or_ignore(blocks.iter().filter_map(assistant_event).collect())
}

fn assistant_event(block: &Value) -> Option<AgentEvent> {
    match block_type(block)? {
        "text" => json_text(block, "text").map(AgentEvent::MessageChunk),
        "thinking" => json_text(block, "thinking").map(AgentEvent::ThoughtChunk),
        "tool_use" => Some(tool_use_event(block)),
        _ => None,
    }
}

fn tool_use_event(block: &Value) -> AgentEvent {
    let name = json_text(block, "name").unwrap_or_default();
    let input = block.get("input").unwrap_or(&Value::Null);
    if name == TODO_WRITE {
        return AgentEvent::Plan(todo_items(input));
    }
    AgentEvent::ToolCall {
        id: json_text(block, "id").unwrap_or_default(),
        title: tool_title(&name, input),
        kind: kind_for(&name).to_owned(),
        status: ToolStatus::InProgress,
    }
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

fn parse_user(value: &Value) -> ClaudeLine {
    events_or_ignore(
        content_blocks(value)
            .iter()
            .filter(|block| block_type(block) == Some("tool_result"))
            .map(tool_result_event)
            .collect(),
    )
}

fn tool_result_event(block: &Value) -> AgentEvent {
    let status = if flag(block, "is_error") {
        ToolStatus::Failed
    } else {
        ToolStatus::Completed
    };
    AgentEvent::ToolCallUpdate {
        id: json_text(block, "tool_use_id").unwrap_or_default(),
        status: Some(status),
        output: Some(truncate(&result_text(block), OUTPUT_LIMIT)).filter(|text| !text.is_empty()),
        title: None,
    }
}

fn result_text(block: &Value) -> String {
    match block.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter(|item| block_type(item) == Some("text"))
            .filter_map(|item| json_text(item, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn parse_control_request(value: &Value) -> ClaudeLine {
    let Some(request) = value.get("request") else {
        return ClaudeLine::Ignore;
    };
    if request.get("subtype").and_then(Value::as_str) != Some("can_use_tool") {
        return ClaudeLine::Ignore;
    }
    let Some(request_id) = json_text(value, "request_id").filter(|id| !id.is_empty()) else {
        return ClaudeLine::Ignore;
    };
    let tool_name = json_text(request, "tool_name").unwrap_or_default();
    let input = request.get("input").cloned().unwrap_or_else(|| json!({}));
    ClaudeLine::Permission {
        request_id,
        ask: PermissionAsk {
            tool_call_id: json_text(request, "tool_use_id"),
            kind: kind_for(&tool_name).to_owned(),
            title: tool_title(&tool_name, &input),
            paths: input_paths(&input),
            options: native_options_for(kind_for(&tool_name)),
        },
        tool_name,
        input,
    }
}

fn parse_result(value: &Value) -> ClaudeLine {
    let subtype = json_text(value, "subtype").unwrap_or_default();
    let text = json_text(value, "result").filter(|text| !text.is_empty());
    if subtype == "success" && !flag(value, "is_error") {
        return ClaudeLine::TurnEnd {
            stop_reason: "end_turn".to_owned(),
            error: None,
        };
    }
    if subtype == MAX_TURNS_SUBTYPE {
        return ClaudeLine::TurnEnd {
            stop_reason: "max_turns".to_owned(),
            error: Some(text.unwrap_or_else(|| MAX_TURNS_MESSAGE.to_owned())),
        };
    }
    ClaudeLine::TurnEnd {
        stop_reason: "error".to_owned(),
        error: Some(text.unwrap_or(subtype)),
    }
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
