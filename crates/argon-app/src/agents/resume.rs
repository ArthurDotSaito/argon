use std::path::PathBuf;

use argon_agents::defaults::agent_defaults;
use argon_agents::{
    AgentSessionHandle, SessionContext, Transport, WorktreeManager, agent_spec_for, resume_session,
};
use argon_core::{Config, PermissionMode, SessionKind};
use argon_store::{ARCHIVE_PENDING, SessionRow, SessionStatus};
use gpui_kit::{Context, SharedString};

use super::panel_labels as labels;
use super::sessions::{ActiveSession, AgentSessions, ResumeState};
use super::sessions_list::subject_number;
use super::sink::SessionStore;
use crate::services::{Services, StoreHandle, flatten};

struct Resumed {
    row: SessionRow,
    worktree: PathBuf,
    head: String,
    handle: AgentSessionHandle,
}

fn is_archived(row: &SessionRow) -> bool {
    row.status == SessionStatus::Archived || row.failure_reason.as_deref() == Some(ARCHIVE_PENDING)
}

fn resume_target(row: &SessionRow) -> Result<(String, PathBuf), &'static str> {
    if is_archived(row) {
        return Err(labels::SESSION_ARCHIVED);
    }
    let acp_session = row.acp_session_id.clone().ok_or(labels::NO_ACP_SESSION)?;
    let worktree = row.worktree_path.clone().ok_or(labels::WORKTREE_GONE)?;
    Ok((acp_session, worktree))
}

fn session_context(row: &SessionRow, worktree: PathBuf) -> SessionContext {
    SessionContext {
        id: row.id.clone(),
        pr: row.pr.clone(),
        template_id: row.template.clone(),
        nonce: row.nonce.clone(),
        allowed_files: row.allowed_files.clone(),
        cwd: worktree,
        read_roots: row.related_worktrees.clone(),
        repo: row.repo.clone().unwrap_or_default(),
        number: row.pr_number.unwrap_or_default(),
        refresh_project: row.refresh_project,
        model: row.model.clone(),
        permission_mode: PermissionMode::parse(&row.permission_mode).unwrap_or_default(),
        kind: row.kind,
        base_sha: row.base_sha.clone(),
    }
}

async fn still_resumable(rows: &StoreHandle, id: &str) -> bool {
    let id = id.to_owned();
    match flatten(rows.call(move |store| store.session(&id)).await) {
        Ok(Some(row)) => !is_archived(&row),
        _ => false,
    }
}

async fn resume(
    row: SessionRow,
    config: Config,
    store: SessionStore,
    worktrees: WorktreeManager,
    rows: StoreHandle,
) -> Result<Resumed, String> {
    let (acp_session, worktree) = resume_target(&row).map_err(str::to_owned)?;
    let transport = Transport::parse(&row.transport)
        .ok_or_else(|| labels::unknown_transport(&row.transport))?;
    let spec = agent_spec_for(row.agent, &config, transport)
        .ok_or_else(|| labels::transport_unavailable(&row.transport))?;
    if !tokio::fs::try_exists(&worktree).await.unwrap_or(false) {
        return Err(labels::WORKTREE_GONE.to_owned());
    }
    let head = worktrees
        .head_oid(&worktree)
        .await
        .map_err(|error| labels::worktree_invalid(&error.to_string()))?;
    let writer = tokio::task::spawn_blocking(move || store.writer())
        .await
        .map_err(|error| error.to_string())??;
    let ctx = session_context(&row, worktree.clone());
    let (_, handle) = resume_session(&spec, transport, &acp_session, ctx, writer)
        .await
        .map_err(|error| error.to_string())?;
    if !still_resumable(&rows, &row.id).await {
        handle.shutdown().await;
        return Err(labels::SESSION_ARCHIVED.to_owned());
    }
    Ok(Resumed {
        row,
        worktree,
        head,
        handle,
    })
}

