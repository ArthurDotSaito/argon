use std::time::Duration;

use argon_core::{DraftComment, DraftReview, PrKey, ReviewEvent, build_submission};
use argon_store::Store;
use gpui_kit::component::notification::Notification;
use gpui_kit::{AnyWindowHandle, Context, EventEmitter, SharedString, Subscription, Task};

use super::bus::{DraftsBus, DraftsChanged};
use super::store_ops::{SentReview, clear_sent, empty_review, load, store_job};
use super::summary::{head_moved, review_error, suspect_drafts};
use crate::review::labels;
use crate::review::mutation::run_detached;
use crate::services::{AppServices, Services, flatten};

const SAVE_DELAY: Duration = Duration::from_millis(600);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmitState {
    Idle,
    Sending,
    Failed(SharedString),
}

pub enum DraftsEvent {
    Submitted,
    Failed(SharedString),
}

pub struct DraftsModel {
    pub(super) services: Option<Services>,
    pub(super) key: Option<PrKey>,
    number: u64,
    head: Option<String>,
    diff_head: Option<String>,
    pub(super) drafts: Vec<DraftComment>,
    review: DraftReview,
    submit: SubmitState,
    suspects: Vec<i64>,
    epoch: u64,
    load_task: Option<Task<()>>,
    pub(super) reload_task: Option<Task<()>>,
    save_task: Option<Task<()>>,
    _bus: Subscription,
}

impl EventEmitter<DraftsEvent> for DraftsModel {}

