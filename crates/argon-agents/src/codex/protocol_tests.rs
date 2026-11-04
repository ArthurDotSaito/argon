use std::path::Path;

use serde_json::json;

use super::*;

fn events(decoder: &mut Decoder, method: &str, params: Value) -> Vec<AgentEvent> {
    match decoder.notification(method, &params) {
        CodexEvent::Events(events) => events,
        other => panic!("expected events, got {other:?}"),
    }
}

fn started(decoder: &mut Decoder, item: Value) -> AgentEvent {
    let mut events = events(decoder, "item/started", json!({"item": item}));
    assert_eq!(events.len(), 1);
    events.remove(0)
}

fn completed(decoder: &mut Decoder, item: Value) -> AgentEvent {
    let mut events = events(decoder, "item/completed", json!({"item": item}));
    assert_eq!(events.len(), 1);
    events.remove(0)
}

fn turn_end(params: Value) -> (String, String, Option<String>) {
    match Decoder::new().notification("turn/completed", &params) {
        CodexEvent::TurnEnd {
            turn_id,
            stop_reason,
            error,
        } => (turn_id, stop_reason, error),
        other => panic!("expected turn end, got {other:?}"),
    }
}

fn ask(decoder: &mut Decoder, method: &str, params: Value) -> PermissionAsk {
    match decoder.request(method, &params) {
        CodexRequest::Ask(ask) => ask,
        other => panic!("expected ask, got {other:?}"),
    }
}

fn selected(id: &str) -> PermissionAnswer {
    PermissionAnswer::Selected(id.to_owned())
}

fn option_ids(ask: &PermissionAsk) -> Vec<&str> {
    ask.options
        .iter()
        .map(|option| option.id.as_str())
        .collect()
}

fn file_change_item(id: &str, paths: &[&str]) -> Value {
    let changes: Vec<Value> = paths
        .iter()
        .map(
            |path| json!({"path": path, "kind": {"type": "update", "move_path": null}, "diff": ""}),
        )
        .collect();
    json!({"type": "fileChange", "id": id, "changes": changes})
}

#[test]
fn turn_started_carries_turn_id() {
    let event = Decoder::new().notification(
        "turn/started",
        &json!({"threadId": "t", "turn": {"id": "turn_1"}}),
    );
    assert_eq!(
        event,
        CodexEvent::TurnStarted {
            turn_id: "turn_1".to_owned()
        }
    );
}

#[test]
fn agent_message_delta_becomes_message_chunk() {
    let mut decoder = Decoder::new();
    let params = json!({"threadId": "t", "turnId": "u", "itemId": "i", "delta": "olá"});
    assert_eq!(
        events(&mut decoder, "item/agentMessage/delta", params),
        vec![AgentEvent::MessageChunk("olá".to_owned())]
    );
}

#[test]
fn first_reasoning_kind_wins_per_item() {
    let mut decoder = Decoder::new();
    let summary = |delta: &str| json!({"itemId": "r1", "delta": delta});
    assert_eq!(
        events(
            &mut decoder,
            "item/reasoning/summaryTextDelta",
            summary("a")
        ),
        vec![AgentEvent::ThoughtChunk("a".to_owned())]
    );
    assert_eq!(
        decoder.notification("item/reasoning/textDelta", &summary("raw")),
        CodexEvent::Ignore
    );
    assert_eq!(
        events(
            &mut decoder,
            "item/reasoning/summaryTextDelta",
            summary("b")
        ),
        vec![AgentEvent::ThoughtChunk("b".to_owned())]
    );
}

#[test]
fn reasoning_kind_is_tracked_per_item() {
    let mut decoder = Decoder::new();
    let first = json!({"itemId": "r1", "delta": "x"});
    let second = json!({"itemId": "r2", "delta": "y"});
    events(&mut decoder, "item/reasoning/textDelta", first.clone());
    assert_eq!(
        events(&mut decoder, "item/reasoning/summaryTextDelta", second),
        vec![AgentEvent::ThoughtChunk("y".to_owned())]
    );
    assert_eq!(
        decoder.notification("item/reasoning/summaryTextDelta", &first),
        CodexEvent::Ignore
    );
    assert_eq!(
        events(&mut decoder, "item/reasoning/textDelta", first),
        vec![AgentEvent::ThoughtChunk("x".to_owned())]
    );
}

#[test]
fn command_execution_start_uses_string_command() {
    let item =
        json!({"type": "commandExecution", "id": "c1", "command": "cargo test", "cwd": "/repo"});
    assert_eq!(
        started(&mut Decoder::new(), item),
        AgentEvent::ToolCall {
            id: "c1".to_owned(),
            title: "Bash: cargo test".to_owned(),
            kind: "execute".to_owned(),
            status: ToolStatus::InProgress,
        }
    );
}

#[test]
fn command_execution_start_joins_array_command() {
    let item = json!({"type": "commandExecution", "id": "c1", "command": ["git", "status", "-s"]});
    match started(&mut Decoder::new(), item) {
        AgentEvent::ToolCall { title, .. } => assert_eq!(title, "Bash: git status -s"),
        other => panic!("expected tool call, got {other:?}"),
    }
}

#[test]
fn command_title_replaces_control_characters() {
    let item = json!({"type": "commandExecution", "id": "c1", "command": "echo a\nb\tc"});
    match started(&mut Decoder::new(), item) {
        AgentEvent::ToolCall { title, .. } => assert_eq!(title, "Bash: echo a b c"),
        other => panic!("expected tool call, got {other:?}"),
    }
}

#[test]
fn file_change_start_lists_all_paths() {
    let item = file_change_item("f1", &["src/a.rs", "src/b.rs"]);
    assert_eq!(
        started(&mut Decoder::new(), item),
        AgentEvent::ToolCall {
            id: "f1".to_owned(),
            title: "Edit src/a.rs · src/b.rs".to_owned(),
            kind: "edit".to_owned(),
            status: ToolStatus::InProgress,
        }
    );
}

