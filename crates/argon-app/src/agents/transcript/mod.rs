use std::time::{Duration, Instant};

use argon_agents::{AgentEvent, PermissionOption, ToolStatus};
use argon_core::PermissionMode;
use argon_store::SessionStatus;

const END_TURN: &str = "end_turn";
const OUTPUT_LIMIT: usize = 16 * 1024;
const OUTPUT_TRUNCATED: &str = "
… saída truncada";
const OUTSIDE_PR_MARKER: &str = "is not part of the PR";

fn clip_output(output: &str) -> String {
    if output.len() <= OUTPUT_LIMIT {
        return output.to_owned();
    }
    let mut cut = OUTPUT_LIMIT;
    while !output.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{OUTPUT_TRUNCATED}", &output[..cut])
}

pub fn split_skipped(skipped: &[String]) -> (usize, usize) {
    let outside = skipped
        .iter()
        .filter(|reason| reason.ends_with(OUTSIDE_PR_MARKER))
        .count();
    (outside, skipped.len() - outside)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionState {
    Pending,
    Answered(String),
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionItem {
    pub request_id: String,
    pub title: String,
    pub kind: String,
    pub options: Vec<PermissionOption>,
    pub state: PermissionState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolItem {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: ToolStatus,
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThoughtItem {
    pub text: String,
    pub started: Option<Instant>,
    pub finished: Option<Instant>,
}

impl ThoughtItem {
    pub fn duration(&self) -> Option<Duration> {
        Some(self.finished?.saturating_duration_since(self.started?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    User(String),
    Agent(String),
    Thought(ThoughtItem),
    Tool(ToolItem),
    Plan(Vec<String>),
    Permission(PermissionItem),
    Findings {
        ids: Vec<i64>,
        outside: usize,
        unreadable: usize,
    },
    PlanDraft(i64),
    Summary {
        id: i64,
        project_updated: bool,
    },
    Interrupted(String),
    Error(String),
}

#[derive(Debug, Default, Clone)]
pub struct Transcript {
    items: Vec<Item>,
    started: bool,
    in_turn: bool,
    failed: bool,
    open_thought: bool,
    plan: Option<usize>,
    turns: usize,
    permission_mode: Option<PermissionMode>,
}

impl Transcript {
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn in_turn(&self) -> bool {
        self.in_turn
    }

    pub fn pending_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| {
                matches!(item, Item::Permission(permission) if permission.state == PermissionState::Pending)
            })
            .count()
    }

    pub fn status(&self) -> SessionStatus {
        match (self.failed, self.in_turn || !self.started) {
            (true, _) => SessionStatus::Failed,
            (false, true) if self.pending_count() > 0 => SessionStatus::AwaitingUser,
            (false, true) => SessionStatus::Running,
            (false, false) => SessionStatus::Completed,
        }
    }

    pub fn is_streaming(&self) -> bool {
        self.in_turn && matches!(self.items.last(), Some(Item::Agent(_)))
    }

    pub fn is_thinking(&self) -> bool {
        self.in_turn
            && self.pending_count() == 0
            && !matches!(self.items.last(), Some(Item::Agent(_)))
    }

    pub fn apply(&mut self, event: &AgentEvent, now: Option<Instant>) {
        if !matches!(event, AgentEvent::ThoughtChunk(_)) {
            self.close_thought(now);
        }
        match event {
            AgentEvent::UserMessage(text) => {
                self.started = true;
                self.in_turn = true;
                self.failed = false;
                self.plan = None;
                self.turns += 1;
                self.items.push(Item::User(text.clone()));
            }
            AgentEvent::MessageChunk(text) => match self.items.last_mut() {
                Some(Item::Agent(body)) => body.push_str(text),
                _ => self.items.push(Item::Agent(text.clone())),
            },
            AgentEvent::ThoughtChunk(text) => self.push_thought(text, now),
            AgentEvent::ToolCall {
                id,
                title,
                kind,
                status,
            } => self.push_tool(ToolItem {
                id: id.clone(),
                title: title.clone(),
                kind: kind.clone(),
                status: *status,
                output: None,
            }),
            AgentEvent::ToolCallUpdate {
                id,
                status,
                output,
                title,
            } => {
                if let Some(tool) = self.tool_mut(id) {
                    if let Some(status) = status {
                        tool.status = *status;
                    }
                    if let Some(title) = title {
                        tool.title = title.clone();
                    }
                    if let Some(output) = output {
                        tool.output = Some(clip_output(output));
                    }
                }
            }
            AgentEvent::Plan(entries) => match self.plan {
                Some(ix) => self.items[ix] = Item::Plan(entries.clone()),
                None => {
                    self.plan = Some(self.items.len());
                    self.items.push(Item::Plan(entries.clone()));
                }
            },
            AgentEvent::PermissionRequest {
                request_id,
                title,
                kind,
                options,
            } => self.items.push(Item::Permission(PermissionItem {
                request_id: request_id.clone(),
                title: title.clone(),
                kind: kind.clone(),
                options: options.clone(),
                state: PermissionState::Pending,
            })),
            AgentEvent::PermissionResolved { request_id } => {
                self.expire_pending(request_id);
            }
            AgentEvent::PermissionModeChanged(mode) => self.permission_mode = Some(*mode),
            AgentEvent::TurnEnded { stop_reason } => {
                self.end_turn();
                if stop_reason != END_TURN {
                    self.items.push(Item::Interrupted(stop_reason.clone()));
                }
            }
            AgentEvent::Findings { findings, skipped } => {
                if self.turns > 1 && findings.is_empty() && skipped.is_empty() {
                    return;
                }
                let (outside, unreadable) = split_skipped(skipped);
                self.items.push(Item::Findings {
                    ids: findings.iter().map(|saved| saved.id).collect(),
                    outside,
                    unreadable,
                });
            }
            AgentEvent::Error(message) => self.items.push(Item::Error(message.clone())),
            AgentEvent::PlanDraft { draft_id, .. } => self.push_plan_draft(*draft_id),
            AgentEvent::Summary {
                id,
                project_updated,
                ..
            } => self.items.push(Item::Summary {
                id: *id,
                project_updated: *project_updated,
            }),
            AgentEvent::ModelsAvailable(_) | AgentEvent::Resync { .. } => {}
        }
    }

    pub fn mark_failed(&mut self) {
        self.end_turn();
        self.failed = true;
    }

    pub fn settle(&mut self, status: SessionStatus, pending: &[String]) {
        for item in &mut self.items {
            if let Item::Permission(permission) = item
                && permission.state == PermissionState::Pending
                && !pending.contains(&permission.request_id)
            {
                permission.state = PermissionState::Expired;
            }
        }
        match status {
            SessionStatus::Running | SessionStatus::AwaitingUser => {
                if !self.started {
                    self.started = true;
                    self.in_turn = true;
                }
            }
            SessionStatus::Completed | SessionStatus::Archived => {
                self.in_turn = false;
                self.failed = false;
            }
            SessionStatus::Failed => self.mark_failed(),
        }
    }

    pub fn resolve(&mut self, request_id: &str, state: PermissionState) {
        for item in &mut self.items {
            if let Item::Permission(permission) = item
                && permission.request_id == request_id
            {
                permission.state = state.clone();
            }
        }
    }

    fn expire_pending(&mut self, request_id: &str) {
        for item in &mut self.items {
            if let Item::Permission(permission) = item
                && permission.request_id == request_id
                && permission.state == PermissionState::Pending
            {
                permission.state = PermissionState::Expired;
            }
        }
    }

    pub fn permission_mode(&self) -> Option<PermissionMode> {
        self.permission_mode
    }

    pub fn set_permission_mode(&mut self, mode: PermissionMode) {
        self.permission_mode = Some(mode);
    }

    fn end_turn(&mut self) {
        self.in_turn = false;
        for item in &mut self.items {
            if let Item::Permission(permission) = item
                && permission.state == PermissionState::Pending
            {
                permission.state = PermissionState::Expired;
            }
        }
    }

    fn close_thought(&mut self, now: Option<Instant>) {
        if !std::mem::take(&mut self.open_thought) {
            return;
        }
        if let Some(Item::Thought(thought)) = self.items.last_mut() {
            thought.finished = now;
        }
    }

    fn push_thought(&mut self, text: &str, now: Option<Instant>) {
        if self.open_thought
            && let Some(Item::Thought(thought)) = self.items.last_mut()
        {
            thought.text.push_str(text);
            return;
        }
        self.open_thought = true;
        self.items.push(Item::Thought(ThoughtItem {
            text: text.to_owned(),
            started: now,
            finished: None,
        }));
    }

    fn push_tool(&mut self, tool: ToolItem) {
        match self.tool_mut(&tool.id) {
            Some(existing) => {
                existing.title = tool.title;
                existing.kind = tool.kind;
                existing.status = tool.status;
            }
            None => self.items.push(Item::Tool(tool)),
        }
    }

    fn push_plan_draft(&mut self, id: i64) {
        let draft = Item::PlanDraft(id);
        if let Some(ix) = self.items.iter().position(|item| *item == draft) {
            self.items.remove(ix);
            if let Some(plan) = self.plan.as_mut()
                && *plan > ix
            {
                *plan -= 1;
            }
        }
        self.items.push(draft);
    }

    fn tool_mut(&mut self, id: &str) -> Option<&mut ToolItem> {
        self.items.iter_mut().rev().find_map(|item| match item {
            Item::Tool(tool) if tool.id == id => Some(tool),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests;
