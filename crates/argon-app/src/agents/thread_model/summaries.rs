use std::path::{Path, PathBuf};

use argon_core::SessionKind;
use argon_store::{PrSummary, ProjectOverview, Store, StoreError};
use chrono::{DateTime, Utc};
use gpui_kit::Context;

use super::ThreadModel;
use crate::agents::transcript::Item;
use crate::services::flatten;

#[derive(Default)]
pub struct SummaryState {
    items: Vec<PrSummary>,
    overview: Option<ProjectOverview>,
    repo: Option<String>,
    related: Vec<PathBuf>,
}

pub(super) struct SummaryData {
    summaries: Vec<PrSummary>,
    overview: Option<ProjectOverview>,
    repo: Option<String>,
    related: Vec<PathBuf>,
}

pub(super) fn load_summary_data(
    store: &Store,
    session_id: &str,
    host: &str,
) -> Result<SummaryData, StoreError> {
    let session = store.session(session_id)?;
    let summaries = store.summaries_for_session(session_id)?;
    let (repo, related) = session
        .map(|row| (row.repo, row.related_worktrees))
        .unwrap_or_default();
    let overview = match &repo {
        Some(repo) => store.project_overview(host, repo)?,
        None => None,
    };
    Ok(SummaryData {
        summaries,
        overview,
        repo,
        related,
    })
}

impl SummaryState {
    pub(super) fn record(
        &mut self,
        id: i64,
        body: &str,
        pr: (&str, &str),
        session_id: &str,
        now: DateTime<Utc>,
    ) -> bool {
        if self.items.iter().any(|summary| summary.id == id) {
            return false;
        }
        self.items.push(PrSummary {
            id,
            host: pr.0.to_owned(),
            pr_id: pr.1.to_owned(),
            session_id: session_id.to_owned(),
            body: body.to_owned(),
            head_sha: String::new(),
            touched: Vec::new(),
            created_at: now,
        });
        true
    }

    pub(super) fn load(&mut self, data: SummaryData) {
        let SummaryData {
            summaries,
            overview,
            repo,
            related,
        } = data;
        self.items = summaries;
        self.overview = overview;
        self.repo = repo;
        self.related = related;
    }
}

impl ThreadModel {
    pub fn summary(&self, id: i64) -> Option<&PrSummary> {
        self.summaries.items.iter().find(|summary| summary.id == id)
    }

    pub fn project_overview(&self) -> Option<&ProjectOverview> {
        self.summaries.overview.as_ref()
    }

    pub fn project_repo(&self) -> Option<&str> {
        self.summaries.repo.as_deref()
    }

    pub fn related_roots(&self) -> &[PathBuf] {
        &self.summaries.related
    }

    pub fn project_refreshed(&self) -> bool {
        self.transcript.items().iter().any(|item| {
            matches!(
                item,
                Item::Summary {
                    project_updated: true,
                    ..
                }
            )
        })
    }

    pub(super) fn record_summary(&mut self, id: i64, body: &str) -> bool {
        self.summaries.record(
            id,
            body,
            (&self.info.pr.host, &self.info.pr.id.0),
            &self.info.session_id,
            Utc::now(),
        )
    }

    pub(super) fn reload_summaries(&mut self, cx: &mut Context<Self>) {
        if self.info.kind != SessionKind::Summary {
            return;
        }
        let Some(services) = self.services.clone() else {
            return;
        };
        let id = self.info.session_id.clone();
        let host = self.info.pr.host.clone();
        cx.spawn(async move |this, cx| {
            let loaded = flatten(
                services
                    .store()
                    .call(move |store| load_summary_data(store, &id, &host))
                    .await,
            );
            _ = this.update(cx, |this, cx| match loaded {
                Ok(data) => {
                    this.summaries.load(data);
                    cx.notify();
                }
                Err(error) => tracing::warn!(%error, "failed to load session summaries"),
            });
        })
        .detach();
    }
}

pub fn related_label(path: &Path) -> String {
    let folder = path.file_name().map(|name| name.to_string_lossy());
    let repo = path.parent().and_then(|parent| {
        let name = parent.file_name()?.to_string_lossy();
        let owner = parent.parent()?.file_name()?.to_string_lossy();
        Some(format!("{owner}/{name}"))
    });
    match (repo, folder) {
        (Some(repo), _) => repo,
        (None, Some(folder)) => folder.into_owned(),
        (None, None) => path.display().to_string(),
    }
}

pub fn is_in_related(title: &str, roots: &[PathBuf], case_insensitive: bool) -> bool {
    let haystack = normalize(title, case_insensitive);
    roots.iter().any(|root| {
        let root = normalize(&root.to_string_lossy(), case_insensitive);
        let root = root.trim_end_matches('/');
        !root.is_empty() && contains_path(&haystack, root)
    })
}

fn normalize(text: &str, case_insensitive: bool) -> String {
    let text = text.replace('\\', "/");
    if case_insensitive {
        text.to_lowercase()
    } else {
        text
    }
}

fn contains_path(haystack: &str, root: &str) -> bool {
    haystack.match_indices(root).any(|(start, _)| {
        let before = haystack[..start].chars().next_back();
        let after = haystack[start + root.len()..].chars().next();
        before.is_none_or(|ch| ch.is_whitespace() || matches!(ch, '"' | '\'' | '(' | '=' | '`'))
            && after.is_none_or(|ch| {
                ch == '/' || ch.is_whitespace() || matches!(ch, '"' | '\'' | ')' | '`')
            })
    })
}

#[cfg(test)]
mod tests;
