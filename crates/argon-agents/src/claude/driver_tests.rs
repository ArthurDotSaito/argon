use super::*;
use crate::session::native_options_for;

const ID: &str = "0f8e4d2c-1b3a-4c5d-9e6f-7a8b9c0d1e2f";

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn stream_args() -> Vec<String> {
    strings(&[
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--permission-prompt-tool",
        "stdio",
        "--permission-mode",
        "default",
        "--setting-sources",
        "user",
        "--strict-mcp-config",
    ])
}

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

fn granted(tools: &[&str]) -> HashSet<String> {
    tools.iter().map(|tool| (*tool).to_owned()).collect()
}

#[test]
fn new_session_args_pin_the_session_id_and_model() {
    let args = launch_args(&strings(&["--debug"]), Start::New(ID), "sonnet");

    let mut expected = strings(&["--debug"]);
    expected.extend(stream_args());
    expected.extend(strings(&["--session-id", ID, "--model", "sonnet"]));
    assert_eq!(args, expected);
}

#[test]
fn resume_args_use_the_resume_flag() {
    let args = launch_args(&[], Start::Resume(ID), "opus");

    let mut expected = stream_args();
    expected.extend(strings(&["--resume", ID, "--model", "opus"]));
    assert_eq!(args, expected);
}

#[test]
fn args_ignore_worktree_settings_and_mcp_config() {
    let args = launch_args(&[], Start::New(ID), "");

    let sources = args.iter().position(|arg| arg == "--setting-sources");
    assert_eq!(sources.map(|index| args[index + 1].as_str()), Some("user"));
    assert!(args.contains(&"--strict-mcp-config".to_owned()));
}

#[test]
fn args_pin_the_default_permission_mode() {
    let args = launch_args(&[], Start::Resume(ID), "");

    let mode = args.iter().position(|arg| arg == "--permission-mode");
    assert_eq!(mode.map(|index| args[index + 1].as_str()), Some("default"));
}

#[test]
fn default_or_empty_model_omits_the_model_flag() {
    for model in ["default", ""] {
        let args = launch_args(&[], Start::New(ID), model);

        assert!(!args.contains(&"--model".to_owned()), "{model}");
        assert_eq!(args.last().map(String::as_str), Some(ID));
    }
}

#[test]
fn uuid_sets_version_and_variant_bits() {
    assert_eq!(
        format_uuid([0xff; UUID_BYTES]),
        "ffffffff-ffff-4fff-bfff-ffffffffffff"
    );
    assert_eq!(
        format_uuid([0x00; UUID_BYTES]),
        "00000000-0000-4000-8000-000000000000"
    );
}

#[test]
fn random_uuids_are_hyphenated_lowercase_v4() {
    let first = new_uuid().unwrap();
    let second = new_uuid().unwrap();

    assert_ne!(first, second);
    let groups: Vec<&str> = first.split('-').collect();
    assert_eq!(
        groups.iter().map(|group| group.len()).collect::<Vec<_>>(),
        [8, 4, 4, 4, 12]
    );
    assert!(
        first
            .chars()
            .all(|c| c == '-' || c.is_ascii_digit() || ('a'..='f').contains(&c))
    );
    assert!(groups[2].starts_with('4'));
    assert!(matches!(
        groups[3].chars().next(),
        Some('8' | '9' | 'a' | 'b')
    ));
}

#[test]
fn exit_message_carries_the_exit_code() {
    assert_eq!(exit_message(Some(1)), "processo do Claude saiu (código 1)");
    assert_eq!(exit_message(None), "processo do Claude saiu");
}

#[test]
fn mode_rules_still_auto_allow_without_a_session_grant() {
    let read = native_ask("read", &[inside("src/a.rs")]);

    assert!(auto_allowed(
        &read,
        "Read",
        &cwd(),
        PermissionMode::Supervised,
        &granted(&[])
    ));
}

#[test]
fn session_grant_allows_contained_local_tools() {
    let edit = native_ask("edit", &[inside("src/a.rs")]);

    assert!(!auto_allowed(
        &edit,
        "Edit",
        &cwd(),
        PermissionMode::Supervised,
        &granted(&[])
    ));
    assert!(auto_allowed(
        &edit,
        "Edit",
        &cwd(),
        PermissionMode::Supervised,
        &granted(&["Edit"])
    ));
}

#[test]
fn session_grant_never_covers_commands() {
    let bash = native_ask("execute", &[]);

    assert!(!auto_allowed(
        &bash,
        "Bash",
        &cwd(),
        PermissionMode::AcceptEdits,
        &granted(&["Bash"])
    ));
}

#[test]
fn session_grant_never_covers_edits_outside_the_worktree() {
    let escaping = native_ask("edit", &[outside()]);
    let unresolvable = native_ask("edit", &["~/x".to_owned()]);
    let pathless = native_ask("edit", &[]);

    for ask in [escaping, unresolvable, pathless] {
        assert!(
            !auto_allowed(
                &ask,
                "Edit",
                &cwd(),
                PermissionMode::AcceptEdits,
                &granted(&["Edit"])
            ),
            "{:?}",
            ask.paths
        );
    }
}

#[test]
fn successful_result_completes_the_turn() {
    let mut turns = Turns::default();
    turns.start();

    assert!(turns.in_turn());
    assert_eq!(turns.finish("end_turn", None), Some(Close::Completed));
    assert!(!turns.in_turn());
}

#[test]
fn api_error_fails_an_otherwise_successful_turn() {
    let mut turns = Turns::default();
    turns.start();
    turns.api_error("session limit".to_owned());

    assert_eq!(
        turns.finish("end_turn", None),
        Some(Close::Failed("session limit".to_owned()))
    );
}

#[test]
fn error_result_prefers_its_own_text() {
    let mut turns = Turns::default();
    turns.start();
    turns.api_error("api".to_owned());

    assert_eq!(
        turns.finish("error", Some("result".to_owned())),
        Some(Close::Failed("result".to_owned()))
    );
}

#[test]
fn error_result_without_text_reports_the_stop_reason() {
    let mut turns = Turns::default();
    turns.start();

    assert_eq!(
        turns.finish("max_turns", None),
        Some(Close::Failed("max_turns".to_owned()))
    );
}

#[test]
fn result_without_a_turn_is_ignored() {
    let mut turns = Turns::default();

    assert_eq!(turns.finish("end_turn", None), None);
}

#[test]
fn cancelled_turn_closes_as_cancelled_when_its_result_arrives() {
    let mut turns = Turns::default();
    turns.start();

    assert!(!turns.is_cancelled());
    assert!(turns.cancel());
    assert!(turns.is_cancelled());
    assert!(!turns.cancel());
    assert_eq!(
        turns.finish("error", Some("interrupted".to_owned())),
        Some(Close::Cancelled)
    );
    assert!(!turns.in_turn());
}

#[test]
fn cancel_without_a_turn_does_nothing() {
    let mut turns = Turns::default();

    assert!(!turns.cancel());
    assert!(!turns.is_cancelled());
    assert_eq!(turns.expire(), None);
}

#[test]
fn expire_only_closes_a_cancelled_turn() {
    let mut turns = Turns::default();
    turns.start();

    assert_eq!(turns.expire(), None);
    assert!(turns.in_turn());
    turns.cancel();
    assert_eq!(turns.expire(), Some(Close::Cancelled));
    assert!(!turns.in_turn());
}
