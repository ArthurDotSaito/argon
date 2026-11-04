use std::collections::HashMap;
use std::path::Path;

use argon_core::PermissionMode;
use serde_json::{Map, Value, json};

use crate::registry::ModelInfo;
use crate::session::{
    ALLOW_ALWAYS, ALLOW_ONCE, AgentEvent, PermissionAnswer, PermissionAsk, REJECT_ONCE, ToolStatus,
    native_options_for,
};
use crate::util::json_text;

const OUTPUT_LIMIT: usize = 8000;
const COMPACT_TITLE_LIMIT: usize = 300;
const FULL_TITLE_LIMIT: usize = 2000;
const REASON_LIMIT: usize = 300;
const TITLE_SEPARATOR: &str = " · ";
const TITLE_ELLIPSIS: &str = " …";
const DETAIL_SEPARATOR: &str = " — ";
const TURN_FAILED_MESSAGE: &str = "falha no Codex";
const LEGACY_REJECTION: &str = "Negado pelo usuário no Argon";

const COMMAND_APPROVAL_METHODS: [&str; 2] = [
    "item/commandExecution/requestApproval",
    "execCommandApproval",
];
const FILE_APPROVAL_METHODS: [&str; 2] = ["item/fileChange/requestApproval", "applyPatchApproval"];
const LEGACY_APPROVAL_METHODS: [&str; 2] = ["execCommandApproval", "applyPatchApproval"];
const APPLY_PATCH_METHOD: &str = "applyPatchApproval";
const PERMISSIONS_APPROVAL_METHOD: &str = "item/permissions/requestApproval";
const USER_INPUT_METHODS: [&str; 2] = ["item/tool/requestUserInput", "requestUserInput"];
const ELICITATION_METHOD: &str = "mcpServer/elicitation/request";
const DYNAMIC_TOOL_METHOD: &str = "item/tool/call";

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

#[derive(Debug, Clone, Default)]
struct FileTargets {
    paths: Vec<String>,
    trusted: bool,
}

