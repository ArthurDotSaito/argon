use super::*;

fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn port_comes_from_the_url_of_a_json_line() {
    assert_eq!(
        port_from_line(r#"{"url":"http://127.0.0.1:55973"}"#),
        Some(55973)
    );
    assert_eq!(
        port_from_line(r#"  {"url":"http://127.0.0.1:4096/","extra":1}  "#),
        Some(4096)
    );
}

#[test]
fn lines_without_a_usable_port_are_ignored() {
    for line in [
        "",
        "opencode server listening",
        r#"{"url":"http://127.0.0.1:0"}"#,
        r#"{"url":"http://127.0.0.1"}"#,
        r#"{"url":"not a url"}"#,
        r#"{"url":42}"#,
        r#"{"port":4096}"#,
        r#"["http://127.0.0.1:4096"]"#,
    ] {
        assert_eq!(port_from_line(line), None, "{line}");
    }
}

#[test]
fn major_version_is_parsed_from_the_leading_number() {
    assert_eq!(major_version("2.2.5"), Some(2));
    assert_eq!(major_version("v2.0.32"), Some(2));
    assert_eq!(major_version("1.18.31"), Some(1));
    assert_eq!(major_version(" 10.0.0 "), Some(10));
    assert_eq!(major_version("dev"), None);
    assert_eq!(major_version(""), None);
}

#[test]
fn only_version_two_or_newer_is_supported() {
    assert!(check_version("2.2.5").is_ok());
    assert!(check_version("3.0.0").is_ok());
    assert_eq!(
        check_version("1.18.31").unwrap_err(),
        "OpenCode 1.18.31 não suportado; o Argon requer a 2.x"
    );
    assert!(check_version("dev").is_err());
}

#[test]
fn env_adds_the_isolation_defaults() {
    let result = server_env(&env(&[("KEY", "value")]), Path::new("cfg"), "pw");
    assert_eq!(result["KEY"], "value");
    assert_eq!(result[CONFIG_DIR_ENV], "cfg");
    assert_eq!(result[DISABLE_PROJECT_CONFIG_ENV], "true");
    assert_eq!(result[DISABLE_AUTOUPDATE_ENV], "true");
    assert_eq!(result[PASSWORD_ENV], "pw");
}

#[test]
fn user_env_overrides_the_defaults_but_never_the_password() {
    let user = env(&[
        (CONFIG_DIR_ENV, "mine"),
        (DISABLE_PROJECT_CONFIG_ENV, "false"),
        (DISABLE_AUTOUPDATE_ENV, "false"),
        (PASSWORD_ENV, "stolen"),
    ]);
    let result = server_env(&user, Path::new("cfg"), "pw");
    assert_eq!(result[CONFIG_DIR_ENV], "mine");
    assert_eq!(result[DISABLE_PROJECT_CONFIG_ENV], "false");
    assert_eq!(result[DISABLE_AUTOUPDATE_ENV], "false");
    assert_eq!(result[PASSWORD_ENV], "pw");
}

#[test]
fn password_is_32_base64url_chars_without_padding() {
    let first = new_password().unwrap();
    let second = new_password().unwrap();
    assert_eq!(first.len(), 32);
    assert!(
        first
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '-' || char == '_')
    );
    assert_ne!(first, second);
}

#[test]
fn config_dir_lives_under_the_argon_data_dir() {
    let dir = config_dir();
    assert!(dir.ends_with(Path::new("agents").join("opencode")));
    assert!(dir.starts_with(Paths::resolve().data_dir));
}

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn missing_config_becomes_an_ask_everything_file() {
    let written = permission_config(None).unwrap().unwrap();
    assert_eq!(
        parsed(&written),
        json!({"$schema": "https://opencode.ai/config.json", "permission": {"*": "ask"}})
    );
}

#[test]
fn config_that_already_asks_is_left_alone() {
    let existing = r#"{"model":"x/y","permission":{"*":"ask"}}"#;
    assert_eq!(permission_config(Some(existing)).unwrap(), None);
}

#[test]
fn existing_keys_survive_and_only_permission_is_replaced() {
    let existing = r#"{"model":"x/y","permission":{"bash":"allow","*":"allow"}}"#;
    let written = permission_config(Some(existing)).unwrap().unwrap();
    assert_eq!(
        parsed(&written),
        json!({"model": "x/y", "permission": {"*": "ask"}})
    );
}

#[test]
fn unreadable_config_is_reported_instead_of_overwritten() {
    assert!(permission_config(Some("{not json")).is_err());
    assert!(permission_config(Some("[1]")).is_err());
}

struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(name: &str) -> ScratchDir {
        let serial = CONFIG_WRITE_SERIAL.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "argon-opencode-{name}-{}-{serial}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        ScratchDir(path)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn permission_config_is_written_once_without_leftovers() {
    let dir = ScratchDir::new("config");
    ensure_permission_config(&dir.0).await.unwrap();
    let path = dir.0.join(CONFIG_FILE);
    let first = std::fs::read_to_string(&path).unwrap();
    assert_eq!(parsed(&first)["permission"], json!({"*": "ask"}));
    ensure_permission_config(&dir.0).await.unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
    let entries: Vec<_> = std::fs::read_dir(&dir.0).unwrap().collect();
    assert_eq!(entries.len(), 1);
}

#[tokio::test]
async fn permission_config_merges_into_an_existing_file() {
    let dir = ScratchDir::new("merge");
    let path = dir.0.join(CONFIG_FILE);
    std::fs::write(&path, r#"{"theme":"dark","permission":"allow"}"#).unwrap();
    ensure_permission_config(&dir.0).await.unwrap();
    assert_eq!(
        parsed(&std::fs::read_to_string(&path).unwrap()),
        json!({"theme": "dark", "permission": {"*": "ask"}})
    );
}

#[tokio::test]
async fn broken_permission_config_stops_the_start() {
    let dir = ScratchDir::new("broken");
    std::fs::write(dir.0.join(CONFIG_FILE), "{oops").unwrap();
    let error = ensure_permission_config(&dir.0).await.unwrap_err();
    assert!(
        error.starts_with("falha ao preparar a configuração do OpenCode"),
        "{error}"
    );
}

#[test]
fn env_keys_are_matched_case_insensitively() {
    let user = env(&[
        ("opencode_password", "stolen"),
        ("Opencode_Config_Dir", "mine"),
        ("opencode_disable_project_config", "false"),
        ("Opencode_Disable_Autoupdate", "false"),
    ]);
    let result = server_env(&user, Path::new("cfg"), "pw");
    assert_eq!(result.len(), 4);
    assert_eq!(result["Opencode_Config_Dir"], "mine");
    assert_eq!(result["opencode_disable_project_config"], "false");
    assert_eq!(result["Opencode_Disable_Autoupdate"], "false");
    assert_eq!(result[PASSWORD_ENV], "pw");
}

async fn read_all_lines(bytes: &[u8]) -> Vec<String> {
    let mut reader = bytes;
    let mut lines = Vec::new();
    while let Some(line) = read_line_lossy(&mut reader, MAX_LINE_BYTES).await.unwrap() {
        lines.push(line);
    }
    lines
}

#[tokio::test]
async fn lines_are_split_on_newlines_and_the_last_one_may_be_unterminated() {
    assert_eq!(
        read_all_lines(b"one\r\ntwo\n\nlast").await,
        ["one", "two", "", "last"]
    );
    assert!(read_all_lines(b"").await.is_empty());
}

#[tokio::test]
async fn invalid_utf8_never_aborts_the_read() {
    let lines = read_all_lines(b"bad \xff\xfe bytes\n{\"url\":\"http://127.0.0.1:4096\"}\n").await;
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains('\u{fffd}'));
    assert_eq!(port_from_line(&lines[1]), Some(4096));
}

#[tokio::test]
async fn overlong_lines_are_truncated_and_the_next_line_survives() {
    let mut bytes = vec![b'a'; MAX_LINE_BYTES * 3];
    bytes.extend_from_slice(b"\nnext\n");
    let lines = read_all_lines(&bytes).await;
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].len(), MAX_LINE_BYTES);
    assert_eq!(lines[1], "next");
}

