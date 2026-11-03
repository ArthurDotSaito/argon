use std::path::PathBuf;
use std::time::{Duration, Instant};

use argon_agents::AgentSessionHandle;
use argon_store::{SessionStatus, Store};
use gpui_kit::Context;

use super::sessions::{AgentSessions, AgentsEvent, Closing, ResumeState};
use super::sink::SessionStore;
use crate::services::{Services, StoreHandle, flatten};

pub const APP_CLOSED_REASON: &str = "Argon foi encerrado durante a sessão";
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);
const LAUNCH_CANCEL_WAIT: Duration = Duration::from_secs(5);
const CLOSE_POLL: Duration = Duration::from_millis(100);

fn record_archive(store: &mut Store, id: &str, removed: bool) -> argon_store::Result<()> {
    if removed {
        store.set_session_status(id, SessionStatus::Archived)
    } else {
        store.mark_archive_pending(id)
    }
}

pub async fn shutdown_all(
    handles: Vec<(String, AgentSessionHandle)>,
    sessions: SessionStore,
    store: StoreHandle,
) {
    if handles.is_empty() {
        return;
    }
    let shutdowns = handles.iter().map(|(_, handle)| async move {
        if tokio::time::timeout(SHUTDOWN_TIMEOUT, handle.shutdown())
            .await
            .is_err()
        {
            tracing::warn!("agent session did not shut down in time; the job object kills it");
        }
    });
    futures::future::join_all(shutdowns).await;
    if let Ok(writer) = sessions.writer() {
        _ = tokio::time::timeout(FLUSH_TIMEOUT, writer.flush()).await;
    }
    let ids: Vec<String> = handles.into_iter().map(|(id, _)| id).collect();
    let noted = store
        .call(move |store| store.note_failure_reason(&ids, APP_CLOSED_REASON))
        .await;
    if let Err(error) = flatten(noted) {
        tracing::warn!(%error, "failed to record why agent sessions stopped");
    }
}

impl AgentSessions {
    pub fn begin_close(&mut self, services: &Services, cx: &mut Context<Self>) -> bool {
        match self.closing() {
            Closing::Done => return true,
            Closing::ShuttingDown => return false,
            Closing::Open => {}
        }
        let handles = self.live_handles();
        let launches = self.pending_launches();
        if handles.is_empty() && launches.is_empty() {
            self.set_closing(Closing::Done);
            return true;
        }
        tracing::info!(
            sessions = handles.len(),
            launches = launches.len(),
            "shutting down agent sessions before quitting"
        );
        for id in &launches {
            self.cancel_launch(id, cx);
        }
        self.set_closing(Closing::ShuttingDown);
        let job = services.spawn_runtime(shutdown_all(handles, self.store(), services.store()));
        cx.spawn(async move |this, cx| {
            _ = job.await;
            let deadline = Instant::now() + LAUNCH_CANCEL_WAIT;
            while Instant::now() < deadline {
                let pending = this
                    .update(cx, |this, _| !this.pending_launches().is_empty())
                    .unwrap_or(false);
                if !pending {
                    break;
                }
                cx.background_executor().timer(CLOSE_POLL).await;
            }
            _ = this.update(cx, |this, cx| {
                this.set_closing(Closing::Done);
                cx.quit();
            });
        })
        .detach();
        false
    }

    pub fn can_archive(&self, session_id: &str) -> bool {
        !matches!(self.resume_state(session_id), Some(ResumeState::Running))
            && !self.pending_launches().iter().any(|id| id == session_id)
    }

    pub fn archive(
        &mut self,
        session_id: String,
        worktree: Option<PathBuf>,
        services: &Services,
        cx: &mut Context<Self>,
    ) {
        if !self.can_archive(&session_id) {
            return;
        }
        let handle = self.handle(&session_id);
        let worktrees = self.worktrees();
        let sessions = self.store();
        let store = services.store();
        let id = session_id.clone();
        let job = services.spawn_runtime(async move {
            if let Some(handle) = handle {
                _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, handle.shutdown()).await;
                if let Ok(writer) = sessions.writer() {
                    _ = tokio::time::timeout(FLUSH_TIMEOUT, writer.flush()).await;
                }
            }
            let removed = match &worktree {
                Some(path) => worktrees
                    .remove(path)
                    .await
                    .map_err(|error| error.to_string()),
                None => Ok(()),
            };
            let archived = removed.is_ok();
            let stored = flatten(
                store
                    .call(move |store| record_archive(store, &id, archived))
                    .await,
            );
            removed.and(stored)
        });
        cx.spawn(async move |this, cx| {
            let result = job.await.map_err(|error| error.to_string()).flatten();
            _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => tracing::info!(session = %session_id, "agent session archived"),
                    Err(error) => tracing::warn!(
                        session = %session_id,
                        %error,
                        "archive left pending; it is retried on the next start"
                    ),
                }
                this.remove(&session_id, cx);
                cx.emit(AgentsEvent::SessionArchived { session_id });
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use argon_core::{AgentKind, PrKey};
    use argon_store::{ARCHIVE_PENDING, NewAgentSession};

    use super::*;

    fn store_with(id: &str) -> Store {
        let mut store = Store::open_in_memory().unwrap();
        store
            .create_session(&NewAgentSession {
                id: id.into(),
                pr: PrKey::new("github.com", "PR_1"),
                agent: AgentKind::ClaudeCode,
                model: "default".into(),
                template: "review-padrao".into(),
                worktree_path: Some(PathBuf::from(r"C:\wt\pr-1")),
                acp_session_id: Some("acp".into()),
                nonce: String::new(),
                allowed_files: Vec::new(),
            })
            .unwrap();
        store
    }

    #[test]
    fn failed_worktree_removal_leaves_the_archive_pending() {
        let mut store = store_with("s1");
        record_archive(&mut store, "s1", false).unwrap();
        let row = store.session("s1").unwrap().unwrap();
        assert_ne!(row.status, SessionStatus::Archived);
        assert_eq!(row.failure_reason.as_deref(), Some(ARCHIVE_PENDING));
        record_archive(&mut store, "s1", true).unwrap();
        let row = store.session("s1").unwrap().unwrap();
        assert_eq!(row.status, SessionStatus::Archived);
        assert_eq!(row.failure_reason, None);
    }
}
