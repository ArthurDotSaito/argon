use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::json;

use super::*;
use crate::usage::UsageSeverity;

const NOW: i64 = 1_790_000_000_000;
static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let path = std::env::temp_dir().join(format!(
            "argon-usage-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, name: &str, content: &str) {
        std::fs::write(self.0.join(name), content).unwrap();
    }

    fn database(&self, rows: &[(&str, serde_json::Value, i64, i64)]) {
        let connection = Connection::open(self.0.join(DATABASE_FILE)).unwrap();
        connection
            .execute(
                "CREATE TABLE credential (integration_id TEXT NOT NULL, value TEXT NOT NULL, \
                 active INTEGER NOT NULL, time_updated INTEGER NOT NULL)",
                [],
            )
            .unwrap();
        for (integration, value, active, updated) in rows {
            connection
                .execute(
                    "INSERT INTO credential VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![integration, value.to_string(), active, updated],
                )
                .unwrap();
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn secrets(tokens: &[Token]) -> Vec<&str> {
    tokens.iter().map(Token::secret).collect()
}

fn oauth(access: &str, expires: i64) -> serde_json::Value {
    json!({"type": "oauth", "access": access, "refresh": "refresh-secret", "expires": expires})
}

fn go_key(key: &str) -> serde_json::Value {
    json!({"key": key})
}

#[test]
fn database_tokens_skip_expired_oauth_and_put_active_first() {
    let temp = TempDir::new();
    temp.database(&[
        ("opencode", oauth("expired-oauth", NOW - 1), 1, 500),
        ("opencode-go", go_key("go-active"), 1, 100),
        ("opencode", oauth("oauth-valid", NOW + 60_000), 0, 300),
        ("opencode-go", go_key("go-old"), 0, 200),
        ("opencode", oauth("has space", NOW + 60_000), 0, 400),
        ("github", go_key("unrelated"), 1, 900),
        ("opencode-go", go_key("go-active"), 0, 50),
        (
            "opencode",
            json!({"type": "api", "access": "wrong-type"}),
            0,
            10,
        ),
    ]);

    let tokens = load_tokens(temp.path(), NOW).tokens;

    assert_eq!(secrets(&tokens), ["go-active", "oauth-valid", "go-old"]);
}

#[test]
fn oauth_without_expiry_is_accepted() {
    let temp = TempDir::new();
    temp.database(&[(
        "opencode",
        json!({"type": "oauth", "access": "no-expiry"}),
        1,
        1,
    )]);

    assert_eq!(
        secrets(&load_tokens(temp.path(), NOW).tokens),
        ["no-expiry"]
    );
}

#[test]
fn auth_file_is_the_fallback_when_there_is_no_database() {
    let temp = TempDir::new();
    temp.write(
        AUTH_FILE,
        r#"{"opencode-go":{"type":"api","key":"file-key"},"anthropic":{"key":"other"}}"#,
    );

    assert_eq!(secrets(&load_tokens(temp.path(), NOW).tokens), ["file-key"]);
}

#[test]
fn auth_file_is_used_when_the_database_has_no_usable_token() {
    let temp = TempDir::new();
    temp.database(&[("opencode", oauth("expired", NOW - 1), 1, 1)]);
    temp.write(AUTH_FILE, r#"{"opencode-go":{"key":"file-key"}}"#);

    assert_eq!(secrets(&load_tokens(temp.path(), NOW).tokens), ["file-key"]);
}

#[test]
fn auth_file_is_ignored_when_the_database_has_a_token() {
    let temp = TempDir::new();
    temp.database(&[("opencode-go", go_key("db-key"), 1, 1)]);
    temp.write(AUTH_FILE, r#"{"opencode-go":{"key":"file-key"}}"#);

    assert_eq!(secrets(&load_tokens(temp.path(), NOW).tokens), ["db-key"]);
}

#[test]
fn missing_or_broken_sources_yield_no_tokens() {
    let temp = TempDir::new();
    assert!(load_tokens(temp.path(), NOW).tokens.is_empty());

    temp.write(DATABASE_FILE, "this is not a sqlite database");
    temp.write(AUTH_FILE, "{not json");
    assert!(load_tokens(temp.path(), NOW).tokens.is_empty());

    temp.write(AUTH_FILE, r#"{"opencode-go":{"key":"bad key"}}"#);
    assert!(load_tokens(temp.path(), NOW).tokens.is_empty());
}

#[test]
fn reading_the_database_never_modifies_it() {
    let temp = TempDir::new();
    temp.database(&[("opencode-go", go_key("db-key"), 1, 1)]);
    let before = std::fs::read(temp.path().join(DATABASE_FILE)).unwrap();

    let _ = load_tokens(temp.path(), NOW);

    assert_eq!(
        std::fs::read(temp.path().join(DATABASE_FILE)).unwrap(),
        before
    );
}

#[test]
fn token_validation_rejects_unsafe_values() {
    assert!(Token::parse("sk-abc_123.DEF").is_some());
    assert!(Token::parse("").is_none());
    assert!(Token::parse("two words").is_none());
    assert!(Token::parse(" leading").is_none());
    assert!(Token::parse("trailing\n").is_none());
    assert!(Token::parse("tab\there").is_none());
    assert!(Token::parse("nul\0byte").is_none());
    assert!(Token::parse("carriage\rreturn").is_none());
    assert!(Token::parse(&"a".repeat(MAX_TOKEN_LEN + 1)).is_none());
    assert!(Token::parse(&"a".repeat(MAX_TOKEN_LEN)).is_some());
}

#[test]
fn token_debug_never_prints_the_secret() {
    let token = Token::parse("super-secret-value").unwrap();

    assert_eq!(format!("{token:?}"), "Token(<redacted>)");
    assert!(!format!("{:?}", Fetched::Failed("x".to_owned())).contains("secret"));
}

#[test]
fn parses_the_three_windows() {
    let response = json!({"usage": {
        "rolling": {"percent": 35, "status": "ok", "resetsAt": "2026-09-24T03:10:00+00:00"},
        "weekly": {"percent": 85, "status": "ok", "resetsAt": 1_790_400_000},
        "monthly": {"percent": 100, "status": "rate-limited"}
    }});

    assert_eq!(
        parse(&response),
        AccountUsage::Ok {
            plan: "OpenCode Go".to_owned(),
            windows: vec![
                UsageWindow {
                    label: "5 horas".to_owned(),
                    percent: 35.0,
                    resets_at: Some(UNIX_EPOCH + Duration::from_secs(1_790_219_400)),
                    severity: UsageSeverity::Normal,
                },
                UsageWindow {
                    label: "Semanal".to_owned(),
                    percent: 85.0,
                    resets_at: Some(UNIX_EPOCH + Duration::from_secs(1_790_400_000)),
                    severity: UsageSeverity::Warning,
                },
                UsageWindow {
                    label: "Mensal".to_owned(),
                    percent: 100.0,
                    resets_at: None,
                    severity: UsageSeverity::Blocked,
                },
            ]
        }
    );
}

#[test]
fn provider_status_raises_the_severity() {
    let response = json!({"usage": {"rolling": {"percent": 10, "status": "rate-limited"}}});

    let AccountUsage::Ok { windows, .. } = parse(&response) else {
        panic!("expected usage");
    };
    assert_eq!(windows[0].severity, UsageSeverity::Blocked);
}

#[test]
fn unexpected_response_is_unavailable() {
    for response in [
        json!({}),
        json!({"usage": {}}),
        json!({"usage": {"weekly": {}}}),
    ] {
        assert_eq!(
            parse(&response),
            AccountUsage::Unavailable("Resposta inesperada do OpenCode Go".to_owned())
        );
    }
}

fn scripted(outcomes: Vec<Fetched>) -> (Mutex<Vec<String>>, Mutex<VecDeque<Fetched>>) {
    (Mutex::new(Vec::new()), Mutex::new(outcomes.into()))
}

async fn run_scripted(temp: &TempDir, outcomes: Vec<Fetched>) -> (AccountUsage, Vec<String>) {
    let (tried, queue) = scripted(outcomes);
    let dir = temp.path().to_path_buf();
    let usage = usage_with(
        true,
        move || Some(dir),
        |token| {
            tried.lock().unwrap().push(token.secret().to_owned());
            let next = queue
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Fetched::Unauthorized);
            async move { next }
        },
    )
    .await;
    let tried = tried.into_inner().unwrap();
    (usage, tried)
}

fn temp_with_two_keys() -> TempDir {
    let temp = TempDir::new();
    temp.database(&[
        ("opencode-go", go_key("first"), 1, 2),
        ("opencode-go", go_key("second"), 0, 1),
    ]);
    temp
}

fn weekly_only() -> serde_json::Value {
    json!({"usage": {"weekly": {"percent": 5}}})
}

#[tokio::test]
async fn unauthorized_moves_on_to_the_next_token() {
    let temp = temp_with_two_keys();

    let (usage, tried) = run_scripted(
        &temp,
        vec![Fetched::Unauthorized, Fetched::Usage(weekly_only())],
    )
    .await;

    assert_eq!(tried, ["first", "second"]);
    assert!(matches!(usage, AccountUsage::Ok { .. }));
}

#[tokio::test]
async fn every_token_unauthorized_asks_for_login() {
    let temp = temp_with_two_keys();

    let (usage, tried) = run_scripted(&temp, vec![]).await;

    assert_eq!(tried, ["first", "second"]);
    assert_eq!(
        usage,
        AccountUsage::Unavailable("Faça login no OpenCode Go".to_owned())
    );
}

#[tokio::test]
async fn forbidden_stops_with_no_subscription() {
    let temp = temp_with_two_keys();

    let (usage, tried) = run_scripted(&temp, vec![Fetched::Forbidden]).await;

    assert_eq!(tried, ["first"]);
    assert_eq!(
        usage,
        AccountUsage::Unavailable("Sem assinatura do OpenCode Go".to_owned())
    );
}

#[tokio::test]
async fn other_failures_stop_with_the_short_reason() {
    let temp = temp_with_two_keys();

    let (usage, tried) = run_scripted(
        &temp,
        vec![Fetched::Failed("OpenCode Go respondeu HTTP 500".to_owned())],
    )
    .await;

    assert_eq!(tried, ["first"]);
    assert_eq!(
        usage,
        AccountUsage::Unavailable("OpenCode Go respondeu HTTP 500".to_owned())
    );
}

#[tokio::test]
async fn no_tokens_means_login_is_required() {
    let temp = TempDir::new();

    let (usage, tried) = run_scripted(&temp, vec![]).await;

    assert!(tried.is_empty());
    assert_eq!(
        usage,
        AccountUsage::Unavailable("Faça login no OpenCode Go".to_owned())
    );
}

#[tokio::test]
async fn without_consent_nothing_is_resolved_read_or_fetched() {
    let temp = TempDir::new();
    temp.database(&[("opencode-go", go_key("must-not-be-read"), 1, 1)]);
    temp.write(AUTH_FILE, r#"{"opencode-go":{"key":"must-not-be-read"}}"#);

    let usage = usage_with(
        false,
        || panic!("the data dir must not be resolved without consent"),
        |_| async { panic!("the network must not be touched without consent") },
    )
    .await;

    assert_eq!(
        usage,
        AccountUsage::Unavailable(
            "Autorize a leitura do login do OpenCode para ver o uso do Go".to_owned()
        )
    );
}

#[tokio::test]
async fn a_missing_data_dir_asks_for_login() {
    let usage = usage_with(
        true,
        || None,
        |_| async { panic!("no token means no request") },
    )
    .await;

    assert_eq!(
        usage,
        AccountUsage::Unavailable("Faça login no OpenCode Go".to_owned())
    );
}

#[test]
fn oauth_expiry_in_seconds_is_normalized_before_comparing() {
    let temp = TempDir::new();
    let now_secs = NOW / 1000;
    temp.database(&[
        ("opencode", oauth("expired-seconds", now_secs - 1), 1, 3),
        ("opencode", oauth("valid-seconds", now_secs + 60), 1, 2),
        ("opencode", oauth("valid-millis", NOW + 60_000), 1, 1),
    ]);

    assert_eq!(
        secrets(&load_tokens(temp.path(), NOW).tokens),
        ["valid-seconds", "valid-millis"]
    );
}

#[test]
fn an_unreadable_database_without_auth_file_is_reported() {
    let temp = TempDir::new();
    temp.write(DATABASE_FILE, "this is not a sqlite database");

    let loaded = load_tokens(temp.path(), NOW);

    assert!(loaded.tokens.is_empty());
    assert!(loaded.database_failed);
}

#[test]
fn a_missing_database_is_not_a_failure() {
    let temp = TempDir::new();

    assert!(!load_tokens(temp.path(), NOW).database_failed);
}

#[test]
fn an_unreadable_database_still_falls_back_to_the_auth_file() {
    let temp = TempDir::new();
    temp.write(DATABASE_FILE, "this is not a sqlite database");
    temp.write(AUTH_FILE, r#"{"opencode-go":{"key":"file-key"}}"#);

    assert_eq!(secrets(&load_tokens(temp.path(), NOW).tokens), ["file-key"]);
}

#[tokio::test]
async fn an_unreadable_database_without_tokens_does_not_ask_for_login() {
    let temp = TempDir::new();
    temp.write(DATABASE_FILE, "this is not a sqlite database");
    let dir = temp.path().to_path_buf();

    let usage = usage_with(
        true,
        move || Some(dir),
        |_| async { panic!("no token means no request") },
    )
    .await;

    assert_eq!(
        usage,
        AccountUsage::Unavailable(
            "não consegui ler o login do OpenCode (banco em uso?)".to_owned()
        )
    );
}

#[test]
fn percent_is_clamped_into_range() {
    let response = json!({"usage": {
        "rolling": {"percent": 250},
        "weekly": {"percent": -5},
    }});

    let AccountUsage::Ok { windows, .. } = parse(&response) else {
        panic!("expected usage");
    };
    assert_eq!(windows[0].percent, 100.0);
    assert_eq!(windows[0].severity, UsageSeverity::Blocked);
    assert_eq!(windows[1].percent, 0.0);
}
