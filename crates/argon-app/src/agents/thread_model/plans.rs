use argon_core::{PrKey, SessionKind};
use argon_store::PlanDraft;
use chrono::{DateTime, Utc};
use gpui_kit::{App, Context, SharedString, Task};

use super::ThreadModel;
use crate::agents::panel_labels as labels;
use crate::agents::plan_card::plan_comment_body;
use crate::inbox::InboxModel;
use crate::pr::{SentComment, is_transport_error};
use crate::review::DraftsBus;
use crate::services::{Services, flatten};
use crate::workspace::Workspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanBusy {
    Resolving,
    Publishing,
}

#[derive(Default)]
pub struct PlanDrafts {
    drafts: Vec<PlanDraft>,
    busy: Option<(i64, PlanBusy)>,
    error: Option<(i64, SharedString)>,
}

struct Published {
    comment_id: String,
    comment_url: String,
    issue_updated_at: Option<DateTime<Utc>>,
}

fn mark_issue_read(
    services: &Services,
    key: PrKey,
    updated_at: Option<DateTime<Utc>>,
    cx: &mut App,
) {
    let view = Workspace::global(cx)
        .and_then(|workspace| workspace.upgrade())
        .and_then(|workspace| workspace.read(cx).issue_view(&key, cx));
    if let Some(view) = view {
        view.update(cx, |view, cx| view.reload_and_mark_read(cx));
        return;
    }
    let Some(inbox) = InboxModel::global(cx) else {
        return;
    };
    if let Some(updated_at) = updated_at {
        inbox.update(cx, |inbox, cx| inbox.mark_seen(&key, updated_at, cx));
        services.sync_now();
        return;
    }
    let id = key.id.clone();
    let reload = services.spawn_github(cx, key.host.clone(), move |client| async move {
        client.issue_detail(&id).await
    });
    let services = services.clone();
    cx.spawn(async move |cx| match reload.await {
        Ok(detail) => {
            let updated_at = detail.issue.updated_at;
            inbox.update(cx, |inbox, cx| inbox.mark_seen(&key, updated_at, cx));
            services.sync_now();
        }
        Err(error) => tracing::warn!(%error, "failed to reload the issue after publishing a plan"),
    })
    .detach();
}

impl ThreadModel {
    pub fn plan_draft(&self, id: i64) -> Option<&PlanDraft> {
        self.plans.drafts.iter().find(|draft| draft.id == id)
    }

    pub fn is_latest_plan(&self, id: i64) -> bool {
        self.plans.drafts.iter().map(|draft| draft.id).max() == Some(id)
    }

    pub fn plan_busy(&self, id: i64) -> Option<PlanBusy> {
        self.plans
            .busy
            .filter(|(busy, _)| *busy == id)
            .map(|(_, busy)| busy)
    }

    pub fn plan_error(&self, id: i64) -> Option<&SharedString> {
        self.plans
            .error
            .as_ref()
            .filter(|(failed, _)| *failed == id)
            .map(|(_, error)| error)
    }

    pub(super) fn reload_plans(&mut self, cx: &mut Context<Self>) {
        if self.info.kind != SessionKind::Plan {
            return;
        }
        let Some(services) = self.services.clone() else {
            return;
        };
        let id = self.info.session_id.clone();
        cx.spawn(async move |this, cx| {
            let stored = flatten(
                services
                    .store()
                    .call(move |store| store.plans_for_session(&id))
                    .await,
            );
            _ = this.update(cx, |this, cx| match stored {
                Ok(drafts) => {
                    this.plans.drafts = drafts;
                    cx.notify();
                }
                Err(error) => tracing::warn!(%error, "failed to load plan drafts"),
            });
        })
        .detach();
    }

    fn plans_changed(&self, cx: &mut App) {
        DraftsBus::notify(self.info.pr.clone(), cx);
    }

    fn fail_plan(&mut self, id: i64, error: String, cx: &mut Context<Self>) {
        self.plans.busy = None;
        self.plans.error = Some((id, error.into()));
        cx.notify();
    }

    pub fn save_plan(
        &mut self,
        id: i64,
        body: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), SharedString>> {
        let Some(services) = self.services.clone() else {
            return Task::ready(Err(labels::SERVICES_UNAVAILABLE.into()));
        };
        cx.spawn(async move |this, cx| {
            let saved_body = body.clone();
            let saved = flatten(
                services
                    .store()
                    .call(move |store| store.edit_plan(id, &saved_body))
                    .await,
            );
            match saved {
                Ok(()) => {
                    _ = this.update(cx, |this, cx| {
                        if let Some(draft) =
                            this.plans.drafts.iter_mut().find(|draft| draft.id == id)
                        {
                            draft.body = body;
                            draft.edited = true;
                        }
                        if this.plan_error(id).is_some() {
                            this.plans.error = None;
                        }
                        cx.notify();
                        this.plans_changed(cx);
                    });
                    Ok(())
                }
                Err(error) => {
                    tracing::warn!(%error, "failed to save the edited plan");
                    Err(labels::plan_save_failed(&error).into())
                }
            }
        })
    }

