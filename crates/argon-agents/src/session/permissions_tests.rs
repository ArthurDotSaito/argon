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
    decide(
        &acp_ask(request, known_kind),
        &cwd(),
        &[],
        mode,
        SessionKind::Review,
    )
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

const READ_ONLY_SESSIONS: [SessionKind; 2] = [SessionKind::Plan, SessionKind::Summary];

fn decide_read_only(
    request: &Value,
    mode: PermissionMode,
    kind: SessionKind,
    read_roots: &[PathBuf],
) -> PermissionDecision {
    decide(&acp_ask(request, None), &cwd(), read_roots, mode, kind)
}

fn decide_plan(request: &Value, mode: PermissionMode) -> PermissionDecision {
    decide_read_only(request, mode, SessionKind::Plan, &[])
}

fn denied_with(decision: &PermissionDecision, expected: &str) -> bool {
    matches!(decision, PermissionDecision::Deny { option_id } if option_id == expected)
}

const READ_ONLY_DENIED_KINDS: [&str; 5] = ["edit", "delete", "move", "switch_mode", "permissions"];

#[test]
fn read_only_sessions_deny_write_kinds_with_the_reject_option() {
    for session in READ_ONLY_SESSIONS {
        for mode in [
            PermissionMode::Supervised,
            PermissionMode::AcceptEdits,
            PermissionMode::FullAccess,
        ] {
            for kind in READ_ONLY_DENIED_KINDS {
                for paths in [
                    vec![inside("src/a.rs")],
                    vec![outside().to_string()],
                    vec![],
                ] {
                    let decision = decide_read_only(
                        &request(Some(kind), &paths, json!({})),
                        mode,
                        session,
                        &[],
                    );
                    assert!(
                        denied_with(&decision, "reject-once"),
                        "{session:?} {mode:?} {kind} {paths:?}: {decision:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn read_only_sessions_ask_for_write_kinds_that_offer_no_reject_option() {
    for session in READ_ONLY_SESSIONS {
        for kind in READ_ONLY_DENIED_KINDS {
            let mut req = request(Some(kind), &[inside("src/a.rs")], json!({}));
            req["options"] = json!([
                {"optionId": "allow-once", "name": "Allow", "kind": "allow_once"},
                {"optionId": "reject-always", "name": "Never", "kind": "reject_always"}
            ]);
            for mode in [PermissionMode::Supervised, PermissionMode::FullAccess] {
                assert!(
                    asks(&decide_read_only(&req, mode, session, &[])),
                    "{session:?} {mode:?} {kind}"
                );
            }
        }
    }
}

#[test]
fn read_only_sessions_keep_other_kinds_as_cards() {
    for (session, modes) in [
        (SessionKind::Plan, &PLAN_MODES[..]),
        (SessionKind::Summary, &[PermissionMode::Supervised][..]),
    ] {
        for kind in ["execute", "fetch", "other", "think"] {
            for &mode in modes {
                let req = request(Some(kind), &[], json!({"command": "ls"}));
                assert!(
                    asks(&decide_read_only(&req, mode, session, &[])),
                    "{session:?} {mode:?} {kind}"
                );
            }
        }
    }
}

const PLAN_MODES: [PermissionMode; 3] = [
    PermissionMode::Supervised,
    PermissionMode::AcceptEdits,
    PermissionMode::FullAccess,
];

#[test]
fn summary_full_access_allows_commands_searches_and_network_without_a_card() {
    for kind in ["execute", "fetch", "think", "search"] {
        let req = request(Some(kind), &[], json!({"command": "git log"}));
        let decision =
            decide_read_only(&req, PermissionMode::FullAccess, SessionKind::Summary, &[]);
        assert!(allowed(&decision), "{kind}: {decision:?}");
    }
    let outside_read = request(Some("read"), &[outside().to_string()], json!({}));
    assert!(allowed(&decide_read_only(
        &outside_read,
        PermissionMode::FullAccess,
        SessionKind::Summary,
        &[]
    )));
}

#[test]
fn summary_full_access_asks_for_other_and_missing_kinds() {
    for kind in [Some("other"), None, Some("brand_new_kind")] {
        let req = request(kind, &[], json!({"command": "rm -rf x"}));
        let decision =
            decide_read_only(&req, PermissionMode::FullAccess, SessionKind::Summary, &[]);
        assert!(asks(&decision), "{kind:?}: {decision:?}");
    }
}

#[test]
fn summary_full_access_still_denies_edits_ahead_of_the_blanket_approval() {
    for kind in READ_ONLY_DENIED_KINDS {
        let req = request(Some(kind), &[inside("src/a.rs")], json!({}));
        let decision =
            decide_read_only(&req, PermissionMode::FullAccess, SessionKind::Summary, &[]);
        assert!(
            denied_with(&decision, "reject-once"),
            "{kind}: {decision:?}"
        );
    }
}

#[test]
fn summary_accept_edits_behaves_as_supervised() {
    let execute = request(Some("execute"), &[], json!({"command": "ls"}));
    assert!(asks(&decide_read_only(
        &execute,
        PermissionMode::AcceptEdits,
        SessionKind::Summary,
        &[]
    )));
    let read = request(Some("read"), &[inside("src/a.rs")], json!({}));
    assert!(allowed(&decide_read_only(
        &read,
        PermissionMode::AcceptEdits,
        SessionKind::Summary,
        &[]
    )));
}

#[test]
fn plan_full_access_is_decided_as_supervised() {
    let execute = request(Some("execute"), &[], json!({"command": "ls"}));
    assert!(asks(&decide_plan(&execute, PermissionMode::FullAccess)));
    let outside_read = request(Some("read"), &[outside().to_string()], json!({}));
    assert!(asks(&decide_plan(
        &outside_read,
        PermissionMode::FullAccess
    )));
}

#[test]
fn plan_sessions_read_inside_the_worktree_and_ask_outside() {
    let inside_read = request(Some("read"), &[inside("src/a.rs")], json!({}));
    let outside_read = request(Some("read"), &[outside().to_string()], json!({}));
    for mode in [PermissionMode::Supervised, PermissionMode::FullAccess] {
        assert!(allowed(&decide_plan(&inside_read, mode)), "{mode:?}");
        assert!(asks(&decide_plan(&outside_read, mode)), "{mode:?}");
    }
}

fn related_root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\work\rel-abc")
    } else {
        PathBuf::from("/work/rel-abc")
    }
}

fn in_related(name: &str) -> String {
    related_root().join(name).to_string_lossy().into_owned()
}

#[test]
fn reads_and_searches_inside_a_read_root_are_allowed_in_every_mode() {
    let roots = [related_root()];
    let cases = [
        request(Some("read"), &[in_related("src/lib.rs")], json!({})),
        request(Some("search"), &[in_related("src")], json!({})),
        request(
            Some("search"),
            &[],
            json!({"glob": format!("{}/**/*.rs", related_root().display())}),
        ),
        request(
            Some("read"),
            &[inside("src/a.rs"), in_related("src/lib.rs")],
            json!({}),
        ),
    ];
    for session in READ_ONLY_SESSIONS {
        for mode in [
            PermissionMode::Supervised,
            PermissionMode::AcceptEdits,
            PermissionMode::FullAccess,
        ] {
            for req in &cases {
                let decision = decide_read_only(req, mode, session, &roots);
                assert!(
                    allowed(&decision),
                    "{session:?} {mode:?} {req}: {decision:?}"
                );
            }
        }
    }
}

#[test]
fn writes_inside_a_read_root_are_denied_in_read_only_sessions() {
    let roots = [related_root()];
    for session in READ_ONLY_SESSIONS {
        for kind in ["edit", "delete", "move"] {
            for mode in [
                PermissionMode::Supervised,
                PermissionMode::AcceptEdits,
                PermissionMode::FullAccess,
            ] {
                let req = request(Some(kind), &[in_related("src/lib.rs")], json!({}));
                let decision = decide_read_only(&req, mode, session, &roots);
                assert!(
                    denied_with(&decision, "reject-once"),
                    "{session:?} {mode:?} {kind}: {decision:?}"
                );
            }
        }
    }
}

#[test]
fn read_roots_never_cover_commands_or_unknown_kinds() {
    let roots = [related_root()];
    for kind in [Some("execute"), Some("fetch"), Some("other"), None] {
        let req = request(kind, &[in_related("src/lib.rs")], json!({}));
        let decision = decide_read_only(
            &req,
            PermissionMode::Supervised,
            SessionKind::Summary,
            &roots,
        );
        assert!(asks(&decision), "{kind:?}: {decision:?}");
    }
}

#[test]
fn reads_outside_the_worktree_and_every_read_root_ask() {
    let roots = [related_root()];
    let sibling = related_root()
        .with_file_name("rel-other")
        .join("a.rs")
        .to_string_lossy()
        .into_owned();
    for paths in [
        vec![outside().to_string()],
        vec![sibling],
        vec![in_related("src/lib.rs"), outside().to_string()],
        vec![in_related("../repo-secret/a.rs")],
        vec![],
    ] {
        let req = request(Some("read"), &paths, json!({}));
        let decision = decide_read_only(
            &req,
            PermissionMode::Supervised,
            SessionKind::Summary,
            &roots,
        );
        assert!(asks(&decision), "{paths:?}: {decision:?}");
    }
}

#[test]
fn a_path_that_only_shares_the_read_root_prefix_is_outside() {
    let short = if cfg!(windows) {
        PathBuf::from(r"C:\work\rel-ab")
    } else {
        PathBuf::from("/work/rel-ab")
    };
    let req = request(Some("read"), &[in_related("src/lib.rs")], json!({}));
    let decision = decide_read_only(
        &req,
        PermissionMode::Supervised,
        SessionKind::Summary,
        &[short],
    );
    assert!(asks(&decision), "{decision:?}");
}

#[test]
fn filesystem_roots_and_relative_read_roots_are_ignored() {
    let filesystem_root = if cfg!(windows) {
        PathBuf::from(r"C:\")
    } else {
        PathBuf::from("/")
    };
    let dotted_root = if cfg!(windows) {
        PathBuf::from(r"C:\x\..")
    } else {
        PathBuf::from("/x/..")
    };
    for root in [
        filesystem_root,
        dotted_root,
        PathBuf::new(),
        PathBuf::from("rel-abc"),
    ] {
        let req = request(Some("read"), &[outside().to_string()], json!({}));
        let decision = decide_read_only(
            &req,
            PermissionMode::Supervised,
            SessionKind::Summary,
            std::slice::from_ref(&root),
        );
        assert!(asks(&decision), "{root:?}: {decision:?}");
    }
}

struct TempTree(PathBuf);

impl TempTree {
    fn new(name: &str) -> TempTree {
        let base = std::env::temp_dir().join(format!("argon-roots-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("rel-abc").join("src")).unwrap();
        std::fs::create_dir_all(base.join("other")).unwrap();
        std::fs::write(base.join("rel-file"), "").unwrap();
        TempTree(base)
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn only_existing_related_directories_are_valid_read_roots() {
    let tree = TempTree::new("valid");
    let base = &tree.0;
    let related = base.join("rel-abc");

    let kept = valid_read_roots(&[
        related.clone(),
        related.join(".."),
        base.join("rel-abc").join("src").join("..").join(".."),
        base.join("other"),
        base.join("rel-missing"),
        base.join("rel-file"),
        PathBuf::from("rel-abc"),
        if cfg!(windows) {
            PathBuf::from(r"C:\")
        } else {
            PathBuf::from("/")
        },
    ]);

    assert_eq!(kept, [related]);
}

fn local_root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"D:\src\pacientes-api")
    } else {
        PathBuf::from("/src/pacientes-api")
    }
}

fn in_local(name: &str) -> String {
    local_root().join(name).to_string_lossy().into_owned()
}

#[test]
fn a_local_folder_is_read_and_searched_but_never_written_in_read_only_sessions() {
    let roots = [related_root(), local_root()];
    let reads = [
        request(Some("read"), &[in_local("src/lib.rs")], json!({})),
        request(Some("search"), &[in_local("src")], json!({})),
        request(
            Some("search"),
            &[],
            json!({"glob": format!("{}/**/*.cs", local_root().display())}),
        ),
        request(
            Some("read"),
            &[in_related("a.rs"), in_local("b.cs"), inside("c.rs")],
            json!({}),
        ),
    ];
    for session in READ_ONLY_SESSIONS {
        for mode in [PermissionMode::Supervised, PermissionMode::FullAccess] {
            for req in &reads {
                let decision = decide_read_only(req, mode, session, &roots);
                assert!(
                    allowed(&decision),
                    "{session:?} {mode:?} {req}: {decision:?}"
                );
            }
            for kind in ["edit", "delete", "move"] {
                let req = request(Some(kind), &[in_local("src/lib.rs")], json!({}));
                let decision = decide_read_only(&req, mode, session, &roots);
                assert!(
                    denied_with(&decision, "reject-once"),
                    "{session:?} {mode:?} {kind}: {decision:?}"
                );
            }
        }
    }
}

#[test]
fn a_local_folder_releases_nothing_in_a_review() {
    let roots = [local_root()];
    for kind in ["read", "search", "edit", "delete", "move"] {
        let req = request(Some(kind), &[in_local("src/lib.rs")], json!({}));
        let decision = decide(
            &acp_ask(&req, None),
            &cwd(),
            &roots,
            PermissionMode::AcceptEdits,
            SessionKind::Review,
        );
        assert!(asks(&decision), "{kind}: {decision:?}");
    }
}

#[test]
fn local_roots_keep_real_folders_without_the_related_prefix() {
    let tree = TempTree::new("local");
    let base = &tree.0;
    let drive_root = base.ancestors().last().unwrap().to_path_buf();

    let kept = valid_local_roots(
        &[
            base.join("other"),
            base.join("other").join("..").join("other"),
            base.join("rel-abc"),
            base.join("missing"),
            base.join("rel-file"),
            PathBuf::from("other"),
            drive_root,
            base.clone(),
        ],
        Some(base),
    );

    assert_eq!(
        kept,
        [
            dunce::canonicalize(base.join("other")).unwrap(),
            dunce::canonicalize(base.join("rel-abc")).unwrap(),
        ]
    );
}

#[test]
fn related_dir_names_need_the_rel_prefix_and_a_suffix() {
    assert!(is_related_dir_name("rel-abc"));
    for name in ["rel", "rel-", "relabc", "other", "pr-1-abc", "issue-2-abc"] {
        assert!(!is_related_dir_name(name), "{name}");
    }
}

#[test]
fn globs_with_a_parent_segment_after_a_wildcard_ask() {
    let tree = TempTree::new("globs");
    let roots = [tree.0.join("rel-abc")];
    let in_cwd = format!("{}/**/../../x", cwd().display());
    let in_root = format!("{}/*/../../secret", roots[0].display());
    for input in [
        json!({"glob": "src/*/../../x"}),
        json!({"glob": in_cwd}),
        json!({"glob": in_root}),
        json!({"glob": "{..,x}/**"}),
        json!({"glob": format!("{}/{{..,x}}/**", roots[0].display())}),
        json!({"include": "**/{x,..}/*"}),
        json!({"pattern": "**/../../*.env"}),
        json!({"pattern": "{..,src}/**"}),
    ] {
        for session in [SessionKind::Review, SessionKind::Summary] {
            let decision = decide_read_only(
                &request(Some("search"), &[], input.clone()),
                PermissionMode::Supervised,
                session,
                &roots,
            );
            assert!(asks(&decision), "{session:?} {input}: {decision:?}");
        }
    }
}

#[test]
fn a_glob_ending_in_a_partial_name_does_not_cover_sibling_folders() {
    let roots = [related_root()];
    let sibling_cwd = format!("{}*/**", cwd().display());
    let sibling_root = format!("{}*/**", related_root().display());
    for (glob, session) in [
        (sibling_cwd, SessionKind::Review),
        (sibling_root, SessionKind::Summary),
    ] {
        let decision = decide_read_only(
            &request(Some("search"), &[], json!({"glob": glob})),
            PermissionMode::Supervised,
            session,
            &roots,
        );
        assert!(asks(&decision), "{glob}: {decision:?}");
    }
    let inside_partial = format!("{}/src/ma*", cwd().display());
    let decision = decide_read_only(
        &request(Some("search"), &[], json!({"glob": inside_partial})),
        PermissionMode::Supervised,
        SessionKind::Review,
        &[],
    );
    assert!(allowed(&decision), "{decision:?}");
}

#[test]
fn read_roots_release_nothing_in_a_review() {
    let roots = [related_root()];
    for mode in [PermissionMode::Supervised, PermissionMode::AcceptEdits] {
        for kind in ["read", "search", "edit", "delete", "move", "execute"] {
            let req = request(Some(kind), &[in_related("src/lib.rs")], json!({}));
            let decision = decide(
                &acp_ask(&req, None),
                &cwd(),
                &roots,
                mode,
                SessionKind::Review,
            );
            assert!(asks(&decision), "{mode:?} {kind}: {decision:?}");
        }
    }
}

#[test]
fn session_grant_never_reaches_a_read_root() {
    let roots = [related_root()];
    for kind in ["read", "search", "edit"] {
        let ask = native_ask(kind, &[in_related("src/lib.rs")]);
        assert!(!session_grant_applies(&ask, &cwd()), "{kind}");
        assert!(!paths_contained(&ask, &cwd()), "{kind}");
    }
    let edit = native_ask("edit", &[in_related("src/lib.rs")]);
    assert!(asks(&decide(
        &edit,
        &cwd(),
        &roots,
        PermissionMode::AcceptEdits,
        SessionKind::Review
    )));
}

#[test]
fn review_sessions_never_deny_and_match_the_mode_rules() {
    for mode in [
        PermissionMode::Supervised,
        PermissionMode::AcceptEdits,
        PermissionMode::FullAccess,
    ] {
        for kind in ["read", "search", "edit", "execute", "fetch", "other"] {
            let req = request(Some(kind), &[inside("src/a.rs")], json!({}));
            let decision = decide_acp(&req, None, mode);
            let auto = match mode {
                PermissionMode::FullAccess => true,
                PermissionMode::AcceptEdits => ["read", "search", "edit"].contains(&kind),
                PermissionMode::Supervised => ["read", "search"].contains(&kind),
            };
            assert_eq!(allowed(&decision), auto, "{mode:?} {kind}: {decision:?}");
            assert_eq!(asks(&decision), !auto, "{mode:?} {kind}: {decision:?}");
        }
    }
}
