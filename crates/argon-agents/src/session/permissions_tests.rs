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

fn decide_acp(
    request: &Value,
    known_kind: Option<&str>,
    mode: PermissionMode,
) -> PermissionDecision {
    decide(&acp_ask(request, known_kind), &cwd(), mode)
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
            PermissionMode::Supervised,
        );
        assert!(allowed(&decision), "{kind}: {decision:?}");
    }
    let relative = decide_acp(
        &request(Some("read"), &["src/a.rs".into()], json!({})),
        None,
        PermissionMode::Supervised,
    );
    assert!(allowed(&relative));
    let search_in_cwd = decide_acp(
        &request(
            Some("search"),
            &[],
            json!({"pattern": "fn main", "glob": "src/**/*.rs"}),
        ),
        None,
        PermissionMode::Supervised,
    );
    assert!(allowed(&search_in_cwd));
}

#[test]
fn fetch_always_asks() {
    for input in [json!({"url": "https://example.com"}), json!({})] {
        let decision = decide_acp(
            &request(Some("fetch"), &[], input),
            None,
            PermissionMode::Supervised,
        );
        assert!(asks(&decision), "{decision:?}");
    }
}

#[test]
fn read_without_any_path_asks() {
    let decision = decide_acp(
        &request(Some("read"), &[], json!({})),
        None,
        PermissionMode::Supervised,
    );
    assert!(asks(&decision));
}