#[test]
fn startup_messages_never_contain_the_password() {
    let tail = StderrTail::default();
    tail.push("boom with secret-pw inside".to_owned());
    let startup = Startup {
        password: "secret-pw",
        stderr: &tail,
        last_probe_error: Mutex::new(Some("HTTP 401: secret-pw".to_owned())),
    };
    let timeout = startup.timeout_message();
    assert!(timeout.starts_with("o OpenCode não iniciou em 90s"));
    assert!(timeout.contains("HTTP 401: [redacted]"));
    assert!(!timeout.contains("secret-pw"));
    let scrubbed = startup.scrub(&tail.last(STDERR_LINES_IN_ERROR).join("\n"));
    assert_eq!(scrubbed, "boom with [redacted] inside");
}

#[test]
fn timeout_message_without_a_probe_error_is_plain() {
    let tail = StderrTail::default();
    let startup = Startup {
        password: "pw",
        stderr: &tail,
        last_probe_error: Mutex::new(None),
    };
    assert_eq!(startup.timeout_message(), "o OpenCode não iniciou em 90s");
}

#[tokio::test]
#[ignore = "starts a real local opencode server"]
async fn smoke_private_server_ignores_the_project_config() {
    let dir = std::env::temp_dir().join(format!("argon-opencode-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("opencode.json"),
        r#"{"plugin":["definitely-not-a-real-plugin-xyz"]}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join(".env"),
        "OPENCODE_CONFIG_CONTENT={\"plugin\":[\"dotenv-injected-plugin-xyz\"]}\nOPENCODE_DISABLE_PROJECT_CONFIG=false\n",
    )
    .unwrap();
    let mut spec = crate::agent_spec(
        argon_core::AgentKind::OpenCode,
        &argon_core::Config::default(),
    );
    spec.args.clear();
    let started = start(&spec, &dir, Arc::new(StderrTail::default())).await;
    let outcome = match &started {
        Ok(server) => server.api.get("/api/config").await,
        Err(error) => Err(error.clone()),
    };
    if let Ok(server) = started {
        server.stop().await;
    }
    let _ = std::fs::remove_dir_all(&dir);
    let sources = outcome.unwrap().to_string();
    assert!(sources.contains("Argon"));
    assert!(!sources.contains("definitely-not-a-real-plugin-xyz"));
    assert!(!sources.contains("dotenv-injected-plugin-xyz"));
}

#[tokio::test]
#[ignore = "starts a real local opencode server"]
async fn smoke_private_server_asks_before_every_tool() {
    let dir = ScratchDir::new("smoke-permission");
    let mut spec = crate::agent_spec(
        argon_core::AgentKind::OpenCode,
        &argon_core::Config::default(),
    );
    spec.args.clear();
    let started = start(&spec, &dir.0, Arc::new(StderrTail::default())).await;
    let outcome = match &started {
        Ok(server) => match server.api.get("/api/config").await {
            Ok(config) => server
                .api
                .get("/api/agent")
                .await
                .map(|agents| (config, agents)),
            Err(error) => Err(error),
        },
        Err(error) => Err(error.clone()),
    };
    if let Ok(server) = started {
        server.stop().await;
    }
    let (config, agents) = outcome.unwrap();
    let ours = config
        .as_array()
        .unwrap()
        .iter()
        .find(|source| {
            source["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(CONFIG_FILE))
        })
        .unwrap();
    assert_eq!(
        ours["info"]["permissions"],
        json!([{"action": "*", "resource": "*", "effect": "ask"}])
    );
    let build = agents["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["id"] == "build")
        .unwrap();
    let last_wildcard = build["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|rule| rule["action"] == "*" && rule["resource"] == "*")
        .unwrap();
    assert_eq!(last_wildcard["effect"], "ask");
}
