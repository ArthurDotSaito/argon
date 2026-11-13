use serde_json::json;

use super::*;

const CLAUDE_TURN: &str = include_str!("../../tests/fixtures/acp_claude_turn.jsonl");
const OPENCODE_TURN: &str = include_str!("../../tests/fixtures/acp_opencode_turn.jsonl");
const OPENCODE_LOAD: &str = include_str!("../../tests/fixtures/acp_opencode_load.jsonl");

fn replay(fixture: &str) -> Vec<AgentEvent> {
    fixture
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter_map(|message| message.pointer("/params/update").cloned())
        .flat_map(|update| map_update(&update))
        .collect()
}

fn message_text(events: &[AgentEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::MessageChunk(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn claude_turn_maps_tool_call_and_message() {
    let events = replay(CLAUDE_TURN);
    assert!(events.contains(&AgentEvent::ToolCall {
        id: "toolu_0136rZNGDx3zjhtz8z4vJR4Q".into(),
        title: "Terminal".into(),
        kind: "execute".into(),
        status: ToolStatus::Pending,
    }));
    assert!(events.contains(&AgentEvent::ToolCallUpdate {
        id: "toolu_0136rZNGDx3zjhtz8z4vJR4Q".into(),
        status: Some(ToolStatus::Completed),
        output: Some("(PowerShell completed with no output)".into()),
        title: None,
    }));
    assert!(events.contains(&AgentEvent::ToolCallUpdate {
        id: "toolu_0136rZNGDx3zjhtz8z4vJR4Q".into(),
        status: None,
        output: None,
        title: Some("Get-ChildItem -Force | Select-Object Mode, Length, Name".into()),
    }));
    assert_eq!(
        message_text(&events),
        "O diretório atual está vazio: não há nenhum arquivo nem pasta, nem itens ocultos."
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::UserMessage(_)))
    );
}

#[test]
fn opencode_turn_maps_read_tool_with_locations_output() {
    let events = replay(OPENCODE_TURN);
    let first = events
        .iter()
        .find(|event| matches!(event, AgentEvent::ToolCall { .. }))
        .unwrap();
    assert!(matches!(first, AgentEvent::ToolCall { kind, .. } if kind == "read"));
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::ToolCallUpdate { status: Some(ToolStatus::Completed), output: Some(text), .. }
            if text.contains("0 entries")
    )));
    assert!(message_text(&events).contains("está vazio"));
}

#[test]
fn opencode_load_replays_user_message_and_thoughts() {
    let events = replay(OPENCODE_LOAD);
    assert!(matches!(&events[0], AgentEvent::UserMessage(text) if text.starts_with("Liste")));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::ThoughtChunk(_)))
    );
}

#[test]
fn usage_and_unknown_updates_are_ignored() {
    assert!(map_update(&json!({"sessionUpdate": "usage_update", "used": 1})).is_empty());
    assert!(map_update(&json!({"sessionUpdate": "something_new"})).is_empty());
    assert!(
        map_update(&json!({"sessionUpdate": "tool_call_update", "toolCallId": "t"})).is_empty()
    );
    assert!(
        map_update(&json!({
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "image", "data": "", "mimeType": "image/png"}
        }))
        .is_empty()
    );
}

#[test]
fn plan_entries_become_plan_event() {
    let update = json!({
        "sessionUpdate": "plan",
        "entries": [
            {"content": "Ler o diff", "priority": "high", "status": "completed"},
            {"content": "Escrever achados", "priority": "medium", "status": "pending"}
        ]
    });
    assert_eq!(
        map_update(&update),
        [AgentEvent::Plan(vec![
            "Ler o diff".into(),
            "Escrever achados".into()
        ])]
    );
}

#[test]
fn config_option_values_flatten_groups() {
    let session = json!({"configOptions": [
        {"id": "mode", "type": "select", "currentValue": "default", "options": []},
        {"id": "model", "category": "model", "type": "select", "currentValue": "b", "options": [
            {"value": "a", "name": "A"},
            {"group": "g", "name": "G", "options": [{"value": "b", "name": "B"}, {"value": "c"}]}
        ]}
    ]});
    let option = config_option(&session, "model").unwrap();
    assert_eq!(current_value(option).as_deref(), Some("b"));
    let ids: Vec<_> = config_values(option)
        .into_iter()
        .map(|m| (m.id, m.label))
        .collect();
    assert_eq!(
        ids,
        [
            ("a".to_string(), "A".to_string()),
            ("b".to_string(), "B".to_string()),
            ("c".to_string(), "c".to_string())
        ]
    );
    assert!(config_option(&session, "effort").is_none());
}

#[test]
fn events_round_trip_through_json() {
    let events = vec![
        AgentEvent::MessageChunk("oi".into()),
        AgentEvent::PermissionRequest {
            request_id: "perm-1".into(),
            title: "Edit src/a.rs".into(),
            kind: "edit".into(),
            options: vec![PermissionOption {
                id: "allow".into(),
                label: "Allow".into(),
                kind: "allow_once".into(),
            }],
        },
        AgentEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        },
        AgentEvent::ModelsAvailable(vec![ModelInfo {
            id: "sonnet".into(),
            label: "Sonnet".into(),
        }]),
    ];
    let json = serde_json::to_string(&events).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<AgentEvent>>(&json).unwrap(),
        events
    );
}

#[test]
fn permission_events_round_trip_through_json() {
    let events = vec![
        AgentEvent::PermissionResolved {
            request_id: "perm-3".into(),
        },
        AgentEvent::PermissionModeChanged(PermissionMode::AcceptEdits),
    ];
    let json = serde_json::to_string(&events).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<AgentEvent>>(&json).unwrap(),
        events
    );
    assert_eq!(
        serde_json::to_value(&events[1]).unwrap(),
        json!({"type": "permission_mode_changed", "data": "accept-edits"})
    );
}

#[test]
fn plan_draft_round_trips_under_its_own_tag() {
    let event = AgentEvent::PlanDraft {
        markdown: "## Passos\n\n1. Ler".into(),
    };
    let value = serde_json::to_value(&event).unwrap();
    assert_eq!(
        value,
        json!({"type": "plan_draft", "data": {"markdown": "## Passos\n\n1. Ler"}})
    );
    assert_eq!(serde_json::from_value::<AgentEvent>(value).unwrap(), event);
}

#[test]
fn rows_stored_before_plan_drafts_still_deserialize_as_agent_todo_plans() {
    let row = r#"{"type":"plan","data":["Ler o diff","Escrever achados"]}"#;
    assert_eq!(
        serde_json::from_str::<AgentEvent>(row).unwrap(),
        AgentEvent::Plan(vec!["Ler o diff".into(), "Escrever achados".into()])
    );
}

#[test]
fn rows_stored_before_the_permission_events_still_deserialize() {
    let old_rows = [
        r#"{"type":"message_chunk","data":"oi"}"#,
        r#"{"type":"permission_request","data":{"request_id":"perm-1","title":"t","kind":"edit","options":[]}}"#,
        r#"{"type":"turn_ended","data":{"stop_reason":"end_turn"}}"#,
        r#"{"type":"models_available","data":[{"id":"a","label":"A"}]}"#,
    ];
    for row in old_rows {
        assert!(serde_json::from_str::<AgentEvent>(row).is_ok(), "{row}");
    }
}
