use std::time::{Duration, Instant};

use argon_agents::{AgentEvent, PermissionOption, SavedFinding, ToolStatus};
use argon_core::{Finding, Severity, Side};
use argon_store::SessionStatus;

use super::*;

fn message(text: &str) -> AgentEvent {
    AgentEvent::MessageChunk(text.into())
}

fn thought(text: &str) -> AgentEvent {
    AgentEvent::ThoughtChunk(text.into())
}

fn user(text: &str) -> AgentEvent {
    AgentEvent::UserMessage(text.into())
}

fn ended(reason: &str) -> AgentEvent {
    AgentEvent::TurnEnded {
        stop_reason: reason.into(),
    }
}

fn permission(id: &str) -> AgentEvent {
    AgentEvent::PermissionRequest {
        request_id: id.into(),
        title: "Rodar pnpm test".into(),
        kind: "execute".into(),
        options: vec![PermissionOption {
            id: "allow".into(),
            label: "Allow".into(),
            kind: "allow_once".into(),
        }],
    }
}

fn replay(events: &[AgentEvent]) -> Transcript {
    let mut transcript = Transcript::default();
    for event in events {
        transcript.apply(event, None);
    }
    transcript
}

#[test]
fn consecutive_message_chunks_form_one_message() {
    let transcript = replay(&[
        user("revise"),
        message("Encontrei "),
        message("2 pontos."),
        thought("hmm"),
        message("Fim."),
    ]);
    assert_eq!(
        transcript.items(),
        [
            Item::User("revise".into()),
            Item::Agent("Encontrei 2 pontos.".into()),
            Item::Thought(ThoughtItem {
                text: "hmm".into(),
                started: None,
                finished: None,
            }),
            Item::Agent("Fim.".into()),
        ]
    );
    assert!(transcript.is_streaming());
}

#[test]
fn thought_records_its_duration_when_something_else_arrives() {
    let start = Instant::now();
    let mut transcript = Transcript::default();
    transcript.apply(&user("go"), Some(start));
    transcript.apply(&thought("a"), Some(start));
    transcript.apply(&thought("b"), Some(start + Duration::from_secs(1)));
    transcript.apply(&message("ok"), Some(start + Duration::from_secs(4)));
    let Item::Thought(item) = &transcript.items()[1] else {
        panic!("expected thought");
    };
    assert_eq!(item.text, "ab");
    assert_eq!(item.duration(), Some(Duration::from_secs(4)));
}

#[test]
fn tool_updates_change_status_and_output_in_place() {
    let transcript = replay(&[
        user("go"),
        AgentEvent::ToolCall {
            id: "t1".into(),
            title: "Read src/a.rs".into(),
            kind: "read".into(),
            status: ToolStatus::Pending,
        },
        message("lendo"),
        AgentEvent::ToolCallUpdate {
            id: "t1".into(),
            status: Some(ToolStatus::Completed),
            output: Some("fn main() {}".into()),
            title: None,
        },
        AgentEvent::ToolCallUpdate {
            id: "t1".into(),
            status: None,
            output: None,
            title: Some("cat src/a.rs".into()),
        },
        AgentEvent::ToolCallUpdate {
            id: "unknown".into(),
            status: Some(ToolStatus::Failed),
            output: None,
            title: None,
        },
    ]);
    assert_eq!(transcript.items().len(), 3);
    assert_eq!(
        transcript.items()[1],
        Item::Tool(ToolItem {
            id: "t1".into(),
            title: "cat src/a.rs".into(),
            kind: "read".into(),
            status: ToolStatus::Completed,
            output: Some("fn main() {}".into()),
        })
    );
}

#[test]
fn plan_updates_replace_the_plan_of_the_current_turn() {
    let transcript = replay(&[
        user("go"),
        AgentEvent::Plan(vec!["ler".into()]),
        message("x"),
        AgentEvent::Plan(vec!["ler".into(), "testar".into()]),
        ended("end_turn"),
        user("de novo"),
        AgentEvent::Plan(vec!["outro".into()]),
    ]);
    let plans: Vec<_> = transcript
        .items()
        .iter()
        .filter_map(|item| match item {
            Item::Plan(entries) => Some(entries.len()),
            _ => None,
        })
        .collect();
    assert_eq!(plans, [2, 1]);
}