#[test]
fn search_without_any_path_asks() {
    for input in [
        json!({}),
        json!({"pattern": "fn main"}),
        json!({"query": "token"}),
    ] {
        let decision = decide_acp(
            &request(Some("search"), &[], input),
            None,
            PermissionMode::Supervised,
        );
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
        let decision = decide_acp(
            &request(Some("read"), &locations, json!({})),
            None,
            PermissionMode::Supervised,
        );
        assert!(asks(&decision), "{locations:?}");
    }
    let via_input = decide_acp(
        &request(Some("read"), &[], json!({"file_path": outside()})),
        None,
        PermissionMode::Supervised,
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
        let decision = decide_acp(
            &request(Some("search"), &[], input.clone()),
            None,
            PermissionMode::Supervised,
        );
        assert!(asks(&decision), "{input}");
    }
    let inside_glob = format!("{}/**/*.rs", cwd().display());
    let decision = decide_acp(
        &request(Some("search"), &[], json!({"glob": inside_glob})),
        None,
        PermissionMode::Supervised,
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
            PermissionMode::Supervised,
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
    assert!(allowed(&decide_acp(
        &req,
        Some("read"),
        PermissionMode::Supervised
    )));
    assert!(asks(&decide_acp(
        &req,
        Some("edit"),
        PermissionMode::Supervised
    )));
}

#[test]
fn read_without_allow_once_option_asks() {
    let mut req = request(Some("read"), &[inside("a.rs")], json!({}));
    req["options"] = json!([{"optionId": "always", "name": "Always", "kind": "allow_always"}]);
    assert!(asks(&decide_acp(&req, None, PermissionMode::Supervised)));
}

#[test]
fn unrestricted_mode_allows_every_kind_with_allow_once() {
    let cases = [
        request(Some("execute"), &[], json!({"command": "rm -rf /"})),
        request(Some("edit"), &[inside("src/a.rs")], json!({})),
        request(Some("fetch"), &[], json!({"url": "https://example.com"})),
        request(Some("read"), &[outside().to_string()], json!({})),
        request(None, &[], json!({})),
    ];
    for req in cases {
        let decision = decide_acp(&req, None, PermissionMode::FullAccess);
        assert!(allowed(&decision), "{req}: {decision:?}");
    }
}

#[test]
fn unrestricted_mode_never_picks_allow_always() {
    let mut req = request(Some("execute"), &[], json!({}));
    req["options"] = json!([
        {"optionId": "allow-always", "name": "Always allow", "kind": "allow_always"},
        {"optionId": "allow-once", "name": "Allow", "kind": "allow_once"}
    ]);
    assert!(allowed(&decide_acp(&req, None, PermissionMode::FullAccess)));
}

#[test]
fn decide_matrix_covers_every_mode_kind_and_path_scope() {
    use PermissionMode::{AcceptEdits, FullAccess, Supervised};
    let scopes = [
        ("inside", vec![inside("src/a.rs")]),
        ("outside", vec![outside().to_string()]),
        ("empty", Vec::new()),
    ];
    let expected = |mode: PermissionMode, kind: &str, scope: &str| match mode {
        FullAccess => true,
        AcceptEdits => scope == "inside" && ["read", "search", "edit"].contains(&kind),
        Supervised => scope == "inside" && ["read", "search"].contains(&kind),
    };
    for mode in [Supervised, AcceptEdits, FullAccess] {
        for kind in ["read", "search", "edit", "execute"] {
            for (scope, paths) in &scopes {
                let decision = decide_acp(&request(Some(kind), paths, json!({})), None, mode);
                assert_eq!(
                    allowed(&decision),
                    expected(mode, kind, scope),
                    "{mode:?} {kind} {scope}: {decision:?}"
                );
                assert_eq!(asks(&decision), !expected(mode, kind, scope));
            }
        }
    }
}

#[test]
fn no_allow_once_option_always_asks() {
    for mode in [
        PermissionMode::Supervised,
        PermissionMode::AcceptEdits,
        PermissionMode::FullAccess,
    ] {
        for kind in ["read", "edit", "execute"] {
            let mut req = request(Some(kind), &[inside("src/a.rs")], json!({}));
            req["options"] = json!([
                {"optionId": "always", "name": "Always", "kind": "allow_always"},
                {"optionId": "reject", "name": "Reject", "kind": "reject_once"}
            ]);
            assert!(asks(&decide_acp(&req, None, mode)), "{mode:?} {kind}");
        }
    }
}

#[test]
fn unresolvable_paths_are_never_auto_allowed() {
    let candidates = [
        "~/.bashrc".to_owned(),
        "~".to_owned(),
        "$HOME/x".to_owned(),
        "src/${SECRET}/a.rs".to_owned(),
        r"%USERPROFILE%\x".to_owned(),
        "src/../../x".to_owned(),
        "link/../x".to_owned(),
        inside("src/../a.rs"),
    ];
    for candidate in &candidates {
        assert!(!is_inside(&cwd(), candidate), "{candidate}");
        for mode in [PermissionMode::Supervised, PermissionMode::AcceptEdits] {
            for kind in ["read", "search", "edit"] {
                let req = request(Some(kind), std::slice::from_ref(candidate), json!({}));
                let decision = decide_acp(&req, None, mode);
                assert!(asks(&decision), "{mode:?} {kind} {candidate}: {decision:?}");
            }
        }
    }
    let via_input = request(Some("read"), &[], json!({"file_path": "~/.ssh/id_rsa"}));
    assert!(asks(&decide_acp(
        &via_input,
        None,
        PermissionMode::Supervised
    )));
}

#[test]
fn plain_worktree_paths_are_still_auto_allowed() {
    for candidate in ["src/a.rs".to_owned(), inside("src/a.rs")] {
        for kind in ["read", "search", "edit"] {
            let req = request(Some(kind), std::slice::from_ref(&candidate), json!({}));
            let decision = decide_acp(&req, None, PermissionMode::AcceptEdits);
            assert!(allowed(&decision), "{kind} {candidate}: {decision:?}");
        }
    }
}

#[test]
fn inside_handles_prefix_collisions_and_dots() {
    let root = cwd();
    assert!(!is_inside(&root, &inside("a/../b.rs")));
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
    let direct = format!("{}/src/a.rs", repo.display());
    let escape = format!("{}/src/../../other", repo.display());
    let result = (
        is_inside(&repo, &direct),
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

fn option_triples(options: &[PermissionOption]) -> Vec<(&str, &str, &str)> {
    options
        .iter()
        .map(|o| (o.id.as_str(), o.kind.as_str(), o.label.as_str()))
        .collect()
}

fn native_ask(kind: &str, paths: &[String]) -> PermissionAsk {
    PermissionAsk {
        tool_call_id: None,
        kind: kind.to_owned(),
        title: "Tool".to_owned(),
        paths: paths.to_vec(),
        options: native_options_for(kind),
    }
}

#[test]
fn native_options_offer_session_grant_for_local_kinds() {
    for kind in ["read", "search", "edit"] {
        assert_eq!(
            option_triples(&native_options_for(kind)),
            [
                ("allow_once", "allow_once", "Permitir"),
                ("allow_always", "allow_always", "Permitir nesta sessão"),
                ("reject_once", "reject_once", "Negar")
            ],
            "{kind}"
        );
    }
}

#[test]
fn native_options_never_offer_session_grant_for_risky_kinds() {
    for kind in ["execute", "fetch", "other"] {
        assert_eq!(
            option_triples(&native_options_for(kind)),
            [
                ("allow_once", "allow_once", "Permitir"),
                ("reject_once", "reject_once", "Negar")
            ],
            "{kind}"
        );
    }
}

#[test]
fn paths_contained_needs_every_path_inside_the_worktree() {
    let cwd = cwd();
    assert!(paths_contained(
        &native_ask("edit", &[inside("src/a.rs")]),
        &cwd
    ));
    assert!(paths_contained(
        &native_ask("edit", &["src/a.rs".to_owned(), inside("b.rs")]),
        &cwd
    ));
    assert!(!paths_contained(&native_ask("edit", &[]), &cwd));
    assert!(!paths_contained(
        &native_ask("edit", &[inside("a.rs"), outside().to_owned()]),
        &cwd
    ));
    for unresolvable in ["~/x", "$HOME/x", "%USERPROFILE%/x", "src/../../x"] {
        assert!(
            !paths_contained(&native_ask("edit", &[unresolvable.to_owned()]), &cwd),
            "{unresolvable}"
        );
    }
}

#[test]
fn session_grant_applies_only_to_contained_local_kinds() {
    let cwd = cwd();
    let local = [inside("src/a.rs")];
    for kind in ["read", "search", "edit"] {
        assert!(
            session_grant_applies(&native_ask(kind, &local), &cwd),
            "{kind}"
        );
    }
    for kind in ["execute", "fetch", "other"] {
        assert!(
            !session_grant_applies(&native_ask(kind, &local), &cwd),
            "{kind}"
        );
    }
    assert!(!session_grant_applies(
        &native_ask("edit", &[outside().to_owned()]),
        &cwd
    ));
}

#[test]
fn prompt_title_replaces_hidden_and_control_chars_and_collapses_spaces() {
    let title =
        "Bash:  a\u{202E}b\u{200B}c\u{2028}d\u{FEFF}e\u{2066}f\u{2029}g\u{200F}h\t\r\ni\u{1b}[31mj";
    assert_eq!(
        prompt_title(title, "execute"),
        "Bash: a b c d e f g h i [31mj"
    );
    assert_eq!(
        prompt_title("  edit   src/lib.rs  ", "edit"),
        "edit src/lib.rs"
    );
}

#[test]
fn prompt_title_keeps_short_titles_whole() {
    let command = format!("Bash: {}", "x".repeat(1994));
    assert_eq!(prompt_title(&command, "execute"), command);
    let path = format!("Read {}", "p".repeat(295));
    assert_eq!(prompt_title(&path, "read"), path);
}

#[test]
fn prompt_title_cuts_wide_kinds_at_2000_with_a_marker() {
    for kind in ["execute", "fetch", "other", "unknown"] {
        let title = "y".repeat(2001);
        assert_eq!(
            prompt_title(&title, kind),
            format!("{} …", "y".repeat(2000)),
            "{kind}"
        );
    }
}

#[test]
fn prompt_title_cuts_narrow_kinds_at_300_with_a_marker() {
    for kind in ["read", "search", "edit"] {
        let title = "z".repeat(301);
        assert_eq!(
            prompt_title(&title, kind),
            format!("{} …", "z".repeat(300)),
            "{kind}"
        );
    }
}

#[test]
fn prompt_title_never_leaves_a_space_before_the_marker() {
    let title = format!("{} tail", "w".repeat(299));
    assert_eq!(
        prompt_title(&title, "read"),
        format!("{} …", "w".repeat(299))
    );
}
