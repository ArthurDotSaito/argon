mod actions;
mod feed;

use std::collections::HashSet;
use std::path::PathBuf;

use argon_agents::{AgentSessionHandle, ModelInfo};
use argon_core::PrKey;
use argon_store::SessionStatus;
use chrono::{DateTime, TimeDelta, Utc};
use gpui_kit::{App, Context, EventEmitter, SharedString, Task};

use super::card_status::{CardStatus, card_status};
use super::finding_set::{FindingSet, SeverityCounts};
use super::transcript::Transcript;
use crate::services::{AppServices, Services};

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub pr: PrKey,
    pub number: Option<u64>,
    pub agent_label: &'static str,
    pub model: String,
    pub model_label: String,
    pub worktree: Option<PathBuf>,
    pub created_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadEvent {
    Activity,
    ModelsAvailable(Vec<ModelInfo>),
}

pub struct ThreadModel {
    info: SessionInfo,
    handle: Option<AgentSessionHandle>,
    stored_status: SessionStatus,
    closed: bool,
    transcript: Transcript,
    findings: FindingSet,
    models: Vec<ModelInfo>,
    loading: bool,
    load_error: Option<SharedString>,
    action_error: Option<SharedString>,
    busy_findings: HashSet<i64>,
    changing_model: bool,
    services: Option<Services>,
    feed: Option<Task<()>>,
}

impl EventEmitter<ThreadEvent> for ThreadModel {}

impl ThreadModel {
    pub fn live(info: SessionInfo, handle: AgentSessionHandle, cx: &mut Context<Self>) -> Self {
        let mut this = Self::empty(info, Some(handle), SessionStatus::Running, cx);
        this.start_feed(cx);
        this
    }

    pub fn historical(info: SessionInfo, status: SessionStatus, cx: &mut Context<Self>) -> Self {
        let mut this = Self::empty(info, None, status, cx);
        this.load_history(cx);
        this
    }

    fn empty(
        info: SessionInfo,
        handle: Option<AgentSessionHandle>,
        stored_status: SessionStatus,
        cx: &App,
    ) -> Self {
        Self {
            info,
            handle,
            stored_status,
            closed: false,
            transcript: Transcript::default(),
            findings: FindingSet::default(),
            models: Vec::new(),
            loading: true,
            load_error: None,
            action_error: None,
            busy_findings: HashSet::new(),
            changing_model: false,
            services: cx
                .try_global::<AppServices>()
                .map(|global| global.0.clone()),
            feed: None,
        }
    }

    pub fn info(&self) -> &SessionInfo {
        &self.info
    }

    pub fn transcript(&self) -> &Transcript {
        &self.transcript
    }

    pub fn findings(&self) -> &FindingSet {
        &self.findings
    }

    pub fn models(&self) -> &[ModelInfo] {
        &self.models
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn load_error(&self) -> Option<&SharedString> {
        self.load_error.as_ref()
    }

    pub fn action_error(&self) -> Option<&SharedString> {
        self.action_error.as_ref()
    }

    pub fn is_finding_busy(&self, id: i64) -> bool {
        self.busy_findings.contains(&id)
    }

    pub fn is_changing_model(&self) -> bool {
        self.changing_model
    }

    pub fn is_live(&self) -> bool {
        !self.closed
            && self
                .handle
                .as_ref()
                .is_some_and(|handle| !handle.is_finished())
    }

    pub fn status(&self) -> SessionStatus {
        if self.handle.is_some() {
            self.transcript.status()
        } else {
            self.stored_status
        }
    }

    pub fn card_status(&self) -> CardStatus {
        card_status(
            self.status(),
            self.is_live(),
            self.transcript.pending_count(),
        )
    }

    pub fn counts(&self) -> SeverityCounts {
        self.findings.counts()
    }

    pub fn elapsed(&self, now: DateTime<Utc>) -> TimeDelta {
        let end = if self.card_status().is_active() {
            now
        } else {
            self.info.ended_at.unwrap_or(now)
        };
        end - self.info.created_at
    }

    pub fn dismiss_error(&mut self, cx: &mut Context<Self>) {
        if self.action_error.take().is_some() {
            cx.notify();
        }
    }

    fn activity(&self) -> (CardStatus, SeverityCounts) {
        (self.card_status(), self.counts())
    }

    fn notify_activity(&mut self, before: (CardStatus, SeverityCounts), cx: &mut Context<Self>) {
        if self.activity() != before {
            if !self.card_status().is_active() {
                self.info.ended_at = Some(Utc::now());
            }
            cx.emit(ThreadEvent::Activity);
        }
        cx.notify();
    }

    fn set_model_id(&mut self, model: String) {
        if model.is_empty() {
            return;
        }
        self.info.model_label = self
            .models
            .iter()
            .find(|info| info.id == model)
            .map_or_else(|| model.clone(), |info| info.label.clone());
        self.info.model = model;
    }
}