    pub fn resolve_plan_version(&mut self, id: i64, use_agent: bool, cx: &mut Context<Self>) {
        if self.plans.busy.is_some() {
            return;
        }
        let Some(services) = self.services.clone() else {
            self.fail_plan(id, labels::SERVICES_UNAVAILABLE.to_owned(), cx);
            return;
        };
        self.plans.busy = Some((id, PlanBusy::Resolving));
        self.plans.error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let resolved = flatten(
                services
                    .store()
                    .call(move |store| store.resolve_agent_version(id, use_agent))
                    .await,
            );
            _ = this.update(cx, |this, cx| match resolved {
                Ok(()) => {
                    this.plans.busy = None;
                    this.plans_changed(cx);
                    cx.notify();
                }
                Err(error) => {
                    tracing::warn!(%error, "failed to resolve the agent plan version");
                    this.fail_plan(id, labels::plan_update_failed(&error), cx);
                }
            });
        })
        .detach();
    }

    pub fn publish_plan(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.plans.busy.is_some() {
            return;
        }
        let Some(draft) = self
            .plan_draft(id)
            .filter(|draft| draft.published_at.is_none() && draft.agent_body.is_none())
            .cloned()
        else {
            return;
        };
        let Some(services) = self.services.clone() else {
            self.fail_plan(id, labels::SERVICES_UNAVAILABLE.to_owned(), cx);
            return;
        };
        let Some(login) = services
            .accounts()
            .into_iter()
            .find(|account| account.host.eq_ignore_ascii_case(&draft.host))
            .map(|account| account.login)
        else {
            self.fail_plan(
                id,
                labels::publish_failed(&labels::no_account(&draft.host)),
                cx,
            );
            return;
        };
        let comment = plan_comment_body(
            &draft.body,
            self.info.agent_label,
            &self.info.model_label,
            &login,
        );
        self.plans.busy = Some((id, PlanBusy::Publishing));
        self.plans.error = None;
        cx.notify();
        let key = PrKey::new(&draft.host, &draft.issue_id);
        let body_sent = draft.body;
        cx.spawn(async move |this, cx| {
            let reserved = flatten(
                services
                    .store()
                    .call(move |store| store.reserve_plan_publish(id, &body_sent, Utc::now()))
                    .await,
            );
            match reserved {
                Ok(true) => {}
                Ok(false) => {
                    tracing::info!(draft = id, "plan draft changed before publishing");
                    _ = this.update(cx, |this, cx| {
                        this.fail_plan(id, labels::PLAN_CHANGED.to_owned(), cx);
                        this.reload_plans(cx);
                    });
                    return;
                }
                Err(error) => {
                    tracing::warn!(%error, "failed to reserve the plan draft for publishing");
                    _ = this.update(cx, |this, cx| {
                        this.fail_plan(id, labels::publish_failed(&error), cx)
                    });
                    return;
                }
            }
            let sent = SentComment::new(login, comment);
            let error = match deliver(&services, &key, sent).await {
                Delivery::Landed(published) => {
                    tracing::info!(draft = id, "plan published as an issue comment");
                    let (comment_id, comment_url) =
                        (published.comment_id.clone(), published.comment_url.clone());
                    let recorded = flatten(
                        services
                            .store()
                            .call(move |store| {
                                store.complete_plan_publish(id, &comment_id, &comment_url)
                            })
                            .await,
                    );
                    _ = this.update(cx, |this, cx| {
                        mark_issue_read(&services, key, published.issue_updated_at, cx);
                        match recorded {
                            Ok(()) => {
                                this.plans.busy = None;
                                cx.notify();
                            }
                            Err(error) => {
                                tracing::warn!(%error, "failed to record the published plan");
                                this.fail_plan(id, labels::published_not_recorded(&error), cx);
                            }
                        }
                        this.plans_changed(cx);
                    });
                    return;
                }
                Delivery::Unconfirmed(error) => {
                    tracing::warn!(%error, "could not confirm whether the plan was published");
                    labels::publish_unconfirmed(&error)
                }
                Delivery::Failed(error) => {
                    tracing::warn!(%error, "failed to publish the plan");
                    let released = flatten(
                        services
                            .store()
                            .call(move |store| store.release_plan_publish(id))
                            .await,
                    );
                    if let Err(release_error) = released {
                        tracing::warn!(error = %release_error, "failed to release the plan draft");
                    }
                    labels::publish_failed(&error)
                }
            };
            _ = this.update(cx, |this, cx| {
                this.fail_plan(id, error, cx);
                this.plans_changed(cx);
            });
        })
        .detach();
    }
}

enum Delivery {
    Landed(Published),
    Failed(String),
    Unconfirmed(String),
}

async fn deliver(services: &Services, key: &PrKey, sent: SentComment) -> Delivery {
    let issue = key.id.clone();
    let body = sent.body().to_owned();
    let send = services.spawn_github_detached(key.host.clone(), move |client| async move {
        client.add_comment(&issue, &body).await
    });
    let error = match send.await {
        Ok(comment) => {
            return Delivery::Landed(Published {
                comment_id: comment.id,
                comment_url: comment.url,
                issue_updated_at: None,
            });
        }
        Err(error) if is_transport_error(&error) => error.to_string(),
        Err(error) => return Delivery::Failed(error.to_string()),
    };
    tracing::warn!(%error, "plan comment may have landed; checking the issue");
    let issue = key.id.clone();
    let check = services.spawn_github_detached(key.host.clone(), move |client| async move {
        client.issue_detail(&issue).await
    });
    let detail = match check.await {
        Ok(detail) => detail,
        Err(check_error) => {
            tracing::warn!(error = %check_error, "failed to check the issue for the plan comment");
            return Delivery::Unconfirmed(error);
        }
    };
    match sent.landed_id(&detail.timeline).map(str::to_owned) {
        Some(comment_id) => Delivery::Landed(Published {
            comment_id,
            comment_url: detail.issue.url.clone(),
            issue_updated_at: Some(detail.issue.updated_at),
        }),
        None => Delivery::Failed(error),
    }
}
