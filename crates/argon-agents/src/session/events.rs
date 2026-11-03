use argon_core::Finding;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::registry::ModelInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl ToolStatus {
    fn parse(value: &str) -> Option<ToolStatus> {
        match value {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionOption {
    pub id: String,
    pub label: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum AgentEvent {
    UserMessage(String),
    MessageChunk(String),
    ThoughtChunk(String),
    ToolCall {
        id: String,
        title: String,
        kind: String,
        status: ToolStatus,
    },
    ToolCallUpdate {
        id: String,
        status: Option<ToolStatus>,
        output: Option<String>,
    },
    Plan(Vec<String>),
    PermissionRequest {
        request_id: String,
        title: String,
        kind: String,
        options: Vec<PermissionOption>,
    },
    TurnEnded {
        stop_reason: String,
    },
    Findings {
        findings: Vec<Finding>,
        skipped: Vec<String>,
    },
    ModelsAvailable(Vec<ModelInfo>),
    Error(String),
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn content_text(content: &Value) -> Option<String> {
    match content.get("type").and_then(Value::as_str) {
        Some("text") => text(content, "text"),
        Some("content") => content.get("content").and_then(content_text),
        Some("diff") => text(content, "path").map(|path| format!("diff: {path}")),
        Some("terminal") => text(content, "terminalId").map(|id| format!("terminal: {id}")),
        _ => None,
    }
}

fn tool_output(update: &Value) -> Option<String> {
    let from_content = update
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(content_text)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|joined| !joined.is_empty());
    from_content.or_else(|| match update.get("rawOutput") {
        Some(Value::String(raw)) if !raw.is_empty() => Some(raw.clone()),
        _ => None,
    })
}

fn tool_status(update: &Value) -> Option<ToolStatus> {
    update
        .get("status")
        .and_then(Value::as_str)
        .and_then(ToolStatus::parse)
}

fn chunk(update: &Value, build: fn(String) -> AgentEvent) -> Vec<AgentEvent> {
    update
        .get("content")
        .and_then(content_text)
        .map(build)
        .into_iter()
        .collect()
}

pub fn map_update(update: &Value) -> Vec<AgentEvent> {
    match update.get("sessionUpdate").and_then(Value::as_str) {
        Some("user_message_chunk") => chunk(update, AgentEvent::UserMessage),
        Some("agent_message_chunk") => chunk(update, AgentEvent::MessageChunk),
        Some("agent_thought_chunk") => chunk(update, AgentEvent::ThoughtChunk),
        Some("tool_call") => {
            let mut events = vec![AgentEvent::ToolCall {
                id: text(update, "toolCallId").unwrap_or_default(),
                title: text(update, "title").unwrap_or_default(),
                kind: text(update, "kind").unwrap_or_else(|| "other".to_owned()),
                status: tool_status(update).unwrap_or(ToolStatus::Pending),
            }];
            if let Some(output) = tool_output(update) {
                events.push(AgentEvent::ToolCallUpdate {
                    id: text(update, "toolCallId").unwrap_or_default(),
                    status: None,
                    output: Some(output),
                });
            }
            events
        }
        Some("tool_call_update") => {
            let status = tool_status(update);
            let output = tool_output(update);
            if status.is_none() && output.is_none() {
                return Vec::new();
            }
            vec![AgentEvent::ToolCallUpdate {
                id: text(update, "toolCallId").unwrap_or_default(),
                status,
                output,
            }]
        }
        Some("plan") => {
            let entries = update
                .get("entries")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|entry| text(entry, "content"))
                        .collect()
                })
                .unwrap_or_default();
            vec![AgentEvent::Plan(entries)]
        }
        _ => Vec::new(),
    }
}

fn select_values(options: &Value, out: &mut Vec<ModelInfo>) {
    let Some(items) = options.as_array() else {
        return;
    };
    for item in items {
        if let Some(id) = text(item, "value") {
            let label = text(item, "name").unwrap_or_else(|| id.clone());
            out.push(ModelInfo { id, label });
        }
        if let Some(nested) = item.get("options") {
            select_values(nested, out);
        }
    }
}

pub fn config_option<'a>(session: &'a Value, config_id: &str) -> Option<&'a Value> {
    session
        .get("configOptions")?
        .as_array()?
        .iter()
        .find(|option| text(option, "id").as_deref() == Some(config_id))
}

pub fn config_values(option: &Value) -> Vec<ModelInfo> {
    let mut models = Vec::new();
    if let Some(options) = option.get("options") {
        select_values(options, &mut models);
    }
    models
}

pub fn current_value(option: &Value) -> Option<String> {
    text(option, "currentValue")
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
