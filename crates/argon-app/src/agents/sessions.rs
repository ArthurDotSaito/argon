use std::path::PathBuf;

use argon_agents::{AgentSessionHandle, WorktreeManager};
use argon_core::{AgentKind, Paths, PrKey};
use gpui_kit::{App, AppContext as _, Context, Entity, EventEmitter, Global};

use super::detection::DetectionCache;
use super::labels;
use super::sink::SessionStore;

pub struct ActiveSession {
    pub session_id: String,
    pub pr: PrKey,
    pub number: u64,
    pub agent: AgentKind,
    pub agent_label: &'static str,
    pub model: String,
    pub model_label: String,
    #[expect(
        dead_code,
        reason = "read by the agent panel lifecycle (Tasks 23 and 24)"
    )]
    pub worktree: PathBuf,
    #[expect(dead_code, reason = "driven by the agent panel thread view (Task 23)")]
    pub handle: AgentSessionHandle,
}

impl ActiveSession {
    pub fn summary(&self) -> String {
        labels::session_line(self.agent_label, &self.model_label, self.number)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentsEvent {
    SessionStarted { session_id: String, pr: PrKey },
}

pub struct AgentSessions {
    sessions: Vec<ActiveSession>,
    last_agent: Option<AgentKind>,
    detection: DetectionCache,
    paths: Paths,
    store: SessionStore,
    worktrees: WorktreeManager,
}

struct AgentSessionsHandle(Entity<AgentSessions>);

impl Global for AgentSessionsHandle {}

impl EventEmitter<AgentsEvent> for AgentSessions {}

impl AgentSessions {
    pub fn init(paths: Paths, cx: &mut App) -> Entity<Self> {
        if let Some(existing) = Self::global(cx) {
            return existing;
        }
        let sessions = cx.new(|_| Self {
            sessions: Vec::new(),
            last_agent: None,
            detection: DetectionCache::default(),
            store: SessionStore::new(paths.db_file()),
            worktrees: WorktreeManager::new(&paths),
            paths,
        });
        cx.set_global(AgentSessionsHandle(sessions.clone()));
        sessions
    }

    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<AgentSessionsHandle>()
            .map(|handle| handle.0.clone())
    }

    pub fn sessions(&self) -> &[ActiveSession] {
        &self.sessions
    }

    pub fn last_agent(&self) -> Option<AgentKind> {
        self.last_agent
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn store(&self) -> SessionStore {
        self.store.clone()
    }

    pub fn worktrees(&self) -> WorktreeManager {
        self.worktrees.clone()
    }

    pub fn detection(&self) -> &DetectionCache {
        &self.detection
    }

    pub fn detection_mut(&mut self) -> &mut DetectionCache {
        &mut self.detection
    }

    pub fn add(&mut self, session: ActiveSession, cx: &mut Context<Self>) {
        let event = AgentsEvent::SessionStarted {
            session_id: session.session_id.clone(),
            pr: session.pr.clone(),
        };
        tracing::info!(
            session = %session.session_id,
            agent = ?session.agent,
            model = %session.model,
            "agent review session started"
        );
        self.last_agent = Some(session.agent);
        self.sessions.push(session);
        cx.emit(event);
        cx.notify();
    }
}
