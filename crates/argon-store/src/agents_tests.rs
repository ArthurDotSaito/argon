use serde::{Deserialize, Serialize};

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum Event {
    Chunk(String),
    Ended { reason: String },
}

fn new_session(id: &str) -> NewAgentSession {
    NewAgentSession {
        id: id.to_owned(),
        pr: PrKey::new("github.com", "PR_kw1"),
        agent: AgentKind::OpenCode,
        model: "opencode-go/glm-5.2".to_owned(),
        template: "review-padrao".to_owned(),
        worktree_path: Some(PathBuf::from(r"C:\work\pr-1")),
        acp_session_id: Some("ses_1".to_owned()),
        nonce: "abcd".to_owned(),
        allowed_files: vec!["src/a.rs".to_owned(), "README.md".to_owned()],
    }
}

fn finding(title: &str, severity: Severity, side: Side) -> Finding {
    Finding {
        severity,
        file: "src/a.rs".to_owned(),
        line: 12,
        side,
        title: title.to_owned(),
        body: "corpo".to_owned(),
        suggestion: Some("let x = 1;".to_owned()),
    }
}

#[test]
fn creates_and_lists_sessions_with_context() {
    let mut store = Store::open_in_memory().unwrap();
    store.create_session(&new_session("s1")).unwrap();
    let rows = store.sessions().unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.id, "s1");
    assert_eq!(row.pr, PrKey::new("github.com", "PR_kw1"));
    assert_eq!(row.agent, AgentKind::OpenCode);
    assert_eq!(row.status, SessionStatus::Running);
    assert_eq!(row.acp_session_id.as_deref(), Some("ses_1"));
    assert_eq!(row.nonce, "abcd");
    assert_eq!(row.allowed_files, ["src/a.rs", "README.md"]);
    assert_eq!(row.worktree_path, Some(PathBuf::from(r"C:\work\pr-1")));
    assert_eq!(store.session("s1").unwrap().as_ref(), Some(row));
    assert!(store.session("missing").unwrap().is_none());
}

#[test]
fn updates_status_and_model() {
    let mut store = Store::open_in_memory().unwrap();
    store.create_session(&new_session("s1")).unwrap();
    for status in [
        SessionStatus::AwaitingUser,
        SessionStatus::Completed,
        SessionStatus::Failed,
        SessionStatus::Running,
    ] {
        store.set_session_status("s1", status).unwrap();
        assert_eq!(store.session("s1").unwrap().unwrap().status, status);
    }
    store.set_session_model("s1", "opus").unwrap();
    assert_eq!(store.session("s1").unwrap().unwrap().model, "opus");
}

#[test]
fn events_round_trip_in_order_per_session() {
    let mut store = Store::open_in_memory().unwrap();
    store.create_session(&new_session("s1")).unwrap();
    store.create_session(&new_session("s2")).unwrap();
    let first = Event::Chunk("olá".to_owned());
    let second = Event::Ended {
        reason: "end_turn".to_owned(),
    };
    assert_eq!(store.append_event("s1", &first).unwrap(), 1);
    assert_eq!(store.append_event("s2", &first).unwrap(), 1);
    assert_eq!(store.append_event("s1", &second).unwrap(), 2);
    assert_eq!(
        store.events::<Event>("s1").unwrap(),
        [first.clone(), second]
    );
    assert_eq!(store.events::<Event>("s2").unwrap(), [first]);
}

#[test]
fn findings_are_saved_listed_and_change_state() {
    let mut store = Store::open_in_memory().unwrap();
    store.create_session(&new_session("s1")).unwrap();
    let saved = [
        finding("a", Severity::High, Side::Right),
        finding("b", Severity::Low, Side::Left),
    ];
    let ids = store.save_findings("s1", &saved).unwrap();
    assert_eq!(ids.len(), 2);
    let listed = store.findings("s1").unwrap();
    assert_eq!(
        listed.iter().map(|f| f.finding.clone()).collect::<Vec<_>>(),
        saved
    );
    assert!(listed.iter().all(|f| f.state == FindingState::New));
    store
        .set_finding_state(ids[1], FindingState::Ignored)
        .unwrap();
    store
        .set_finding_state(ids[0], FindingState::Added)
        .unwrap();
    let states: Vec<_> = store
        .findings("s1")
        .unwrap()
        .into_iter()
        .map(|f| f.state)
        .collect();
    assert_eq!(states, [FindingState::Added, FindingState::Ignored]);
}

#[test]
fn deleting_a_session_cascades() {
    let mut store = Store::open_in_memory().unwrap();
    store.create_session(&new_session("s1")).unwrap();
    store.append_event("s1", &Event::Chunk("x".into())).unwrap();
    store
        .save_findings("s1", &[finding("a", Severity::Medium, Side::Right)])
        .unwrap();
    store
        .conn
        .execute("DELETE FROM agent_sessions WHERE id = 's1'", [])
        .unwrap();
    assert!(store.events::<Event>("s1").unwrap().is_empty());
    assert!(store.findings("s1").unwrap().is_empty());
}

#[test]
fn status_and_state_strings_round_trip() {
    for status in [
        SessionStatus::Running,
        SessionStatus::AwaitingUser,
        SessionStatus::Completed,
        SessionStatus::Failed,
    ] {
        assert_eq!(SessionStatus::parse(status.as_str()), Some(status));
    }
    for state in [
        FindingState::New,
        FindingState::Added,
        FindingState::Ignored,
    ] {
        assert_eq!(FindingState::parse(state.as_str()), Some(state));
    }
    assert_eq!(SessionStatus::parse("bogus"), None);
}
