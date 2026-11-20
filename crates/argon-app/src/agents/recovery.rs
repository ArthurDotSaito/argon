use std::path::PathBuf;
use std::time::SystemTime;

use argon_agents::{AgentEvent, WorktreeManager, orphan_worktrees};
use argon_store::{SessionStatus, StaleWorktree, Store};
use chrono::{DateTime, TimeDelta, Utc};
use gpui_kit::{App, AsyncApp, Entity};

use super::catalog::SessionCatalog;
use super::lifecycle::APP_CLOSED_REASON;
use super::sessions::AgentSessions;
use crate::services::{Services, StoreHandle, flatten};

const STALE_AFTER: TimeDelta = TimeDelta::days(7);

#[derive(Debug, PartialEq, Eq)]
struct StaleSession {
    session_id: String,
    paths: Vec<PathBuf>,
    archive: bool,
}

struct Recovered {
    stale: Vec<StaleSession>,
    known: Vec<PathBuf>,
}

fn stale_sessions(store: &Store, before: DateTime<Utc>) -> argon_store::Result<Vec<StaleSession>> {
    store
        .stale_worktrees(before)?
        .into_iter()
        .map(
            |StaleWorktree {
                 session_id,
                 path,
                 archive,
             }| {
                let related = store
                    .session(&session_id)?
                    .map(|row| row.related_worktrees)
                    .unwrap_or_default();
                Ok(StaleSession {
                    session_id,
                    paths: std::iter::once(path).chain(related).collect(),
                    archive,
                })
            },
        )
        .collect()
}

fn recover_store(store: &mut Store) -> argon_store::Result<Recovered> {
    for id in store.fail_orphaned_sessions(APP_CLOSED_REASON)? {
        tracing::info!(session = %id, "marked orphaned agent session as failed");
        store.append_event(&id, &AgentEvent::Error(APP_CLOSED_REASON.to_owned()))?;
    }
    Ok(Recovered {
        stale: stale_sessions(store, Utc::now() - STALE_AFTER)?,
        known: store.worktree_paths()?,
    })
}

async fn remove_stale(worktrees: &WorktreeManager, store: &StoreHandle, stale: Vec<StaleSession>) {
    for StaleSession {
        session_id,
        paths,
        archive,
    } in stale
    {
        let mut failed = false;
        for path in &paths {
            let present = tokio::fs::try_exists(path).await.unwrap_or(false);
            if present && let Err(error) = worktrees.remove(path).await {
                tracing::warn!(session = %session_id, %error, "failed to remove stale worktree");
                failed = true;
                continue;
            }
            if present {
                tracing::info!(
                    session = %session_id,
                    path = %path.display(),
                    "removed worktree of a finished agent session"
                );
            }
        }
        if failed {
            continue;
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

#[cfg(test)]
mod tests {
    use argon_core::{AgentKind, PrKey, SessionKind};
    use argon_store::NewAgentSession;

    use super::*;

    fn summary_session(id: &str, related: Vec<PathBuf>) -> NewAgentSession {
        NewAgentSession {
            id: id.into(),
            pr: PrKey::new("github.com", "PR_1"),
            agent: AgentKind::ClaudeCode,
            model: "default".into(),
            template: "resumo-padrao".into(),
            worktree_path: Some(PathBuf::from(r"C:\wt\pr-1")),
            acp_session_id: Some("acp".into()),
            transport: "acp".into(),
            permission_mode: "supervised".into(),
            nonce: String::new(),
            allowed_files: Vec::new(),
            kind: SessionKind::Summary,
            base_sha: None,
            related_worktrees: related,
            repo: Some("acme/api".into()),
            pr_number: Some(1),
            refresh_project: false,
            local_roots: Vec::new(),
        }
    }

    fn finished_store(related: Vec<PathBuf>) -> Store {
        finished_store_with(summary_session("s1", related))
    }

    fn finished_store_with(session: NewAgentSession) -> Store {
        let mut store = Store::open_in_memory().unwrap();
        store.create_session(&session).unwrap();
        store
            .set_session_status("s1", SessionStatus::Completed)
            .unwrap();
        store
    }

    #[test]
    fn stale_sessions_include_the_related_worktrees() {
        let related = vec![PathBuf::from(r"C:\wt\rel-a"), PathBuf::from(r"C:\wt\rel-b")];
        let store = finished_store(related.clone());
        let stale = stale_sessions(&store, Utc::now() + TimeDelta::days(1)).unwrap();
        assert_eq!(
            stale,
            [StaleSession {
                session_id: "s1".into(),
                paths: vec![
                    PathBuf::from(r"C:\wt\pr-1"),
                    related[0].clone(),
                    related[1].clone()
                ],
                archive: false,
            }]
        );
    }

    #[test]
    fn cleaning_finished_sessions_never_touches_a_local_folder() {
        let related = PathBuf::from(r"C:\wt\rel-a");
        let local = PathBuf::from(r"D:\src\pacientes-api");
        let store = finished_store_with(NewAgentSession {
            local_roots: vec![local.clone()],
            ..summary_session("s1", vec![related.clone()])
        });

        let stale = stale_sessions(&store, Utc::now() + TimeDelta::days(1)).unwrap();
        let known = store.worktree_paths().unwrap();

        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].paths, [PathBuf::from(r"C:\wt\pr-1"), related]);
        assert!(!stale[0].paths.contains(&local));
        assert!(!known.contains(&local));
    }

    #[test]
    fn related_worktrees_are_known_so_the_orphan_scan_keeps_them() {
        let related = vec![PathBuf::from(r"C:\wt\rel-a")];
        let store = finished_store(related.clone());
        let known = store.worktree_paths().unwrap();
        assert!(known.contains(&PathBuf::from(r"C:\wt\pr-1")));
        assert!(known.contains(&related[0]));
    }
}
