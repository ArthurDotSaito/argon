use serde_json::json;

use super::*;

fn line(value: Value) -> ClaudeLine {
    parse_line(&value.to_string())
}

fn assistant(blocks: Value) -> Value {
    json!({
        "type": "assistant",
        "message": {"content": blocks},
        "parent_tool_use_id": null,
    })
}

fn tool_use(name: &str, input: Value) -> Value {
    json!({"type": "tool_use", "id": "toolu_1", "name": name, "input": input})
}

fn events(parsed: ClaudeLine) -> Vec<AgentEvent> {
    match parsed {
        ClaudeLine::Events(events) => events,
        other => panic!("expected events, got {other:?}"),
    }
}

fn turn_end(value: Value) -> (String, Option<String>) {
    match line(value) {
        ClaudeLine::TurnEnd { stop_reason, error } => (stop_reason, error),
        other => panic!("expected turn end, got {other:?}"),
    }
}

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn init_carries_session_and_model() {
    let parsed = line(json!({
        "type": "system",
        "subtype": "init",
        "session_id": "sess-1",
        "model": "claude-sonnet-4-5",
        "tools": ["Bash"],
    }));

    assert_eq!(
        parsed,
        ClaudeLine::Init {
            session_id: "sess-1".to_owned(),
            model: "claude-sonnet-4-5".to_owned(),
        }
    );
}

#[test]
fn other_system_subtypes_are_ignored() {
    let parsed = line(json!({"type": "system", "subtype": "compact_boundary"}));

    assert_eq!(parsed, ClaudeLine::Ignore);
}

#[test]
fn assistant_text_becomes_message_chunk() {
    let parsed = line(assistant(json!([{"type": "text", "text": "olá"}])));

    assert_eq!(
        events(parsed),
        vec![AgentEvent::MessageChunk("olá".to_owned())]
    );
}

#[test]
fn assistant_thinking_becomes_thought_chunk() {
    let parsed = line(assistant(json!([
        {"type": "thinking", "thinking": "hmm", "signature": "x"}
    ])));

    assert_eq!(
        events(parsed),
        vec![AgentEvent::ThoughtChunk("hmm".to_owned())]
    );
}

#[test]
fn assistant_tool_use_becomes_in_progress_tool_call() {
    let parsed = line(assistant(json!([tool_use(
        "Read",
        json!({"file_path": "src/a.rs"})
    )])));

    assert_eq!(
        events(parsed),
        vec![AgentEvent::ToolCall {
            id: "toolu_1".to_owned(),
            title: "Read src/a.rs".to_owned(),
            kind: "read".to_owned(),
            status: ToolStatus::InProgress,
        }]
    );
}

#[test]
fn assistant_blocks_keep_their_order() {
    let parsed = line(assistant(json!([
        {"type": "text", "text": "vou ler"},
        tool_use("Bash", json!({"command": "git diff"})),
    ])));

    let events = events(parsed);

    assert_eq!(events.len(), 2);
    assert_eq!(events[0], AgentEvent::MessageChunk("vou ler".to_owned()));
    assert!(matches!(&events[1], AgentEvent::ToolCall { title, .. } if title == "Bash: git diff"));
}

#[test]
fn todo_write_becomes_plan() {
    let parsed = line(assistant(json!([tool_use(
        "TodoWrite",
        json!({"todos": [
            {"content": "ler o código", "status": "pending"},
            {"content": "escrever teste", "status": "pending"},
        ]})
    )])));

    assert_eq!(
        events(parsed),
        vec![AgentEvent::Plan(vec![
            "ler o código".to_owned(),
            "escrever teste".to_owned(),
        ])]
    );
}

#[test]
fn subagent_assistant_traffic_is_skipped() {
    let parsed = line(json!({
        "type": "assistant",
        "message": {"content": [
            {"type": "text", "text": "dentro do subagente"},
            tool_use("Bash", json!({"command": "ls"})),
        ]},
        "parent_tool_use_id": "toolu_parent",
    }));

    assert_eq!(parsed, ClaudeLine::Ignore);
}

#[test]
fn api_error_message_becomes_api_error() {
    let parsed = line(json!({
        "type": "assistant",
        "isApiErrorMessage": true,
        "message": {"content": [
            {"type": "text", "text": "You've hit your session limit · resets 6:50pm"}
        ]},
        "parent_tool_use_id": null,
    }));

    assert_eq!(
        parsed,
        ClaudeLine::ApiError("You've hit your session limit · resets 6:50pm".to_owned())
    );
}

