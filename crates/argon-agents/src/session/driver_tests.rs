use argon_core::Finding;
use argon_store::NewAgentSession;
use serde_json::json;

use super::*;

const NONCE: &str = "00112233445566778899aabbccddeeff";

#[derive(Default)]
struct Recording {
    events: Mutex<Vec<AgentEvent>>,
    statuses: Mutex<Vec<SessionStatus>>,
    findings: Mutex<Vec<Finding>>,
}

impl StoreSink for Recording {
    fn create_session(&self, _session: &NewAgentSession) {}

    fn set_status(&self, _id: &str, status: SessionStatus) {
        self.statuses.lock().unwrap().push(status);
    }

    fn set_model(&self, _id: &str, _model: &str) {}

    fn append_event(&self, _id: &str, event: &AgentEvent) {
        self.events.lock().unwrap().push(event.clone());
    }

    fn save_findings(&self, _id: &str, findings: &[Finding]) {
        self.findings.lock().unwrap().extend_from_slice(findings);
    }
}

fn shared(sink: &Arc<Recording>) -> (Shared, broadcast::Receiver<AgentEvent>) {
    let (events, receiver) = broadcast::channel(64);
    let sink: Arc<dyn StoreSink> = sink.clone();
    let shared = Shared::new(
        "s1".into(),
        PathBuf::from("repo"),
        NONCE.into(),
        vec!["src/lib.rs".into()],
        sink,
        events,
    );
    (shared, receiver)
}

fn chunk(text: &str) -> Value {
    json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})
}

#[test]
fn updates_before_activation_are_dropped() {
    let sink = Arc::new(Recording::default());
    let (shared, mut receiver) = shared(&sink);
    shared.on_update(&chunk("replayed"));
    assert!(sink.events.lock().unwrap().is_empty());
    assert!(receiver.try_recv().is_err());
    shared.activate(SessionStatus::Completed);
    shared.on_update(&chunk("live"));
    assert_eq!(
        *sink.events.lock().unwrap(),
        [AgentEvent::MessageChunk("live".into())]
    );
    assert_eq!(
        receiver.try_recv().unwrap(),
        AgentEvent::MessageChunk("live".into())
    );
}

#[test]
fn turn_persists_events_findings_and_status() {
    let sink = Arc::new(Recording::default());
    let (shared, _receiver) = shared(&sink);
    shared.activate(SessionStatus::Completed);
    shared.begin_turn("revise");
    assert!(shared.in_turn());
    let block = format!(
        "Resumo.\n```argon-findings-{NONCE}\n[{{\"severity\":\"medium\",\"file\":\"src/lib.rs\",\"line\":4,\"title\":\"t\",\"body\":\"b\"}}]\n```"
    );
    let (first, second) = block.split_at(20);
    shared.on_update(&chunk(first));
    shared.on_update(&json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "hmm"}}));
    shared.on_update(&chunk(second));
    shared.end_turn("end_turn");
    assert!(!shared.in_turn());

    let findings = sink.findings.lock().unwrap().clone();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, 4);
    let events = sink.events.lock().unwrap().clone();
    assert_eq!(events[0], AgentEvent::UserMessage("revise".into()));
    assert!(
        matches!(&events[events.len() - 2], AgentEvent::TurnEnded { stop_reason } if stop_reason == "end_turn")
    );
    assert!(
        matches!(&events[events.len() - 1], AgentEvent::Findings { findings, .. } if findings.len() == 1)
    );
    assert_eq!(
        *sink.statuses.lock().unwrap(),
        [
            SessionStatus::Completed,
            SessionStatus::Running,
            SessionStatus::Completed
        ]
    );
}

#[test]
fn second_turn_starts_with_empty_buffer() {
    let sink = Arc::new(Recording::default());
    let (shared, _receiver) = shared(&sink);
    shared.activate(SessionStatus::Completed);
    shared.begin_turn("um");
    shared.on_update(&chunk(&format!(
        "```argon-findings-{NONCE}\n[{{broken\n```"
    )));
    shared.end_turn("end_turn");
    shared.begin_turn("dois");
    shared.on_update(&chunk("Sem achados."));
    shared.end_turn("end_turn");
    let events = sink.events.lock().unwrap().clone();
    assert!(events.iter().any(
        |event| matches!(event, AgentEvent::Error(text) if text.starts_with("Não consegui ler"))
    ));
    assert_eq!(
        events.last(),
        Some(&AgentEvent::Findings {
            findings: Vec::new(),
            skipped: Vec::new()
        })
    );
    assert!(sink.findings.lock().unwrap().is_empty());
}

#[test]
fn failure_marks_session_failed() {
    let sink = Arc::new(Recording::default());
    let (shared, _receiver) = shared(&sink);
    shared.activate(SessionStatus::Completed);
    shared.begin_turn("x");
    shared.fail("caiu".into());
    assert!(!shared.in_turn());
    assert_eq!(
        sink.statuses.lock().unwrap().last(),
        Some(&SessionStatus::Failed)
    );
    assert_eq!(
        sink.events.lock().unwrap().last(),
        Some(&AgentEvent::Error("caiu".into()))
    );
}

#[test]
fn unknown_permission_is_an_error() {
    let sink = Arc::new(Recording::default());
    let (shared, _receiver) = shared(&sink);
    assert!(matches!(
        shared.respond("perm-9", "allow"),
        Err(SessionError::UnknownPermission(id)) if id == "perm-9"
    ));
}
