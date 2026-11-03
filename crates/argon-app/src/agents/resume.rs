use std::path::PathBuf;

use argon_agents::defaults::agent_defaults;
use argon_agents::{
    AgentSessionHandle, SessionContext, WorktreeManager, agent_spec, resume_session,
};
use argon_core::Config;
use argon_store::SessionRow;
use gpui_kit::{Context, SharedString};

use super::panel_labels as labels;
use super::sessions::{ActiveSession, AgentSessions, ResumeState};
use super::sink::SessionStore;
use crate::inbox::InboxModel;
use crate::services::{Services, flatten};

struct Resumed {
    row: SessionRow,
    worktree: PathBuf,
    head: String,
    handle: AgentSessionHandle,
}

async fn resume(
    row: SessionRow,
    config: Config,
    store: SessionStore,
    worktrees: WorktreeManager,
) -> Result<Resumed, String> {
    let acp_session = row
        .acp_session_id
        .clone()
        .ok_or_else(|| labels::NO_ACP_SESSION.to_owned())?;
    let worktree = row
        .worktree_path
        .clone()
        .ok_or_else(|| labels::WORKTREE_GONE.to_owned())?;
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
    let spec = agent_spec(row.agent, &config);
    let ctx = SessionContext {
        id: row.id.clone(),
        pr: row.pr.clone(),
        template_id: row.template.clone(),
        nonce: row.nonce.clone(),
        allowed_files: row.allowed_files.clone(),
        cwd: worktree.clone(),
        model: row.model.clone(),
    };
    let (_, handle) = resume_session(&spec, &acp_session, ctx, writer)
        .await
        .map_err(|error| error.to_string())?;
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
            resume(row, config, session_store, worktrees).await
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
        let number =
            InboxModel::global(cx).and_then(|inbox| inbox.read(cx).pr(&row.pr).map(|pr| pr.number));
        let session_id = row.id.clone();
        let pr = row.pr.clone();
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
                handle,
                head_oid: Some(head.clone()),
                created_at: row.created_at,
            },
            cx,
        );
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
