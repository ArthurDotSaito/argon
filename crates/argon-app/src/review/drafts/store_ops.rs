use argon_core::{DraftComment, DraftReview, PrKey, ReviewEvent};
use argon_store::Store;
use gpui_kit::SharedString;

use crate::review::labels;
use crate::services::{StoreHandle, flatten};

pub async fn store_job<R: Send + 'static>(
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

pub struct SentReview {
    pub ids: Vec<i64>,
    pub body: String,
}

pub fn clear_sent(
    store: &mut Store,
    key: &PrKey,
    ids: &[i64],
    body: &str,
) -> argon_store::Result<()> {
    store.delete_drafts(key, ids)?;
    if store
        .draft_review(key)?
        .is_some_and(|review| review.body == body)
    {
        store.clear_draft_review(key)?;
    }
    Ok(())
}

pub fn load(
    store: &mut Store,
    key: &PrKey,
) -> argon_store::Result<(Vec<DraftComment>, Option<DraftReview>)> {
    Ok((store.drafts(key)?, store.draft_review(key)?))
}

pub fn empty_review() -> DraftReview {
    DraftReview {
        body: String::new(),
        event: ReviewEvent::Comment,
    }
}
