use std::path::PathBuf;

use chrono::{TimeZone, Utc};

use super::*;

fn roots() -> Vec<PathBuf> {
    vec![PathBuf::from(
        "C:\\wt\\github.com\\acme\\pacientes-api\\rel-ab",
    )]
}

#[test]
fn a_path_inside_a_related_folder_is_related() {
    assert!(is_in_related(
        "C:\\wt\\github.com\\acme\\pacientes-api\\rel-ab\\src\\Endpoint.cs",
        &roots(),
        false
    ));
    assert!(is_in_related(
        "Read C:/wt/github.com/acme/pacientes-api/rel-ab/src/Endpoint.cs",
        &roots(),
        false
    ));
}

#[test]
fn a_path_outside_the_related_folders_is_not_related() {
    assert!(!is_in_related("src/Agenda.Api/Program.cs", &roots(), false));
    assert!(!is_in_related(
        "C:\\wt\\github.com\\acme\\agenda-api\\pr-214-abc\\Program.cs",
        &roots(),
        false
    ));
    assert!(!is_in_related("src/x.cs", &[], false));
}

#[test]
fn a_folder_with_a_similar_prefix_is_not_related() {
    let similar = "C:\\wt\\github.com\\acme\\pacientes-api\\rel-abc\\src\\x.cs";
    assert!(!is_in_related(similar, &roots(), false));
    let longer = vec![PathBuf::from(
        "C:\\wt\\github.com\\acme\\pacientes-api\\rel-abc",
    )];
    assert!(is_in_related(similar, &longer, false));
}

#[test]
fn a_path_suffix_of_the_root_is_not_related() {
    assert!(!is_in_related(
        "D:/mirror/C:/wt/github.com/acme/pacientes-api/rel-ab/x.cs",
        &roots(),
        false
    ));
}

#[test]
fn case_only_matters_when_the_file_system_is_case_sensitive() {
    let title = "c:\\WT\\github.com\\acme\\pacientes-api\\REL-AB\\src\\x.cs";
    assert!(is_in_related(title, &roots(), true));
    assert!(!is_in_related(title, &roots(), false));
}

#[test]
fn the_related_label_is_the_repository_the_folder_belongs_to() {
    assert_eq!(
        related_label(&PathBuf::from(
            "C:\\wt\\github.com\\acme\\pacientes-api\\rel-ab"
        )),
        "acme/pacientes-api"
    );
    assert_eq!(related_label(&PathBuf::from("rel-ab")), "rel-ab");
}

fn record(state: &mut SummaryState, id: i64) -> bool {
    let at = Utc.with_ymd_and_hms(2026, 11, 19, 10, 0, 0).unwrap();
    state.record(id, "## PR", ("github.com", "PR_1"), "s1", at)
}

#[test]
fn a_summary_event_adds_one_summary_per_id() {
    let mut state = SummaryState::default();
    assert!(record(&mut state, 1));
    assert!(!record(&mut state, 1));
    assert!(record(&mut state, 2));
    let ids: Vec<i64> = state.items.iter().map(|summary| summary.id).collect();
    assert_eq!(ids, [1, 2]);
    assert_eq!(state.items[0].body, "## PR");
}

#[test]
fn loading_from_the_store_replaces_what_the_events_added() {
    let mut state = SummaryState::default();
    record(&mut state, 1);
    let mut stored = state.items[0].clone();
    stored.head_sha = "abc123".into();
    stored.touched = vec!["api".into()];
    state.load(SummaryData {
        summaries: vec![stored.clone()],
        overview: None,
        repo: Some("acme/agenda-api".into()),
        related: vec![PathBuf::from("rel-a")],
        local: vec![PathBuf::from("pacientes-api")],
    });
    assert_eq!(state.items, [stored]);
    assert_eq!(state.repo.as_deref(), Some("acme/agenda-api"));
    assert_eq!(state.related, [PathBuf::from("rel-a")]);
    assert_eq!(
        state.read_roots(),
        [PathBuf::from("rel-a"), PathBuf::from("pacientes-api")]
    );
}

#[test]
fn a_read_inside_a_local_folder_is_marked_related() {
    let mut state = SummaryState::default();
    state.load(SummaryData {
        summaries: Vec::new(),
        overview: None,
        repo: None,
        related: roots(),
        local: vec![PathBuf::from("D:\\src\\pacientes-api")],
    });
    let roots = state.read_roots();

    assert!(is_in_related(
        "D:\\src\\pacientes-api\\src\\Program.cs",
        &roots,
        true
    ));
    assert!(is_in_related(
        "C:\\wt\\github.com\\acme\\pacientes-api\\rel-ab\\x.cs",
        &roots,
        true
    ));
    assert!(!is_in_related("D:\\src\\fila\\x.cs", &roots, true));
}

#[test]
fn the_store_provides_summaries_overview_repo_and_related_folders() {
    use argon_core::{AgentKind, PrKey, SessionKind};
    use argon_store::{NewAgentSession, NewProject, NewSummary};

    let mut store = Store::open_in_memory().unwrap();
    store
        .create_session(&NewAgentSession {
            id: "s1".into(),
            pr: PrKey::new("github.com", "PR_1"),
            agent: AgentKind::ClaudeCode,
            model: "default".into(),
            template: "resumo-padrao".into(),
            worktree_path: Some(PathBuf::from("C:/wt/pr-1")),
            acp_session_id: None,
            transport: "acp".into(),
            permission_mode: "supervised".into(),
            nonce: String::new(),
            allowed_files: Vec::new(),
            kind: SessionKind::Summary,
            base_sha: None,
            related_worktrees: vec![PathBuf::from("C:/wt/rel-a")],
            repo: Some("acme/agenda-api".into()),
            pr_number: Some(214),
            refresh_project: true,
            local_roots: vec![PathBuf::from("D:/src/fila")],
        })
        .unwrap();
    store
        .record_summary(NewSummary {
            session_id: "s1",
            host: "github.com",
            pr_id: "PR_1",
            repo: "acme/agenda-api",
            pr_number: 214,
            body: "## Este PR",
            head_sha: "abc123",
            touched: &["api".to_owned()],
            project: Some(NewProject {
                body: "## O projeto",
                diagram: None,
            }),
        })
        .unwrap();

    let data = load_summary_data(&store, "s1", "github.com").unwrap();
    let mut state = SummaryState::default();
    state.load(data);
    assert_eq!(state.items.len(), 1);
    assert_eq!(state.items[0].touched, ["api"]);
    assert_eq!(state.overview.unwrap().body, "## O projeto");
    assert_eq!(state.repo.as_deref(), Some("acme/agenda-api"));
    assert_eq!(state.related, [PathBuf::from("C:/wt/rel-a")]);
    assert_eq!(state.local, [PathBuf::from("D:/src/fila")]);
}
