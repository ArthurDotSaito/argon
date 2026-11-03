use std::path::PathBuf;
use std::time::Duration;

use argon_agents::{AgentEvent, AgentSessionHandle, WorktreeManager};
use argon_store::{SessionStatus, Store};
use chrono::{TimeDelta, Utc};
use gpui_kit::{App, Context, Entity};

use super::sessions::{AgentSessions, AgentsEvent, Closing};
use super::sink::SessionStore;
use crate::services::{Services, StoreHandle, flatten};

pub const APP_CLOSED_REASON: &str = "Argon foi encerrado durante a sessão";
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);
const STALE_AFTER: TimeDelta = TimeDelta::days(7);

fn recover_store(store: &mut Store) -> argon_store::Result<Vec<(String, PathBuf)>> {
    for id in store.fail_orphaned_sessions(APP_CLOSED_REASON)? {
        tracing::info!(session = %id, "marked orphaned agent session as failed");
        store.append_event(&id, &AgentEvent::Error(APP_CLOSED_REASON.to_owned()))?;
    }
    store.stale_worktrees(Utc::now() - STALE_AFTER)
}

async fn remove_stale(worktrees: WorktreeManager, stale: Vec<(String, PathBuf)>) {
    for (session, path) in stale {
        match tokio::fs::try_exists(&path).await {
            Ok(true) => {}
            _ => continue,
        }
        match worktrees.remove(&path).await {
            Ok(()) => tracing::info!(
                %session,
                path = %path.display(),
                "removed worktree of agent session finished more than 7 days ago"
            ),
            Err(error) => tracing::warn!(%session, %error, "failed to remove stale worktree"),
        }
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
    pub fn recover(sessions: &Entity<Self>, services: &Services, cx: &mut App) {
        let worktrees = sessions.read(cx).worktrees();
        let store = services.store();
        let services = services.clone();
        cx.background_executor()
            .spawn(async move {
                match flatten(store.call(recover_store).await) {
                    Ok(stale) if !stale.is_empty() => {
                        _ = services.spawn_runtime(remove_stale(worktrees, stale)).await;
                    }
                    Ok(_) => {}
                    Err(error) => tracing::warn!(%error, "failed to recover agent sessions"),
                }
            })
            .detach();
    }

    pub fn begin_close(&mut self, services: &Services, cx: &mut Context<Self>) -> bool {
        match self.closing() {
            Closing::Done => return true,
            Closing::ShuttingDown => return false,
            Closing::Open => {}
        }
        let handles = self.live_handles();
        if handles.is_empty() {
            self.set_closing(Closing::Done);
            return true;
        }
        tracing::info!(
            count = handles.len(),
            "shutting down agent sessions before quitting"
        );
        self.set_closing(Closing::ShuttingDown);
        let job = services.spawn_runtime(shutdown_all(handles, self.store(), services.store()));
        cx.spawn(async move |this, cx| {
            _ = job.await;
            _ = this.update(cx, |this, cx| {
                this.set_closing(Closing::Done);
                cx.quit();
            });
        })
        .detach();
        false
    }

    pub fn archive(
        &mut self,
        session_id: String,
        worktree: Option<PathBuf>,
        services: &Services,
        cx: &mut Context<Self>,
    ) {
        let handle = self.handle(&session_id);
        let worktrees = self.worktrees();
        let store = services.store();
        let id = session_id.clone();
        let job = services.spawn_runtime(async move {
            if let Some(handle) = handle {
                _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, handle.shutdown()).await;
            }
            let removed = match &worktree {
                Some(path) => worktrees
                    .remove(path)
                    .await
                    .map_err(|error| error.to_string()),
                None => Ok(()),
            };
            let stored = flatten(
                store
                    .call(move |store| store.set_session_status(&id, SessionStatus::Archived))
                    .await,
            );
            removed.and(stored)
        });
        cx.spawn(async move |this, cx| {
            let result = job.await.map_err(|error| error.to_string()).flatten();
            _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => tracing::info!(session = %session_id, "agent session archived"),
                    Err(error) => {
                        tracing::warn!(session = %session_id, %error, "failed to archive session")
                    }
                }
                this.remove(&session_id, cx);
                cx.emit(AgentsEvent::SessionArchived { session_id });
            });
        })
        .detach();
    }
}
