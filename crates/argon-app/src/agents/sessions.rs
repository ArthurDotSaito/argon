use std::collections::HashMap;
use std::path::PathBuf;

use argon_agents::defaults::agent_defaults;
use argon_agents::{AgentSessionHandle, WorktreeManager};
use argon_core::{AgentKind, Paths, PrKey};
use chrono::{DateTime, Utc};
use gpui_kit::{App, AppContext as _, Context, Entity, EventEmitter, Global, Subscription};

use super::card_status::CardStatus;
use super::detection::DetectionCache;
use super::launches::PendingLaunch;
use super::model_listing::ModelListings;
use super::sink::SessionStore;
use super::thread_model::{SessionInfo, ThreadEvent, ThreadModel};
use super::usage_cache::UsageCache;

pub struct ActiveSession {
    pub session_id: String,
    pub pr: PrKey,
    pub number: Option<u64>,
    pub agent: AgentKind,
    pub agent_label: &'static str,
    pub model: String,
    pub model_label: String,
    pub worktree: PathBuf,
    pub handle: AgentSessionHandle,
    pub head_oid: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl ActiveSession {
    fn info(&self) -> SessionInfo {
        SessionInfo {
            session_id: self.session_id.clone(),
            pr: self.pr.clone(),
            number: self.number,
            agent_label: self.agent_label,
            model: self.model.clone(),
            model_label: self.model_label.clone(),
            worktree: Some(self.worktree.clone()),
            created_at: self.created_at,
            ended_at: None,
            resumable: agent_defaults(self.agent).load_session,
            failure_reason: None,
            head_oid: self.head_oid.clone(),
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
    SessionArchived { session_id: String },
}

pub struct AgentSessions {
    sessions: Vec<ActiveSession>,
    last_agent: Option<AgentKind>,
    detection: DetectionCache,
    listings: ModelListings,
    usage: UsageCache,
    paths: Paths,
    store: SessionStore,
    worktrees: WorktreeManager,
    launches: HashMap<String, PendingLaunch>,
    threads: HashMap<String, Entity<ThreadModel>>,
    thread_subscriptions: HashMap<String, Subscription>,
    resumes: HashMap<String, ResumeState>,
    closing: Closing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeState {
    Running,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closing {
    Open,
    ShuttingDown,
    Done,
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
            listings: ModelListings::load(&paths),
            usage: UsageCache::default(),
            store: SessionStore::new(paths.db_file()),
            worktrees: WorktreeManager::new(&paths),
            launches: HashMap::new(),
            threads: HashMap::new(),
            thread_subscriptions: HashMap::new(),
            resumes: HashMap::new(),
            closing: Closing::Open,
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
                CardStatus::Completed | CardStatus::Failed | CardStatus::Archived => {}
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

    pub(super) fn listings_mut(&mut self) -> &mut ModelListings {
        &mut self.listings
    }

    pub(super) fn usage(&self) -> &UsageCache {
        &self.usage
    }

    pub(super) fn usage_mut(&mut self) -> &mut UsageCache {
        &mut self.usage
    }

    pub(super) fn launches(&self) -> &HashMap<String, PendingLaunch> {
        &self.launches
    }

    pub(super) fn launches_mut(&mut self) -> &mut HashMap<String, PendingLaunch> {
        &mut self.launches
    }

    pub fn resume_state(&self, session_id: &str) -> Option<&ResumeState> {
        self.resumes.get(session_id)
    }

    pub(super) fn resumes_mut(&mut self) -> &mut HashMap<String, ResumeState> {
        &mut self.resumes
    }

    pub(super) fn closing(&self) -> Closing {
        self.closing
    }

    pub(super) fn set_closing(&mut self, closing: Closing) {
        self.closing = closing;
    }

    pub(super) fn handle(&self, session_id: &str) -> Option<AgentSessionHandle> {
        self.sessions
            .iter()
            .find(|session| session.session_id == session_id)
            .map(|session| session.handle.clone())
    }

    pub(super) fn live_handles(&self) -> Vec<(String, AgentSessionHandle)> {
        self.sessions
            .iter()
            .filter(|session| !session.handle.is_finished())
            .map(|session| (session.session_id.clone(), session.handle.clone()))
            .collect()
    }

    pub(super) fn remove(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.sessions
            .retain(|session| session.session_id != session_id);
        self.threads.remove(session_id);
        self.thread_subscriptions.remove(session_id);
        cx.notify();
    }

    pub(super) fn add(&mut self, session: ActiveSession, cx: &mut Context<Self>) {
        self.sessions
            .retain(|existing| existing.session_id != session.session_id);
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
        self.thread_subscriptions
            .insert(session.session_id.clone(), subscription);
        self.threads.insert(session.session_id.clone(), thread);
        self.sessions.push(session);
        cx.emit(event);
        cx.notify();
    }
}
