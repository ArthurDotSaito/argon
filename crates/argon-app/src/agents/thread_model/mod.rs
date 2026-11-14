mod actions;
mod diff;
mod feed;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use argon_agents::{AgentSessionHandle, ModelInfo};
use argon_core::{PermissionMode, PrKey, SessionKind};
use argon_store::SessionStatus;
use chrono::{DateTime, TimeDelta, Utc};
use gpui_kit::{App, Context, EventEmitter, SharedString, Subscription, Task};

use super::card_status::{CardStatus, ResumeAction, card_status, resume_action};
use super::diff_lines::DiffLines;
use super::finding_set::{FindingSet, SeverityCounts};
use super::lifecycle::APP_CLOSED_REASON;
use super::transcript::Transcript;
use crate::review::{DraftsBus, DraftsChanged};
use crate::services::{AppServices, Services, flatten};

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub pr: PrKey,
    pub kind: SessionKind,
    pub number: Option<u64>,
    pub agent_label: &'static str,
    pub model: String,
    pub model_label: String,
    pub worktree: Option<PathBuf>,
    pub created_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub resumable: bool,
    pub failure_reason: Option<String>,
    pub head_oid: Option<String>,
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
    changing_permission_mode: bool,
    services: Option<Services>,
    feed: Option<Task<()>>,
    notice: Option<SharedString>,
    diff_lines: Option<Arc<DiffLines>>,
    diff_requested: bool,
    _drafts: Option<Subscription>,
}

impl EventEmitter<ThreadEvent> for ThreadModel {}

impl ThreadModel {
    pub fn live(info: SessionInfo, handle: AgentSessionHandle, cx: &mut Context<Self>) -> Self {
        let mut this = Self::empty(info, Some(handle), SessionStatus::Running, cx);
        this.watch_drafts(cx);
        this.start_feed(cx);
        this
    }

    pub fn historical(info: SessionInfo, status: SessionStatus, cx: &mut Context<Self>) -> Self {
        let mut this = Self::empty(info, None, status, cx);
        this.watch_drafts(cx);
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
            changing_permission_mode: false,
            services: cx
                .try_global::<AppServices>()
                .map(|global| global.0.clone()),
            feed: None,
            notice: None,
            diff_lines: None,
            diff_requested: false,
            _drafts: None,
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

    pub fn is_permission_mode_locked(&self) -> bool {
        self.info.kind == SessionKind::Plan
    }

    pub fn is_changing_permission_mode(&self) -> bool {
        self.changing_permission_mode
    }

    pub fn permission_mode(&self) -> Option<PermissionMode> {
        self.transcript.permission_mode()
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
        self.ensure_diff(cx);
        cx.notify();
    }

    pub fn notice(&self) -> Option<&SharedString> {
        self.notice.as_ref()
    }

    pub fn set_notice(&mut self, notice: Option<SharedString>, cx: &mut Context<Self>) {
        self.notice = notice;
        cx.notify();
    }

    pub fn resume_action(&self) -> Option<ResumeAction> {
        if self.is_live() || self.info.worktree.is_none() {
            return None;
        }
        let app_closed = self.info.failure_reason.as_deref() == Some(APP_CLOSED_REASON);
        resume_action(self.card_status(), self.info.resumable, app_closed)
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
        if let (Some(_), Some(services)) = (&self.handle, &self.services) {
            let id = self.info.session_id.clone();
            let label = self.info.model_label.clone();
            services.store().submit(move |store| {
                if let Err(error) = store.set_session_model_label(&id, &label) {
                    tracing::warn!(%error, "failed to store session model label");
                }
            });
        }
    }

    fn watch_drafts(&mut self, cx: &mut Context<Self>) {
        let bus = DraftsBus::global(cx);
        self._drafts = Some(cx.subscribe(&bus, |this, _, event: &DraftsChanged, cx| {
            if event.pr == this.info.pr {
                this.reload_findings(cx);
            }
        }));
    }

    fn reload_findings(&mut self, cx: &mut Context<Self>) {
        let Some(services) = self.services.clone() else {
            return;
        };
        let id = self.info.session_id.clone();
        cx.spawn(async move |this, cx| {
            let stored = flatten(
                services
                    .store()
                    .call(move |store| store.findings(&id))
                    .await,
            );
            _ = this.update(cx, |this, cx| match stored {
                Ok(stored) => {
                    let before = this.activity();
                    this.findings.merge_stored(stored);
                    this.notify_activity(before, cx);
                }
                Err(error) => tracing::warn!(%error, "failed to reload agent findings"),
            });
        })
        .detach();
    }
}
