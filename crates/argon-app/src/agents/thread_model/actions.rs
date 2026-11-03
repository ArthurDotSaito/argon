use argon_agents::SessionError;
use argon_store::FindingState;
use gpui_kit::{Context, SharedString};

use super::{ThreadEvent, ThreadModel};
use crate::agents::finding_set::draft_for;
use crate::agents::panel_labels as labels;
use crate::agents::sessions::AgentSessions;
use crate::agents::transcript::PermissionState;
use crate::review::DraftsBus;
use crate::services::flatten;

impl ThreadModel {
    fn fail_action(&mut self, error: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.action_error = Some(error.into());
        cx.notify();
    }

    pub fn respond(
        &mut self,
        request_id: &str,
        option_id: &str,
        label: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let before = self.activity();
        match handle.respond_permission(request_id, option_id) {
            Ok(()) => self
                .transcript
                .resolve(request_id, PermissionState::Answered(label.to_owned())),
            Err(SessionError::UnknownPermission(_)) => self
                .transcript
                .resolve(request_id, PermissionState::Expired),
            Err(error) => {
                self.fail_action(labels::action_failed(&error.to_string()), cx);
                return;
            }
        }
        self.action_error = None;
        self.notify_activity(before, cx);
    }

    pub fn send(&mut self, text: String, cx: &mut Context<Self>) -> bool {
        let Some(handle) = self.handle.clone() else {
            return false;
        };
        match handle.prompt(text) {
            Ok(()) => {
                self.action_error = None;
                cx.notify();
                true
            }
            Err(SessionError::Busy) => {
                self.fail_action(labels::BUSY, cx);
                false
            }
            Err(error) => {
                self.fail_action(labels::action_failed(&error.to_string()), cx);
                false
            }
        }
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        if let Err(error) = handle.cancel() {
            self.fail_action(labels::action_failed(&error.to_string()), cx);
        }
    }

    pub fn change_model(&mut self, model: String, cx: &mut Context<Self>) {
        let (Some(handle), Some(services)) = (self.handle.clone(), self.services.clone()) else {
            return;
        };
        if self.changing_model || model == self.info.model {
            return;
        }
        self.changing_model = true;
        cx.notify();
        let target = model.clone();
        let job = services.spawn_runtime(async move { handle.set_model(&target).await });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            _ = this.update(cx, |this, cx| {
                this.changing_model = false;
                match result {
                    Ok(Ok(())) => {
                        this.action_error = None;
                        this.set_model_id(model);
                    }
                    Ok(Err(SessionError::Busy)) => this.action_error = Some(labels::BUSY.into()),
                    Ok(Err(error)) => {
                        this.action_error = Some(labels::action_failed(&error.to_string()).into())
                    }
                    Err(error) => {
                        this.action_error = Some(labels::action_failed(&error.to_string()).into())
                    }
                }
                cx.emit(ThreadEvent::Activity);
                cx.notify();
            });
        })
        .detach();
    }

    pub fn add_finding(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(entry) = self.findings.get(id).cloned() else {
            return;
        };
        let Some(services) = self.services.clone() else {
            self.fail_action(labels::SERVICES_UNAVAILABLE, cx);
            return;
        };
        if !self.busy_findings.insert(id) {
            return;
        }
        cx.notify();
        let worktrees = AgentSessions::global(cx).map(|sessions| sessions.read(cx).worktrees());
        let head = self
            .info
            .worktree
            .clone()
            .zip(worktrees)
            .map(|(path, worktrees)| {
                services.spawn_runtime(async move { worktrees.head_oid(&path).await })
            });
        let pr = self.info.pr.clone();
        let session = self.info.session_id.clone();
        let store = services.store();
        cx.spawn(async move |this, cx| {
            let head_oid = match head {
                Some(job) => match job.await {
                    Ok(Ok(oid)) => Some(oid),
                    Ok(Err(error)) => {
                        tracing::warn!(%error, "failed to read session worktree head");
                        None
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to read session worktree head");
                        None
                    }
                },
                None => None,
            };
            let draft = draft_for(&entry.finding, pr.clone(), &session, head_oid);
            let result = flatten(
                store
                    .call(move |store| {
                        store.add_draft(&draft)?;
                        store.set_finding_state(id, FindingState::Added)
                    })
                    .await,
            );
            _ = this.update(cx, |this, cx| {
                this.busy_findings.remove(&id);
                this.finish_finding(id, FindingState::Added, result, cx);
                DraftsBus::notify(pr, cx);
            });
        })
        .detach();
    }

    pub fn ignore_finding(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(services) = self.services.clone() else {
            self.fail_action(labels::SERVICES_UNAVAILABLE, cx);
            return;
        };
        if !self.busy_findings.insert(id) {
            return;
        }
        cx.notify();
        let store = services.store();
        cx.spawn(async move |this, cx| {
            let result = flatten(
                store
                    .call(move |store| store.set_finding_state(id, FindingState::Ignored))
                    .await,
            );
            _ = this.update(cx, |this, cx| {
                this.busy_findings.remove(&id);
                this.finish_finding(id, FindingState::Ignored, result, cx);
            });
        })
        .detach();
    }

    fn finish_finding(
        &mut self,
        id: i64,
        state: FindingState,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        let before = self.activity();
        match result {
            Ok(()) => {
                self.findings.set_state(id, state);
                self.action_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "failed to update agent finding");
                self.action_error = Some(labels::action_failed(&error).into());
            }
        }
        self.notify_activity(before, cx);
    }
}
