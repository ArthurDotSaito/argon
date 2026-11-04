use argon_core::{AgentKind, PrKey};
use argon_store::{NewAgentSession, Store};
use futures::StreamExt;
use serde_json::{Value, json};

use super::*;
use crate::session::events::{PermissionOption, map_update};
use crate::session::permissions::{PermissionAnswer, PermissionAsk, PermissionReply};
use crate::session::{SessionError, event_stream};

const NONCE: &str = "00112233445566778899aabbccddeeff";

fn writer() -> SessionWriter {
    SessionWriter::from_store(Store::open_in_memory().unwrap()).unwrap()
}

async fn shared_with(capacity: usize) -> (Shared, SessionWriter) {
    let writer = writer();
    writer
        .create_session(NewAgentSession {
            id: "s1".into(),
            pr: PrKey::new("github.com", "PR_1"),
            agent: AgentKind::ClaudeCode,
            model: "sonnet".into(),
            template: "review-padrao".into(),
            worktree_path: None,
            acp_session_id: Some("acp".into()),
            transport: "acp".into(),
            nonce: NONCE.into(),
            allowed_files: vec!["src/lib.rs".into()],
        })
        .await
        .unwrap();
    let (events, _) = broadcast::channel(capacity);
    let shared = Shared::new(
        SharedConfig {
            id: "s1".into(),
            cwd: PathBuf::from("repo"),
            nonce: NONCE.into(),
            allowed_files: vec!["src/lib.rs".into()],
            writer: writer.clone(),
            stderr: Arc::default(),
            full_access: false,
        },
        events,
    );
    (shared, writer)
}

fn chunk(text: &str) -> Value {
    json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})
}

fn findings_message(title: &str) -> String {
    format!(
        "Resumo.\n```argon-findings-{NONCE}\n[{{\"severity\":\"medium\",\"file\":\"src/lib.rs\",\"line\":4,\"title\":\"{title}\",\"body\":\"b\"}}]\n```"
    )
}

fn models() -> Vec<ModelInfo> {
    vec![ModelInfo {
        id: "sonnet".into(),
        label: "Sonnet".into(),
    }]
}

#[tokio::test]
async fn updates_before_activation_are_dropped() {
    let (shared, writer) = shared_with(64).await;
    let (_, mut receiver) = shared.subscribe();
    shared.emit_all(map_update(&chunk("replayed")));
    assert!(receiver.try_recv().is_err());
    shared.activate(SessionStatus::Running, 0, "sonnet".into(), models());
    shared.emit_all(map_update(&chunk("live")));
    shared.close();
    assert_eq!(
        receiver.try_recv().unwrap(),
        (1, AgentEvent::MessageChunk("live".into()))
    );
    assert_eq!(
        writer.events_since("s1", 0).await.unwrap(),
        [(1, AgentEvent::MessageChunk("live".into()))]
    );
}

#[tokio::test]
async fn turn_coalesces_chunks_and_saves_findings_with_ids() {
    let (shared, writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, "sonnet".into(), models());
    assert!(shared.try_acquire());
    shared.begin_turn("revise");
    let message = findings_message("t");
    for piece in message.as_bytes().chunks(9) {
        shared.emit_all(map_update(&chunk(std::str::from_utf8(piece).unwrap())));
    }
    shared.emit_all(map_update(&json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "hmm"}})));
    shared.end_turn("end_turn").await;
    assert!(!shared.in_turn());
    assert!(shared.try_acquire());

    let stored = writer.events_since("s1", 0).await.unwrap();
    let kinds: Vec<_> = stored
        .iter()
        .map(|(seq, event)| (*seq, serde_json::to_value(event).unwrap()["type"].clone()))
        .collect();
    assert_eq!(
        kinds,
        [
            (1, json!("user_message")),
            (2, json!("message_chunk")),
            (3, json!("thought_chunk")),
            (4, json!("turn_ended")),
            (5, json!("findings")),
        ]
    );
    assert_eq!(stored[1].1, AgentEvent::MessageChunk(message));
    let AgentEvent::Findings { findings, .. } = &stored[4].1 else {
        panic!("expected findings");
    };
    assert_eq!(findings.len(), 1);
    assert!(findings[0].id > 0);
    assert_eq!(shared.subscribe().0.status, SessionStatus::Completed);
}

#[tokio::test]
async fn identical_finding_keeps_its_id_on_second_turn() {
    let (shared, writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, "sonnet".into(), models());
    let mut ids = Vec::new();
    for _ in 0..2 {
        shared.begin_turn("revise");
        shared.emit_all(map_update(&chunk(&findings_message("mesmo"))));
        shared.end_turn("end_turn").await;
        let events = writer.events_since("s1", 0).await.unwrap();
        let Some((_, AgentEvent::Findings { findings, .. })) = events.last() else {
            panic!("expected findings");
        };
        ids.push(findings[0].id);
    }
    assert_eq!(ids[0], ids[1]);
}

