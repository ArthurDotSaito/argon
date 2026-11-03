use std::collections::HashMap;
use std::path::PathBuf;

use argon_agents::{AgentSessionHandle, WorktreeManager};
use argon_core::{AgentKind, Paths, PrKey};
use chrono::Utc;
use gpui_kit::{App, AppContext as _, Context, Entity, EventEmitter, Global, Subscription};

use super::card_status::CardStatus;
use super::detection::DetectionCache;
use super::launches::PendingLaunch;
use super::sink::SessionStore;
use super::thread_model::{SessionInfo, ThreadEvent, ThreadModel};

pub struct ActiveSession {
    pub session_id: String,
    pub pr: PrKey,
    pub number: u64,
    pub agent: AgentKind,
    pub agent_label: &'static str,
    pub model: String,
    pub model_label: String,
    pub worktree: PathBuf,
    pub handle: AgentSessionHandle,
}

impl ActiveSession {
    fn info(&self) -> SessionInfo {
        SessionInfo {
            session_id: self.session_id.clone(),
            pr: self.pr.clone(),
            number: Some(self.number),
            agent_label: self.agent_label,
            model: self.model.clone(),
            model_label: self.model_label.clone(),
            worktree: Some(self.worktree.clone()),
            created_at: Utc::now(),
            ended_at: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Activity {
    pub running: usize,
    pub awaiting: usize,
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
    launches: HashMap<String, PendingLaunch>,
    threads: HashMap<String, Entity<ThreadModel>>,
    _thread_subscriptions: Vec<Subscription>,
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
            launches: HashMap::new(),
            threads: HashMap::new(),
            _thread_subscriptions: Vec::new(),
            paths,
        });
        cx.set_global(AgentSessionsHandle(sessions.clone()));
        sessions
    }

    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<AgentSessionsHandle>()
            .map(|handle| handle.0.clone())
    }

    pub fn thread(&self, session_id: &str) -> Option<Entity<ThreadModel>> {
        self.threads.get(session_id).cloned()
    }

    pub fn threads(&self) -> impl Iterator<Item = &Entity<ThreadModel>> {
        self.sessions
            .iter()
            .filter_map(|session| self.threads.get(&session.session_id))
    }

    pub fn activity(&self, cx: &App) -> Activity {
        let mut activity = Activity::default();
        for thread in self.threads() {
            match thread.read(cx).card_status() {
                CardStatus::Running => activity.running += 1,
                CardStatus::Awaiting => activity.awaiting += 1,
                CardStatus::Completed | CardStatus::Failed => {}
            }
        }
        activity
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

    pub(super) fn launches(&self) -> &HashMap<String, PendingLaunch> {
        &self.launches
    }

    pub(super) fn launches_mut(&mut self) -> &mut HashMap<String, PendingLaunch> {
        &mut self.launches
    }

    pub(super) fn add(&mut self, session: ActiveSession, cx: &mut Context<Self>) {
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
        let kind = session.agent;
        let info = session.info();
        let handle = session.handle.clone();
        let thread = cx.new(|cx| ThreadModel::live(info, handle, cx));
        let subscription = cx.subscribe(&thread, move |this, _, event, cx| match event {
            ThreadEvent::Activity => cx.notify(),
            ThreadEvent::ModelsAvailable(models) => this.detection.report_models(kind, models),
        });
        self._thread_subscriptions.push(subscription);
        self.threads.insert(session.session_id.clone(), thread);
        self.sessions.push(session);
        cx.emit(event);
        cx.notify();
    }
}