#[test]
fn file_change_title_marks_hidden_paths() {
    let paths: Vec<String> = (0..60)
        .map(|index| format!("src/module_{index}.rs"))
        .collect();
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    match started(&mut Decoder::new(), file_change_item("f1", &refs)) {
        AgentEvent::ToolCall { title, .. } => {
            assert!(title.starts_with("Edit src/module_0.rs · src/module_1.rs"));
            assert!(title.contains(" …"));
            assert!(title.ends_with(')') && title.contains("(+"));
            assert!(title.chars().count() < 400);
        }
        other => panic!("expected tool call, got {other:?}"),
    }
}

#[test]
fn mcp_tool_call_start_names_server_and_tool() {
    let item = json!({"type": "mcpToolCall", "id": "m1", "server": "docs", "tool": "search", "arguments": {}});
    assert_eq!(
        started(&mut Decoder::new(), item),
        AgentEvent::ToolCall {
            id: "m1".to_owned(),
            title: "docs.search".to_owned(),
            kind: "other".to_owned(),
            status: ToolStatus::InProgress,
        }
    );
}

#[test]
fn web_search_start_uses_query() {
    let item = json!({"type": "webSearch", "id": "w1", "query": "rust jsonrpc"});
    assert_eq!(
        started(&mut Decoder::new(), item),
        AgentEvent::ToolCall {
            id: "w1".to_owned(),
            title: "Web search rust jsonrpc".to_owned(),
            kind: "fetch".to_owned(),
            status: ToolStatus::InProgress,
        }
    );
}

#[test]
fn web_search_start_without_query_has_plain_title() {
    let item = json!({"type": "webSearch", "id": "w1"});
    match started(&mut Decoder::new(), item) {
        AgentEvent::ToolCall { title, .. } => assert_eq!(title, "Web search"),
        other => panic!("expected tool call, got {other:?}"),
    }
}

#[test]
fn other_item_types_are_ignored() {
    let mut decoder = Decoder::new();
    for kind in ["agentMessage", "reasoning", "userMessage", "plan"] {
        let params = json!({"item": {"type": kind, "id": "x"}});
        assert_eq!(
            decoder.notification("item/started", &params),
            CodexEvent::Ignore
        );
        assert_eq!(
            decoder.notification("item/completed", &params),
            CodexEvent::Ignore
        );
    }
}

#[test]
fn command_completion_reports_output_and_status() {
    let mut decoder = Decoder::new();
    let item = json!({"type": "commandExecution", "id": "c1", "aggregatedOutput": "ok\n", "exitCode": 0, "status": "completed"});
    assert_eq!(
        completed(&mut decoder, item),
        AgentEvent::ToolCallUpdate {
            id: "c1".to_owned(),
            status: Some(ToolStatus::Completed),
            output: Some("ok\n".to_owned()),
            title: None,
        }
    );
}

#[test]
fn command_completion_failed_and_declined_are_failed() {
    for status in ["failed", "declined"] {
        let item = json!({"type": "commandExecution", "id": "c1", "aggregatedOutput": "no", "status": status});
        match completed(&mut Decoder::new(), item) {
            AgentEvent::ToolCallUpdate { status, .. } => {
                assert_eq!(status, Some(ToolStatus::Failed))
            }
            other => panic!("expected update, got {other:?}"),
        }
    }
}

#[test]
fn command_completion_empty_output_is_none() {
    let item = json!({"type": "commandExecution", "id": "c1", "aggregatedOutput": "", "status": "completed"});
    match completed(&mut Decoder::new(), item) {
        AgentEvent::ToolCallUpdate { output, .. } => assert_eq!(output, None),
        other => panic!("expected update, got {other:?}"),
    }
}

#[test]
fn command_completion_output_is_char_safe_and_bounded() {
    let output = "é".repeat(9000);
    let item = json!({"type": "commandExecution", "id": "c1", "aggregatedOutput": output, "status": "completed"});
    match completed(&mut Decoder::new(), item) {
        AgentEvent::ToolCallUpdate { output, .. } => {
            assert_eq!(output.unwrap().chars().count(), 8000)
        }
        other => panic!("expected update, got {other:?}"),
    }
}

#[test]
fn file_change_completion_has_no_output() {
    let mut decoder = Decoder::new();
    let done = |status: &str| json!({"type": "fileChange", "id": "f1", "status": status});
    assert_eq!(
        completed(&mut decoder, done("completed")),
        AgentEvent::ToolCallUpdate {
            id: "f1".to_owned(),
            status: Some(ToolStatus::Completed),
            output: None,
            title: None,
        }
    );
    match completed(&mut decoder, done("declined")) {
        AgentEvent::ToolCallUpdate { status, output, .. } => {
            assert_eq!(status, Some(ToolStatus::Failed));
            assert_eq!(output, None);
        }
        other => panic!("expected update, got {other:?}"),
    }
}

#[test]
fn mcp_completion_uses_string_result_as_output() {
    let item =
        json!({"type": "mcpToolCall", "id": "m1", "status": "completed", "result": "found it"});
    assert_eq!(
        completed(&mut Decoder::new(), item),
        AgentEvent::ToolCallUpdate {
            id: "m1".to_owned(),
            status: Some(ToolStatus::Completed),
            output: Some("found it".to_owned()),
            title: None,
        }
    );
}

