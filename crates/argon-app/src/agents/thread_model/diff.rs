use std::sync::Arc;

use argon_core::Finding;
use gpui_kit::Context;

use super::ThreadModel;
use crate::agents::diff_lines::DiffLines;
use crate::inbox::InboxModel;

impl ThreadModel {
    pub fn finding_in_diff(&self, finding: &Finding) -> Option<bool> {
        self.diff_lines
            .as_ref()
            .map(|lines| lines.contains(&finding.file, finding.line, finding.side))
    }

    pub(super) fn ensure_diff(&mut self, cx: &mut Context<Self>) {
        if self.diff_requested || self.findings.is_empty() {
            return;
        }
        let Some(services) = self.services.clone() else {
            return;
        };
        let Some(pr) =
            InboxModel::global(cx).and_then(|inbox| inbox.read(cx).pr(&self.info.pr).cloned())
        else {
            return;
        };
        self.diff_requested = true;
        let (repo, number) = (pr.repo.clone(), pr.number);
        let files = services.spawn_github(cx, pr.host.clone(), move |client| async move {
            client.pr_files(&repo, number).await
        });
        cx.spawn(async move |this, cx| {
            let files = match files.await {
                Ok(files) => files,
                Err(error) => {
                    tracing::warn!(%error, "failed to load PR files for agent findings");
                    return;
                }
            };
            let lines = DiffLines::from_patches(
                files
                    .iter()
                    .map(|file| (file.path.as_str(), file.patch.as_deref())),
            );
            _ = this.update(cx, |this, cx| {
                this.diff_lines = Some(Arc::new(lines));
                cx.notify();
            });
        })
        .detach();
    }
}
