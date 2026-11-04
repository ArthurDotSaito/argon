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

fn decide_acp(request: &Value, known_kind: Option<&str>, full_access: bool) -> PermissionDecision {
    decide(&acp_ask(request, known_kind), &cwd(), full_access)
}

fn allowed(decision: &PermissionDecision) -> bool {
    matches!(decision, PermissionDecision::Allow { option_id } if option_id == "allow-once")
}

fn asks(decision: &PermissionDecision) -> bool {
    matches!(decision, PermissionDecision::Ask(_))
}

fn outside() -> &'static str {
    if cfg!(windows) {
        r"C:\Users\me\.ssh\id_rsa"
    } else {
        "/home/me/.ssh/id_rsa"
    }
}

#[test]
fn read_and_search_inside_worktree_are_auto_allowed() {
    for kind in ["read", "search"] {
        let decision = decide_acp(
            &request(Some(kind), &[inside("src/a.rs")], json!({})),
            None,
            false,
        );
        assert!(allowed(&decision), "{kind}: {decision:?}");
    }
    let relative = decide_acp(
        &request(Some("read"), &["src/a.rs".into()], json!({})),
        None,
        false,
    );
    assert!(allowed(&relative));
    let search_in_cwd = decide_acp(
        &request(
            Some("search"),
            &[],
            json!({"pattern": "fn main", "glob": "src/**/*.rs"}),
        ),
        None,
        false,
    );
    assert!(allowed(&search_in_cwd));
}

#[test]
fn fetch_always_asks() {
    for input in [json!({"url": "https://example.com"}), json!({})] {
        let decision = decide_acp(&request(Some("fetch"), &[], input), None, false);
        assert!(asks(&decision), "{decision:?}");
    }
}

#[test]
fn read_without_any_path_asks() {
    let decision = decide_acp(&request(Some("read"), &[], json!({})), None, false);
    assert!(asks(&decision));
}

#[test]
fn search_without_any_path_asks() {
    for input in [
        json!({}),
        json!({"pattern": "fn main"}),
        json!({"query": "token"}),
    ] {
        let decision = decide_acp(&request(Some("search"), &[], input), None, false);
        assert!(asks(&decision), "{decision:?}");
    }
}

#[test]
fn reads_outside_worktree_ask() {
    for locations in [
        vec![outside().to_string()],
        vec!["../other/secret.txt".to_string()],
        vec![inside("src/a.rs"), outside().to_string()],
    ] {
        let decision = decide_acp(&request(Some("read"), &locations, json!({})), None, false);
        assert!(asks(&decision), "{locations:?}");
    }
    let via_input = decide_acp(
        &request(Some("read"), &[], json!({"file_path": outside()})),
        None,
        false,
    );
    assert!(asks(&via_input));
}