#[test]
fn api_error_joins_text_blocks() {
    let parsed = line(json!({
        "type": "assistant",
        "isApiErrorMessage": true,
        "message": {"content": [
            {"type": "text", "text": "primeira"},
            {"type": "text", "text": "segunda"},
        ]},
    }));

    assert_eq!(parsed, ClaudeLine::ApiError("primeira\nsegunda".to_owned()));
}

#[test]
fn api_error_without_text_is_ignored() {
    let parsed = line(json!({
        "type": "assistant",
        "isApiErrorMessage": true,
        "message": {"content": [{"type": "image", "source": {}}]},
    }));

    assert_eq!(parsed, ClaudeLine::Ignore);
}

#[test]
fn tool_result_without_text_has_no_output() {
    let parsed = line(json!({
        "type": "user",
        "message": {"content": [
            {"type": "tool_result", "tool_use_id": "toolu_1", "content": ""}
        ]},
    }));

    assert_eq!(
        events(parsed),
        vec![AgentEvent::ToolCallUpdate {
            id: "toolu_1".to_owned(),
            status: Some(ToolStatus::Completed),
            output: None,
            title: None,
        }]
    );
}

#[test]
fn tool_result_with_string_content_completes_the_call() {
    let parsed = line(json!({
        "type": "user",
        "message": {"content": [
            {"type": "tool_result", "tool_use_id": "toolu_1", "content": "ok", "is_error": false}
        ]},
    }));

    assert_eq!(
        events(parsed),
        vec![AgentEvent::ToolCallUpdate {
            id: "toolu_1".to_owned(),
            status: Some(ToolStatus::Completed),
            output: Some("ok".to_owned()),
            title: None,
        }]
    );
}

#[test]
fn tool_result_with_block_content_joins_text_blocks() {
    let parsed = line(json!({
        "type": "user",
        "message": {"content": [
            {"type": "tool_result", "tool_use_id": "toolu_1", "content": [
                {"type": "text", "text": "linha 1"},
                {"type": "image", "source": {}},
                {"type": "text", "text": "linha 2"},
            ]}
        ]},
    }));

    assert_eq!(
        events(parsed),
        vec![AgentEvent::ToolCallUpdate {
            id: "toolu_1".to_owned(),
            status: Some(ToolStatus::Completed),
            output: Some("linha 1\nlinha 2".to_owned()),
            title: None,
        }]
    );
}

#[test]
fn tool_result_error_flag_fails_the_call() {
    let parsed = line(json!({
        "type": "user",
        "message": {"content": [
            {"type": "tool_result", "tool_use_id": "toolu_1", "content": "boom", "is_error": true}
        ]},
    }));

    assert!(matches!(
        events(parsed).as_slice(),
        [AgentEvent::ToolCallUpdate {
            status: Some(ToolStatus::Failed),
            ..
        }]
    ));
}

#[test]
fn tool_result_output_is_truncated_to_limit() {
    let long = "é".repeat(OUTPUT_LIMIT + 50);
    let parsed = line(json!({
        "type": "user",
        "message": {"content": [
            {"type": "tool_result", "tool_use_id": "toolu_1", "content": long}
        ]},
    }));

    let events = events(parsed);

    let AgentEvent::ToolCallUpdate { output, .. } = &events[0] else {
        panic!("expected tool call update");
    };
    assert_eq!(output.as_ref().unwrap().chars().count(), OUTPUT_LIMIT);
}

#[test]
fn user_string_content_is_ignored() {
    let parsed = line(json!({"type": "user", "message": {"content": "oi"}}));

    assert_eq!(parsed, ClaudeLine::Ignore);
}

#[test]
fn can_use_tool_becomes_permission_with_ask() {
    let parsed = line(json!({
        "type": "control_request",
        "request_id": "req-1",
        "request": {
            "subtype": "can_use_tool",
            "tool_name": "Edit",
            "input": {"file_path": "src/a.rs", "old_string": "a", "new_string": "b"},
            "tool_use_id": "toolu_9",
        },
    }));

    let ClaudeLine::Permission {
        request_id,
        tool_name,
        ask,
        input,
    } = parsed
    else {
        panic!("expected permission");
    };
    assert_eq!(request_id, "req-1");
    assert_eq!(tool_name, "Edit");
    assert_eq!(input["new_string"], "b");
    assert_eq!(ask.tool_call_id.as_deref(), Some("toolu_9"));
    assert_eq!(ask.kind, "edit");
    assert_eq!(ask.title, "Edit src/a.rs");
    assert_eq!(ask.paths, vec!["src/a.rs".to_owned()]);
    assert_eq!(ask.options, native_options_for("edit"));
    assert!(ask.options.iter().any(|option| option.id == "allow_always"));
}

