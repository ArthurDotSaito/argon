use argon_core::{PrKey, SessionKind};
use argon_store::SessionRow;
use chrono::{DateTime, Utc};
use gpui_kit::{App, SharedString};

use super::card_status::{CardStatus, card_status, resume_action};
use super::finding_set::SeverityCounts;
use super::grouping::Groupable;
use super::lifecycle::APP_CLOSED_REASON;
use super::panel_labels as labels;
use super::session_actions::SessionControls;
use super::thread_model::ThreadModel;
use super::transcript::{Item, PermissionState};
use crate::bottom::duration_label;
use crate::inbox::InboxModel;

pub struct SessionCard {
    pub id: String,
    pub pr: PrKey,
    pub kind: SessionKind,
    pub number: Option<u64>,
    pub agent: SharedString,
    pub model: SharedString,
    pub status: CardStatus,
    pub counts: SeverityCounts,
    pub elapsed: SharedString,
    pub created_at: DateTime<Utc>,
    pub note: Option<SharedString>,
    pub controls: SessionControls,
}

fn subject_ref(key: &PrKey, kind: SessionKind, cx: &App) -> Option<(String, String, u64)> {
    let inbox = InboxModel::global(cx)?;
    let inbox = inbox.read(cx);
    match kind {
        SessionKind::Review => inbox
            .pr(key)
            .map(|pr| (pr.title.clone(), pr.repo.clone(), pr.number)),
        SessionKind::Plan => inbox
            .issue(key)
            .map(|issue| (issue.title.clone(), issue.repo.clone(), issue.number)),
    }
}

pub fn subject_number(key: &PrKey, kind: SessionKind, cx: &App) -> Option<u64> {
    subject_ref(key, kind, cx).map(|(_, _, number)| number)
}

pub fn subject_labels(
    key: &PrKey,
    kind: SessionKind,
    number: Option<u64>,
    cx: &App,
) -> (SharedString, SharedString) {
    match subject_ref(key, kind, cx) {
        Some((title, repo, number)) => (title.into(), format!("{repo} #{number}").into()),
        None => (
            labels::unknown_subject(kind).into(),
            number.map_or_else(String::new, |n| format!("#{n}")).into(),
        ),
    }
}

impl Groupable for SessionCard {
    fn pr(&self) -> &PrKey {
        &self.pr
    }

    fn status(&self) -> CardStatus {
        self.status
    }

    fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

impl SessionCard {
    pub fn from_thread(thread: &ThreadModel, now: DateTime<Utc>) -> Self {
        let info = thread.info();
        let note = thread
            .transcript()
            .items()
            .iter()
            .find_map(|item| match item {
                Item::Permission(permission) if permission.state == PermissionState::Pending => {
                    Some(SharedString::from(permission.title.clone()))
                }
                _ => None,
            });
        Self {
            id: info.session_id.clone(),
            pr: info.pr.clone(),
            kind: info.kind,
            number: info.number,
            agent: info.agent_label.into(),
            model: info.model_label.clone().into(),
            status: thread.card_status(),
            counts: thread.counts(),
            elapsed: duration_label(thread.elapsed(now)).into(),
            created_at: info.created_at,
            note,
            controls: SessionControls {
                id: info.session_id.clone(),
                status: thread.card_status(),
                resume: thread.resume_action(),
                worktree: info.worktree.clone(),
            },
        }
    }

    pub fn from_row(
        row: &SessionRow,
        counts: SeverityCounts,
        worktree_present: bool,
        cx: &App,
    ) -> Self {
        let info = super::catalog::session_info(row, worktree_present, cx);
        let status = card_status(row.status, false, 0);
        let app_closed = row.failure_reason.as_deref() == Some(APP_CLOSED_REASON);
        let resume = resume_action(
            status,
            info.resumable && info.worktree.is_some(),
            app_closed,
        );
        Self {
            id: row.id.clone(),
            pr: row.pr.clone(),
            kind: row.kind,
            number: info.number,
            agent: info.agent_label.into(),
            model: info.model_label.clone().into(),
            status,
            counts,
            elapsed: match status {
                CardStatus::Archived => SharedString::default(),
                _ => duration_label(row.updated_at - row.created_at).into(),
            },
            created_at: row.created_at,
            note: row.failure_reason.clone().map(SharedString::from),
            controls: SessionControls {
                id: row.id.clone(),
                status,
                resume,
                worktree: info.worktree.clone(),
            },
        }
    }
}
