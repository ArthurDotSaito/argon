use gpui_kit::Context;

use super::model::DraftsModel;
use crate::services::flatten;

impl DraftsModel {
    pub(super) fn reload_drafts(&mut self, cx: &mut Context<Self>) {
        let (Some(services), Some(key)) = (self.services.clone(), self.key.clone()) else {
            return;
        };
        self.reload_task = Some(cx.spawn(async move |this, cx| {
            let loaded_key = key.clone();
            let loaded = flatten(services.store().call(move |store| store.drafts(&key)).await);
            _ = this.update(cx, |this, cx| {
                if this.key.as_ref() != Some(&loaded_key) {
                    return;
                }
                match loaded {
                    Ok(drafts) => this.drafts = drafts,
                    Err(error) => tracing::warn!(%error, "failed to reload review drafts"),
                }
                cx.notify();
            });
        }));
    }
}
