use std::time::{Duration, Instant};

use argon_agents::{AgentEvent, PermissionOption, SavedFinding, ToolStatus};
use argon_core::{Finding, PermissionMode, Severity, Side};
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

fn plan_draft(id: i64) -> AgentEvent {
    AgentEvent::PlanDraft {
        draft_id: id,
        markdown: "## Plano".into(),
    }
}

#[test]
fn a_redrafted_plan_moves_to_the_latest_turn() {
    let transcript = replay(&[
        user("planeje"),
        message("a"),
        plan_draft(1),
        ended("end_turn"),
        user("ajuste"),
        AgentEvent::Plan(vec!["ler".into()]),
        message("b"),
        plan_draft(1),
        AgentEvent::Plan(vec!["ler".into(), "testar".into()]),
        ended("end_turn"),
    ]);
    assert_eq!(
        transcript.items(),
        [
            Item::User("planeje".into()),
            Item::Agent("a".into()),
            Item::User("ajuste".into()),
            Item::Plan(vec!["ler".into(), "testar".into()]),
            Item::Agent("b".into()),
            Item::PlanDraft(1),
        ]
    );
}

#[test]
fn a_new_draft_after_publishing_gets_its_own_card() {
    let transcript = replay(&[user("planeje"), plan_draft(1), plan_draft(2)]);
    assert_eq!(
        transcript.items(),
        [
            Item::User("planeje".into()),
            Item::PlanDraft(1),
            Item::PlanDraft(2),
        ]
    );
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
            skipped: vec![
                "finding `a`: file `vendor/x.rs` is not part of the PR".into(),
                "item 2: missing field `line`".into(),
                "finding `b`: file `y` is not part of the PR".into(),
            ],
        },
        AgentEvent::Error("aviso".into()),
    ]);
    assert_eq!(
        &transcript.items()[1..],
        [
            Item::Findings {
                ids: vec![7],
                outside: 2,
                unreadable: 1,
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

#[test]
fn long_tool_output_is_clipped_when_applied() {
    let transcript = replay(&[
        user("go"),
        AgentEvent::ToolCall {
            id: "t".into(),
            title: "cat".into(),
            kind: "read".into(),
            status: ToolStatus::InProgress,
        },
        AgentEvent::ToolCallUpdate {
            id: "t".into(),
            status: None,
            output: Some("é".repeat(20_000)),
            title: None,
        },
    ]);
    let Item::Tool(tool) = &transcript.items()[1] else {
        panic!("expected tool");
    };
    let output = tool.output.as_deref().unwrap();
    assert!(output.len() <= 16 * 1024 + 32);
    assert!(output.ends_with("… saída truncada"));
}

fn resolved(id: &str) -> AgentEvent {
    AgentEvent::PermissionResolved {
        request_id: id.into(),
    }
}

#[test]
fn permission_resolved_closes_only_the_matching_pending_card() {
    let mut transcript = replay(&[
        user("go"),
        permission("perm-1"),
        permission("perm-2"),
        resolved("perm-1"),
    ]);
    let states: Vec<_> = transcript
        .items()
        .iter()
        .filter_map(|item| match item {
            Item::Permission(item) => Some(item.state.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(states, [PermissionState::Expired, PermissionState::Pending]);
    assert_eq!(transcript.pending_count(), 1);
    transcript.apply(&resolved("perm-2"), None);
    assert_eq!(transcript.pending_count(), 0);
    assert_eq!(transcript.status(), SessionStatus::Running);
}

#[test]
fn permission_resolved_keeps_the_answer_given_by_the_user() {
    let mut transcript = replay(&[user("go"), permission("perm-1")]);
    transcript.resolve("perm-1", PermissionState::Answered("Recusar".into()));
    transcript.apply(&resolved("perm-1"), None);
    transcript.apply(&resolved("unknown"), None);
    assert!(matches!(
        &transcript.items()[1],
        Item::Permission(item) if item.state == PermissionState::Answered("Recusar".into())
    ));
}

#[test]
fn replaying_stored_permission_events_leaves_no_pending_card() {
    let transcript = replay(&[
        user("go"),
        permission("perm-1"),
        AgentEvent::PermissionModeChanged(PermissionMode::AcceptEdits),
        resolved("perm-1"),
        ended("end_turn"),
    ]);
    assert_eq!(transcript.pending_count(), 0);
    assert_eq!(transcript.items().len(), 2);
    assert_eq!(
        transcript.permission_mode(),
        Some(PermissionMode::AcceptEdits)
    );
}

#[test]
fn permission_mode_changes_follow_the_latest_event() {
    let mut transcript = Transcript::default();
    assert_eq!(transcript.permission_mode(), None);
    transcript.apply(
        &AgentEvent::PermissionModeChanged(PermissionMode::FullAccess),
        None,
    );
    assert_eq!(
        transcript.permission_mode(),
        Some(PermissionMode::FullAccess)
    );
    transcript.apply(
        &AgentEvent::PermissionModeChanged(PermissionMode::Supervised),
        None,
    );
    assert_eq!(
        transcript.permission_mode(),
        Some(PermissionMode::Supervised)
    );
    transcript.set_permission_mode(PermissionMode::AcceptEdits);
    assert_eq!(
        transcript.permission_mode(),
        Some(PermissionMode::AcceptEdits)
    );
}

fn summary(id: i64, project_updated: bool) -> AgentEvent {
    AgentEvent::Summary {
        id,
        body: "## Este PR".into(),
        project_updated,
    }
}

#[test]
fn a_summary_event_becomes_a_card_where_it_happened() {
    let transcript = replay(&[
        user("resuma"),
        message("pronto"),
        summary(4, true),
        ended("end_turn"),
    ]);
    assert_eq!(
        transcript.items(),
        [
            Item::User("resuma".into()),
            Item::Agent("pronto".into()),
            Item::Summary {
                id: 4,
                project_updated: true,
            },
        ]
    );
}

#[test]
fn each_summary_keeps_its_own_card() {
    let transcript = replay(&[
        user("resuma"),
        summary(1, true),
        ended("end_turn"),
        user("de novo"),
        summary(2, false),
        ended("end_turn"),
    ]);
    let cards: Vec<&Item> = transcript
        .items()
        .iter()
        .filter(|item| matches!(item, Item::Summary { .. }))
        .collect();
    assert_eq!(
        cards,
        [
            &Item::Summary {
                id: 1,
                project_updated: true,
            },
            &Item::Summary {
                id: 2,
                project_updated: false,
            },
        ]
    );
}