impl FileTargets {
    fn askable_paths(&self) -> Vec<String> {
        if self.trusted {
            self.paths.clone()
        } else {
            Vec::new()
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Decoder {
    reasoning: HashMap<String, ReasoningKind>,
    files: HashMap<String, FileTargets>,
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
            self.command_approval(method, params)
        } else if FILE_APPROVAL_METHODS.contains(&method) {
            self.file_approval(method, params)
        } else if method == PERMISSIONS_APPROVAL_METHOD {
            permissions_approval(method, params)
        } else if USER_INPUT_METHODS.contains(&method) {
            CodexRequest::Reply(user_input_reply(params))
        } else if method == ELICITATION_METHOD {
            CodexRequest::Reply(json!({"action": "decline"}))
        } else if method == DYNAMIC_TOOL_METHOD {
            CodexRequest::Reply(json!({"success": false, "contentItems": []}))
        } else {
            CodexRequest::Reply(json!({}))
        }
    }

    fn reasoning(&mut self, params: &Value, kind: ReasoningKind) -> CodexEvent {
        let (Some(delta), Some(item_id)) =
            (text_field(params, "delta"), text_field(params, "itemId"))
        else {
            return CodexEvent::Ignore;
        };
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
                let title = format!("Bash: {}", truncated(&command, FULL_TITLE_LIMIT));
                self.commands.insert(id.clone(), command);
                tool_call(id, "execute", title)
            }
            "fileChange" => {
                let targets = item_file_targets(item);
                let title = list_title("Edit", &targets.paths, COMPACT_TITLE_LIMIT);
                self.files.insert(id.clone(), targets);
                tool_call(id, "edit", title)
            }
            "mcpToolCall" => {
                let server = json_text(item, "server").unwrap_or_default();
                let tool = json_text(item, "tool").unwrap_or_default();
                let title = truncated(&format!("{server}.{tool}"), FULL_TITLE_LIMIT);
                tool_call(id, "other", title)
            }
            "webSearch" => {
                let query = text_field(item, "query").unwrap_or_default();
                let title = format!("Web search {}", truncated(&query, FULL_TITLE_LIMIT));
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
                self.files.remove(&id);
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

    fn command_approval(&self, method: &str, params: &Value) -> CodexRequest {
        let item_id = approval_item_id(params);
        let command = command_text(params.get("command")).or_else(|| {
            item_id
                .as_ref()
                .and_then(|id| self.commands.get(id).cloned())
        });
        let mut title = match command {
            Some(command) => format!("Bash: {}", truncated(&command, FULL_TITLE_LIMIT)),
            None => "Bash".to_owned(),
        };
        let Some(details) = grant_details(params) else {
            return deny(method, params);
        };
        for detail in details {
            title.push_str(DETAIL_SEPARATOR);
            title.push_str(&sanitize(&detail, usize::MAX));
        }
        ask(item_id, "execute", with_reason(title, params), Vec::new())
    }

    fn file_approval(&self, method: &str, params: &Value) -> CodexRequest {
        let item_id = approval_item_id(params);
        let legacy_targets = if method == APPLY_PATCH_METHOD {
            params
                .get("fileChanges")
                .and_then(Value::as_object)
                .map(legacy_file_targets)
        } else {
            None
        };
        let targets = legacy_targets
            .or_else(|| item_id.as_ref().and_then(|id| self.files.get(id).cloned()))
            .unwrap_or_default();
        let mut title = list_title("Edit", &targets.paths, COMPACT_TITLE_LIMIT);
        let mut paths = targets.askable_paths();
        if let Some(root) = params.get("grantRoot").filter(|root| !root.is_null()) {
            let root = match root.as_str() {
                Some(text) => text.to_owned(),
                None => root.to_string(),
            };
            title.push_str(&format!(
                "{DETAIL_SEPARATOR}acesso de escrita a {} pelo resto da sessão",
                truncated(&root, FULL_TITLE_LIMIT)
            ));
            paths.clear();
        }
        ask(item_id, "edit", with_reason(title, params), paths)
    }
}

pub(crate) fn approval_response(method: &str, params: &Value, answer: &PermissionAnswer) -> Value {
    if LEGACY_APPROVAL_METHODS.contains(&method) {
        json!({"decision": legacy_decision(answer)})
    } else if COMMAND_APPROVAL_METHODS.contains(&method) || FILE_APPROVAL_METHODS.contains(&method)
    {
        json!({"decision": decision(answer)})
    } else if method == PERMISSIONS_APPROVAL_METHOD {
        let permissions = if is_allowed(answer) {
            requested_permissions(params)
                .cloned()
                .unwrap_or_else(|| json!({}))
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
    if let Some(message) = item
        .get("error")
        .and_then(|error| text_field(error, "message"))
    {
        return Some(message);
    }
    match item.get("result")? {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        result => Some(
            content_text(result)
                .or_else(|| {
                    result
                        .get("structuredContent")
                        .filter(|content| !content.is_null())
                        .map(Value::to_string)
                })
                .unwrap_or_else(|| result.to_string()),
        ),
    }
}

fn content_text(result: &Value) -> Option<String> {
    let blocks = result.get("content")?.as_array()?;
    let text = blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| json_text(block, "text"))
        .collect::<Vec<_>>()
        .join("\n");
    Some(text).filter(|text| !text.is_empty())
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

fn move_target(kind: &Value) -> Option<Option<&str>> {
    match kind.get("type").and_then(Value::as_str)? {
        "add" | "delete" => Some(None),
        "update" => match kind.get("move_path") {
            None | Some(Value::Null) => Some(None),
            Some(Value::String(path)) if !path.trim().is_empty() => Some(Some(path)),
            Some(_) => None,
        },
        _ => None,
    }
}

fn file_targets<'a>(entries: impl Iterator<Item = (Option<&'a str>, &'a Value)>) -> FileTargets {
    let mut targets = FileTargets {
        paths: Vec::new(),
        trusted: true,
    };
    for (path, kind) in entries {
        match path.filter(|path| !path.trim().is_empty()) {
            Some(path) => targets.paths.push(path.to_owned()),
            None => targets.trusted = false,
        }
        match move_target(kind) {
            Some(Some(target)) => targets.paths.push(target.to_owned()),
            Some(None) => {}
            None => targets.trusted = false,
        }
    }
    targets.trusted &= !targets.paths.is_empty();
    targets
}

fn item_file_targets(item: &Value) -> FileTargets {
    let changes = item
        .get("changes")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    file_targets(changes.iter().map(|change| {
        (
            change.get("path").and_then(Value::as_str),
            change.get("kind").unwrap_or(&Value::Null),
        )
    }))
}

fn legacy_file_targets(changes: &Map<String, Value>) -> FileTargets {
    file_targets(
        changes
            .iter()
            .map(|(path, change)| (Some(path.as_str()), change)),
    )
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

fn deny(method: &str, params: &Value) -> CodexRequest {
    CodexRequest::Reply(approval_response(
        method,
        params,
        &PermissionAnswer::Selected(REJECT_ONCE.to_owned()),
    ))
}

fn grant_details(params: &Value) -> Option<Vec<String>> {
    let present = |key: &str| params.get(key).filter(|value| !value.is_null());
    let mut details = Vec::new();
    if let Some(context) = present("networkApprovalContext") {
        let target = match (text_field(context, "protocol"), text_field(context, "host")) {
            (Some(protocol), Some(host)) => format!("{protocol}://{host}"),
            _ => context.to_string(),
        };
        details.push(format!("rede: {target}"));
    }
    if let Some(extra) = present("additionalPermissions") {
        details.push(format!("permissões adicionais: {extra}"));
    }
    details
        .iter()
        .all(|detail| detail.chars().count() <= FULL_TITLE_LIMIT)
        .then_some(details)
}

fn requested_permissions(params: &Value) -> Option<&Value> {
    params.get("permissions").filter(|value| value.is_object())
}

fn permissions_approval(method: &str, params: &Value) -> CodexRequest {
    let title = match requested_permissions(params) {
        Some(permissions) => {
            let compact = permissions.to_string();
            if compact.chars().count() > FULL_TITLE_LIMIT {
                return deny(method, params);
            }
            format!("Permissões: {}", sanitize(&compact, usize::MAX))
        }
        None => "Permissões".to_owned(),
    };
    ask(
        approval_item_id(params),
        "other",
        with_reason(title, params),
        Vec::new(),
    )
}

fn user_input_reply(params: &Value) -> Value {
    let answers: Map<String, Value> = params
        .get("questions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|question| text_field(question, "id"))
        .map(|id| (id, json!({"answers": []})))
        .collect();
    json!({"answers": answers})
}

fn with_reason(title: String, params: &Value) -> String {
    match text_field(params, "reason").filter(|reason| !reason.trim().is_empty()) {
        Some(reason) => format!(
            "{title}{DETAIL_SEPARATOR}{}",
            truncated(&reason, REASON_LIMIT)
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

fn legacy_decision(answer: &PermissionAnswer) -> Value {
    match answer {
        PermissionAnswer::Cancelled => json!("abort"),
        answer if is_allowed(answer) => json!("approved"),
        _ => json!({"denied": {"rejection": LEGACY_REJECTION}}),
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

fn is_hidden_format(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{FEFF}'
    )
}

fn sanitize(text: &str, limit: usize) -> String {
    text.chars()
        .take(limit)
        .map(|c| {
            if c.is_control() || is_hidden_format(c) {
                ' '
            } else {
                c
            }
        })
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