#[test]
fn search_patterns_escaping_the_worktree_ask() {
    let absolute_glob = if cfg!(windows) {
        r"C:\Users\**\*.pem"
    } else {
        "/home/**/*.pem"
    };
    for input in [
        json!({"pattern": "../**/*.env"}),
        json!({"glob": absolute_glob}),
        json!({"pattern": "secret", "include": "../../**"}),
        json!({"path": outside()}),
    ] {
        let decision = decide_acp(&request(Some("search"), &[], input.clone()), None, false);
        assert!(asks(&decision), "{input}");
    }
    let inside_glob = format!("{}/**/*.rs", cwd().display());
    let decision = decide_acp(
        &request(Some("search"), &[], json!({"glob": inside_glob})),
        None,
        false,
    );
    assert!(allowed(&decision));
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
        let decision = decide_acp(
            &request(kind, &[inside("src/a.rs")], json!({})),
            None,
            false,
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
    assert!(allowed(&decide_acp(&req, Some("read"), false)));
    assert!(asks(&decide_acp(&req, Some("edit"), false)));
}

#[test]
fn read_without_allow_once_option_asks() {
    let mut req = request(Some("read"), &[inside("a.rs")], json!({}));
    req["options"] = json!([{"optionId": "always", "name": "Always", "kind": "allow_always"}]);
    assert!(asks(&decide_acp(&req, None, false)));
}

#[test]
fn full_access_allows_every_kind_with_allow_once() {
    let cases = [
        request(Some("execute"), &[], json!({"command": "rm -rf /"})),
        request(Some("edit"), &[inside("src/a.rs")], json!({})),
        request(Some("fetch"), &[], json!({"url": "https://example.com"})),
        request(Some("read"), &[outside().to_string()], json!({})),
        request(None, &[], json!({})),
    ];
    for req in cases {
        let decision = decide_acp(&req, None, true);
        assert!(allowed(&decision), "{req}: {decision:?}");
    }
}

#[test]
fn full_access_never_picks_allow_always() {
    let mut req = request(Some("execute"), &[], json!({}));
    req["options"] = json!([
        {"optionId": "allow-always", "name": "Always allow", "kind": "allow_always"},
        {"optionId": "allow-once", "name": "Allow", "kind": "allow_once"}
    ]);
    assert!(allowed(&decide_acp(&req, None, true)));
}

#[test]
fn full_access_without_allow_once_option_asks() {
    let mut req = request(Some("execute"), &[], json!({}));
    req["options"] = json!([{"optionId": "always", "name": "Always", "kind": "allow_always"}]);
    assert!(asks(&decide_acp(&req, None, true)));
}

#[test]
fn inside_handles_prefix_collisions_and_dots() {
    let root = cwd();
    assert!(is_inside(&root, &inside("a/../b.rs")));
    assert!(!is_inside(&root, &inside("a/../../b.rs")));
    assert!(!is_inside(&root, &format!("{}-evil/x", root.display())));
    assert!(is_inside(&root, &root.to_string_lossy()));
}

#[test]
fn inside_canonicalizes_real_directories() {
    let base = std::env::temp_dir().join(format!("argon-perm-{}", std::process::id()));
    let repo = base.join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::create_dir_all(base.join("other")).unwrap();
    std::fs::write(repo.join("src").join("a.rs"), "").unwrap();
    let via_dots = format!("{}/src/../src/a.rs", repo.display());
    let escape = format!("{}/src/../../other", repo.display());
    let result = (
        is_inside(&repo, &via_dots),
        is_inside(&repo, &escape),
        is_inside(&repo, "src/new_file.rs"),
    );
    std::fs::remove_dir_all(&base).unwrap();
    assert_eq!(result, (true, false, true));
}

#[test]
fn acp_ask_extracts_paths_ids_and_options() {
    let req = request(
        Some("search"),
        &["src/a.rs".into()],
        json!({
            "file_path": "src/b.rs",
            "directory": "docs",
            "glob": "src/**/*.rs",
            "pattern": "fn main",
        }),
    );
    let ask = acp_ask(&req, None);
    assert_eq!(ask.tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(ask.kind, "search");
    assert_eq!(ask.title, "Tool");
    assert_eq!(ask.paths, ["src/a.rs", "src/b.rs", "docs", "src/"]);
    assert_eq!(
        ask.options
            .iter()
            .map(|o| (o.id.as_str(), o.kind.as_str()))
            .collect::<Vec<_>>(),
        [
            ("allow-once", "allow_once"),
            ("allow-always", "allow_always"),
            ("reject-once", "reject_once")
        ]
    );
}

#[test]
fn acp_ask_falls_back_to_known_kind_then_other() {
    let req = request(None, &[], json!({}));
    assert_eq!(acp_ask(&req, Some("read")).kind, "read");
    let ask = acp_ask(&req, None);
    assert_eq!(ask.kind, "other");
    let untitled = json!({"toolCall": {"toolCallId": "call_2"}, "options": []});
    let ask = acp_ask(&untitled, None);
    assert_eq!((ask.title.as_str(), ask.kind.as_str()), ("other", "other"));
}

#[test]
fn native_options_are_allow_once_allow_always_reject_once() {
    let options = native_options();
    assert_eq!(
        options
            .iter()
            .map(|o| (o.id.as_str(), o.kind.as_str(), o.label.as_str()))
            .collect::<Vec<_>>(),
        [
            ("allow_once", "allow_once", "Permitir"),
            ("allow_always", "allow_always", "Permitir nesta sessão"),
            ("reject_once", "reject_once", "Negar")
        ]
    );
}
