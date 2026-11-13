use argon_core::{AgentKind, PermissionMode, PrKey};
use argon_store::{NewAgentSession, Store};
use futures::StreamExt;
use serde_json::{Value, json};

use super::*;
use crate::session::events::{PermissionOption, map_update};
use crate::session::permissions::{PermissionAnswer, PermissionAsk, PermissionReply, acp_ask};
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
            permission_mode: "supervised".into(),
            nonce: NONCE.into(),
            allowed_files: vec!["src/lib.rs".into()],
            kind: argon_core::SessionKind::Review,
            base_sha: None,
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
        },
        PermissionMode::Supervised,
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
async fn acp_ask_titles_are_flattened_and_bounded_before_the_card() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    shared.begin_turn("x");
    let title = format!("rm\u{202E} -rf\u{200B}\n{}", "a".repeat(2100));
    let request = json!({
        "sessionId": "sess_fixture",
        "toolCall": {"toolCallId": "call_1", "kind": "execute", "title": title},
        "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"}],
    });
    let (reply, _answers) = recording_reply();
    shared.on_permission(acp_ask(&request, None), reply);
    let (snapshot, _) = shared.subscribe();
    let shown = &snapshot.pending_permissions[0].title;
    assert!(shown.starts_with("rm -rf aaa"), "{shown}");
    assert!(shown.ends_with(" …"));
    assert_eq!(shown.chars().count(), 2000 + " …".chars().count());
}

#[tokio::test]
async fn respond_maps_reply_failure_to_session_error() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    let reply: PermissionReply = Box::new(|_| Err("gone".into()));
    shared.on_permission(ask("edit", &[]), reply);
    assert!(matches!(
        shared.respond("perm-1", "allow_once"),
        Err(SessionError::Agent(message)) if message == "gone"
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
async fn switching_to_accept_edits_answers_pending_local_edits() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    shared.begin_turn("x");
    let (edit_reply, edit_answers) = recording_reply();
    shared.on_permission(ask("edit", &["repo/src/lib.rs"]), edit_reply);
    let (exec_reply, exec_answers) = recording_reply();
    shared.on_permission(ask("execute", &[]), exec_reply);
    assert_eq!(shared.subscribe().0.pending_permissions.len(), 2);

    shared.set_permission_mode(PermissionMode::AcceptEdits);

    assert_eq!(
        *edit_answers.lock().unwrap(),
        [PermissionAnswer::Selected("allow_once".into())]
    );
    assert!(exec_answers.lock().unwrap().is_empty());
    let (snapshot, _) = shared.subscribe();
    assert_eq!(snapshot.pending_permissions.len(), 1);
    assert_eq!(snapshot.pending_permissions[0].request_id, "perm-2");
    assert_eq!(snapshot.status, SessionStatus::AwaitingUser);

    shared.set_permission_mode(PermissionMode::FullAccess);

    assert_eq!(
        *exec_answers.lock().unwrap(),
        [PermissionAnswer::Selected("allow_once".into())]
    );
    let (snapshot, _) = shared.subscribe();
    assert!(snapshot.pending_permissions.is_empty());
    assert_eq!(snapshot.status, SessionStatus::Running);
}

#[tokio::test]
async fn new_permission_mode_applies_to_later_asks() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    shared.set_permission_mode(PermissionMode::AcceptEdits);
    let (reply, answers) = recording_reply();
    shared.on_permission(ask("edit", &["repo/src/lib.rs"]), reply);
    assert_eq!(
        *answers.lock().unwrap(),
        [PermissionAnswer::Selected("allow_once".into())]
    );
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

fn drained(receiver: &mut broadcast::Receiver<(u64, AgentEvent)>) -> Vec<AgentEvent> {
    std::iter::from_fn(|| receiver.try_recv().ok())
        .map(|(_, event)| event)
        .collect()
}

fn resolved(id: &str) -> AgentEvent {
    AgentEvent::PermissionResolved {
        request_id: id.into(),
    }
}

#[tokio::test]
async fn responding_emits_permission_resolved() {
    let (shared, writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    let (reply, _answers) = recording_reply();
    shared.on_permission(ask("edit", &["repo/src/lib.rs"]), reply);
    let (_, mut receiver) = shared.subscribe();
    shared.respond("perm-1", "allow_once").unwrap();
    assert_eq!(drained(&mut receiver), [resolved("perm-1")]);
    shared.close();
    let stored = writer.events_since("s1", 0).await.unwrap();
    assert_eq!(
        stored.last().map(|(_, event)| event),
        Some(&resolved("perm-1"))
    );
}

#[tokio::test]
async fn failed_respond_still_resolves_the_permission() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    let reply: PermissionReply = Box::new(|_| Err("gone".into()));
    shared.on_permission(ask("edit", &[]), reply);
    let (_, mut receiver) = shared.subscribe();
    assert!(shared.respond("perm-1", "allow_once").is_err());
    assert_eq!(drained(&mut receiver), [resolved("perm-1")]);
}

#[tokio::test]
async fn rejected_option_keeps_the_permission_pending() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    let (reply, _answers) = recording_reply();
    shared.on_permission(ask("edit", &[]), reply);
    let (_, mut receiver) = shared.subscribe();
    assert!(shared.respond("perm-1", "nope").is_err());
    assert!(drained(&mut receiver).is_empty());
}