#[test]
fn can_use_tool_without_tool_use_id_has_no_call_id() {
    let parsed = line(json!({
        "type": "control_request",
        "request_id": "req-2",
        "request": {"subtype": "can_use_tool", "tool_name": "Bash", "input": {"command": "ls"}},
    }));

    let ClaudeLine::Permission { ask, .. } = parsed else {
        panic!("expected permission");
    };
    assert_eq!(ask.tool_call_id, None);
    assert_eq!(ask.kind, "execute");
    assert_eq!(ask.title, "Bash: ls");
    assert!(ask.paths.is_empty());
    assert!(ask.options.iter().all(|option| option.id != "allow_always"));
}

#[test]
fn can_use_tool_without_request_id_is_ignored() {
    let request =
        json!({"subtype": "can_use_tool", "tool_name": "Bash", "input": {"command": "ls"}});

    assert_eq!(
        line(json!({"type": "control_request", "request": request.clone()})),
        ClaudeLine::Ignore
    );
    assert_eq!(
        line(json!({"type": "control_request", "request_id": "", "request": request})),
        ClaudeLine::Ignore
    );
}

#[test]
fn other_control_request_subtypes_are_ignored() {
    let parsed = line(json!({
        "type": "control_request",
        "request_id": "req-3",
        "request": {"subtype": "hook_callback"},
    }));

    assert_eq!(parsed, ClaudeLine::Ignore);
}

#[test]
fn result_success_ends_the_turn() {
    let (stop_reason, error) = turn_end(json!({
        "type": "result",
        "subtype": "success",
        "is_error": false,
        "result": "feito",
        "session_id": "sess-1",
    }));

    assert_eq!(stop_reason, "end_turn");
    assert_eq!(error, None);
}

#[test]
fn result_success_flagged_as_error_is_an_error() {
    let (stop_reason, error) = turn_end(json!({
        "type": "result",
        "subtype": "success",
        "is_error": true,
        "result": "Invalid API key",
    }));

    assert_eq!(stop_reason, "error");
    assert_eq!(error.as_deref(), Some("Invalid API key"));
}

#[test]
fn result_max_turns_reports_its_own_stop_reason() {
    let (stop_reason, error) = turn_end(json!({
        "type": "result",
        "subtype": "error_max_turns",
        "is_error": true,
    }));

    assert_eq!(stop_reason, "max_turns");
    assert_eq!(error.as_deref(), Some("limite de turnos atingido"));
}

#[test]
fn result_max_turns_prefers_result_text() {
    let (_, error) = turn_end(json!({
        "type": "result",
        "subtype": "error_max_turns",
        "result": "acabaram os turnos",
    }));

    assert_eq!(error.as_deref(), Some("acabaram os turnos"));
}

#[test]
fn result_other_error_uses_result_text_or_subtype() {
    let (stop_reason, error) = turn_end(json!({
        "type": "result",
        "subtype": "error_during_execution",
        "is_error": true,
        "result": "falhou",
    }));
    let (_, fallback) = turn_end(json!({
        "type": "result",
        "subtype": "error_during_execution",
        "is_error": true,
    }));

    assert_eq!(stop_reason, "error");
    assert_eq!(error.as_deref(), Some("falhou"));
    assert_eq!(fallback.as_deref(), Some("error_during_execution"));
}

#[test]
fn unknown_type_malformed_and_blank_lines_are_ignored() {
    assert_eq!(
        line(json!({"type": "stream_event", "event": {}})),
        ClaudeLine::Ignore
    );
    assert_eq!(
        line(json!({"type": "control_response", "response": {}})),
        ClaudeLine::Ignore
    );
    assert_eq!(parse_line("{not json"), ClaudeLine::Ignore);
    assert_eq!(parse_line(""), ClaudeLine::Ignore);
    assert_eq!(parse_line("   "), ClaudeLine::Ignore);
}

