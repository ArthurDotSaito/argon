use serde_json::json;

use argon_core::{AgentKind, Config, PrKey, SessionKind};
use argon_store::{NewAgentSession, SessionStatus, Store};

use super::state::*;
use super::*;
use crate::codex::protocol::{resume_params, thread_params};
use crate::registry::agent_spec;
use crate::session::SessionWriter;
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
        SessionKind::Review,
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
        SessionKind::Review,
        &granted(&[])
    ));
    assert!(auto_allowed(
        &edit,
        &cwd(),
        PermissionMode::Supervised,
        SessionKind::Review,
        &granted(&["edit"])
    ));
}

#[test]
fn plan_sessions_never_auto_allow_edits_even_with_a_grant() {
    let edit = native_ask("edit", &[inside("src/a.rs")]);

    assert!(!auto_allowed(
        &edit,
        &cwd(),
        PermissionMode::FullAccess,
        SessionKind::Plan,
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
        SessionKind::Review,
        &granted(&["execute"])
    ));
    for paths in [vec![outside()], vec!["~/x".to_owned()], Vec::new()] {
        let edit = native_ask("edit", &paths);
        assert!(
            !auto_allowed(
                &edit,
                &cwd(),
                PermissionMode::Supervised,
                SessionKind::Review,
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
        switch.request(PermissionMode::FullAccess, PermissionMode::FullAccess, true),
        ModeChange::ApplyNow(PermissionMode::FullAccess)
    );
}

#[test]
fn leaving_full_access_mid_turn_interrupts_the_turn() {
    for mode in [PermissionMode::Supervised, PermissionMode::AcceptEdits] {
        let mut switch = ModeSwitch::default();
        switch.request(PermissionMode::FullAccess, PermissionMode::Supervised, true);
        assert_eq!(
            switch.request(mode, PermissionMode::FullAccess, true),
            ModeChange::InterruptTurn(mode)
        );
        assert_eq!(switch.take(), None);
        assert_eq!(
            switch.request(mode, PermissionMode::FullAccess, false),
            ModeChange::ApplyNow(mode)
        );
    }
}

async fn live_session(mode: PermissionMode, turns: Turns) -> Session {
    let writer = SessionWriter::from_store(Store::open_in_memory().unwrap()).unwrap();
    writer
        .create_session(NewAgentSession {
            id: "s1".into(),
            pr: PrKey::new("github.com", "PR_1"),
            agent: AgentKind::Codex,
            model: String::new(),
            template: "review-padrao".into(),
            worktree_path: None,
            acp_session_id: Some("th_1".into()),
            transport: "codex".into(),
            permission_mode: mode.as_str().into(),
            nonce: String::new(),
            allowed_files: Vec::new(),
            kind: argon_core::SessionKind::Review,
            base_sha: None,
        })
        .await
        .unwrap();
    let tail = Arc::new(StderrTail::default());
    let shared = Arc::new(Shared::for_tests("s1", cwd(), writer, Arc::clone(&tail)));
    shared.activate(SessionStatus::Running, 0, String::new(), Vec::new());
    shared.set_permission_mode(mode);
    let (feed, _) = mpsc::unbounded_channel();
    let (answers, _) = mpsc::unbounded_channel();
    let timeout = Duration::from_secs(600);
    Session {
        shared,
        spec: agent_spec(AgentKind::Codex, &Config::default()),
        cwd: cwd(),
        agent_cwd: cwd(),
        tail,
        untrusted: Vec::new(),
        thread: "th_1".to_owned(),
        models: Vec::new(),
        chosen_model: None,
        mode,
        switch: ModeSwitch::default(),
        sandbox: Ok(()),
        codex: None,
        generation: 1,
        decoder: Decoder::new(),
        feed,
        turns,
        grants: HashSet::new(),
        answers,
        cancel_deadline: None,
        turn_timeout: timeout,
        remaining: timeout,
        running_since: None,
        restart_after_turn: false,
    }
}

fn idle_process(session: &Session) -> Codex {
    let mut spec = session.spec.clone();
    if cfg!(windows) {
        spec.command = PathBuf::from("cmd");
        spec.args = vec!["/k".to_owned()];
    } else {
        spec.command = PathBuf::from("sh");
        spec.args = vec!["-c".to_owned(), "cat".to_owned()];
    }
    let (feed, _) = mpsc::unbounded_channel();
    Codex::spawn(
        &spec,
        &std::env::temp_dir(),
        Arc::clone(&session.tail),
        session.generation,
        &feed,
    )
    .unwrap()
}

#[tokio::test]
async fn closing_a_turn_cut_short_by_leaving_full_access_stops_the_app_server() {
    let mut session = live_session(PermissionMode::FullAccess, running("u1")).await;
    session.codex = Some(idle_process(&session));

    assert!(session.change_mode(PermissionMode::Supervised).is_ok());
    assert!(session.codex.is_some());
    let close = session.turns.finish("u1", "cancelled", None);
    session.close_turn(close).await;

    assert!(session.codex.is_none());
    assert!(!session.restart_after_turn);
}

#[tokio::test]
async fn other_turn_endings_keep_the_app_server() {
    for (current, requested) in [
        (PermissionMode::FullAccess, PermissionMode::FullAccess),
        (PermissionMode::AcceptEdits, PermissionMode::Supervised),
    ] {
        let mut session = live_session(current, running("u1")).await;
        session.codex = Some(idle_process(&session));

        assert!(session.change_mode(requested).is_ok());
        session.cancel();
        let close = session.turns.finish("u1", "cancelled", None);
        session.close_turn(close).await;

        assert!(session.codex.is_some(), "{current:?} -> {requested:?}");
        session.stop_process().await;
    }
}

fn notices(events: &mut tokio::sync::broadcast::Receiver<(u64, AgentEvent)>) -> Vec<String> {
    std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|(_, event)| match event {
            AgentEvent::Error(message) => Some(message),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn session_leaving_full_access_mid_turn_cancels_it_and_says_why() {
    for mode in [PermissionMode::Supervised, PermissionMode::AcceptEdits] {
        let mut session = live_session(PermissionMode::FullAccess, running("u1")).await;
        let (_, mut events) = session.shared.subscribe();

        assert!(session.change_mode(mode).is_ok());

        assert_eq!(session.mode, mode);
        assert_eq!(session.shared.subscribe().0.permission_mode, mode);
        assert!(session.turns.is_cancelled());
        assert!(session.cancel_deadline.is_some());
        assert_eq!(notices(&mut events), vec![LEFT_FULL_ACCESS.to_owned()]);
        assert!(LEFT_FULL_ACCESS.contains("interrompido"));
    }
}

#[tokio::test]
async fn session_mode_changes_that_keep_the_turn_running() {
    let cases = [
        (
            PermissionMode::FullAccess,
            Turns::default(),
            PermissionMode::Supervised,
        ),
        (
            PermissionMode::AcceptEdits,
            running("u1"),
            PermissionMode::Supervised,
        ),
        (
            PermissionMode::Supervised,
            running("u1"),
            PermissionMode::FullAccess,
        ),
        (
            PermissionMode::FullAccess,
            running("u1"),
            PermissionMode::FullAccess,
        ),
    ];
    for (current, turns, requested) in cases {
        let in_turn = turns.in_turn();
        let mut session = live_session(current, turns).await;
        let (_, mut events) = session.shared.subscribe();

        assert!(session.change_mode(requested).is_ok());

        assert!(
            !session.turns.is_cancelled(),
            "{current:?} -> {requested:?}"
        );
        assert_eq!(session.turns.in_turn(), in_turn);
        assert!(session.cancel_deadline.is_none());
        assert!(
            notices(&mut events).is_empty(),
            "{current:?} -> {requested:?}"
        );
    }
}

#[tokio::test]
async fn leaving_full_access_after_a_cancel_does_not_repeat_the_notice() {
    let mut turns = running("u1");
    turns.cancel();
    let mut session = live_session(PermissionMode::FullAccess, turns).await;
    let (_, mut events) = session.shared.subscribe();

    assert!(session.change_mode(PermissionMode::Supervised).is_ok());

    assert_eq!(session.mode, PermissionMode::Supervised);
    assert!(notices(&mut events).is_empty());
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
fn without_a_ready_sandbox_every_mode_asks_before_running_commands() {
    let missing: Sandbox = Err("notConfigured".to_owned());
    for requested in [
        PermissionMode::Supervised,
        PermissionMode::AcceptEdits,
        PermissionMode::FullAccess,
    ] {
        let mode = effective_mode(requested, &missing);
        let thread = thread_params(&cwd(), None, mode);
        let resumed = resume_params("th_1", &cwd(), None, mode);
        let turn = with_mode(turn_params("th_1", "revise", None, None), mode);
        for params in [&thread, &resumed, &turn] {
            assert_eq!(
                params["approvalPolicy"],
                json!("untrusted"),
                "{requested:?}"
            );
        }
        assert_eq!(thread["sandbox"], json!("read-only"), "{requested:?}");
        assert_eq!(turn["sandboxPolicy"], json!({"type": "readOnly"}));
    }
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
