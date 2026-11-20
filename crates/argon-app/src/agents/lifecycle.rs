use std::path::PathBuf;
use std::time::{Duration, Instant};

use argon_agents::{AgentSessionHandle, WorktreeManager};
use argon_store::{SessionRow, SessionStatus, Store};
use gpui_kit::{App, Context};

use super::sessions::{AgentSessions, AgentsEvent, Closing, ResumeState};
use super::sink::SessionStore;
use crate::services::{Services, StoreHandle, flatten};

enum CloseStart {
    Done,
    InProgress,
    Work(Vec<(String, AgentSessionHandle)>, Vec<String>),
}

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

fn archive_paths(
    worktree: Option<PathBuf>,
    active_related: Vec<PathBuf>,
    stored_related: &[PathBuf],
) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for path in worktree
        .into_iter()
        .chain(active_related)
        .chain(stored_related.iter().cloned())
    {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

fn removable_related(row: Option<SessionRow>) -> Vec<PathBuf> {
    row.map(|row| row.related_worktrees).unwrap_or_default()
}

async fn stored_related(store: &StoreHandle, id: &str) -> Result<Vec<PathBuf>, String> {
    let id = id.to_owned();
    let row = flatten(store.call(move |store| store.session(&id)).await)?;
    Ok(removable_related(row))
}

async fn remove_all(worktrees: &WorktreeManager, paths: &[PathBuf]) -> Result<(), String> {
    let mut failure = None;
    for path in paths {
        if let Err(error) = worktrees.remove(path).await {
            failure.get_or_insert_with(|| error.to_string());
        }
    }
    failure.map_or(Ok(()), Err)
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
        match self.start_close() {
            CloseStart::Done => true,
            CloseStart::InProgress => false,
            CloseStart::Work(handles, launches) => {
                self.shut_down(handles, launches, services, |cx| cx.quit(), cx);
                false
            }
        }
    }

    pub fn shutdown_then(
        &mut self,
        services: &Services,
        after: impl FnOnce(&mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        match self.start_close() {
            CloseStart::Done => after(cx),
            CloseStart::InProgress => {}
            CloseStart::Work(handles, launches) => {
                self.shut_down(handles, launches, services, after, cx);
            }
        }
    }

    pub fn reopen(&mut self) {
        if self.closing() == Closing::Done {
            self.set_closing(Closing::Open);
        }
    }

    fn start_close(&mut self) -> CloseStart {
        match self.closing() {
            Closing::Done => return CloseStart::Done,
            Closing::ShuttingDown => return CloseStart::InProgress,
            Closing::Open => {}
        }
        let handles = self.live_handles();
        let launches = self.pending_launches();
        if handles.is_empty() && launches.is_empty() {
            self.set_closing(Closing::Done);
            return CloseStart::Done;
        }
        CloseStart::Work(handles, launches)
    }

    fn shut_down(
        &mut self,
        handles: Vec<(String, AgentSessionHandle)>,
        launches: Vec<String>,
        services: &Services,
        after: impl FnOnce(&mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
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
                after(cx);
            });
        })
        .detach();
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
        let active_related = self.related_worktrees(&session_id);
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
            let stored = stored_related(&store, &id).await;
            let paths = archive_paths(
                worktree,
                active_related,
                stored.as_ref().map(Vec::as_slice).unwrap_or_default(),
            );
            let removed = remove_all(&worktrees, &paths).await.and(stored.map(|_| ()));
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
        store_with_roots(id, Vec::new(), Vec::new())
    }

    fn store_with_roots(id: &str, related: Vec<PathBuf>, local: Vec<PathBuf>) -> Store {
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
                transport: "acp".into(),
                permission_mode: "supervised".into(),
                nonce: String::new(),
                allowed_files: Vec::new(),
                kind: argon_core::SessionKind::Review,
                base_sha: None,
                related_worktrees: related,
                repo: None,
                pr_number: None,
                refresh_project: false,
                local_roots: local,
            })
            .unwrap();
        store
    }

    #[test]
    fn archiving_never_removes_a_local_folder() {
        let related = PathBuf::from(r"C:\wt\rel-fila");
        let local = PathBuf::from(r"D:\src\pacientes-api");
        let store = store_with_roots("s1", vec![related.clone()], vec![local.clone()]);

        let stored = removable_related(store.session("s1").unwrap());
        let paths = archive_paths(Some(PathBuf::from(r"C:\wt\pr-1")), Vec::new(), &stored);

        assert_eq!(paths, [PathBuf::from(r"C:\wt\pr-1"), related]);
        assert!(!paths.contains(&local));
        assert!(removable_related(None).is_empty());
    }

    #[test]
    fn archiving_removes_the_pr_worktree_and_every_related_one_once() {
        let pr = PathBuf::from(r"C:\wt\pr-1");
        let fila = PathBuf::from(r"C:\wt\rel-fila");
        let pacientes = PathBuf::from(r"C:\wt\rel-pacientes");
        let paths = archive_paths(
            Some(pr.clone()),
            vec![fila.clone()],
            &[fila.clone(), pacientes.clone()],
        );
        assert_eq!(paths, [pr, fila, pacientes]);
        assert!(archive_paths(None, Vec::new(), &[]).is_empty());
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