#[tokio::test]
async fn late_subscriber_gets_consistent_snapshot() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 10, "sonnet".into(), models());
    shared.begin_turn("x");
    shared.emit_all(map_update(&chunk("parcial ")));
    let (snapshot, stream) = {
        let (snapshot, receiver) = shared.subscribe();
        (snapshot, event_stream(receiver, 11))
    };
    assert_eq!(snapshot.last_seq, 11);
    assert_eq!(
        snapshot.open_record,
        Some((12, AgentEvent::MessageChunk("parcial ".into())))
    );
    assert_eq!(snapshot.model, "sonnet");
    assert_eq!(snapshot.models, models());
    assert_eq!(snapshot.status, SessionStatus::Running);
    assert!(snapshot.pending_permissions.is_empty());
    shared.emit_all(map_update(&chunk("final")));
    let mut stream = Box::pin(stream);
    assert_eq!(
        stream.next().await.unwrap(),
        (12, AgentEvent::MessageChunk("final".into()))
    );
}

#[tokio::test]
async fn lagging_subscriber_receives_resync() {
    let (shared, _writer) = shared_with(2).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    let (snapshot, receiver) = shared.subscribe();
    let mut stream = Box::pin(event_stream(receiver, snapshot.last_seq));
    for index in 0..6 {
        shared.emit(AgentEvent::Error(format!("e{index}")));
    }
    assert_eq!(
        stream.next().await.unwrap(),
        (0, AgentEvent::Resync { last_seq: 0 })
    );
    assert_eq!(stream.next().await.unwrap().0, 5);
}

#[tokio::test]
async fn failure_marks_session_failed_and_releases() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    assert!(shared.try_acquire());
    assert!(!shared.try_acquire());
    shared.begin_turn("x");
    shared.fail("caiu".into());
    assert!(!shared.in_turn());
    assert_eq!(shared.subscribe().0.status, SessionStatus::Failed);
    assert!(shared.try_acquire());
}

#[tokio::test]
async fn permission_answers_are_validated() {
    let (shared, _writer) = shared_with(64).await;
    assert!(matches!(
        shared.respond("perm-9", "allow"),
        Err(SessionError::UnknownPermission(id)) if id == "perm-9"
    ));
    let permission = PendingPermission {
        request_id: "perm-1".into(),
        title: "Edit".into(),
        kind: "edit".into(),
        options: vec![PermissionOption {
            id: "reject".into(),
            label: "Reject".into(),
            kind: "reject_once".into(),
        }],
    };
    assert!(validate_option(&permission, "reject").is_ok());
    assert!(matches!(
        validate_option(&permission, "allow-always"),
        Err(SessionError::InvalidPermissionOption { .. })
    ));
}

type Answers = Arc<Mutex<Vec<PermissionAnswer>>>;

fn recording_reply() -> (PermissionReply, Answers) {
    let answers = Answers::default();
    let sink = Arc::clone(&answers);
    let reply: PermissionReply = Box::new(move |answer| {
        sink.lock().unwrap().push(answer);
        Ok(())
    });
    (reply, answers)
}

fn ask(kind: &str, paths: &[&str]) -> PermissionAsk {
    PermissionAsk {
        tool_call_id: Some("t1".into()),
        kind: kind.into(),
        title: "Tool".into(),
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        options: ["allow_once", "reject_once"]
            .into_iter()
            .map(|id| PermissionOption {
                id: id.into(),
                label: id.into(),
                kind: id.into(),
            })
            .collect(),
    }
}

#[tokio::test]
async fn local_read_is_allowed_without_asking() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    let (reply, answers) = recording_reply();
    shared.on_permission(ask("read", &["repo/src/lib.rs"]), reply);
    assert_eq!(
        *answers.lock().unwrap(),
        [PermissionAnswer::Selected("allow_once".into())]
    );
    assert!(shared.subscribe().0.pending_permissions.is_empty());
}

#[tokio::test]
async fn asked_permission_waits_and_replies_on_respond() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    shared.begin_turn("x");
    let (reply, answers) = recording_reply();
    shared.on_permission(ask("edit", &["repo/src/lib.rs"]), reply);
    let (snapshot, _) = shared.subscribe();
    assert_eq!(snapshot.status, SessionStatus::AwaitingUser);
    assert_eq!(snapshot.pending_permissions[0].request_id, "perm-1");
    assert!(answers.lock().unwrap().is_empty());
    shared.respond("perm-1", "reject_once").unwrap();
    assert_eq!(
        *answers.lock().unwrap(),
        [PermissionAnswer::Selected("reject_once".into())]
    );
    assert_eq!(shared.subscribe().0.status, SessionStatus::Running);
}

#[tokio::test]
async fn respond_maps_reply_failure_to_session_error() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    let reply: PermissionReply = Box::new(|_| Err("gone".into()));
    shared.on_permission(ask("edit", &[]), reply);
    assert!(matches!(
        shared.respond("perm-1", "allow_once"),
        Err(SessionError::Acp(message)) if message == "gone"
    ));
}

#[tokio::test]
async fn ending_turn_cancels_pending_permissions() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    shared.begin_turn("x");
    let (reply, answers) = recording_reply();
    shared.on_permission(ask("edit", &[]), reply);
    shared.end_turn("cancelled").await;
    assert_eq!(*answers.lock().unwrap(), [PermissionAnswer::Cancelled]);
    assert!(shared.subscribe().0.pending_permissions.is_empty());
}

#[tokio::test]
async fn known_kind_comes_from_emitted_tool_calls() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    assert_eq!(shared.known_kind("t1"), None);
    shared.emit_all(map_update(&json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "t1",
        "title": "Read",
        "kind": "read"
    })));
    assert_eq!(shared.known_kind("t1").as_deref(), Some("read"));
}