#[test]
fn status_follows_turn_and_pending_permissions() {
    let mut transcript = replay(&[user("go"), permission("perm-1")]);
    assert_eq!(transcript.status(), SessionStatus::AwaitingUser);
    assert_eq!(transcript.pending_count(), 1);
    assert!(!transcript.is_thinking());
    transcript.resolve(
        "perm-1",
        PermissionState::Answered("Permitir uma vez".into()),
    );
    assert_eq!(transcript.status(), SessionStatus::Running);
    assert!(transcript.is_thinking());
    transcript.apply(&ended("end_turn"), None);
    assert_eq!(transcript.status(), SessionStatus::Completed);
    transcript.apply(&user("mais"), None);
    transcript.mark_failed();
    assert_eq!(transcript.status(), SessionStatus::Failed);
}

#[test]
fn turn_end_expires_unanswered_permissions_and_notes_interruptions() {
    let transcript = replay(&[user("go"), permission("perm-1"), ended("cancelled")]);
    assert!(matches!(
        &transcript.items()[1],
        Item::Permission(item) if item.state == PermissionState::Expired
    ));
    assert_eq!(
        transcript.items().last(),
        Some(&Item::Interrupted("cancelled".into()))
    );
    assert_eq!(transcript.pending_count(), 0);
}

#[test]
fn settle_keeps_only_permissions_still_pending_in_the_agent() {
    let mut transcript = replay(&[user("go"), permission("perm-1"), permission("perm-2")]);
    transcript.settle(SessionStatus::AwaitingUser, &["perm-2".to_owned()]);
    let states: Vec<_> = transcript
        .items()
        .iter()
        .filter_map(|item| match item {
            Item::Permission(item) => Some(item.state.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(states, [PermissionState::Expired, PermissionState::Pending]);
    assert_eq!(transcript.status(), SessionStatus::AwaitingUser);
}

#[test]
fn settle_does_not_reopen_a_finished_turn() {
    let mut fresh = Transcript::default();
    fresh.settle(SessionStatus::Running, &[]);
    assert!(fresh.in_turn());
    let mut finished = replay(&[user("go"), ended("end_turn")]);
    finished.settle(SessionStatus::Running, &[]);
    assert!(!finished.in_turn());
    finished.settle(SessionStatus::Failed, &[]);
    assert_eq!(finished.status(), SessionStatus::Failed);
}

#[test]
fn findings_event_keeps_ids_and_skipped_count() {
    let finding = Finding {
        severity: Severity::High,
        file: "src/a.rs".into(),
        line: 3,
        side: Side::Right,
        title: "t".into(),
        body: "b".into(),
        suggestion: None,
    };
    let transcript = replay(&[
        user("go"),
        ended("end_turn"),
        AgentEvent::Findings {
            findings: vec![SavedFinding { id: 7, finding }],
            skipped: vec!["vendor/x.rs".into(), "y".into()],
        },
        AgentEvent::Error("aviso".into()),
    ]);
    assert_eq!(
        &transcript.items()[1..],
        [
            Item::Findings {
                ids: vec![7],
                skipped: 2
            },
            Item::Error("aviso".into()),
        ]
    );
    assert_eq!(transcript.status(), SessionStatus::Completed);
}

#[test]
fn empty_findings_only_show_for_the_review_turn() {
    let empty = || AgentEvent::Findings {
        findings: Vec::new(),
        skipped: Vec::new(),
    };
    let transcript = replay(&[
        user("revise"),
        ended("end_turn"),
        empty(),
        user("e isso?"),
        ended("end_turn"),
        empty(),
    ]);
    let blocks = transcript
        .items()
        .iter()
        .filter(|item| matches!(item, Item::Findings { .. }))
        .count();
    assert_eq!(blocks, 1);
}
