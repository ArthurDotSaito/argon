use argon_core::{Finding, PermissionMode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::registry::ModelInfo;
use crate::util::json_text;

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
pub struct SavedFinding {
    pub id: i64,
    pub finding: Finding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingPermission {
    pub request_id: String,
    pub title: String,
    pub kind: String,
    pub options: Vec<PermissionOption>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    Plan(Vec<String>),
    PermissionRequest {
        request_id: String,
        title: String,
        kind: String,
        options: Vec<PermissionOption>,
    },
    PermissionResolved {
        request_id: String,
    },
    PermissionModeChanged(PermissionMode),
    TurnEnded {
        stop_reason: String,
    },
    Findings {
        findings: Vec<SavedFinding>,
        skipped: Vec<String>,
    },
    PlanDraft {
        draft_id: i64,
        markdown: String,
    },
    Summary {
        id: i64,
        body: String,
        project_updated: bool,
    },
    ModelsAvailable(Vec<ModelInfo>),
    Error(String),
    Resync {
        last_seq: u64,
    },
}

impl AgentEvent {
    pub fn permission(pending: &PendingPermission) -> AgentEvent {
        AgentEvent::PermissionRequest {
            request_id: pending.request_id.clone(),
            title: pending.title.clone(),
            kind: pending.kind.clone(),
            options: pending.options.clone(),
        }
    }

    pub fn permission_resolved(pending: &PendingPermission) -> AgentEvent {
        AgentEvent::PermissionResolved {
            request_id: pending.request_id.clone(),
        }
    }
}

fn content_text(content: &Value) -> Option<String> {
    match content.get("type").and_then(Value::as_str) {
        Some("text") => json_text(content, "text"),
        Some("content") => content.get("content").and_then(content_text),
        Some("diff") => json_text(content, "path").map(|path| format!("diff: {path}")),
        Some("terminal") => json_text(content, "terminalId").map(|id| format!("terminal: {id}")),
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
                id: json_text(update, "toolCallId").unwrap_or_default(),
                title: json_text(update, "title").unwrap_or_default(),
                kind: json_text(update, "kind").unwrap_or_else(|| "other".to_owned()),
                status: tool_status(update).unwrap_or(ToolStatus::Pending),
            }];
            if let Some(output) = tool_output(update) {
                events.push(AgentEvent::ToolCallUpdate {
                    id: json_text(update, "toolCallId").unwrap_or_default(),
                    status: None,
                    output: Some(output),
                    title: None,
                });
            }
            events
        }
        Some("tool_call_update") => {
            let status = tool_status(update);
            let output = tool_output(update);
            let title = json_text(update, "title").filter(|title| !title.is_empty());
            if status.is_none() && output.is_none() && title.is_none() {
                return Vec::new();
            }
            vec![AgentEvent::ToolCallUpdate {
                id: json_text(update, "toolCallId").unwrap_or_default(),
                status,
                output,
                title,
            }]
        }
        Some("plan") => {
            let entries = update
                .get("entries")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|entry| json_text(entry, "content"))
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
        if let Some(id) = json_text(item, "value") {
            let label = json_text(item, "name").unwrap_or_else(|| id.clone());
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
        .find(|option| json_text(option, "id").as_deref() == Some(config_id))
}

pub fn config_values(option: &Value) -> Vec<ModelInfo> {
    let mut models = Vec::new();
    if let Some(options) = option.get("options") {
        select_values(options, &mut models);
    }
    models
}

pub fn current_value(option: &Value) -> Option<String> {
    json_text(option, "currentValue")
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
