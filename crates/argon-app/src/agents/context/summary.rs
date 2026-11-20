use std::path::{Path, PathBuf};

use argon_core::{
    MAX_RELATED_REPOS, RelatedEntry, RelatedRepo, SummaryContext, local_folder_name,
    same_local_folder,
};
use argon_github::PrDetail;
use argon_store::ProjectOverview;
use gpui_kit::Task;

use super::super::labels;
use crate::services::{ServiceError, StoreHandle, flatten};

#[derive(Debug, Clone)]
pub struct SummaryGathering {
    pub detail: PrDetail,
    pub related: Vec<RelatedEntry>,
    pub overview: Option<ProjectOverview>,
}

fn is_same_entry(left: &RelatedEntry, right: &RelatedEntry) -> bool {
    match (left, right) {
        (RelatedEntry::Repo(left), RelatedEntry::Repo(right)) => left.eq_ignore_ascii_case(right),
        (RelatedEntry::Local(left), RelatedEntry::Local(right)) => same_local_folder(left, right),
        _ => false,
    }
}

pub fn related_list(configured: &[RelatedEntry], own_repo: &str) -> Vec<RelatedEntry> {
    let mut related: Vec<RelatedEntry> = Vec::new();
    for entry in configured {
        let own = matches!(entry, RelatedEntry::Repo(repo) if repo.eq_ignore_ascii_case(own_repo));
        let duplicate = related
            .iter()
            .any(|existing| is_same_entry(existing, entry));
        if !own && !duplicate {
            related.push(entry.clone());
        }
    }
    related.truncate(MAX_RELATED_REPOS);
    related
}

pub fn local_related(path: &Path) -> RelatedRepo {
    RelatedRepo {
        repo: local_folder_name(path),
        path: Some(path.display().to_string()),
        branch: String::new(),
        error: (!path.is_dir()).then(|| labels::LOCAL_FOLDER_GONE.to_owned()),
        local: true,
    }
}

pub fn related_repos(entries: &[RelatedEntry]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            RelatedEntry::Repo(repo) => Some(repo.clone()),
            RelatedEntry::Local(_) => None,
        })
        .collect()
}

pub fn local_folders(entries: &[RelatedEntry]) -> Vec<PathBuf> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            RelatedEntry::Local(path) => Some(path.clone()),
            RelatedEntry::Repo(_) => None,
        })
        .collect()
}

