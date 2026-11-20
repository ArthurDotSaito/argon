use argon_core::{ArchEdge, ArchNode, Lane, NodeKind, Via};
use rusqlite::params;

use super::*;

const HOST: &str = "github.com";
const PR: &str = "PR_1";
const REPO: &str = "acme/api";

fn graph() -> ArchGraph {
    ArchGraph {
        nodes: vec![
            ArchNode {
                id: "api".into(),
                label: "API".into(),
                lane: Lane::Project,
                kind: NodeKind::Service,
                repo: Some(REPO.into()),
                read: true,
            },
            ArchNode {
                id: "db".into(),
                label: "Postgres".into(),
                lane: Lane::Uses,
                kind: NodeKind::Database,
                repo: None,
                read: false,
            },
        ],
        edges: vec![ArchEdge {
            from: "api".into(),
            to: "db".into(),
            label: "reads".into(),
            via: Via::Call,
        }],
    }
}

fn new<'a>(session: &'a str, body: &'a str, sha: &'a str) -> NewSummary<'a> {
    NewSummary {
        session_id: session,
        host: HOST,
        pr_id: PR,
        repo: REPO,
        pr_number: 7,
        body,
        head_sha: sha,
        touched: &[],
        project: None,
    }
}

fn corrupt(store: &Store, table: &str, column: &str) {
    store
        .conn
        .execute(
            &format!("UPDATE {table} SET {column} = ?1"),
            params!["{not json"],
        )
        .unwrap();
}

#[test]
fn summary_without_project_leaves_overview_untouched() {
    let mut store = Store::open_in_memory().unwrap();
    let summary = store.record_summary(new("s1", "resumo", "sha1")).unwrap();

    assert_eq!(summary.body, "resumo");
    assert_eq!(summary.head_sha, "sha1");
    assert_eq!(summary.session_id, "s1");
    assert!(summary.touched.is_empty());
    assert!(store.project_overview(HOST, REPO).unwrap().is_none());
    assert!(store.project_overviews().unwrap().is_empty());
}

#[test]
fn summary_with_project_creates_then_overwrites_overview() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .record_summary(NewSummary {
            project: Some(NewProject {
                body: "visão 1",
                diagram: None,
            }),
            ..new("s1", "resumo 1", "sha1")
        })
        .unwrap();
    let first = store.project_overview(HOST, REPO).unwrap().unwrap();
    assert_eq!(first.body, "visão 1");
    assert_eq!(first.source_sha, "sha1");
    assert_eq!(first.source_pr, 7);
    assert_eq!(first.session_id, "s1");
    assert_eq!(first.diagram, None);

    let diagram = graph();
    store
        .record_summary(NewSummary {
            pr_number: 9,
            project: Some(NewProject {
                body: "visão 2",
                diagram: Some(&diagram),
            }),
            ..new("s2", "resumo 2", "sha2")
        })
        .unwrap();
    let second = store.project_overview(HOST, REPO).unwrap().unwrap();
    assert_eq!(second.body, "visão 2");
    assert_eq!(second.source_sha, "sha2");
    assert_eq!(second.source_pr, 9);
    assert_eq!(second.session_id, "s2");
    assert_eq!(second.diagram, Some(diagram));
    assert!(second.updated_at >= first.updated_at);
    assert_eq!(store.project_overviews().unwrap().len(), 1);

    store.record_summary(new("s3", "resumo 3", "sha3")).unwrap();
    let kept = store.project_overview(HOST, REPO).unwrap().unwrap();
    assert_eq!(kept.body, "visão 2");
    assert_eq!(kept.source_sha, "sha2");
}

#[test]
fn touched_round_trips() {
    let mut store = Store::open_in_memory().unwrap();
    let touched = vec!["api".to_owned(), "db".to_owned()];
    let recorded = store
        .record_summary(NewSummary {
            touched: &touched,
            ..new("s1", "resumo", "sha1")
        })
        .unwrap();

    assert_eq!(recorded.touched, touched);
    let read = store.latest_summary_for_pr(HOST, PR).unwrap().unwrap();
    assert_eq!(read, recorded);
}

#[test]
fn unreadable_json_is_tolerated_on_read() {
    let mut store = Store::open_in_memory().unwrap();
    let diagram = graph();
    store
        .record_summary(NewSummary {
            touched: &["api".to_owned()],
            project: Some(NewProject {
                body: "visão",
                diagram: Some(&diagram),
            }),
            ..new("s1", "resumo", "sha1")
        })
        .unwrap();
    corrupt(&store, "pr_summaries", "touched");
    corrupt(&store, "project_overviews", "diagram");

    let summary = store.latest_summary_for_pr(HOST, PR).unwrap().unwrap();
    assert!(summary.touched.is_empty());
    assert_eq!(summary.body, "resumo");
    let overview = store.project_overview(HOST, REPO).unwrap().unwrap();
    assert_eq!(overview.diagram, None);
    assert_eq!(overview.body, "visão");
}