#[tokio::test]
async fn mode_change_emits_mode_and_resolves_auto_decided_permissions() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    let (edit_reply, _edit) = recording_reply();
    shared.on_permission(ask("edit", &["repo/src/lib.rs"]), edit_reply);
    let (exec_reply, _exec) = recording_reply();
    shared.on_permission(ask("execute", &[]), exec_reply);
    let (_, mut receiver) = shared.subscribe();

    shared.set_permission_mode(PermissionMode::AcceptEdits);

    assert_eq!(
        drained(&mut receiver),
        [
            AgentEvent::PermissionModeChanged(PermissionMode::AcceptEdits),
            resolved("perm-1"),
        ]
    );
    assert_eq!(
        shared.subscribe().0.permission_mode,
        PermissionMode::AcceptEdits
    );
}

#[tokio::test]
async fn repeating_the_current_mode_emits_nothing() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    let (_, mut receiver) = shared.subscribe();
    shared.set_permission_mode(PermissionMode::Supervised);
    assert!(drained(&mut receiver).is_empty());
}

#[tokio::test]
async fn snapshot_carries_the_starting_mode() {
    let (shared, _writer) = shared_with(64).await;
    assert_eq!(
        shared.subscribe().0.permission_mode,
        PermissionMode::Supervised
    );
    shared.start_in_mode(PermissionMode::FullAccess);
    assert_eq!(
        shared.subscribe().0.permission_mode,
        PermissionMode::FullAccess
    );
}

#[tokio::test]
async fn ending_turn_resolves_every_pending_permission() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    shared.begin_turn("x");
    let (first, _a) = recording_reply();
    shared.on_permission(ask("edit", &[]), first);
    let (second, _b) = recording_reply();
    shared.on_permission(ask("execute", &[]), second);
    let (_, mut receiver) = shared.subscribe();
    shared.end_turn("cancelled").await;
    let events = drained(&mut receiver);
    assert_eq!(events[..2], [resolved("perm-1"), resolved("perm-2")]);
    assert!(matches!(events[2], AgentEvent::TurnEnded { .. }));
}

#[tokio::test]
async fn failing_the_session_resolves_pending_permissions() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    shared.begin_turn("x");
    let (reply, _answers) = recording_reply();
    shared.on_permission(ask("edit", &[]), reply);
    let (_, mut receiver) = shared.subscribe();
    shared.fail("caiu".into());
    assert_eq!(
        drained(&mut receiver),
        [resolved("perm-1"), AgentEvent::Error("caiu".into())]
    );
}

#[tokio::test]
async fn cancel_pending_resolves_without_a_turn_end() {
    let (shared, _writer) = shared_with(64).await;
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    let (reply, answers) = recording_reply();
    shared.on_permission(ask("edit", &[]), reply);
    let (_, mut receiver) = shared.subscribe();
    shared.cancel_pending();
    assert_eq!(*answers.lock().unwrap(), [PermissionAnswer::Cancelled]);
    assert_eq!(drained(&mut receiver), [resolved("perm-1")]);
}