pub fn merge_related(
    entries: &[RelatedEntry],
    prepared: impl IntoIterator<Item = RelatedRepo>,
) -> Vec<RelatedRepo> {
    let mut prepared = prepared.into_iter();
    entries
        .iter()
        .filter_map(|entry| match entry {
            RelatedEntry::Repo(_) => prepared.next(),
            RelatedEntry::Local(path) => Some(local_related(path)),
        })
        .collect()
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
    entries: &[RelatedEntry],
    planned: impl Fn(&str) -> Result<PathBuf, String>,
) -> Vec<RelatedRepo> {
    let repos = related_repos(entries)
        .into_iter()
        .map(|repo| match planned(&repo) {
            Ok(path) => RelatedRepo {
                path: Some(path.display().to_string()),
                branch: labels::PLAN_BASE_PENDING.to_owned(),
                error: None,
                local: false,
                repo,
            },
            Err(error) => RelatedRepo {
                path: None,
                branch: labels::PLAN_BASE_PENDING.to_owned(),
                error: Some(error),
                local: false,
                repo,
            },
        });
    merge_related(entries, repos)
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
    configured: Vec<RelatedEntry>,
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

    fn repos(items: &[&str]) -> Vec<RelatedEntry> {
        items
            .iter()
            .map(|item| RelatedEntry::Repo((*item).to_owned()))
            .collect()
    }

    fn names(entries: &[RelatedEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| match entry {
                RelatedEntry::Repo(repo) => repo.clone(),
                RelatedEntry::Local(path) => path.display().to_string(),
            })
            .collect()
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let path = std::env::temp_dir()
                .join(format!("argon-summary-ctx-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn related_list_drops_duplicates_and_the_own_repo() {
        let configured = repos(&[
            "acme/pacientes-api",
            "Acme/Widgets",
            "acme/pacientes-api",
            "ACME/Pacientes-API",
            "acme/fila",
        ]);
        assert_eq!(
            names(&related_list(&configured, "acme/widgets")),
            ["acme/pacientes-api", "acme/fila"]
        );
        let configured = repos(&["acme/a", "ACME/A", "acme/b"]);
        assert_eq!(
            names(&related_list(&configured, "acme/x")),
            ["acme/a", "acme/b"]
        );
    }

    #[test]
    fn related_list_keeps_local_folders_once_in_their_place() {
        let dir = TempDir::new("list");
        let fila = dir.0.join("fila");
        std::fs::create_dir_all(&fila).unwrap();
        let configured = vec![
            RelatedEntry::Local(fila.clone()),
            RelatedEntry::Repo("acme/web".into()),
            RelatedEntry::Local(fila.join("..").join("fila")),
            RelatedEntry::Local(dir.0.join("widgets")),
        ];

        let related = related_list(&configured, "acme/widgets");

        assert_eq!(
            related,
            [
                RelatedEntry::Local(fila),
                RelatedEntry::Repo("acme/web".into()),
                RelatedEntry::Local(dir.0.join("widgets")),
            ]
        );
    }

    #[test]
    fn related_list_stops_at_the_limit() {
        let names_in: Vec<String> = (0..12).map(|n| format!("acme/repo-{n}")).collect();
        let configured: Vec<RelatedEntry> =
            names_in.iter().cloned().map(RelatedEntry::Repo).collect();
        let related = names(&related_list(&configured, "acme/widgets"));
        assert_eq!(related.len(), MAX_RELATED_REPOS);
        assert_eq!(related[0], "acme/repo-0");
        assert_eq!(related[MAX_RELATED_REPOS - 1], "acme/repo-7");
    }

    #[test]
    fn planned_related_lists_paths_and_failures() {
        let related = planned_related(&repos(&["acme/a", "acme/b"]), |repo| {
            if repo == "acme/a" {
                Ok(PathBuf::from("wt-a"))
            } else {
                Err("repo inválido".into())
            }
        });
        assert_eq!(related[0].path.as_deref(), Some("wt-a"));
        assert_eq!(related[0].branch, labels::PLAN_BASE_PENDING);
        assert_eq!(related[0].error, None);
        assert!(!related[0].local);
        assert_eq!(related[1].path, None);
        assert_eq!(related[1].error.as_deref(), Some("repo inválido"));
    }

    #[test]
    fn a_local_folder_is_never_planned_and_keeps_its_place() {
        let dir = TempDir::new("planned");
        let gone = dir.0.join("gone");
        let entries = vec![
            RelatedEntry::Local(dir.0.clone()),
            RelatedEntry::Repo("acme/a".into()),
            RelatedEntry::Local(gone.clone()),
        ];

        let related = planned_related(&entries, |repo| {
            assert_eq!(repo, "acme/a");
            Ok(PathBuf::from("wt-a"))
        });

        assert_eq!(related.len(), 3);
        assert_eq!(related[0].repo, local_folder_name(&dir.0));
        assert_eq!(related[0].path, Some(dir.0.display().to_string()));
        assert!(related[0].local);
        assert_eq!(related[0].error, None);
        assert_eq!(related[1].repo, "acme/a");
        assert_eq!(related[2].repo, "gone");
        assert_eq!(related[2].error.as_deref(), Some(labels::LOCAL_FOLDER_GONE));
        assert!(related[2].local);
    }

    #[test]
    fn repos_and_local_folders_split_for_the_checkout() {
        let local = std::env::temp_dir().join("pacientes-api");
        let entries = vec![
            RelatedEntry::Repo("acme/a".into()),
            RelatedEntry::Local(local.clone()),
            RelatedEntry::Repo("acme/b".into()),
        ];
        assert_eq!(related_repos(&entries), ["acme/a", "acme/b"]);
        assert_eq!(local_folders(&entries), [local]);
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
