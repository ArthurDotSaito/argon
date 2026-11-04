use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderValue};
use reqwest::redirect::Policy;
use reqwest::{Client, StatusCode};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use super::{AccountUsage, UsageWindow, epoch_millis, parse_time, usage_window};

const USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const DATABASE_BUSY_TIMEOUT: Duration = Duration::from_secs(2);
const DATABASE_FILE: &str = "opencode.db";
const AUTH_FILE: &str = "auth.json";
const DATA_DIR_NAME: &str = "opencode";
const OAUTH_INTEGRATION: &str = "opencode";
const GO_INTEGRATION: &str = "opencode-go";
const MAX_TOKEN_LEN: usize = 4096;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const DATABASE_UNREADABLE: &str = "não consegui ler o login do OpenCode (banco em uso?)";
const PLAN: &str = "OpenCode Go";
const CONSENT_REQUIRED: &str = "Autorize a leitura do login do OpenCode para ver o uso do Go";
const LOGIN_REQUIRED: &str = "Faça login no OpenCode Go";
const NO_SUBSCRIPTION: &str = "Sem assinatura do OpenCode Go";
const UNEXPECTED_RESPONSE: &str = "Resposta inesperada do OpenCode Go";
const TIMED_OUT: &str = "Tempo esgotado ao consultar o OpenCode Go";
const REQUEST_FAILED: &str = "Falha ao consultar o OpenCode Go";
const WINDOWS: [(&str, &str); 3] = [
    ("rolling", "5 horas"),
    ("weekly", "Semanal"),
    ("monthly", "Mensal"),
];
const CREDENTIALS_QUERY: &str = "SELECT integration_id, value FROM credential \
    WHERE integration_id IN ('opencode', 'opencode-go') \
    ORDER BY active DESC, time_updated DESC";

#[derive(Clone)]
struct Token(String);

impl fmt::Debug for Token {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Token(<redacted>)")
    }
}

impl Token {
    fn parse(raw: &str) -> Option<Token> {
        let valid = !raw.is_empty()
            && raw.len() <= MAX_TOKEN_LEN
            && !raw.chars().any(|ch| ch.is_whitespace() || ch.is_control());
        valid.then(|| Token(raw.to_owned()))
    }

    fn secret(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
enum Fetched {
    Usage(Value),
    Unauthorized,
    Forbidden,
    Failed(String),
}

pub(super) async fn read(consent: bool) -> AccountUsage {
    usage_with(consent, data_dir, fetch_usage).await
}

fn data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| dirs::home_dir().map(|home| home.join(".local").join("share")))?;
    Some(base.join(DATA_DIR_NAME))
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or_default()
}

async fn usage_with<D, F, Fut>(consent: bool, data_dir: D, fetch: F) -> AccountUsage
where
    D: FnOnce() -> Option<PathBuf>,
    F: Fn(Token) -> Fut,
    Fut: Future<Output = Fetched>,
{
    if !consent {
        return AccountUsage::Unavailable(CONSENT_REQUIRED.to_owned());
    }
    let Some(dir) = data_dir() else {
        return AccountUsage::Unavailable(LOGIN_REQUIRED.to_owned());
    };
    let now = now_millis();
    let loaded = tokio::task::spawn_blocking(move || load_tokens(&dir, now))
        .await
        .unwrap_or_default();
    if loaded.tokens.is_empty() && loaded.database_failed {
        return AccountUsage::Unavailable(DATABASE_UNREADABLE.to_owned());
    }
    first_usage(loaded.tokens, fetch).await
}

async fn first_usage<F, Fut>(tokens: Vec<Token>, fetch: F) -> AccountUsage
where
    F: Fn(Token) -> Fut,
    Fut: Future<Output = Fetched>,
{
    for token in tokens {
        match fetch(token).await {
            Fetched::Usage(usage) => return parse(&usage),
            Fetched::Unauthorized => {}
            Fetched::Forbidden => return AccountUsage::Unavailable(NO_SUBSCRIPTION.to_owned()),
            Fetched::Failed(reason) => return AccountUsage::Unavailable(reason),
        }
    }
    AccountUsage::Unavailable(LOGIN_REQUIRED.to_owned())
}

#[derive(Default)]
struct Loaded {
    tokens: Vec<Token>,
    database_failed: bool,
}

