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
}
