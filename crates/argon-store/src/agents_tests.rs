use super::*;

pub(crate) fn new_session(id: &str) -> NewAgentSession {
    NewAgentSession {
        id: id.to_owned(),
        pr: PrKey::new("github.com", "PR_kw1"),
        agent: AgentKind::OpenCode,
        model: "opencode-go/glm-5.2".to_owned(),
        template: "review-padrao".to_owned(),
        worktree_path: Some(PathBuf::from(r"C:\work\pr-1")),
        acp_session_id: Some("ses_1".to_owned()),
        transport: "acp".to_owned(),
        permission_mode: "supervised".to_owned(),
        nonce: "abcd".to_owned(),
        allowed_files: vec!["src/a.rs".to_owned(), "README.md".to_owned()],
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
fn updates_the_agent_session_id() {
    let mut store = Store::open_in_memory().unwrap();
    store.create_session(&new_session("s1")).unwrap();
    store.set_session_acp_id("s1", "ses_2").unwrap();
    assert_eq!(
        store
            .session("s1")
            .unwrap()
            .unwrap()
            .acp_session_id
            .as_deref(),
        Some("ses_2")
    );
}

#[test]
fn duplicate_session_id_is_an_error() {
    let mut store = Store::open_in_memory().unwrap();
    store.create_session(&new_session("s1")).unwrap();
    assert!(store.create_session(&new_session("s1")).is_err());
}

#[test]
fn transport_round_trips() {
    let mut store = Store::open_in_memory().unwrap();
    let mut session = new_session("s1");
    session.transport = "claude-stream".to_owned();
    store.create_session(&session).unwrap();
    assert_eq!(
        store.session("s1").unwrap().unwrap().transport,
        "claude-stream"
    );
    assert_eq!(store.sessions().unwrap()[0].transport, "claude-stream");
}

#[test]
fn permission_mode_round_trips_and_updates() {
    let mut store = Store::open_in_memory().unwrap();
    let mut session = new_session("s1");
    session.permission_mode = "accept-edits".to_owned();
    store.create_session(&session).unwrap();
    assert_eq!(
        store.session("s1").unwrap().unwrap().permission_mode,
        "accept-edits"
    );
    assert_eq!(store.sessions().unwrap()[0].permission_mode, "accept-edits");
    store
        .set_session_permission_mode("s1", "full-access")
        .unwrap();
    assert_eq!(
        store.session("s1").unwrap().unwrap().permission_mode,
        "full-access"
    );
}

#[test]
fn status_and_state_strings_round_trip() {
    for status in [
        SessionStatus::Running,
        SessionStatus::AwaitingUser,
        SessionStatus::Completed,
        SessionStatus::Failed,
        SessionStatus::Archived,
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
