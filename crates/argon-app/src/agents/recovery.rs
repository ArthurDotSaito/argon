use std::path::PathBuf;
use std::time::SystemTime;

use argon_agents::{AgentEvent, WorktreeManager, orphan_worktrees};
use argon_store::{SessionStatus, StaleWorktree, Store};
use chrono::{TimeDelta, Utc};
use gpui_kit::{App, AsyncApp, Entity};

use super::catalog::SessionCatalog;
use super::lifecycle::APP_CLOSED_REASON;
use super::sessions::AgentSessions;
use crate::services::{Services, StoreHandle, flatten};

const STALE_AFTER: TimeDelta = TimeDelta::days(7);

struct Recovered {
    stale: Vec<StaleWorktree>,
    known: Vec<PathBuf>,
}

fn recover_store(store: &mut Store) -> argon_store::Result<Recovered> {
    for id in store.fail_orphaned_sessions(APP_CLOSED_REASON)? {
        tracing::info!(session = %id, "marked orphaned agent session as failed");
        store.append_event(&id, &AgentEvent::Error(APP_CLOSED_REASON.to_owned()))?;
    }
    Ok(Recovered {
        stale: store.stale_worktrees(Utc::now() - STALE_AFTER)?,
        known: store.worktree_paths()?,
    })
}

async fn remove_stale(worktrees: &WorktreeManager, store: &StoreHandle, stale: Vec<StaleWorktree>) {
    for StaleWorktree {
        session_id,
        path,
        archive,
    } in stale
    {
        let present = tokio::fs::try_exists(&path).await.unwrap_or(false);
        if present && let Err(error) = worktrees.remove(&path).await {
            tracing::warn!(session = %session_id, %error, "failed to remove stale worktree");
            continue;
        }
        if present {
            tracing::info!(
                session = %session_id,
                path = %path.display(),
                "removed worktree of a finished agent session"
            );
        }
        if archive {
            let id = session_id.clone();
            let stored = store
                .call(move |store| store.set_session_status(&id, SessionStatus::Archived))
                .await;
            match flatten(stored) {
                Ok(()) => tracing::info!(session = %session_id, "finished pending archive"),
                Err(error) => tracing::warn!(session = %session_id, %error, "failed to archive"),
            }
        }
    }
}

async fn remove_orphans(worktrees: &WorktreeManager, known: Vec<PathBuf>, started: SystemTime) {
    let scanner = worktrees.clone();
    let Ok(found) = tokio::task::spawn_blocking(move || scanner.scan()).await else {
        return;
    };
    for path in orphan_worktrees(&found, &known, started) {
        match worktrees.discard(&path).await {
            Ok(()) => tracing::info!(
                path = %path.display(),
                "removed worktree without an agent session"
            ),
            Err(error) => tracing::warn!(path = %path.display(), %error, "failed to remove orphan"),
        }
    }
}

fn reload_catalog(cx: &AsyncApp) {
    cx.update(|cx| {
        if let Some(catalog) = SessionCatalog::global(cx) {
            catalog.update(cx, |catalog, cx| catalog.reload(cx));
        }
    });
}

impl AgentSessions {
    pub fn recover(sessions: &Entity<Self>, services: &Services, cx: &mut App) {
        let started = SystemTime::now();
        let worktrees = sessions.read(cx).worktrees();
        let store = services.store();
        let services = services.clone();
        cx.spawn(async move |cx| {
            let recovered = match flatten(store.call(recover_store).await) {
                Ok(recovered) => recovered,
                Err(error) => {
                    tracing::warn!(%error, "failed to recover agent sessions");
                    return;
                }
            };
            reload_catalog(cx);
            let job = async move {
                remove_stale(&worktrees, &store, recovered.stale).await;
                remove_orphans(&worktrees, recovered.known, started).await;
            };
            _ = services.spawn_runtime(job).await;
            reload_catalog(cx);
        })
        .detach();
    }
}
