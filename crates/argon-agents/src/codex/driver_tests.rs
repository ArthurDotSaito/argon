use serde_json::json;

use super::state::*;
use super::*;
use crate::session::{REJECT_ONCE, native_options_for};

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

fn outside() -> String {
    if cfg!(windows) {
        r"C:\Users\me\.ssh\id_rsa".to_owned()
    } else {
        "/home/me/.ssh/id_rsa".to_owned()
    }
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

fn granted(kinds: &[&str]) -> HashSet<String> {
    kinds.iter().map(|kind| (*kind).to_owned()).collect()
}

fn selected(option: &str) -> PermissionAnswer {
    PermissionAnswer::Selected(option.to_owned())
}

fn running(id: &str) -> Turns {
    let mut turns = Turns::default();
    turns.start(id.to_owned());
    turns
}

fn models(ids: &[&str]) -> Vec<ModelInfo> {
    ids.iter()
        .map(|id| ModelInfo {
            id: (*id).to_owned(),
            label: (*id).to_owned(),
        })
        .collect()
}

#[test]
fn only_the_current_turn_owns_messages() {
    let turns = running("u2");

    assert!(turns.owns(Some("u2")));
    assert!(!turns.owns(Some("u1")));
    assert!(turns.owns(None));
    assert!(!Turns::default().owns(Some("u2")));
    assert!(!Turns::default().owns(None));
}

#[test]
fn a_stale_turn_end_neither_closes_nor_disturbs_the_current_turn() {
    let mut turns = running("u2");

    assert_eq!(turns.finish("u1", "end_turn", None), None);
    assert_eq!(turns.current_id(), Some("u2"));
    assert_eq!(turns.finish("u2", "end_turn", None), Some(Close::Completed));
    assert!(!turns.in_turn());
}

#[test]
fn turn_end_without_a_turn_is_ignored() {
    assert_eq!(Turns::default().finish("u1", "end_turn", None), None);
}

#[test]
fn failed_turn_reports_the_error_or_the_stop_reason() {
    assert_eq!(
        running("u1").finish("u1", "error", Some("quota".to_owned())),
        Some(Close::Failed("quota".to_owned()))
    );
    assert_eq!(
        running("u1").finish("u1", "error", None),
        Some(Close::Failed("error".to_owned()))
    );
}

#[test]
fn interrupted_turn_closes_as_cancelled() {
    assert_eq!(
        running("u1").finish("u1", "cancelled", None),
        Some(Close::Cancelled)
    );
}

#[test]
fn a_cancelled_turn_closes_as_cancelled_whatever_the_agent_reports() {
    let mut turns = running("u1");

    assert!(turns.cancel());
    assert!(!turns.cancel());
    assert!(turns.is_cancelled());
    assert_eq!(
        turns.finish("u1", "error", Some("boom".to_owned())),
        Some(Close::Cancelled)
    );
}

#[test]
fn requests_are_accepted_only_for_the_live_current_turn() {
    let mut turns = running("u1");

    assert!(turns.accepts_request(Some("u1")));
    assert!(turns.accepts_request(None));
    assert!(!turns.accepts_request(Some("u0")));
    turns.cancel();
    assert!(!turns.accepts_request(Some("u1")));
    assert!(!Turns::default().accepts_request(None));
}

#[test]
fn expiry_restarts_only_after_an_unconfirmed_cancel() {
    let mut turns = running("u1");

    assert_eq!(turns.expire(), None);
    assert!(turns.in_turn());
    turns.cancel();
    assert_eq!(turns.expire(), Some(Close::Cancelled));
    assert!(!turns.in_turn());
    assert_eq!(Turns::default().expire(), None);
}

#[test]
fn mode_rules_auto_allow_without_a_session_grant() {
    let read = native_ask("read", &[inside("src/a.rs")]);

    assert!(auto_allowed(
        &read,
        &cwd(),
        PermissionMode::Supervised,
        &granted(&[])
    ));
}

#[test]
fn session_grant_covers_contained_edits_of_the_granted_kind() {
    let edit = native_ask("edit", &[inside("src/a.rs")]);

    assert!(!auto_allowed(
        &edit,
        &cwd(),
        PermissionMode::Supervised,
        &granted(&[])
    ));
    assert!(auto_allowed(
        &edit,
        &cwd(),
        PermissionMode::Supervised,
        &granted(&["edit"])
    ));
}

#[test]
fn session_grant_never_covers_commands_or_escaping_edits() {
    let command = native_ask("execute", &[]);
    assert!(!auto_allowed(
        &command,
        &cwd(),
        PermissionMode::AcceptEdits,
        &granted(&["execute"])
    ));
    for paths in [vec![outside()], vec!["~/x".to_owned()], Vec::new()] {
        let edit = native_ask("edit", &paths);
        assert!(
            !auto_allowed(
                &edit,
                &cwd(),
                PermissionMode::Supervised,
                &granted(&["edit"])
            ),
            "{paths:?}"
        );
    }
}

#[test]
fn only_allow_always_grants_the_session() {
    assert!(grants_session(&selected("allow_always")));
    assert!(!grants_session(&selected(ALLOW_ONCE)));
    assert!(!grants_session(&selected(REJECT_ONCE)));
    assert!(!grants_session(&PermissionAnswer::Cancelled));
}

#[test]
fn model_changes_are_checked_against_known_models() {
    let known = models(&["gpt-5", "gpt-5-mini"]);

    assert!(check_model(&known, "gpt-5").is_ok());
    assert!(matches!(
        check_model(&known, "o9"),
        Err(SessionError::ModelUnavailable { model }) if model == "o9"
    ));
    assert!(check_model(&[], "anything").is_ok());
}

#[test]
fn model_pages_follow_cursors_and_assemble_all_entries() {
    let mut pages = ModelPages::default();

    assert_eq!(
        pages.add(&json!({"data": [{"id": "a"}], "nextCursor": "p2"})),
        Some("p2".to_owned())
    );
    assert_eq!(
        pages.add(&json!({"data": [{"id": "b", "isDefault": true}], "nextCursor": null})),
        None
    );
    let result = pages.result();
    assert_eq!(
        parse_models(&result)
            .into_iter()
            .map(|model| model.id)
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(default_model(&result), Some("b".to_owned()));
}

#[test]
fn model_pages_stop_on_a_repeated_cursor() {
    let mut pages = ModelPages::default();
    let page = json!({"data": [{"id": "a"}], "nextCursor": "same"});

    assert_eq!(pages.add(&page), Some("same".to_owned()));
    assert_eq!(pages.add(&page), None);
}

#[test]
fn model_pages_stop_at_five_hundred_entries() {
    let mut pages = ModelPages::default();
    let mut cursor = 0;
    let mut more = true;
    while more {
        cursor += 1;
        let data: Vec<Value> = (0..120)
            .map(|n| json!({"id": format!("m{cursor}-{n}")}))
            .collect();
        more = pages
            .add(&json!({"data": data, "nextCursor": format!("c{cursor}")}))
            .is_some();
    }
    assert_eq!(cursor, 5);
    assert_eq!(parse_models(&pages.result()).len(), 500);
}

#[test]
fn model_pages_stop_when_empty_pages_keep_coming() {
    let mut pages = ModelPages::default();
    let mut calls = 0;
    while pages
        .add(&json!({"data": [], "nextCursor": format!("c{calls}")}))
        .is_some()
    {
        calls += 1;
        assert!(calls < 1000);
    }
    assert!(calls < 100, "{calls}");
}

#[test]
fn archived_errors_are_recognized() {
    assert!(is_archived("thread/resume: -32600 thread th_1 is archived"));
    assert!(is_archived("Thread Archived"));
    assert!(!is_archived("thread/resume: -32600 no rollout found"));
}

#[test]
fn exit_message_names_codex_and_the_exit_code() {
    assert_eq!(
        exit_message(Some(3)),
        "o Codex encerrou: o processo saiu (código 3)"
    );
    assert_eq!(
        exit_message(None),
        "o Codex encerrou: a conexão foi fechada"
    );
}

#[test]
fn project_roots_stop_at_the_first_git_root() {
    let cwd = cwd().join("sub");
    let repo = cwd.parent().unwrap().to_path_buf();

    let roots = project_roots(&cwd, |dir| dir == repo);
    assert_eq!(roots, vec![cwd.as_path(), repo.as_path()]);

    let all = project_roots(&cwd, |_| false);
    assert_eq!(all.len(), cwd.ancestors().count());
}

#[test]
fn trust_keys_add_a_lowercase_variant_on_windows_without_duplicates() {
    let upper = cwd();
    let keys = trust_keys(&[upper.clone(), upper.clone()]);
    let text = upper.display().to_string();
    if cfg!(windows) {
        assert_eq!(keys, vec![text.clone(), text.to_lowercase()]);
    } else {
        assert_eq!(keys, vec![text]);
    }
}

#[test]
fn trust_keys_cover_the_worktree_and_the_main_repository() {
    let worktree = cwd();
    let main = cwd().with_file_name("Main");
    let keys = trust_keys(&[worktree.clone(), main.clone()]);
    for root in [&worktree, &main] {
        let text = root.display().to_string();
        assert!(keys.contains(&text), "{keys:?}");
        if cfg!(windows) {
            assert!(keys.contains(&text.to_lowercase()), "{keys:?}");
        }
    }
}

#[test]
fn gitfile_points_to_the_worktree_gitdir() {
    let root = cwd();
    let absolute = if cfg!(windows) {
        "C:/mirror/.git/worktrees/wt"
    } else {
        "/mirror/.git/worktrees/wt"
    };
    assert_eq!(
        gitdir_target(&format!("gitdir: {absolute}\n"), &root),
        Some(PathBuf::from(absolute))
    );
    assert_eq!(
        gitdir_target("gitdir: ../mirror/.git/worktrees/wt", &root),
        Some(root.join("../mirror/.git/worktrees/wt"))
    );
    assert_eq!(gitdir_target("gitdir:   \n", &root), None);
    assert_eq!(gitdir_target("ref: refs/heads/main", &root), None);
}

#[test]
fn commondir_is_resolved_against_the_gitdir() {
    let gitdir = cwd().join(".git").join("worktrees").join("wt");
    assert_eq!(common_dir(&gitdir, "../..\n"), Some(gitdir.join("../..")));
    assert_eq!(common_dir(&gitdir, "  "), None);
}

#[test]
fn widening_the_mode_mid_turn_waits_for_the_turn_to_end() {
    let mut switch = ModeSwitch::default();

    assert_eq!(
        switch.request(PermissionMode::FullAccess, PermissionMode::Supervised, true),
        ModeChange::AtTurnEnd(PermissionMode::FullAccess)
    );
    assert_eq!(
        switch.request(
            PermissionMode::AcceptEdits,
            PermissionMode::Supervised,
            true
        ),
        ModeChange::AtTurnEnd(PermissionMode::AcceptEdits)
    );
    assert_eq!(switch.take(), Some(PermissionMode::AcceptEdits));
    assert_eq!(switch.take(), None);
}

#[test]
fn restricting_or_keeping_the_mode_mid_turn_applies_at_once() {
    let mut switch = ModeSwitch::default();

    switch.request(
        PermissionMode::FullAccess,
        PermissionMode::AcceptEdits,
        true,
    );
    assert_eq!(
        switch.request(
            PermissionMode::Supervised,
            PermissionMode::AcceptEdits,
            true
        ),
        ModeChange::ApplyNow(PermissionMode::Supervised)
    );
    assert_eq!(switch.take(), None);
    assert_eq!(
        switch.request(
            PermissionMode::AcceptEdits,
            PermissionMode::AcceptEdits,
            true
        ),
        ModeChange::ApplyNow(PermissionMode::AcceptEdits)
    );
    assert_eq!(
        switch.request(
            PermissionMode::AcceptEdits,
            PermissionMode::FullAccess,
            true
        ),
        ModeChange::ApplyNow(PermissionMode::AcceptEdits)
    );
}

#[test]
fn any_mode_change_while_idle_applies_at_once_and_drops_older_pending() {
    let mut switch = ModeSwitch::default();

    switch.request(PermissionMode::FullAccess, PermissionMode::Supervised, true);
    assert_eq!(
        switch.request(
            PermissionMode::FullAccess,
            PermissionMode::Supervised,
            false
        ),
        ModeChange::ApplyNow(PermissionMode::FullAccess)
    );
    assert_eq!(switch.take(), None);
}

#[test]
fn sandbox_readiness_needs_the_ready_status() {
    assert_eq!(readiness(Ok(json!({"status": "ready"}))), Ok(()));
    assert_eq!(
        readiness(Ok(json!({"status": "notConfigured"}))),
        Err("notConfigured".to_owned())
    );
    assert_eq!(
        readiness(Ok(json!({"status": "updateRequired"}))),
        Err("updateRequired".to_owned())
    );
    assert_eq!(readiness(Ok(json!({}))), Err("{}".to_owned()));
    assert_eq!(
        readiness(Err("windowsSandbox/readiness: -32601 x".to_owned())),
        Err("windowsSandbox/readiness: -32601 x".to_owned())
    );
}

#[test]
fn full_access_is_unavailable_without_the_sandbox() {
    let missing: Sandbox = Err("notConfigured".to_owned());
    let ready: Sandbox = Ok(());

    assert_eq!(
        effective_mode(PermissionMode::FullAccess, &missing),
        PermissionMode::Supervised
    );
    assert_eq!(
        effective_mode(PermissionMode::FullAccess, &ready),
        PermissionMode::FullAccess
    );
    for mode in [PermissionMode::Supervised, PermissionMode::AcceptEdits] {
        for sandbox in [&missing, &ready] {
            assert_eq!(effective_mode(mode, sandbox), mode);
            assert_eq!(sandbox_warning(mode, sandbox), None);
            assert!(mode_refusal(mode, sandbox).is_none());
        }
    }
    assert_eq!(sandbox_warning(PermissionMode::FullAccess, &ready), None);
    assert!(mode_refusal(PermissionMode::FullAccess, &ready).is_none());
    assert_eq!(
        sandbox_warning(PermissionMode::FullAccess, &missing).as_deref(),
        Some(
            "a sandbox do Codex no Windows não está pronta (notConfigured); a sessão abriu em Supervisionado — Acesso total fica indisponível"
        )
    );
    assert!(matches!(
        mode_refusal(PermissionMode::FullAccess, &missing),
        Some(SessionError::SetPermissionMode { reason, .. }) if reason.contains("notConfigured")
    ));
}

#[test]
fn model_choice_prefers_the_user_then_the_thread_then_the_default() {
    assert_eq!(
        pick_model(Some("o3"), Some("gpt-5"), Some("gpt-6")),
        ModelChoice {
            shown: "o3".to_owned(),
            sent: Some("o3".to_owned())
        }
    );
    assert_eq!(
        pick_model(None, Some("gpt-5"), Some("gpt-6")),
        ModelChoice {
            shown: "gpt-5".to_owned(),
            sent: None
        }
    );
    assert_eq!(
        pick_model(None, None, Some("gpt-6")),
        ModelChoice {
            shown: "gpt-6".to_owned(),
            sent: Some("gpt-6".to_owned())
        }
    );
    assert_eq!(
        pick_model(None, None, None),
        ModelChoice {
            shown: String::new(),
            sent: None
        }
    );
}
