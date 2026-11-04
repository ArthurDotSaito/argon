use argon_core::{AgentKind, Config, PrKey};
use argon_store::{NewAgentSession, SessionStatus, Store};

use super::*;
use crate::Transport;
use crate::registry::agent_spec;
use crate::session::{SessionContext, SessionOptions, SessionWriter, native_options_for};

const ID: &str = "0f8e4d2c-1b3a-4c5d-9e6f-7a8b9c0d1e2f";
const SESSION: &str = "s1";

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

const FAKE_WINDOWS: &str = r#"@echo off
>>"%~dp0args.txt" echo %*
if "%1"=="early" goto early
if "%1"=="fork" goto fork
echo {"type":"system","subtype":"init"}
goto prompt
:fork
echo {"type":"system","subtype":"init","session_id":"forked-id"}
:prompt
set /p line=
if "%1"=="crash" goto crash
if "%1"=="auth" goto auth
echo {"type":"result","subtype":"success","is_error":false}
exit /b 0
:auth
echo {"type":"result","subtype":"success","is_error":true,"result":"Invalid API key - Please run /login"}
exit /b 0
:crash
echo boom 1>&2
exit /b 3
:early
echo boom 1>&2
exit /b 1
"#;

const FAKE_UNIX: &str = r#"#!/bin/sh
echo "$*" >> "$(dirname "$0")/args.txt"
if [ "$1" = early ]; then echo boom >&2; exit 1; fi
if [ "$1" = fork ]; then
  echo '{"type":"system","subtype":"init","session_id":"forked-id"}'
else
  echo '{"type":"system","subtype":"init"}'
fi
read line
if [ "$1" = crash ]; then echo boom >&2; exit 3; fi
if [ "$1" = auth ]; then
  echo '{"type":"result","subtype":"success","is_error":true,"result":"Invalid API key - Please run /login"}'
  exit 0
fi
echo '{"type":"result","subtype":"success","is_error":false}'
"#;

const SETTLE_TIMEOUT: Duration = Duration::from_secs(20);

struct Fake {
    dir: PathBuf,
    writer: SessionWriter,
    session: Session,
    outputs: mpsc::UnboundedReceiver<Tagged>,
    _answers: mpsc::UnboundedReceiver<Answer>,
}

