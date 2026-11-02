use std::sync::Arc;

use gpui_kit::Context;
use gpui_kit::component::ActiveTheme as _;

use super::{Files, ReviewScope, ReviewTab};
use crate::review::file_diff::{FileDiff, prepare_all};
use crate::review::file_tree::TreeFile;
use crate::review::highlight::{highlight_file, language_for};
use crate::review::labels;
use crate::services::AppServices;

impl ReviewTab {
    pub(super) fn load_files(&mut self, cx: &mut Context<Self>) {
        self.epoch += 1;
        self.load_task = None;
        self.highlight_tasks.clear();
        let Some(detail) = self.detail.clone() else {
            self.files = Files::Idle;
            return;
        };
        let Some(services) = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone())
        else {
            self.files = Files::Failed(labels::files_failed(labels::SERVICES_UNAVAILABLE).into());
            return;
        };
        self.files = Files::Loading;
        self.error = None;
        self.diff.update(cx, |diff, cx| diff.clear(cx));
        let epoch = self.epoch;
        let scope = self.scope.clone();
        let repo = detail.pr.repo.clone();
        let number = detail.pr.number;
        let id = detail.pr.id.clone();
        let head = detail.head_oid.clone();
        let task = services.spawn_github(cx, detail.pr.host.clone(), move |client| async move {
            match scope {
                ReviewScope::All => {
                    let files = client.pr_files(&repo, number).await?;
                    let viewed = match client.viewed_files(&id).await {
                        Ok(viewed) => Some(viewed),
                        Err(error) => {
                            tracing::warn!(%error, "failed to load viewed files");
                            None
                        }
                    };
                    Ok((files, viewed))
                }
                ReviewScope::Commit { oid, .. } => {
                    Ok((client.commit_files(&repo, &oid).await?, None))
                }
            }
        });
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let prepared = match task.await {
                Ok((files, viewed)) => Ok(cx
                    .background_executor()
                    .spawn(async move { prepare_all(files, viewed.as_ref()) })
                    .await),
                Err(error) => Err(error),
            };
            _ = this.update(cx, |this, cx| {
                if epoch != this.epoch {
                    return;
                }
                match prepared {
                    Ok(files) => this.apply_files(files, head, cx),
                    Err(error) => {
                        tracing::warn!(%error, "failed to load review files");
                        this.files = Files::Failed(labels::files_failed(&error.to_string()).into());
                        cx.notify();
                    }
                }
            });
        }));
        cx.notify();
    }

    fn apply_files(&mut self, files: Vec<FileDiff>, head: String, cx: &mut Context<Self>) {
        self.files = Files::Ready;
        let all = self.scope == ReviewScope::All;
        self.loaded_head = all.then_some(head);
        let url = self.detail.as_ref().map(|detail| match &self.scope {
            ReviewScope::All => format!("{}/files", detail.pr.url),
            ReviewScope::Commit { oid, .. } => format!("{}/commits/{oid}", detail.pr.url),
        });
        let tree_files = files
            .iter()
            .map(|file| TreeFile {
                path: file.meta.path.clone(),
                status: file.meta.status,
                additions: file.meta.additions,
                deletions: file.meta.deletions,
                viewed: file.viewed,
            })
            .collect();
        self.tree
            .update(cx, |tree, cx| tree.set_files(tree_files, all, cx));
        let jobs: Vec<_> = files
            .iter()
            .filter_map(|file| {
                let language = language_for(&file.meta.path)?;
                let diff = file.body.diff()?.clone();
                Some((file.meta.path.clone(), language, diff))
            })
            .collect();
        self.diff.update(cx, |diff, cx| {
            diff.set_viewed_enabled(all, cx);
            diff.set_github_url(url);
            diff.set_files(files, cx);
        });
        self.sync_threads(cx);
        let theme = cx.theme().highlight_theme.clone();
        let epoch = self.epoch;
        for (path, language, diff) in jobs {
            let theme = theme.clone();
            self.highlight_tasks.push(cx.spawn(async move |this, cx| {
                let highlights = cx
                    .background_executor()
                    .spawn(async move { highlight_file(language, &diff, theme.as_ref()) })
                    .await;
                _ = this.update(cx, |this, cx| {
                    if this.epoch == epoch {
                        this.diff.update(cx, |diff, cx| {
                            diff.set_highlights(&path, Arc::new(highlights), cx)
                        });
                    }
                });
            }));
        }
        cx.notify();
    }

    pub(super) fn toggle_viewed(&mut self, path: String, viewed: bool, cx: &mut Context<Self>) {
        if self.scope != ReviewScope::All {
            return;
        }
        let Some(detail) = self.detail.clone() else {
            return;
        };
        let Some(services) = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone())
        else {
            return;
        };
        self.error = None;
        self.apply_viewed(&path, viewed, cx);
        let id = detail.pr.id.clone();
        let remote_path = path.clone();
        let task = services.spawn_github(cx, detail.pr.host.clone(), move |client| async move {
            client.set_viewed(&id, &remote_path, viewed).await
        });
        let epoch = self.epoch;
        let key = path.clone();
        let job = cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| {
                let Err(error) = result else {
                    return;
                };
                tracing::warn!(%error, path, "failed to update viewed state");
                if epoch == this.epoch {
                    this.apply_viewed(&path, !viewed, cx);
                }
                this.error = Some(labels::viewed_failed(&path, &error.to_string()).into());
                cx.notify();
            });
        });
        self.viewed_tasks.insert(key, job);
    }

    fn apply_viewed(&mut self, path: &str, viewed: bool, cx: &mut Context<Self>) {
        self.diff
            .update(cx, |diff, cx| diff.set_viewed(path, viewed, cx));
        self.tree
            .update(cx, |tree, cx| tree.set_viewed(path, viewed, cx));
        cx.notify();
    }
}