#[test]
fn kind_for_maps_tool_names() {
    let table = [
        ("Read", "read"),
        ("LS", "read"),
        ("NotebookRead", "read"),
        ("Grep", "search"),
        ("Glob", "search"),
        ("Edit", "edit"),
        ("Write", "edit"),
        ("MultiEdit", "edit"),
        ("NotebookEdit", "edit"),
        ("Bash", "execute"),
        ("BashOutput", "execute"),
        ("KillShell", "execute"),
        ("WebFetch", "fetch"),
        ("WebSearch", "fetch"),
        ("Task", "other"),
        ("mcp__server__tool", "other"),
    ];

    for (name, kind) in table {
        assert_eq!(kind_for(name), kind, "{name}");
    }
}

#[test]
fn title_uses_first_present_subject() {
    let title = |name: &str, input: Value| tool_title(name, &input);

    assert_eq!(
        title("Read", json!({"file_path": "src/a.rs"})),
        "Read src/a.rs"
    );
    assert_eq!(
        title("NotebookEdit", json!({"notebook_path": "n.ipynb"})),
        "NotebookEdit n.ipynb"
    );
    assert_eq!(title("LS", json!({"path": "/tmp"})), "LS /tmp");
    assert_eq!(title("Grep", json!({"pattern": "fn main"})), "Grep fn main");
    assert_eq!(
        title("Bash", json!({"command": "git diff"})),
        "Bash: git diff"
    );
    assert_eq!(
        title("WebFetch", json!({"url": "https://example.com"})),
        "WebFetch https://example.com"
    );
    assert_eq!(
        title("WebSearch", json!({"query": "rust"})),
        "WebSearch rust"
    );
    assert_eq!(
        title("Grep", json!({"pattern": "x", "path": "src"})),
        "Grep src"
    );
    assert_eq!(title("Task", json!({"description": "x"})), "Task");
    assert_eq!(title("Task", Value::Null), "Task");
}

#[test]
fn title_subject_is_truncated() {
    let command = "x".repeat(SUBJECT_LIMIT + 40);

    let title = tool_title("Bash", &json!({"command": command}));

    assert_eq!(title, format!("Bash: {}", "x".repeat(SUBJECT_LIMIT)));
}

#[test]
fn title_replaces_line_breaks_with_spaces() {
    let title = tool_title("Bash", &json!({"command": "git add .\r\ngit commit\nok"}));

    assert_eq!(title, "Bash: git add .  git commit ok");
}

#[test]
fn user_message_builds_user_line() {
    let built = user_message("diga \"oi\"\nagora");

    assert!(!built.contains('\n'));
    assert_eq!(
        parsed(&built),
        json!({"type": "user", "message": {"role": "user", "content": "diga \"oi\"\nagora"}})
    );
}

#[test]
fn allow_builds_success_response_with_updated_input() {
    let built = allow("req-1", &json!({"command": "ls"}));

    assert_eq!(
        parsed(&built),
        json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": "req-1",
                "response": {"behavior": "allow", "updatedInput": {"command": "ls"}},
            },
        })
    );
}

#[test]
fn deny_builds_success_response_with_message() {
    let built = deny("req-1", "negado pelo usuário");

    assert_eq!(
        parsed(&built),
        json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": "req-1",
                "response": {"behavior": "deny", "message": "negado pelo usuário"},
            },
        })
    );
}

#[test]
fn interrupt_builds_control_request() {
    let built = interrupt("req-7");

    assert_eq!(
        parsed(&built),
        json!({
            "type": "control_request",
            "request_id": "req-7",
            "request": {"subtype": "interrupt"},
        })
    );
}

#[test]
fn input_paths_covers_claude_edit_input() {
    let paths = input_paths(&json!({
        "file_path": "src/a.rs",
        "old_string": "a",
        "new_string": "b",
    }));

    assert_eq!(paths, vec!["src/a.rs".to_owned()]);
}

#[test]
fn input_paths_scopes_path_like_glob_pattern() {
    let paths = input_paths(&json!({"pattern": "/etc/**/*.conf"}));

    assert_eq!(paths, vec!["/etc/".to_owned()]);
}

#[test]
fn input_paths_ignores_relative_regex_pattern() {
    let paths = input_paths(&json!({"pattern": "fn main", "path": "src"}));

    assert_eq!(paths, vec!["src".to_owned()]);
}

#[test]
fn input_paths_reads_notebook_path() {
    let paths = input_paths(&json!({"notebook_path": "n.ipynb"}));

    assert_eq!(paths, vec!["n.ipynb".to_owned()]);
}