impl DraftsModel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let bus = DraftsBus::global(cx);
        let bus = cx.subscribe(&bus, |this, _, event: &DraftsChanged, cx| {
            if this.key.as_ref() == Some(&event.pr) {
                this.reload_drafts(cx);
            }
        });
        Self {
            services: cx
                .try_global::<AppServices>()
                .map(|global| global.0.clone()),
            key: None,
            number: 0,
            head: None,
            diff_head: None,
            drafts: Vec::new(),
            review: empty_review(),
            submit: SubmitState::Idle,
            suspects: Vec::new(),
            epoch: 0,
            load_task: None,
            reload_task: None,
            save_task: None,
            _bus: bus,
        }
    }

    pub fn drafts(&self) -> &[DraftComment] {
        &self.drafts
    }

    pub fn review(&self) -> &DraftReview {
        &self.review
    }

    pub fn submit_state(&self) -> &SubmitState {
        &self.submit
    }

    pub fn is_sending(&self) -> bool {
        self.submit == SubmitState::Sending
    }

    pub fn in_progress(&self) -> bool {
        !self.drafts.is_empty() || !self.review.body.trim().is_empty()
    }

    pub fn head_moved(&self) -> bool {
        head_moved(&self.drafts, self.diff_head.as_deref())
    }

    pub fn stale(&self) -> bool {
        self.diff_head.is_none() || self.diff_head != self.head
    }

    pub fn suspects(&self) -> &[i64] {
        &self.suspects
    }

    pub fn set_diff_head(&mut self, head: Option<String>, cx: &mut Context<Self>) {
        if self.diff_head != head {
            self.diff_head = head;
            cx.notify();
        }
    }

    pub fn set_pr(&mut self, key: PrKey, number: u64, head: String, cx: &mut Context<Self>) {
        self.number = number;
        self.head = Some(head);
        if self.key.as_ref() == Some(&key) {
            cx.notify();
            return;
        }
        self.key = Some(key.clone());
        self.epoch += 1;
        let epoch = self.epoch;
        let Some(services) = self.services.clone() else {
            return;
        };
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let loaded = flatten(services.store().call(move |store| load(store, &key)).await);
            _ = this.update(cx, |this, cx| {
                if this.epoch != epoch {
                    return;
                }
                match loaded {
                    Ok((drafts, review)) => {
                        this.drafts = drafts;
                        this.review = review.unwrap_or_else(empty_review);
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to load review drafts");
                        cx.emit(DraftsEvent::Failed(
                            labels::drafts_load_failed(&error).into(),
                        ));
                    }
                }
                cx.notify();
            });
        }));
    }

    pub fn add(
        &mut self,
        draft: DraftComment,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), SharedString>> {
        let stored = draft.clone();
        let job = self.store_job(move |store| store.add_draft(&stored));
        cx.spawn(async move |this, cx| {
            let id = job.await?;
            _ = this.update(cx, |this, cx| {
                this.drafts.push(DraftComment { id, ..draft });
                cx.notify();
            });
            Ok(())
        })
    }

    pub fn update_body(
        &mut self,
        id: i64,
        body: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), SharedString>> {
        let stored = body.clone();
        let job = self.store_job(move |store| store.update_draft(id, &stored));
        cx.spawn(async move |this, cx| {
            job.await?;
            _ = this.update(cx, |this, cx| {
                if let Some(draft) = this.drafts.iter_mut().find(|draft| draft.id == id) {
                    draft.body = body;
                }
                cx.notify();
            });
            Ok(())
        })
    }

    pub fn delete(&mut self, id: i64, cx: &mut Context<Self>) {
        let job = self.store_job(move |store| store.delete_draft(id));
        cx.spawn(async move |this, cx| {
            let result = job.await;
            _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.drafts.retain(|draft| draft.id != id);
                        this.suspects.retain(|suspect| *suspect != id);
                        this.announce(cx);
                    }
                    Err(error) => cx.emit(DraftsEvent::Failed(error)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn save_review(&mut self, body: String, event: ReviewEvent, cx: &mut Context<Self>) {
        if self.review.body == body && self.review.event == event {
            return;
        }
        self.review = DraftReview {
            body: body.clone(),
            event,
        };
        if matches!(self.submit, SubmitState::Failed(_)) {
            self.submit = SubmitState::Idle;
        }
        cx.notify();
        let Some(key) = self.key.clone() else {
            return;
        };
        let job = self.store_job(move |store| store.save_draft_review(&key, &body, event));
        self.save_task = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            if let Err(error) = job.await {
                tracing::warn!(%error, "failed to save draft review");
            }
        }));
    }

    pub fn discard(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.key.clone() else {
            return;
        };
        self.save_task = None;
        let job = self.store_job(move |store| store.clear_review(&key));
        cx.spawn(async move |this, cx| {
            let result = job.await;
            _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.reset();
                        this.announce(cx);
                    }
                    Err(error) => cx.emit(DraftsEvent::Failed(error)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn submit(
        &mut self,
        body: String,
        event: ReviewEvent,
        window: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        if self.is_sending() {
            return;
        }
        if self.stale() {
            self.submit = SubmitState::Failed(labels::DIFF_STALE.into());
            cx.notify();
            return;
        }
        let (Some(services), Some(key), Some(head)) = (
            self.services.clone(),
            self.key.clone(),
            self.diff_head.clone(),
        ) else {
            self.submit =
                SubmitState::Failed(labels::submit_failed(labels::SERVICES_UNAVAILABLE).into());
            cx.notify();
            return;
        };
        let submission = match build_submission(&body, event, &self.drafts) {
            Ok(submission) => submission,
            Err(error) => {
                self.submit = SubmitState::Failed(review_error(&error, &self.drafts).into());
                cx.notify();
                return;
            }
        };
        self.save_review(body.clone(), event, cx);
        self.save_task = None;
        let store = services.store();
        let saved_key = key.clone();
        let saved_body = body.clone();
        store.submit(move |store| {
            if let Err(error) = store.save_draft_review(&saved_key, &saved_body, event) {
                tracing::warn!(%error, "failed to save draft review before submitting");
            }
        });
        self.submit = SubmitState::Sending;
        self.suspects.clear();
        let ids: Vec<i64> = self.drafts.iter().map(|draft| draft.id).collect();
        let sent = SentReview {
            ids: ids.clone(),
            body: body.clone(),
        };
        let number = self.number;
        let id = key.id.clone();
        let request = services.spawn_github_detached(key.host.clone(), move |client| async move {
            client.submit_review(&id, &head, &submission).await
        });
        let job = async move {
            let result = request.await;
            if result.is_ok() {
                let cleanup = flatten(
                    store
                        .call(move |store| clear_sent(store, &key, &ids, &body))
                        .await,
                );
                if let Err(error) = cleanup {
                    tracing::warn!(%error, "failed to clear submitted review drafts");
                }
            }
            result.map_err(|error| error.to_string())
        };
        run_detached(
            window,
            cx,
            job,
            move |this, result, cx| {
                match result {
                    Ok(_) => {
                        this.drafts.retain(|draft| !sent.ids.contains(&draft.id));
                        if this.review.body == sent.body {
                            this.review = empty_review();
                        }
                        this.submit = SubmitState::Idle;
                        cx.emit(DraftsEvent::Submitted);
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to submit review");
                        this.suspects = suspect_drafts(&error, &this.drafts);
                        this.submit = SubmitState::Failed(labels::submit_failed(&error).into());
                    }
                }
                cx.notify();
            },
            move |result| match result {
                Ok(_) => Notification::success(labels::review_sent_in(number)),
                Err(error) => Notification::error(labels::submit_failed_in(number, &error)),
            },
        );
        cx.notify();
    }

    fn reset(&mut self) {
        self.drafts.clear();
        self.review = empty_review();
        self.submit = SubmitState::Idle;
        self.suspects.clear();
        self.save_task = None;
    }

    fn store_job<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Store) -> argon_store::Result<R> + Send + 'static,
    ) -> impl Future<Output = Result<R, SharedString>> + 'static {
        store_job(self.services.as_ref().map(Services::store), f)
    }
}