fn write_fake(dir: &Path) -> PathBuf {
    let (script, body) = if cfg!(windows) {
        (dir.join("claude.cmd"), FAKE_WINDOWS)
    } else {
        (dir.join("claude"), FAKE_UNIX)
    };
    std::fs::write(&script, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    script
}

fn fake_run(dir: &Path, mode: &str, shared: &Arc<Shared>, tail: Arc<StderrTail>) -> DriverRun {
    let mut spec = agent_spec(AgentKind::ClaudeCode, &Config::default());
    spec.transport = Transport::ClaudeStream;
    spec.command = write_fake(dir);
    spec.args = vec![mode.to_owned()];
    spec.env.clear();
    let (_, commands) = mpsc::unbounded_channel();
    let (ready, _) = oneshot::channel();
    DriverRun {
        shared: Arc::clone(shared),
        spec,
        ctx: SessionContext {
            id: SESSION.to_owned(),
            pr: PrKey::new("github.com", "PR_1"),
            template_id: "review-padrao".to_owned(),
            nonce: String::new(),
            allowed_files: Vec::new(),
            cwd: dir.to_path_buf(),
            model: String::new(),
            permission_mode: PermissionMode::Supervised,
        },
        load: None,
        options: SessionOptions::default(),
        commands,
        ready,
        stderr: tail,
    }
}

async fn fake(mode: &str) -> Fake {
    let dir = std::env::temp_dir().join(format!("argon-claude-{mode}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let writer = SessionWriter::from_store(Store::open(&dir.join("argon.db")).unwrap()).unwrap();
    let tail = Arc::new(StderrTail::default());
    let shared = Arc::new(Shared::for_tests(
        SESSION,
        dir.clone(),
        writer.clone(),
        Arc::clone(&tail),
    ));
    let run = fake_run(&dir, mode, &shared, tail);
    let (answers, answered) = mpsc::unbounded_channel();
    let (outputs_tx, outputs) = mpsc::unbounded_channel();
    let session = open(&run, answers, outputs_tx).unwrap();
    writer
        .create_session(NewAgentSession {
            id: SESSION.to_owned(),
            pr: run.ctx.pr.clone(),
            agent: AgentKind::ClaudeCode,
            model: session.model.clone(),
            template: run.ctx.template_id.clone(),
            worktree_path: Some(dir.clone()),
            acp_session_id: Some(session.id.clone()),
            transport: Transport::ClaudeStream.as_str().to_owned(),
            permission_mode: PermissionMode::Supervised.as_str().to_owned(),
            nonce: String::new(),
            allowed_files: Vec::new(),
        })
        .await
        .unwrap();
    shared.activate(SessionStatus::Running, 0, session.model.clone(), Vec::new());
    Fake {
        dir,
        writer,
        session,
        outputs,
        _answers: answered,
    }
}

impl Fake {
    async fn turn(&mut self, text: &str) {
        self.session.prompt(text.to_owned(), false);
        while self.session.claude.is_some() {
            let (generation, output) = tokio::time::timeout(SETTLE_TIMEOUT, self.outputs.recv())
                .await
                .expect("the fake Claude never exited")
                .expect("output channel closed");
            if self.session.is_current(generation) {
                self.session.on_output(output).await;
            }
        }
    }

    fn launches(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("args.txt"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn status(&self) -> SessionStatus {
        self.session.shared.status()
    }

    async fn errors(&self) -> Vec<String> {
        self.writer
            .events_since(SESSION, 0)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|(_, event)| match event {
                AgentEvent::Error(message) => Some(message),
                _ => None,
            })
            .collect()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn started_with(launch: &str, flag: &str, id: &str) -> bool {
    launch.ends_with(&format!("{flag} {id}"))
}

#[tokio::test]
async fn exit_between_turns_keeps_the_session_and_resumes_on_the_next_prompt() {
    let mut fake = fake("idle").await;
    let id = fake.session.id.clone();

    fake.turn("primeiro").await;
    assert_eq!(fake.status(), SessionStatus::Completed);
    assert!(fake.session.transcript);

    fake.turn("segundo").await;
    assert_eq!(fake.status(), SessionStatus::Completed);
    let launches = fake.launches();
    assert_eq!(launches.len(), 2, "{launches:?}");
    assert!(
        started_with(&launches[0], SESSION_ID_FLAG, &id),
        "{launches:?}"
    );
    assert!(started_with(&launches[1], RESUME_FLAG, &id), "{launches:?}");
    assert!(fake.errors().await.is_empty());
}

#[tokio::test]
async fn exit_during_a_turn_fails_the_turn_and_the_next_prompt_resumes() {
    let mut fake = fake("crash").await;
    let id = fake.session.id.clone();

    fake.turn("primeiro").await;
    assert_eq!(fake.status(), SessionStatus::Failed);
    assert!(!fake.session.turns.in_turn());
    let errors = fake.errors().await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].starts_with("processo do Claude saiu (código 3)"),
        "{errors:?}"
    );

    fake.turn("segundo").await;
    let launches = fake.launches();
    assert_eq!(launches.len(), 2, "{launches:?}");
    assert!(started_with(&launches[1], RESUME_FLAG, &id), "{launches:?}");
}

#[tokio::test]
async fn death_before_init_respawns_with_the_same_session_id() {
    let mut fake = fake("early").await;
    let id = fake.session.id.clone();

    fake.turn("primeiro").await;
    assert_eq!(fake.status(), SessionStatus::Failed);
    assert!(!fake.session.transcript);

    fake.turn("segundo").await;
    let launches = fake.launches();
    assert_eq!(launches.len(), 2, "{launches:?}");
    for launch in &launches {
        assert!(started_with(launch, SESSION_ID_FLAG, &id), "{launches:?}");
        assert!(!launch.contains(RESUME_FLAG), "{launches:?}");
    }
}

#[tokio::test]
async fn reported_session_id_is_followed_on_resume_and_persisted() {
    let mut fake = fake("fork").await;
    let id = fake.session.id.clone();

    fake.turn("primeiro").await;
    fake.turn("segundo").await;
    let launches = fake.launches();
    assert_eq!(launches.len(), 2, "{launches:?}");
    assert!(
        started_with(&launches[0], SESSION_ID_FLAG, &id),
        "{launches:?}"
    );
    assert!(
        started_with(&launches[1], RESUME_FLAG, "forked-id"),
        "{launches:?}"
    );
    fake.writer.flush().await;
    let row = Store::open(&fake.dir.join("argon.db"))
        .unwrap()
        .session(SESSION)
        .unwrap()
        .unwrap();
    assert_eq!(row.acp_session_id.as_deref(), Some("forked-id"));
}

#[tokio::test]
async fn first_turn_failing_on_auth_shows_the_claude_login_hint() {
    let mut fake = fake("auth").await;

    fake.turn("primeiro").await;
    assert_eq!(fake.status(), SessionStatus::Failed);
    assert_eq!(
        fake.errors().await,
        [
            "Invalid API key - Please run /login\nrequer login: Execute `claude auth login` no terminal para entrar."
        ]
    );
}

#[test]
fn abort_fails_a_live_turn_and_cancels_a_cancelled_one() {
    let mut turns = Turns::default();
    assert_eq!(turns.abort("gone".to_owned()), None);

    turns.start();
    assert_eq!(
        turns.abort("gone".to_owned()),
        Some(Close::Failed("gone".to_owned()))
    );
    turns.start();
    turns.cancel();
    assert_eq!(turns.abort("gone".to_owned()), Some(Close::Cancelled));
    assert!(!turns.in_turn());
}

#[test]
fn abort_reports_the_api_error_seen_in_the_turn() {
    let mut turns = Turns::default();
    turns.start();
    turns.api_error("session limit".to_owned());

    assert_eq!(
        turns.abort("gone".to_owned()),
        Some(Close::Failed("session limit".to_owned()))
    );
}