fn load_tokens(data_dir: &Path, now_millis: i64) -> Loaded {
    let database = data_dir.join(DATABASE_FILE);
    let (from_database, database_failed) = match query_credentials(&database, now_millis) {
        Ok(tokens) => (tokens, false),
        Err(error) => {
            tracing::debug!(%error, "OpenCode credential database unavailable");
            (Vec::new(), database.exists())
        }
    };
    if from_database.is_empty() {
        Loaded {
            tokens: tokens_from_auth_file(&data_dir.join(AUTH_FILE)),
            database_failed,
        }
    } else {
        Loaded {
            tokens: from_database,
            database_failed: false,
        }
    }
}

fn credential_token(integration: &str, value: &Value, now_millis: i64) -> Option<Token> {
    let secret = match integration {
        OAUTH_INTEGRATION => {
            if value.get("type").and_then(Value::as_str) != Some("oauth") {
                return None;
            }
            let expired = value
                .get("expires")
                .and_then(Value::as_i64)
                .is_some_and(|expires| epoch_millis(expires) <= now_millis);
            if expired {
                return None;
            }
            value.get("access")
        }
        GO_INTEGRATION => value.get("key"),
        _ => None,
    };
    secret.and_then(Value::as_str).and_then(Token::parse)
}

fn query_credentials(path: &Path, now_millis: i64) -> rusqlite::Result<Vec<Token>> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(DATABASE_BUSY_TIMEOUT)?;
    let mut statement = connection.prepare(CREDENTIALS_QUERY)?;
    let mut rows = statement.query([])?;
    let mut tokens: Vec<Token> = Vec::new();
    while let Some(row) = rows.next()? {
        let integration = row.get_ref(0)?.as_str()?;
        let raw = match row.get_ref(1)? {
            ValueRef::Text(bytes) | ValueRef::Blob(bytes) => bytes,
            _ => continue,
        };
        let Ok(value) = serde_json::from_slice::<Value>(raw) else {
            continue;
        };
        let Some(token) = credential_token(integration, &value, now_millis) else {
            continue;
        };
        if !tokens.iter().any(|known| known.secret() == token.secret()) {
            tokens.push(token);
        }
    }
    Ok(tokens)
}

fn tokens_from_auth_file(path: &Path) -> Vec<Token> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|auth| {
            let key = auth.get(GO_INTEGRATION)?.get("key")?.as_str()?;
            Token::parse(key)
        })
        .into_iter()
        .collect()
}

async fn fetch_usage(token: Token) -> Fetched {
    let Ok(client) = Client::builder()
        .redirect(Policy::none())
        .timeout(REQUEST_TIMEOUT)
        .build()
    else {
        return Fetched::Failed(REQUEST_FAILED.to_owned());
    };
    let Ok(mut authorization) = HeaderValue::from_str(&format!("Bearer {}", token.secret())) else {
        return Fetched::Unauthorized;
    };
    authorization.set_sensitive(true);
    let response = client
        .get(USAGE_URL)
        .header(AUTHORIZATION, authorization)
        .header(ACCEPT, "application/json")
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) if error.is_timeout() => return Fetched::Failed(TIMED_OUT.to_owned()),
        Err(_) => return Fetched::Failed(REQUEST_FAILED.to_owned()),
    };
    match response.status() {
        StatusCode::UNAUTHORIZED => Fetched::Unauthorized,
        StatusCode::FORBIDDEN => Fetched::Forbidden,
        status if status.is_success() => match read_limited(response).await {
            Some(usage) => Fetched::Usage(usage),
            None => Fetched::Failed(UNEXPECTED_RESPONSE.to_owned()),
        },
        status => Fetched::Failed(format!("OpenCode Go respondeu HTTP {}", status.as_u16())),
    }
}

async fn read_limited(mut response: reqwest::Response) -> Option<Value> {
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > MAX_BODY_BYTES {
                    return None;
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(_) => return None,
        }
    }
    serde_json::from_slice(&body).ok()
}

fn window(raw: &Value, label: &str) -> Option<UsageWindow> {
    let percent = raw.get("percent").and_then(Value::as_f64)?;
    Some(usage_window(
        label.to_owned(),
        percent,
        raw.get("resetsAt").and_then(parse_time),
        raw.get("status").and_then(Value::as_str),
    ))
}

fn parse(response: &Value) -> AccountUsage {
    let windows: Vec<UsageWindow> = WINDOWS
        .iter()
        .filter_map(|(key, label)| window(response.pointer(&format!("/usage/{key}"))?, label))
        .collect();
    if windows.is_empty() {
        return AccountUsage::Unavailable(UNEXPECTED_RESPONSE.to_owned());
    }
    AccountUsage::Ok {
        plan: PLAN.to_owned(),
        windows,
    }
}

#[cfg(test)]
#[path = "opencode_go_tests.rs"]
mod tests;
