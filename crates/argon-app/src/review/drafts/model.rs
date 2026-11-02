use std::time::Duration;

use argon_core::{DraftComment, DraftReview, PrKey, ReviewEvent, build_submission};
use argon_store::Store;
use gpui_kit::{Context, EventEmitter, SharedString, Task};

use super::summary::{head_moved, review_error};
use crate::review::labels;
use crate::services::{AppServices, Services, StoreHandle};

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
    services: Option<Services>,
    key: Option<PrKey>,
    head: Option<String>,
    session_head: Option<String>,
    drafts: Vec<DraftComment>,
    review: DraftReview,
    submit: SubmitState,
    epoch: u64,
    load_task: Option<Task<()>>,
    save_task: Option<Task<()>>,
    submit_task: Option<Task<()>>,
}

impl EventEmitter<DraftsEvent> for DraftsModel {}

impl DraftsModel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            services: cx
                .try_global::<AppServices>()
                .map(|global| global.0.clone()),
            key: None,
            head: None,
            session_head: None,
            drafts: Vec::new(),
            review: empty_review(),
            submit: SubmitState::Idle,
            epoch: 0,
            load_task: None,
            save_task: None,
            submit_task: None,
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
        head_moved(self.session_head.as_deref(), self.head.as_deref())
    }

    pub fn set_pr(&mut self, key: PrKey, head: String, cx: &mut Context<Self>) {
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
        let head = self.head.clone();
        let stored = draft.clone();
        let job = self.store_job(move |store| store.add_draft(&stored));
        cx.spawn(async move |this, cx| {
            let id = job.await?;
            _ = this.update(cx, |this, cx| {
                if this.session_head.is_none() {
                    this.session_head = head;
                }
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
                    Ok(()) => this.drafts.retain(|draft| draft.id != id),
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
                    Ok(()) => this.reset(),
                    Err(error) => cx.emit(DraftsEvent::Failed(error)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn submit(&mut self, body: String, event: ReviewEvent, cx: &mut Context<Self>) {
        if self.is_sending() {
            return;
        }
        let (Some(services), Some(key), Some(head)) =
            (self.services.clone(), self.key.clone(), self.head.clone())
        else {
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
        self.save_review(body, event, cx);
        self.submit = SubmitState::Sending;
        let id = key.id.clone();
        let task = services.spawn_github(cx, key.host.clone(), move |client| async move {
            client.submit_review(&id, &head, &submission).await
        });
        let store = services.store();
        self.submit_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let cleared = match &result {
                Ok(_) => Some(flatten(
                    store.call(move |store| store.clear_review(&key)).await,
                )),
                Err(_) => None,
            };
            _ = this.update(cx, |this, cx| {
                match (result, cleared) {
                    (Ok(_), cleared) => {
                        if let Some(Err(error)) = cleared {
                            tracing::warn!(%error, "failed to clear submitted review drafts");
                        }
                        this.reset();
                        cx.emit(DraftsEvent::Submitted);
                    }
                    (Err(error), _) => {
                        tracing::warn!(%error, "failed to submit review");
                        this.submit =
                            SubmitState::Failed(labels::submit_failed(&error.to_string()).into());
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn reset(&mut self) {
        self.drafts.clear();
        self.review = empty_review();
        self.session_head = None;
        self.submit = SubmitState::Idle;
        self.save_task = None;
    }

    fn store_job<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Store) -> argon_store::Result<R> + Send + 'static,
    ) -> impl Future<Output = Result<R, SharedString>> + 'static {
        store_job(self.services.as_ref().map(Services::store), f)
    }
}

async fn store_job<R: Send + 'static>(
    store: Option<StoreHandle>,
    f: impl FnOnce(&mut Store) -> argon_store::Result<R> + Send + 'static,
) -> Result<R, SharedString> {
    let Some(store) = store else {
        return Err(labels::draft_failed(labels::SERVICES_UNAVAILABLE).into());
    };
    flatten(store.call(f).await).map_err(|error| {
        tracing::warn!(%error, "draft store operation failed");
        labels::draft_failed(&error).into()
    })
}

fn load(
    store: &mut Store,
    key: &PrKey,
) -> argon_store::Result<(Vec<DraftComment>, Option<DraftReview>)> {
    Ok((store.drafts(key)?, store.draft_review(key)?))
}

fn empty_review() -> DraftReview {
    DraftReview {
        body: String::new(),
        event: ReviewEvent::Comment,
    }
}

fn flatten<T, E: std::fmt::Display, F: std::fmt::Display>(
    result: Result<Result<T, E>, F>,
) -> Result<T, String> {
    result
        .map_err(|error| error.to_string())
        .and_then(|inner| inner.map_err(|error| error.to_string()))
}
