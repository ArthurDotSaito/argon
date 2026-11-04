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