#[test]
fn mcp_completion_joins_text_content_blocks() {
    let result = json!({"content": [{"type": "text", "text": "x"}, {"type": "image"}, {"type": "text", "text": "y"}]});
    let item = json!({"type": "mcpToolCall", "id": "m1", "status": "completed", "result": result});
    match completed(&mut Decoder::new(), item) {
        AgentEvent::ToolCallUpdate { status, output, .. } => {
            assert_eq!(status, Some(ToolStatus::Completed));
            assert_eq!(output.as_deref(), Some("x\ny"));
        }
        other => panic!("expected update, got {other:?}"),
    }
}

#[test]
fn mcp_completion_without_text_content_compacts_the_result() {
    let result = json!({"content": [], "structuredContent": {"n": 1}});
    let item = json!({"type": "mcpToolCall", "id": "m1", "status": "completed", "result": result});
    match completed(&mut Decoder::new(), item) {
        AgentEvent::ToolCallUpdate { output, .. } => {
            assert_eq!(output.as_deref(), Some(r#"{"n":1}"#))
        }
        other => panic!("expected update, got {other:?}"),
    }
}

#[test]
fn mcp_completion_error_message_becomes_output() {
    let item = json!({"type": "mcpToolCall", "id": "m1", "status": "failed", "result": null, "error": {"message": "boom"}});
    match completed(&mut Decoder::new(), item) {
        AgentEvent::ToolCallUpdate { status, output, .. } => {
            assert_eq!(status, Some(ToolStatus::Failed));
            assert_eq!(output.as_deref(), Some("boom"));
        }
        other => panic!("expected update, got {other:?}"),
    }
}

#[test]
fn mcp_completion_without_result_has_no_output() {
    let item = json!({"type": "mcpToolCall", "id": "m1", "status": "completed", "result": null});
    match completed(&mut Decoder::new(), item) {
        AgentEvent::ToolCallUpdate { output, .. } => assert_eq!(output, None),
        other => panic!("expected update, got {other:?}"),
    }
}

#[test]
fn web_search_completion_is_completed() {
    let item = json!({"type": "webSearch", "id": "w1"});
    assert_eq!(
        completed(&mut Decoder::new(), item),
        AgentEvent::ToolCallUpdate {
            id: "w1".to_owned(),
            status: Some(ToolStatus::Completed),
            output: None,
            title: None,
        }
    );
}

#[test]
fn plan_update_lists_steps() {
    let params = json!({"plan": [{"step": "read", "status": "completed"}, {"step": "write", "status": "pending"}]});
    assert_eq!(
        events(&mut Decoder::new(), "turn/plan/updated", params),
        vec![AgentEvent::Plan(vec![
            "read".to_owned(),
            "write".to_owned()
        ])]
    );
}

#[test]
fn turn_completed_maps_each_status() {
    let turn = |status: &str| json!({"threadId": "t", "turn": {"id": "u1", "status": status}});
    assert_eq!(
        turn_end(turn("completed")),
        ("u1".to_owned(), "end_turn".to_owned(), None)
    );
    assert_eq!(
        turn_end(turn("interrupted")),
        ("u1".to_owned(), "cancelled".to_owned(), None)
    );
    assert_eq!(
        turn_end(turn("cancelled")),
        ("u1".to_owned(), "cancelled".to_owned(), None)
    );
    assert_eq!(
        turn_end(turn("inProgress")),
        (
            "u1".to_owned(),
            "error".to_owned(),
            Some("inProgress".to_owned())
        )
    );
}

#[test]
fn turn_completed_failed_uses_message_or_fallback() {
    let with_message =
        json!({"turn": {"id": "u1", "status": "failed", "error": {"message": "quota"}}});
    assert_eq!(
        turn_end(with_message),
        (
            "u1".to_owned(),
            "error".to_owned(),
            Some("quota".to_owned())
        )
    );
    let without = json!({"turn": {"id": "u1", "status": "failed"}});
    assert_eq!(
        turn_end(without),
        (
            "u1".to_owned(),
            "error".to_owned(),
            Some("falha no Codex".to_owned())
        )
    );
}

#[test]
fn ignored_and_malformed_notifications() {
    let mut decoder = Decoder::new();
    let cases = [
        ("serverRequest/resolved", json!({"requestId": 1})),
        ("thread/tokenUsage/updated", json!({})),
        ("thread/started", json!({"thread": {"id": "t"}})),
        ("something/unknown", json!({"x": 1})),
        ("", json!({})),
        ("turn/started", json!({})),
        ("turn/started", json!({"turn": {"id": ""}})),
        ("turn/completed", json!({"turn": {"status": "completed"}})),
        ("turn/completed", json!({"turn": {"id": "u"}})),
        ("turn/completed", json!(null)),
        ("item/agentMessage/delta", json!({"itemId": "i"})),
        ("item/agentMessage/delta", json!({"delta": ""})),
        ("item/reasoning/textDelta", json!({"itemId": "i"})),
        ("item/started", json!({})),
        (
            "item/started",
            json!({"item": {"type": "commandExecution"}}),
        ),
        ("item/completed", json!({"item": "nope"})),
        ("turn/plan/updated", json!({})),
        ("turn/plan/updated", json!("nope")),
    ];
    for (method, params) in cases {
        assert_eq!(
            decoder.notification(method, &params),
            CodexEvent::Ignore,
            "{method} {params}"
        );
    }
}

#[test]
fn command_approval_uses_params_command_and_reason() {
    let params = json!({"threadId": "t", "turnId": "u", "itemId": "c1", "command": "rm -rf build", "cwd": "/repo", "reason": "needs network"});
    let ask = ask(
        &mut Decoder::new(),
        "item/commandExecution/requestApproval",
        params,
    );
    assert_eq!(ask.tool_call_id.as_deref(), Some("c1"));
    assert_eq!(ask.kind, "execute");
    assert_eq!(ask.title, "Bash: rm -rf build — needs network");
    assert!(ask.paths.is_empty());
    assert_eq!(ask.options, native_options_for("execute"));
}

#[test]
fn command_approval_falls_back_to_remembered_command() {
    let mut decoder = Decoder::new();
    started(
        &mut decoder,
        json!({"type": "commandExecution", "id": "c1", "command": ["npm", "test"]}),
    );
    let ask = ask(
        &mut decoder,
        "item/commandExecution/requestApproval",
        json!({"itemId": "c1"}),
    );
    assert_eq!(ask.title, "Bash: npm test");
}

#[test]
fn command_approval_without_any_command_has_plain_title() {
    let ask = ask(
        &mut Decoder::new(),
        "item/commandExecution/requestApproval",
        json!({"itemId": "c9"}),
    );
    assert_eq!(ask.title, "Bash");
    assert!(ask.paths.is_empty());
}

#[test]
fn legacy_exec_approval_matches_command_approval() {
    let params = json!({"itemId": "c1", "command": ["ls", "-la"], "reason": "inspect"});
    let ask = ask(&mut Decoder::new(), "execCommandApproval", params);
    assert_eq!(ask.kind, "execute");
    assert_eq!(ask.title, "Bash: ls -la — inspect");
    assert!(ask.paths.is_empty());
}

#[test]
fn legacy_approvals_use_call_id_when_item_id_is_absent() {
    let mut decoder = Decoder::new();
    let exec = ask(
        &mut decoder,
        "execCommandApproval",
        json!({"callId": "call_1", "conversationId": "t", "command": ["ls"], "cwd": "/repo", "parsedCmd": []}),
    );
    assert_eq!(exec.tool_call_id.as_deref(), Some("call_1"));
    let patch = ask(
        &mut decoder,
        "applyPatchApproval",
        json!({"callId": "call_2", "conversationId": "t", "fileChanges": {}}),
    );
    assert_eq!(patch.tool_call_id.as_deref(), Some("call_2"));
}

#[test]
fn item_id_takes_precedence_over_call_id() {
    let ask = ask(
        &mut Decoder::new(),
        "execCommandApproval",
        json!({"itemId": "item_1", "callId": "call_1", "command": ["ls"]}),
    );
    assert_eq!(ask.tool_call_id.as_deref(), Some("item_1"));
}

#[test]
fn approval_without_any_id_has_no_tool_call_id() {
    let ask = ask(
        &mut Decoder::new(),
        "item/commandExecution/requestApproval",
        json!({"command": "ls"}),
    );
    assert_eq!(ask.tool_call_id, None);
}

#[test]
fn legacy_apply_patch_reads_paths_and_rename_targets_from_file_changes() {
    let params = json!({
        "callId": "call_1",
        "conversationId": "t",
        "fileChanges": {
            "a.rs": {"type": "update", "unified_diff": "", "move_path": "../../out/b.rs"},
            "c.rs": {"type": "add", "content": ""},
        },
    });
    let ask = ask(&mut Decoder::new(), "applyPatchApproval", params);
    assert_eq!(ask.paths, vec!["a.rs", "../../out/b.rs", "c.rs"]);
    assert_eq!(ask.title, "Edit a.rs · ../../out/b.rs · c.rs");
}

#[test]
fn legacy_apply_patch_with_unexpected_change_shape_forces_empty_paths() {
    let params = json!({"callId": "c", "fileChanges": {"a.rs": {"type": "chmod"}}});
    let ask = ask(&mut Decoder::new(), "applyPatchApproval", params);
    assert!(ask.paths.is_empty());
    assert_eq!(ask.title, "Edit a.rs");
}

#[test]
fn command_approval_shows_network_context_and_additional_permissions() {
    let params = json!({
        "itemId": "c1",
        "command": "curl example.com",
        "networkApprovalContext": {"host": "example.com", "protocol": "https"},
        "additionalPermissions": {"fileSystem": {"write": ["/tmp/x"]}},
        "reason": "fetch",
    });
    let ask = ask(
        &mut Decoder::new(),
        "item/commandExecution/requestApproval",
        params,
    );
    assert_eq!(
        ask.title,
        r#"Bash: curl example.com — rede: https://example.com — permissões adicionais: {"fileSystem":{"write":["/tmp/x"]}} — fetch"#
    );
    assert!(ask.paths.is_empty());
}

#[test]
fn command_approval_ignores_null_grant_details() {
    let params = json!({"itemId": "c1", "command": "ls", "networkApprovalContext": null, "additionalPermissions": null});
    let ask = ask(
        &mut Decoder::new(),
        "item/commandExecution/requestApproval",
        params,
    );
    assert_eq!(ask.title, "Bash: ls");
}

#[test]
fn command_approval_with_oversized_grant_details_is_declined() {
    let params = json!({"itemId": "c1", "command": "ls", "additionalPermissions": {"fileSystem": {"write": ["x".repeat(3000)]}}});
    assert_eq!(
        Decoder::new().request("item/commandExecution/requestApproval", &params),
        CodexRequest::Reply(json!({"decision": "decline"}))
    );
}

#[test]
fn long_command_and_reason_are_cut_with_a_marker() {
    let params = json!({"itemId": "c1", "command": "x".repeat(2500), "reason": "r".repeat(400)});
    let ask = ask(
        &mut Decoder::new(),
        "item/commandExecution/requestApproval",
        params,
    );
    let expected = format!("Bash: {} … — {} …", "x".repeat(2000), "r".repeat(300));
    assert_eq!(ask.title, expected);
}

#[test]
fn titles_hide_bidi_and_zero_width_characters() {
    let command = "a\u{202E}b\u{200B}c\u{2028}d\u{FEFF}e\u{2066}f";
    let ask = ask(
        &mut Decoder::new(),
        "item/commandExecution/requestApproval",
        json!({"itemId": "c1", "command": command}),
    );
    assert_eq!(ask.title, "Bash: a b c d e f");
}

#[test]
fn started_item_titles_mark_cut_text() {
    let item = json!({"type": "commandExecution", "id": "c1", "command": "y".repeat(2100)});
    match started(&mut Decoder::new(), item) {
        AgentEvent::ToolCall { title, .. } => {
            assert_eq!(title, format!("Bash: {} …", "y".repeat(2000)))
        }
        other => panic!("expected tool call, got {other:?}"),
    }
}

#[test]
fn reason_is_sanitized_and_blank_reason_is_dropped() {
    let mut decoder = Decoder::new();
    let multiline = ask(
        &mut decoder,
        "item/commandExecution/requestApproval",
        json!({"itemId": "c1", "command": "ls", "reason": "a\nb"}),
    );
    assert_eq!(multiline.title, "Bash: ls — a b");
    let blank = ask(
        &mut decoder,
        "item/commandExecution/requestApproval",
        json!({"itemId": "c1", "command": "ls", "reason": "  "}),
    );
    assert_eq!(blank.title, "Bash: ls");
}

#[test]
fn file_change_approval_uses_remembered_paths() {
    let mut decoder = Decoder::new();
    started(
        &mut decoder,
        file_change_item("f1", &["src/a.rs", "src/b.rs"]),
    );
    let ask = ask(
        &mut decoder,
        "item/fileChange/requestApproval",
        json!({"threadId": "t", "turnId": "u", "itemId": "f1", "reason": "write outside"}),
    );
    assert_eq!(ask.tool_call_id.as_deref(), Some("f1"));
    assert_eq!(ask.kind, "edit");
    assert_eq!(ask.paths, vec!["src/a.rs", "src/b.rs"]);
    assert_eq!(ask.title, "Edit src/a.rs · src/b.rs — write outside");
    assert_eq!(
        option_ids(&ask),
        vec!["allow_once", "allow_always", "reject_once"]
    );
}

#[test]
fn file_change_approval_with_unknown_item_has_no_paths() {
    let ask = ask(
        &mut Decoder::new(),
        "item/fileChange/requestApproval",
        json!({"itemId": "ghost"}),
    );
    assert!(ask.paths.is_empty());
    assert_eq!(ask.title, "Edit");
    assert_eq!(ask.kind, "edit");
}

#[test]
fn file_change_with_unusable_path_forces_empty_paths() {
    let mut decoder = Decoder::new();
    started(
        &mut decoder,
        json!({"type": "fileChange", "id": "f1", "changes": [{"path": "a.rs"}, {"kind": "add"}]}),
    );
    let ask = ask(
        &mut decoder,
        "item/fileChange/requestApproval",
        json!({"itemId": "f1"}),
    );
    assert!(ask.paths.is_empty());
}

#[test]
fn completed_file_change_forgets_paths() {
    let mut decoder = Decoder::new();
    started(&mut decoder, file_change_item("f1", &["a.rs"]));
    completed(
        &mut decoder,
        json!({"type": "fileChange", "id": "f1", "status": "completed"}),
    );
    let ask = ask(
        &mut decoder,
        "item/fileChange/requestApproval",
        json!({"itemId": "f1"}),
    );
    assert!(ask.paths.is_empty());
}

#[test]
fn rename_target_outside_the_workspace_is_remembered_and_shown() {
    let mut decoder = Decoder::new();
    let item = json!({
        "type": "fileChange",
        "id": "f1",
        "changes": [{"path": "a.rs", "kind": {"type": "update", "move_path": "../../out/b.rs"}, "diff": ""}],
    });
    match started(&mut decoder, item) {
        AgentEvent::ToolCall { title, .. } => assert_eq!(title, "Edit a.rs · ../../out/b.rs"),
        other => panic!("expected tool call, got {other:?}"),
    }
    let ask = ask(
        &mut decoder,
        "item/fileChange/requestApproval",
        json!({"itemId": "f1"}),
    );
    assert_eq!(ask.paths, vec!["a.rs", "../../out/b.rs"]);
    let cwd = Path::new("/work/repo");
    assert!(!crate::session::session_grant_applies(&ask, cwd));
}

#[test]
fn patch_updates_add_new_targets_to_the_remembered_paths() {
    let mut decoder = Decoder::new();
    let item = json!({
        "type": "fileChange",
        "id": "f1",
        "changes": [{"path": "a.rs", "kind": {"type": "add"}, "diff": ""}],
    });
    started(&mut decoder, item);
    let update = decoder.notification(
        "item/fileChange/patchUpdated",
        &json!({
            "itemId": "f1",
            "threadId": "t",
            "turnId": "u",
            "changes": [{"path": "a.rs", "kind": {"type": "update", "move_path": "../../out/b.rs"}, "diff": ""}],
        }),
    );
    match update {
        CodexEvent::Events(events) => match events.as_slice() {
            [AgentEvent::ToolCallUpdate { id, title, .. }] => {
                assert_eq!(id, "f1");
                assert_eq!(title.as_deref(), Some("Edit a.rs · ../../out/b.rs"));
            }
            other => panic!("expected one tool update, got {other:?}"),
        },
        other => panic!("expected events, got {other:?}"),
    }
    let ask = ask(
        &mut decoder,
        "item/fileChange/requestApproval",
        json!({"itemId": "f1"}),
    );
    assert_eq!(ask.paths, vec!["a.rs", "../../out/b.rs"]);
    assert!(!crate::session::session_grant_applies(
        &ask,
        Path::new("/work/repo")
    ));
}

#[test]
fn untrusted_patch_updates_force_empty_paths() {
    let mut decoder = Decoder::new();
    let item = json!({"type": "fileChange", "id": "f1", "changes": [{"path": "a.rs", "kind": {"type": "add"}}]});
    started(&mut decoder, item);
    decoder.notification(
        "item/fileChange/patchUpdated",
        &json!({"itemId": "f1", "changes": [{"path": "b.rs", "kind": {"type": "symlink"}}]}),
    );
    let ask = ask(
        &mut decoder,
        "item/fileChange/requestApproval",
        json!({"itemId": "f1"}),
    );
    assert!(ask.paths.is_empty());
}

#[test]
fn unexpected_change_kind_shapes_force_empty_paths() {
    let kinds = [
        json!("update"),
        json!({"type": "update", "move_path": 5}),
        json!({"type": "update", "move_path": ["x"]}),
        json!({"type": "update", "move_path": ""}),
        json!({"type": "symlink"}),
        json!({"move_path": "x"}),
        json!(null),
    ];
    for kind in kinds {
        let mut decoder = Decoder::new();
        let item =
            json!({"type": "fileChange", "id": "f1", "changes": [{"path": "a.rs", "kind": kind}]});
        started(&mut decoder, item);
        let ask = ask(
            &mut decoder,
            "item/fileChange/requestApproval",
            json!({"itemId": "f1"}),
        );
        assert!(ask.paths.is_empty(), "{kind}");
    }
}

#[test]
fn file_change_without_changes_forces_empty_paths() {
    let mut decoder = Decoder::new();
    started(
        &mut decoder,
        json!({"type": "fileChange", "id": "f1", "changes": []}),
    );
    let ask = ask(
        &mut decoder,
        "item/fileChange/requestApproval",
        json!({"itemId": "f1"}),
    );
    assert!(ask.paths.is_empty());
}

#[test]
fn grant_root_forces_ask_and_is_stated_in_the_title() {
    let mut decoder = Decoder::new();
    started(&mut decoder, file_change_item("f1", &["a.rs"]));
    let ask = ask(
        &mut decoder,
        "item/fileChange/requestApproval",
        json!({"itemId": "f1", "grantRoot": "/etc", "reason": "why"}),
    );
    assert!(ask.paths.is_empty());
    assert_eq!(
        ask.title,
        "Edit a.rs — acesso de escrita a /etc pelo resto da sessão — why"
    );
}

#[test]
fn null_grant_root_keeps_the_remembered_paths() {
    let mut decoder = Decoder::new();
    started(&mut decoder, file_change_item("f1", &["a.rs"]));
    let ask = ask(
        &mut decoder,
        "item/fileChange/requestApproval",
        json!({"itemId": "f1", "grantRoot": null}),
    );
    assert_eq!(ask.paths, vec!["a.rs"]);
    assert_eq!(ask.title, "Edit a.rs");
}

#[test]
fn legacy_apply_patch_approval_matches_file_change_approval() {
    let mut decoder = Decoder::new();
    started(&mut decoder, file_change_item("f1", &["a.rs"]));
    let ask = ask(&mut decoder, "applyPatchApproval", json!({"itemId": "f1"}));
    assert_eq!(ask.kind, "edit");
    assert_eq!(ask.paths, vec!["a.rs"]);
}

#[test]
fn permissions_approval_shows_compact_permissions_and_reason() {
    let params = json!({"itemId": "p1", "reason": "needs network", "permissions": {"network": {"enabled": true}}});
    let ask = ask(
        &mut Decoder::new(),
        "item/permissions/requestApproval",
        params,
    );
    assert_eq!(ask.tool_call_id.as_deref(), Some("p1"));
    assert_eq!(ask.kind, "other");
    assert_eq!(
        ask.title,
        r#"Permissões: {"network":{"enabled":true}} — needs network"#
    );
    assert!(ask.paths.is_empty());
    assert_eq!(option_ids(&ask), vec!["allow_once", "reject_once"]);
}

#[test]
fn permissions_approval_too_long_to_show_is_denied() {
    let params =
        json!({"itemId": "p1", "permissions": {"fileSystem": {"write": ["x".repeat(5000)]}}});
    assert_eq!(
        Decoder::new().request("item/permissions/requestApproval", &params),
        CodexRequest::Reply(json!({"permissions": {}, "scope": "turn"}))
    );
}

#[test]
fn permissions_approval_non_object_permissions_grant_nothing() {
    for permissions in [json!(null), json!("all"), json!(["x"])] {
        let params = json!({"itemId": "p1", "permissions": permissions});
        let ask = ask(
            &mut Decoder::new(),
            "item/permissions/requestApproval",
            params.clone(),
        );
        assert_eq!(ask.title, "Permissões");
        assert_eq!(
            approval_response(
                "item/permissions/requestApproval",
                &params,
                &selected("allow_once")
            ),
            json!({"permissions": {}, "scope": "turn"})
        );
    }
}

#[test]
fn permissions_approval_without_permissions_has_plain_title() {
    let ask = ask(
        &mut Decoder::new(),
        "item/permissions/requestApproval",
        json!({"itemId": "p1"}),
    );
    assert_eq!(ask.title, "Permissões");
}

#[test]
fn user_input_requests_answer_every_question_with_nothing() {
    let mut decoder = Decoder::new();
    let params = json!({"questions": [{"id": "q1", "header": "h", "question": "?"}, {"id": "q2", "header": "h", "question": "?"}]});
    for method in ["item/tool/requestUserInput", "requestUserInput"] {
        assert_eq!(
            decoder.request(method, &params),
            CodexRequest::Reply(json!({"answers": {"q1": {"answers": []}, "q2": {"answers": []}}}))
        );
        assert_eq!(
            decoder.request(method, &json!({"questions": []})),
            CodexRequest::Reply(json!({"answers": {}}))
        );
    }
}

#[test]
fn requests_without_a_schema_valid_reply_are_unsupported() {
    let mut decoder = Decoder::new();
    for method in [
        "account/chatgptAuthTokens/refresh",
        "currentTime/read",
        "attestation/generate",
        "something/new",
    ] {
        assert_eq!(
            decoder.request(method, &json!({"x": 1})),
            CodexRequest::Unsupported,
            "{method}"
        );
    }
}

#[test]
fn elicitation_and_dynamic_tool_requests_get_schema_valid_refusals() {
    let mut decoder = Decoder::new();
    assert_eq!(
        decoder.request("mcpServer/elicitation/request", &json!({})),
        CodexRequest::Reply(json!({"action": "decline"}))
    );
    assert_eq!(
        decoder.request("item/tool/call", &json!({})),
        CodexRequest::Reply(json!({"success": false, "contentItems": []}))
    );
}

#[test]
fn reasoning_without_item_id_is_ignored() {
    let mut decoder = Decoder::new();
    assert_eq!(
        decoder.notification("item/reasoning/textDelta", &json!({"delta": "x"})),
        CodexEvent::Ignore
    );
    assert_eq!(
        decoder.notification("item/reasoning/summaryTextDelta", &json!({"delta": "x"})),
        CodexEvent::Ignore
    );
}

#[test]
fn legacy_approvals_use_review_decision_values() {
    for method in ["execCommandApproval", "applyPatchApproval"] {
        let respond = |answer: PermissionAnswer| approval_response(method, &json!({}), &answer);
        assert_eq!(
            respond(selected("allow_once")),
            json!({"decision": "approved"})
        );
        assert_eq!(
            respond(selected("allow_always")),
            json!({"decision": "approved"})
        );
        for denied in [selected("reject_once"), selected("whatever")] {
            let response = respond(denied);
            assert!(response["decision"]["denied"]["rejection"].is_string());
            assert_eq!(response["decision"].as_object().unwrap().len(), 1);
        }
        assert_eq!(
            respond(PermissionAnswer::Cancelled),
            json!({"decision": "abort"})
        );
    }
}

#[test]
fn command_and_file_approvals_map_answers_to_decisions() {
    let methods = [
        "item/commandExecution/requestApproval",
        "item/fileChange/requestApproval",
    ];
    for method in methods {
        let respond = |answer: PermissionAnswer| approval_response(method, &json!({}), &answer);
        assert_eq!(
            respond(selected("allow_once")),
            json!({"decision": "accept"})
        );
        assert_eq!(
            respond(selected("allow_always")),
            json!({"decision": "accept"})
        );
        assert_eq!(
            respond(selected("reject_once")),
            json!({"decision": "decline"})
        );
        assert_eq!(
            respond(selected("whatever")),
            json!({"decision": "decline"})
        );
        assert_eq!(
            respond(PermissionAnswer::Cancelled),
            json!({"decision": "cancel"})
        );
    }
}

#[test]
fn approval_response_never_grants_session_or_global_scope() {
    let methods = [
        "item/commandExecution/requestApproval",
        "execCommandApproval",
        "item/fileChange/requestApproval",
        "applyPatchApproval",
        "item/permissions/requestApproval",
        "unknown",
    ];
    let answers = [
        selected("allow_once"),
        selected("allow_always"),
        selected("reject_once"),
        selected("acceptForSession"),
        selected("acceptGlobal"),
        selected(""),
        PermissionAnswer::Cancelled,
    ];
    for method in methods {
        for answer in &answers {
            let text =
                approval_response(method, &json!({"permissions": {"a": 1}}), answer).to_string();
            assert!(!text.contains("acceptForSession"), "{method} {answer:?}");
            assert!(!text.contains("acceptGlobal"), "{method} {answer:?}");
        }
    }
}

#[test]
fn permissions_approval_grants_requested_permissions_for_the_turn() {
    let params = json!({"permissions": {"network": {"enabled": true}}});
    for id in ["allow_once", "allow_always"] {
        assert_eq!(
            approval_response("item/permissions/requestApproval", &params, &selected(id)),
            json!({"permissions": {"network": {"enabled": true}}, "scope": "turn"})
        );
    }
    assert_eq!(
        approval_response(
            "item/permissions/requestApproval",
            &json!({}),
            &selected("allow_once")
        ),
        json!({"permissions": {}, "scope": "turn"})
    );
}

#[test]
fn permissions_approval_rejection_and_cancel_grant_nothing() {
    let params = json!({"permissions": {"network": {"enabled": true}}});
    let denied = json!({"permissions": {}, "scope": "turn"});
    assert_eq!(
        approval_response(
            "item/permissions/requestApproval",
            &params,
            &selected("reject_once")
        ),
        denied
    );
    assert_eq!(
        approval_response(
            "item/permissions/requestApproval",
            &params,
            &PermissionAnswer::Cancelled
        ),
        denied
    );
}

#[test]
fn initialize_params_identify_argon() {
    assert_eq!(
        initialize_params("1.2.3"),
        json!({
            "clientInfo": {"name": "argon", "title": "Argon", "version": "1.2.3"},
            "capabilities": {"experimentalApi": true, "requestAttestation": false},
        })
    );
}

#[test]
fn thread_params_follow_permission_mode() {
    let cwd = Path::new("repo");
    for mode in [PermissionMode::Supervised, PermissionMode::AcceptEdits] {
        assert_eq!(
            thread_params(cwd, None, mode),
            json!({"cwd": "repo", "approvalPolicy": "on-request", "approvalsReviewer": "user", "sandbox": "read-only"})
        );
    }
    assert_eq!(
        thread_params(cwd, Some("gpt-5"), PermissionMode::FullAccess),
        json!({"cwd": "repo", "approvalPolicy": "never", "approvalsReviewer": "user", "sandbox": "workspace-write", "model": "gpt-5"})
    );
}

#[test]
fn thread_params_skip_empty_model() {
    let params = thread_params(Path::new("repo"), Some(""), PermissionMode::Supervised);
    assert!(params.get("model").is_none());
}

#[test]
fn resume_params_add_thread_id() {
    assert_eq!(
        resume_params(
            "th_1",
            Path::new("repo"),
            Some("gpt-5"),
            PermissionMode::Supervised
        ),
        json!({"threadId": "th_1", "cwd": "repo", "approvalPolicy": "on-request", "approvalsReviewer": "user", "sandbox": "read-only", "model": "gpt-5"})
    );
}

#[test]
fn turn_params_include_optional_model_and_effort() {
    assert_eq!(
        turn_params("th_1", "revise", None, None),
        json!({"threadId": "th_1", "input": [{"type": "text", "text": "revise"}]})
    );
    assert_eq!(
        turn_params("th_1", "revise", Some("gpt-5"), Some("high")),
        json!({"threadId": "th_1", "input": [{"type": "text", "text": "revise"}], "model": "gpt-5", "effort": "high"})
    );
}

#[test]
fn turn_mode_overrides_follow_permission_mode() {
    let base = || turn_params("th_1", "revise", None, None);
    for mode in [PermissionMode::Supervised, PermissionMode::AcceptEdits] {
        let params = with_mode(base(), mode);
        assert_eq!(params["approvalPolicy"], json!("on-request"));
        assert_eq!(params["approvalsReviewer"], json!("user"));
        assert_eq!(params["sandboxPolicy"], json!({"type": "readOnly"}));
        assert_eq!(params["threadId"], json!("th_1"));
    }
    let full = with_mode(base(), PermissionMode::FullAccess);
    assert_eq!(full["approvalPolicy"], json!("never"));
    assert_eq!(full["approvalsReviewer"], json!("user"));
    assert_eq!(full["sandboxPolicy"], json!({"type": "workspaceWrite"}));
}

#[test]
fn untrusted_projects_go_into_the_thread_config() {
    let params = with_untrusted_projects(
        thread_params(Path::new("repo"), None, PermissionMode::Supervised),
        &["C:\\Work\\Repo".to_owned(), "c:\\work\\repo".to_owned()],
    );
    assert_eq!(
        params["config"],
        json!({"projects": {
            "C:\\Work\\Repo": {"trust_level": "untrusted"},
            "c:\\work\\repo": {"trust_level": "untrusted"},
        }})
    );
    assert_eq!(params["sandbox"], json!("read-only"));
}

#[test]
fn response_ids_come_from_thread_and_turn_objects() {
    assert_eq!(
        thread_id(&json!({"thread": {"id": "th_1"}, "model": "gpt-5"})),
        Some("th_1".to_owned())
    );
    assert_eq!(
        turn_id(&json!({"turn": {"id": "u1", "status": "inProgress"}})),
        Some("u1".to_owned())
    );
    for malformed in [
        json!({}),
        json!({"thread": {"id": ""}}),
        json!({"turn": "u1"}),
    ] {
        assert_eq!(thread_id(&malformed), None);
        assert_eq!(turn_id(&malformed), None);
        assert_eq!(thread_model(&malformed), None);
    }
    assert_eq!(
        thread_model(&json!({"thread": {"id": "th_1"}, "model": "gpt-5"})),
        Some("gpt-5".to_owned())
    );
}

#[test]
fn message_turn_reads_turn_id_or_nested_turn() {
    assert_eq!(
        message_turn(&json!({"threadId": "th", "turnId": "u1", "delta": "x"})),
        Some("u1".to_owned())
    );
    assert_eq!(
        message_turn(&json!({"threadId": "th", "turn": {"id": "u2"}})),
        Some("u2".to_owned())
    );
    assert_eq!(message_turn(&json!({"callId": "c"})), None);
}

#[test]
fn interrupt_params_name_thread_and_turn() {
    assert_eq!(
        interrupt_params("th_1", "u1"),
        json!({"threadId": "th_1", "turnId": "u1"})
    );
}

fn model_list() -> Value {
    json!({
        "data": [
            {"id": "gpt-5", "displayName": "GPT-5", "isDefault": true, "hidden": false},
            {"id": "internal", "displayName": "Internal", "isDefault": false, "hidden": true},
            {"id": "gpt-5-mini", "isDefault": false, "hidden": false},
            {"id": "o4", "displayName": "", "isDefault": false},
            {"displayName": "No id"},
        ],
        "nextCursor": "page2",
    })
}

#[test]
fn parse_models_skips_hidden_and_falls_back_to_id() {
    let labels: Vec<(String, String)> = parse_models(&model_list())
        .into_iter()
        .map(|model| (model.id, model.label))
        .collect();
    assert_eq!(
        labels,
        vec![
            ("gpt-5".to_owned(), "GPT-5".to_owned()),
            ("gpt-5-mini".to_owned(), "gpt-5-mini".to_owned()),
            ("o4".to_owned(), "o4".to_owned()),
        ]
    );
}

#[test]
fn default_model_is_the_flagged_one() {
    assert_eq!(default_model(&model_list()), Some("gpt-5".to_owned()));
    assert_eq!(default_model(&json!({"data": [{"id": "a"}]})), None);
}

#[test]
fn next_cursor_is_present_only_when_set() {
    assert_eq!(next_cursor(&model_list()), Some("page2".to_owned()));
    assert_eq!(next_cursor(&json!({"data": [], "nextCursor": null})), None);
    assert_eq!(next_cursor(&json!({"data": [], "nextCursor": ""})), None);
    assert_eq!(next_cursor(&json!({"data": []})), None);
}

#[test]
fn model_parsing_tolerates_malformed_results() {
    for result in [json!(null), json!({}), json!({"data": "x"}), json!([])] {
        assert!(parse_models(&result).is_empty());
        assert_eq!(default_model(&result), None);
        assert_eq!(next_cursor(&result), None);
    }
}
