use serde_json::json;

use super::*;

fn cwd() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\work\repo")
    } else {
        PathBuf::from("/work/repo")
    }
}

fn inside(name: &str) -> String {
    cwd().join(name).to_string_lossy().into_owned()
}

fn request(kind: Option<&str>, locations: &[String], raw_input: Value) -> Value {
    let mut tool_call = json!({
        "toolCallId": "call_1",
        "title": "Tool",
        "locations": locations.iter().map(|path| json!({"path": path})).collect::<Vec<_>>(),
        "rawInput": raw_input,
    });
    if let Some(kind) = kind {
        tool_call["kind"] = json!(kind);
    }
    json!({
        "sessionId": "sess_fixture",
        "toolCall": tool_call,
        "options": [
            {"optionId": "allow-once", "name": "Allow", "kind": "allow_once"},
            {"optionId": "allow-always", "name": "Always allow", "kind": "allow_always"},
            {"optionId": "reject-once", "name": "Reject", "kind": "reject_once"}
        ]
    })
}

fn allowed(decision: &PermissionDecision) -> bool {
    matches!(decision, PermissionDecision::Allow { option_id } if option_id == "allow-once")
}

#[test]
fn read_and_search_inside_worktree_are_auto_allowed() {
    for kind in ["read", "search"] {
        let decision = decide(
            &request(Some(kind), &[inside("src/a.rs")], json!({})),
            None,
            &cwd(),
        );
        assert!(allowed(&decision), "{kind}: {decision:?}");
    }
    let relative = decide(
        &request(Some("read"), &["src/a.rs".into()], json!({})),
        None,
        &cwd(),
    );
    assert!(allowed(&relative));
    let no_locations = decide(
        &request(Some("search"), &[], json!({"pattern": "x"})),
        None,
        &cwd(),
    );
    assert!(allowed(&no_locations));
}

#[test]
fn fetch_is_auto_allowed() {
    let decision = decide(
        &request(Some("fetch"), &[], json!({"url": "https://example.com"})),
        None,
        &cwd(),
    );
    assert!(allowed(&decision));
}

#[test]
fn reads_outside_worktree_ask() {
    let outside = if cfg!(windows) {
        r"C:\Users\me\.ssh\id_rsa"
    } else {
        "/home/me/.ssh/id_rsa"
    };
    for locations in [
        vec![outside.to_string()],
        vec!["../other/secret.txt".to_string()],
    ] {
        let decision = decide(&request(Some("read"), &locations, json!({})), None, &cwd());
        assert!(
            matches!(decision, PermissionDecision::Ask(_)),
            "{locations:?}"
        );
    }
    let via_input = decide(
        &request(Some("read"), &[], json!({"file_path": outside})),
        None,
        &cwd(),
    );
    assert!(matches!(via_input, PermissionDecision::Ask(_)));
}

#[test]
fn edit_execute_and_unknown_kinds_ask_with_all_options() {
    for kind in [
        Some("edit"),
        Some("execute"),
        Some("delete"),
        Some("move"),
        Some("other"),
        None,
    ] {
        let decision = decide(
            &request(kind, &[inside("src/a.rs")], json!({})),
            None,
            &cwd(),
        );
        let PermissionDecision::Ask(prompt) = decision else {
            panic!("{kind:?} should ask");
        };
        assert_eq!(prompt.kind, kind.unwrap_or("other"));
        assert_eq!(prompt.title, "Tool");
        assert_eq!(
            prompt
                .options
                .iter()
                .map(|o| o.kind.as_str())
                .collect::<Vec<_>>(),
            ["allow_once", "allow_always", "reject_once"]
        );
    }
}

#[test]
fn kind_from_earlier_tool_call_is_used_when_request_omits_it() {
    let req = request(None, &[inside("src/a.rs")], json!({}));
    assert!(allowed(&decide(&req, Some("read"), &cwd())));
    assert!(matches!(
        decide(&req, Some("edit"), &cwd()),
        PermissionDecision::Ask(_)
    ));
}

#[test]
fn read_without_allow_once_option_asks() {
    let mut req = request(Some("read"), &[inside("a.rs")], json!({}));
    req["options"] = json!([{"optionId": "always", "name": "Always", "kind": "allow_always"}]);
    assert!(matches!(
        decide(&req, None, &cwd()),
        PermissionDecision::Ask(_)
    ));
}

#[test]
fn inside_handles_prefix_collisions_and_dots() {
    let root = cwd();
    assert!(is_inside(&root, &inside("a/../b.rs")));
    assert!(!is_inside(&root, &format!("{}-evil/x", root.display())));
    assert!(is_inside(&root, &root.to_string_lossy()));
}
