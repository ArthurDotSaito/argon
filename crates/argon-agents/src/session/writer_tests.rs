use std::path::PathBuf;

use argon_core::{AgentKind, PrKey, Severity, Side};

use super::*;

fn session(id: &str) -> NewAgentSession {
    NewAgentSession {
        id: id.to_owned(),
        pr: PrKey::new("github.com", "PR_1"),
        agent: AgentKind::ClaudeCode,
        model: "sonnet".into(),
        template: "review-padrao".into(),
        worktree_path: Some(PathBuf::from("wt")),
        acp_session_id: Some("acp".into()),
        transport: "acp".into(),
        permission_mode: "supervised".into(),
        nonce: "n".into(),
        allowed_files: vec!["a.rs".into()],
    }
}

fn event(seq: u64, text: &str) -> Op {
    Op::Event {
        session: "s1".into(),
        seq,
        data: serde_json::to_string(&AgentEvent::MessageChunk(text.into())).unwrap(),
    }
}

fn writer() -> SessionWriter {
    SessionWriter::from_store(Store::open_in_memory().unwrap()).unwrap()
}

#[tokio::test]
async fn bursts_are_written_in_few_transactions() {
    let writer = writer();
    writer.create_session(session("s1")).await.unwrap();
    let before = writer.transactions();
    for seq in 1..=150 {
        writer.push(event(seq, "x"));
    }
    writer.push(Op::Status {
        session: "s1".into(),
        status: SessionStatus::Completed,
    });
    let events = writer.events_since("s1", 100).await.unwrap();
    assert_eq!(events.len(), 50);
    assert_eq!(events[0].0, 101);
    assert!(
        writer.transactions() - before <= 3,
        "{}",
        writer.transactions() - before
    );
}

#[tokio::test]
async fn pending_events_flush_after_interval() {
    let writer = writer();
    writer.create_session(session("s1")).await.unwrap();
    writer.push(event(1, "a"));
    tokio::time::sleep(FLUSH_INTERVAL * 3).await;
    let before = writer.transactions();
    writer.flush().await;
    assert_eq!(writer.transactions(), before);
    assert_eq!(writer.last_seq("s1").await.unwrap(), 1);
}

#[tokio::test]
async fn write_errors_reach_the_registered_session() {
    let writer = writer();
    let (errors, mut received) = tokio_mpsc::unbounded_channel();
    writer.push(Op::Register {
        session: "s1".into(),
        errors,
    });
    writer.push(event(1, "orphan"));
    writer.flush().await;
    let message = received.recv().await.unwrap();
    assert!(
        message.starts_with("falha ao gravar o histórico"),
        "{message}"
    );
}

#[tokio::test]
async fn create_failure_is_reported() {
    let writer = writer();
    writer.create_session(session("s1")).await.unwrap();
    assert!(writer.create_session(session("s1")).await.is_err());
}

#[tokio::test]
async fn findings_round_trip_with_ids() {
    let writer = writer();
    writer.create_session(session("s1")).await.unwrap();
    let finding = Finding {
        severity: Severity::Low,
        file: "a.rs".into(),
        line: 1,
        side: Side::Right,
        title: "t".into(),
        body: "b".into(),
        suggestion: None,
    };
    let first = writer
        .save_findings("s1", vec![finding.clone()])
        .await
        .unwrap();
    let second = writer.save_findings("s1", vec![finding]).await.unwrap();
    assert_eq!(first[0].id, second[0].id);
}
