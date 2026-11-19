use std::path::PathBuf;

use argon_core::{MAX_RELATED_REPOS, RelatedRepo, SummaryContext};
use argon_github::PrDetail;
use argon_store::ProjectOverview;
use gpui_kit::Task;

use super::super::labels;
use crate::services::{ServiceError, StoreHandle, flatten};

#[derive(Debug, Clone)]
pub struct SummaryGathering {
    pub detail: PrDetail,
    pub related: Vec<String>,
    pub overview: Option<ProjectOverview>,
}

pub fn related_list(configured: &[String], own_repo: &str) -> Vec<String> {
    let mut related: Vec<String> = Vec::new();
    for repo in configured {
        let repo = repo.trim();
        let duplicate = related
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(repo));
        if repo.is_empty() || repo.eq_ignore_ascii_case(own_repo) || duplicate {
            continue;
        }
        related.push(repo.to_owned());
    }
    related.truncate(MAX_RELATED_REPOS);
    related
}

pub fn summary_context(
    detail: &PrDetail,
    overview: Option<&ProjectOverview>,
    refresh_project: bool,
) -> SummaryContext {
    let pr = &detail.pr;
    let saved = overview.filter(|_| !refresh_project);
    SummaryContext {
        pr_title: pr.title.clone(),
        pr_number: pr.number,
        pr_url: pr.url.clone(),
        pr_body: detail.body.clone(),
        repo: pr.repo.clone(),
        base: pr.base.clone(),
        base_sha: detail.base_oid.clone().unwrap_or_default(),
        head: pr.head.clone(),
        head_sha: detail.head_oid.clone(),
        related: Vec::new(),
        project_overview: saved.map(|overview| overview.body.clone()),
        project_nodes: saved
            .and_then(|overview| overview.diagram.as_ref())
            .map(|graph| {
                graph
                    .nodes
                    .iter()
                    .map(|node| (node.id.clone(), node.label.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        refresh_project,
    }
}

pub fn planned_related(
    repos: &[String],
    planned: impl Fn(&str) -> Result<PathBuf, String>,
) -> Vec<RelatedRepo> {
    repos
        .iter()
        .map(|repo| match planned(repo) {
            Ok(path) => RelatedRepo {
                repo: repo.clone(),
                path: Some(path.display().to_string()),
                branch: labels::PLAN_BASE_PENDING.to_owned(),
                error: None,
            },
            Err(error) => RelatedRepo {
                repo: repo.clone(),
                path: None,
                branch: labels::PLAN_BASE_PENDING.to_owned(),
                error: Some(error),
            },
        })
        .collect()
}

pub fn preview_context(
    gathering: &SummaryGathering,
    refresh_project: bool,
    related: Vec<RelatedRepo>,
) -> SummaryContext {
    SummaryContext {
        head_sha: labels::PLAN_SHA_PENDING.to_owned(),
        related,
        ..summary_context(
            &gathering.detail,
            gathering.overview.as_ref(),
            refresh_project,
        )
    }
}

pub async fn gather_summary(
    detail: Option<PrDetail>,
    fetch: Option<Task<Result<PrDetail, ServiceError>>>,
    store: StoreHandle,
    configured: Vec<String>,
) -> Result<SummaryGathering, String> {
    let detail = match (detail, fetch) {
        (Some(detail), _) => detail,
        (None, Some(fetch)) => fetch.await.map_err(|error| error.to_string())?,
        (None, None) => return Err(labels::SERVICES_UNAVAILABLE.to_owned()),
    };
    let (host, repo) = (detail.pr.host.clone(), detail.pr.repo.clone());
    let overview = flatten(
        store
            .call(move |store| store.project_overview(&host, &repo))
            .await,
    )?;
    Ok(SummaryGathering {
        related: related_list(&configured, &detail.pr.repo),
        detail,
        overview,
    })
}

#[cfg(test)]
mod tests {
    use argon_core::{ArchGraph, ArchNode, Lane, NodeKind, fixtures};

    use super::*;

    fn detail() -> PrDetail {
        PrDetail {
            pr: fixtures::pr(),
            body: "Corrige o slot".into(),
            head_oid: "abc123".into(),
            base_oid: Some("base999".into()),
            labels: Vec::new(),
            reviewers: Vec::new(),
            timeline: Vec::new(),
            truncated_timeline: false,
            threads: Vec::new(),
            truncated_threads: false,
            commits: Vec::new(),
        }
    }

    fn graph() -> ArchGraph {
        let node = |id: &str, label: &str| ArchNode {
            id: id.into(),
            label: label.into(),
            lane: Lane::Project,
            kind: NodeKind::Component,
            repo: None,
            read: true,
        };
        ArchGraph {
            nodes: vec![node("slots", "Slots"), node("fila", "Fila de eventos")],
            edges: Vec::new(),
        }
    }

    fn overview(diagram: Option<ArchGraph>) -> ProjectOverview {
        ProjectOverview {
            host: "github.com".into(),
            repo: "acme/widgets".into(),
            body: "API de agendamento.".into(),
            diagram,
            source_sha: "def456".into(),
            source_pr: 198,
            session_id: "s0".into(),
            updated_at: fixtures::now(),
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn pr_fields_fill_the_context() {
        let context = summary_context(&detail(), None, true);
        assert_eq!(context.pr_title, "Fix slot finder");
        assert_eq!(context.pr_number, 482);
        assert_eq!(context.pr_url, "https://github.com/acme/widgets/pull/482");
        assert_eq!(context.pr_body, "Corrige o slot");
        assert_eq!(context.repo, "acme/widgets");
        assert_eq!(context.base, "main");
        assert_eq!(context.base_sha, "base999");
        assert_eq!(context.head, "fix/slot-finder");
        assert_eq!(context.head_sha, "abc123");
        assert!(context.related.is_empty());
        assert!(context.refresh_project);
    }

    #[test]
    fn missing_base_commit_becomes_empty() {
        let mut detail = detail();
        detail.base_oid = None;
        assert_eq!(summary_context(&detail, None, false).base_sha, "");
    }

    #[test]
    fn saved_overview_and_diagram_nodes_are_reused() {
        let saved = overview(Some(graph()));
        let context = summary_context(&detail(), Some(&saved), false);
        assert_eq!(
            context.project_overview.as_deref(),
            Some("API de agendamento.")
        );
        assert_eq!(
            context.project_nodes,
            [
                ("slots".to_owned(), "Slots".to_owned()),
                ("fila".to_owned(), "Fila de eventos".to_owned()),
            ]
        );
        assert!(!context.refresh_project);
    }

    #[test]
    fn saved_overview_without_diagram_has_no_nodes() {
        let saved = overview(None);
        let context = summary_context(&detail(), Some(&saved), false);
        assert_eq!(
            context.project_overview.as_deref(),
            Some("API de agendamento.")
        );
        assert!(context.project_nodes.is_empty());
    }

    #[test]
    fn rereading_the_project_drops_the_saved_overview() {
        let saved = overview(Some(graph()));
        let context = summary_context(&detail(), Some(&saved), true);
        assert_eq!(context.project_overview, None);
        assert!(context.project_nodes.is_empty());
        assert!(context.refresh_project);
    }

    #[test]
    fn related_list_drops_duplicates_blanks_and_the_own_repo() {
        let configured = strings(&[
            "acme/pacientes-api",
            "Acme/Widgets",
            "acme/pacientes-api",
            "  ",
            "ACME/Pacientes-API",
            "acme/fila",
        ]);
        assert_eq!(
            related_list(&configured, "acme/widgets"),
            ["acme/pacientes-api", "acme/fila"]
        );
        let configured = strings(&["acme/a", "ACME/A", "acme/b"]);
        assert_eq!(related_list(&configured, "acme/x"), ["acme/a", "acme/b"]);
    }

    #[test]
    fn related_list_stops_at_the_limit() {
        let configured: Vec<String> = (0..12).map(|n| format!("acme/repo-{n}")).collect();
        let related = related_list(&configured, "acme/widgets");
        assert_eq!(related.len(), MAX_RELATED_REPOS);
        assert_eq!(related[0], "acme/repo-0");
        assert_eq!(related[MAX_RELATED_REPOS - 1], "acme/repo-7");
    }

    #[test]
    fn planned_related_lists_paths_and_failures() {
        let repos = strings(&["acme/a", "acme/b"]);
        let related = planned_related(&repos, |repo| {
            if repo == "acme/a" {
                Ok(PathBuf::from("wt-a"))
            } else {
                Err("repo inválido".into())
            }
        });
        assert_eq!(related[0].path.as_deref(), Some("wt-a"));
        assert_eq!(related[0].branch, labels::PLAN_BASE_PENDING);
        assert_eq!(related[0].error, None);
        assert_eq!(related[1].path, None);
        assert_eq!(related[1].error.as_deref(), Some("repo inválido"));
    }

    #[test]
    fn preview_marks_the_commit_as_pending() {
        let gathering = SummaryGathering {
            detail: detail(),
            related: Vec::new(),
            overview: None,
        };
        let context = preview_context(&gathering, true, Vec::new());
        assert_eq!(context.head_sha, labels::PLAN_SHA_PENDING);
        assert_eq!(context.base_sha, "base999");
    }
}
