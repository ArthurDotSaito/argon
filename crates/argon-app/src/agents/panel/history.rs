use argon_agents::defaults::agent_defaults;
use argon_store::{SessionRow, SessionStatus, Store};
use gpui_kit::{App, AppContext as _, Context, Entity};

use super::AgentPanel;
use crate::agents::finding_set::SeverityCounts;
use crate::agents::thread_model::{SessionInfo, ThreadModel};
use crate::inbox::InboxModel;
use crate::services::{AppServices, flatten};

pub struct HistoryRow {
    pub row: SessionRow,
    pub counts: SeverityCounts,
    pub worktree_present: bool,
}

fn load_rows(store: &mut Store) -> argon_store::Result<Vec<HistoryRow>> {
    let mut rows = Vec::new();
    for row in store.sessions()? {
        let findings = store.findings(&row.id)?;
        let counts = SeverityCounts::tally(
            findings
                .iter()
                .map(|stored| (&stored.finding, stored.state)),
        );
        let worktree_present = row
            .worktree_path
            .as_deref()
            .is_some_and(std::path::Path::exists);
        rows.push(HistoryRow {
            row,
            counts,
            worktree_present,
        });
    }
    Ok(rows)
}

pub fn session_info(row: &SessionRow, worktree_present: bool, cx: &App) -> SessionInfo {
    let number =
        InboxModel::global(cx).and_then(|inbox| inbox.read(cx).pr(&row.pr).map(|pr| pr.number));
    SessionInfo {
        session_id: row.id.clone(),
        pr: row.pr.clone(),
        number,
        agent_label: agent_defaults(row.agent).label,
        model: row.model.clone(),
        model_label: row.model_label.clone().unwrap_or_else(|| row.model.clone()),
        worktree: row.worktree_path.clone().filter(|_| worktree_present),
        created_at: row.created_at,
        ended_at: Some(row.updated_at),
        resumable: agent_defaults(row.agent).load_session
            && row.acp_session_id.is_some()
            && row.status != SessionStatus::Archived,
        failure_reason: row.failure_reason.clone(),
    }
}

impl AgentPanel {
    pub(super) fn reload_history(&mut self, cx: &mut Context<Self>) {
        let Some(services) = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone())
        else {
            return;
        };
        self.history_task = Some(cx.spawn(async move |this, cx| {
            let loaded = flatten(services.store().call(load_rows).await);
            _ = this.update(cx, |this, cx| {
                this.history_loaded = true;
                match loaded {
                    Ok(rows) => this.history = rows,
                    Err(error) => tracing::warn!(%error, "failed to load agent sessions"),
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn historical_thread(
        &mut self,
        session_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ThreadModel>> {
        if let Some(thread) = self.historical.get(session_id) {
            return Some(thread.clone());
        }
        let row = self
            .history
            .iter()
            .find(|history| history.row.id == session_id)?;
        let info = session_info(&row.row, row.worktree_present, cx);
        let status = row.row.status;
        let thread = cx.new(|cx| ThreadModel::historical(info, status, cx));
        self.historical
            .insert(session_id.to_owned(), thread.clone());
        Some(thread)
    }
}