#[test]
fn latest_summary_is_the_most_recent_for_the_pr() {
    let mut store = Store::open_in_memory().unwrap();
    assert!(store.latest_summary_for_pr(HOST, PR).unwrap().is_none());
    store.record_summary(new("s1", "um", "sha1")).unwrap();
    store.record_summary(new("s2", "dois", "sha2")).unwrap();
    store
        .record_summary(NewSummary {
            pr_id: "PR_other",
            ..new("s3", "outro", "sha3")
        })
        .unwrap();

    let latest = store.latest_summary_for_pr(HOST, PR).unwrap().unwrap();
    assert_eq!(latest.body, "dois");
    assert!(
        store
            .latest_summary_for_pr("ghe.acme.io", PR)
            .unwrap()
            .is_none()
    );
}

#[test]
fn summaries_for_session_are_oldest_first() {
    let mut store = Store::open_in_memory().unwrap();
    store.record_summary(new("s1", "um", "sha1")).unwrap();
    store
        .record_summary(new("s2", "outra sessão", "sha2"))
        .unwrap();
    store.record_summary(new("s1", "dois", "sha3")).unwrap();

    let bodies: Vec<String> = store
        .summaries_for_session("s1")
        .unwrap()
        .into_iter()
        .map(|summary| summary.body)
        .collect();
    assert_eq!(bodies, ["um", "dois"]);
    assert!(store.summaries_for_session("none").unwrap().is_empty());
}

#[test]
fn project_overviews_are_ordered_by_repo() {
    let mut store = Store::open_in_memory().unwrap();
    for repo in ["acme/zeta", "acme/alpha", "acme/mid"] {
        store
            .record_summary(NewSummary {
                repo,
                project: Some(NewProject {
                    body: repo,
                    diagram: None,
                }),
                ..new("s1", "resumo", "sha1")
            })
            .unwrap();
    }

    let repos: Vec<String> = store
        .project_overviews()
        .unwrap()
        .into_iter()
        .map(|overview| overview.repo)
        .collect();
    assert_eq!(repos, ["acme/alpha", "acme/mid", "acme/zeta"]);
}

#[test]
fn project_keys_list_host_and_repo_ordered_without_reading_the_views() {
    let mut store = Store::open_in_memory().unwrap();
    assert!(store.project_keys().unwrap().is_empty());
    for (host, repo) in [
        ("github.com", "acme/zeta"),
        ("ghe.local", "acme/alpha"),
        ("github.com", "acme/alpha"),
    ] {
        store
            .record_summary(NewSummary {
                host,
                repo,
                project: Some(NewProject {
                    body: "visão",
                    diagram: Some(&graph()),
                }),
                ..new("s1", "resumo", "sha1")
            })
            .unwrap();
    }
    corrupt(&store, "project_overviews", "diagram");

    assert_eq!(
        store.project_keys().unwrap(),
        [
            ("ghe.local".to_owned(), "acme/alpha".to_owned()),
            ("github.com".to_owned(), "acme/alpha".to_owned()),
            ("github.com".to_owned(), "acme/zeta".to_owned()),
        ]
    );
}

fn save_project(store: &mut Store, host: &str, repo: &str) {
    store
        .record_summary(NewSummary {
            host,
            repo,
            project: Some(NewProject {
                body: "visão",
                diagram: Some(&graph()),
            }),
            ..new("s1", "resumo", "sha1")
        })
        .unwrap();
}

#[test]
fn deleting_a_project_overview_reports_whether_it_existed() {
    let mut store = Store::open_in_memory().unwrap();
    save_project(&mut store, HOST, REPO);

    assert!(store.delete_project_overview(HOST, REPO).unwrap());
    assert!(store.project_overview(HOST, REPO).unwrap().is_none());
    assert!(!store.delete_project_overview(HOST, REPO).unwrap());
}

#[test]
fn deleting_a_project_overview_keeps_other_repos_and_the_pr_summaries() {
    let mut store = Store::open_in_memory().unwrap();
    save_project(&mut store, HOST, REPO);
    save_project(&mut store, HOST, "acme/web");
    save_project(&mut store, "ghe.local", REPO);

    assert!(store.delete_project_overview(HOST, REPO).unwrap());

    assert_eq!(
        store.project_keys().unwrap(),
        [
            ("ghe.local".to_owned(), "acme/api".to_owned()),
            ("github.com".to_owned(), "acme/web".to_owned()),
        ]
    );
    assert!(store.latest_summary_for_pr(HOST, PR).unwrap().is_some());
}