impl AgentSessions {
    pub fn resume(&mut self, session_id: String, services: &Services, cx: &mut Context<Self>) {
        if matches!(self.resume_state(&session_id), Some(ResumeState::Running)) {
            return;
        }
        self.resumes_mut()
            .insert(session_id.clone(), ResumeState::Running);
        cx.notify();
        let store = services.store();
        let session_store = self.store();
        let worktrees = self.worktrees();
        let config = services.config();
        let id = session_id.clone();
        let job = services.spawn_runtime(async move {
            let row = flatten(store.call(move |store| store.session(&id)).await)?
                .ok_or_else(|| labels::SESSION_MISSING.to_owned())?;
            resume(row, config, session_store, worktrees, store).await
        });
        let services = services.clone();
        cx.spawn(async move |this, cx| {
            let result = job.await.map_err(|error| error.to_string()).flatten();
            _ = this.update(cx, |this, cx| match result {
                Ok(resumed) => {
                    this.resumes_mut().remove(&session_id);
                    this.adopt(resumed, &services, cx);
                }
                Err(error) => {
                    tracing::warn!(session = %session_id, %error, "failed to resume agent session");
                    this.resumes_mut()
                        .insert(session_id, ResumeState::Failed(error));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn adopt(&mut self, resumed: Resumed, services: &Services, cx: &mut Context<Self>) {
        let Resumed {
            row,
            worktree,
            head,
            handle,
        } = resumed;
        let number = subject_number(&row.pr, row.kind, cx);
        let session_id = row.id.clone();
        let pr = row.pr.clone();
        let kind = row.kind;
        let related_worktrees = row.related_worktrees.clone();
        self.add(
            ActiveSession {
                session_id: row.id,
                pr: row.pr,
                number,
                agent: row.agent,
                agent_label: agent_defaults(row.agent).label,
                model_label: row.model_label.unwrap_or_else(|| row.model.clone()),
                model: row.model,
                worktree,
                related_worktrees,
                handle,
                head_oid: Some(head.clone()),
                created_at: row.created_at,
            },
            cx,
        );
        if kind == SessionKind::Plan {
            return;
        }
        let check = services.spawn_github(cx, pr.host.clone(), move |client| async move {
            client.pr_detail(&pr.id).await
        });
        cx.spawn(async move |this, cx| {
            let Ok(detail) = check.await else {
                return;
            };
            if detail.head_oid == head {
                return;
            }
            _ = this.update(cx, |this, cx| {
                if let Some(thread) = this.thread(&session_id) {
                    thread.update(cx, |thread, cx| {
                        thread.set_notice(Some(SharedString::from(labels::HEAD_MOVED)), cx)
                    });
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use argon_core::{AgentKind, PrKey, SessionKind};

    use super::*;

    fn row(status: SessionStatus) -> SessionRow {
        SessionRow {
            id: "s1".into(),
            pr: PrKey::new("github.com", "PR_1"),
            agent: AgentKind::ClaudeCode,
            model: "default".into(),
            template: "review-padrao".into(),
            status,
            worktree_path: Some(PathBuf::from(r"C:\wt\pr-1")),
            acp_session_id: Some("acp".into()),
            transport: "acp".into(),
            permission_mode: "supervised".into(),
            nonce: String::new(),
            allowed_files: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            model_label: None,
            failure_reason: None,
            kind: SessionKind::Review,
            base_sha: None,
            related_worktrees: Vec::new(),
            repo: None,
            pr_number: None,
            refresh_project: false,
        }
    }

    #[test]
    fn archived_sessions_are_never_resumed() {
        assert_eq!(
            resume_target(&row(SessionStatus::Archived)),
            Err(labels::SESSION_ARCHIVED)
        );
        let mut pending = row(SessionStatus::Failed);
        pending.failure_reason = Some(ARCHIVE_PENDING.into());
        assert_eq!(resume_target(&pending), Err(labels::SESSION_ARCHIVED));
        assert!(resume_target(&row(SessionStatus::Failed)).is_ok());
    }

    #[test]
    fn resume_needs_acp_id_and_worktree() {
        let mut no_acp = row(SessionStatus::Completed);
        no_acp.acp_session_id = None;
        assert_eq!(resume_target(&no_acp), Err(labels::NO_ACP_SESSION));
        let mut no_worktree = row(SessionStatus::Completed);
        no_worktree.worktree_path = None;
        assert_eq!(resume_target(&no_worktree), Err(labels::WORKTREE_GONE));
    }

    #[test]
    fn resumed_review_session_keeps_review_kind_without_base_sha() {
        let ctx = session_context(&row(SessionStatus::Completed), PathBuf::from("wt"));
        assert_eq!(ctx.kind, SessionKind::Review);
        assert_eq!(ctx.base_sha, None);
        assert_eq!(ctx.cwd, PathBuf::from("wt"));
    }

    #[test]
    fn resumed_plan_session_restores_kind_and_base_sha() {
        let mut plan = row(SessionStatus::Completed);
        plan.pr = PrKey::new("github.com", "I_9");
        plan.kind = SessionKind::Plan;
        plan.base_sha = Some("abc123".into());

        let ctx = session_context(&plan, PathBuf::from("wt"));

        assert_eq!(ctx.kind, SessionKind::Plan);
        assert_eq!(ctx.base_sha.as_deref(), Some("abc123"));
        assert_eq!(ctx.pr, PrKey::new("github.com", "I_9"));
    }

    #[test]
    fn resumed_summary_session_keeps_its_target_and_refresh_choice() {
        let mut summary = row(SessionStatus::Completed);
        summary.kind = SessionKind::Summary;
        summary.base_sha = Some("def456".into());
        summary.related_worktrees = vec![PathBuf::from(r"C:\wt\rel-a")];
        summary.repo = Some("acme/api".into());
        summary.pr_number = Some(42);
        summary.refresh_project = true;

        let ctx = session_context(&summary, PathBuf::from("wt"));

        assert_eq!(ctx.kind, SessionKind::Summary);
        assert_eq!(ctx.repo, "acme/api");
        assert_eq!(ctx.number, 42);
        assert!(ctx.refresh_project);
        assert_eq!(ctx.read_roots, [PathBuf::from(r"C:\wt\rel-a")]);
        assert_eq!(ctx.base_sha.as_deref(), Some("def456"));
    }
}
