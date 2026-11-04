mod claude;
mod codex;
mod opencode_go;

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use argon_core::AgentKind;
use serde_json::Value;
use tokio::process::{ChildStdin, ChildStdout};

use crate::process::KillOnDropChild;
use crate::registry::AgentSpec;
use crate::session::start_agent;

const COMMAND_CODE_USAGE_URL: &str = "https://commandcode.ai/usage";
const NOT_SUPPORTED: &str = "Sem suporte";
const HOME_MISSING: &str = "Não encontrei a pasta do usuário";
const PROBE_BOUND: Duration = Duration::from_secs(20);
const WARNING_PERCENT: f64 = 80.0;
const BLOCKED_PERCENT: f64 = 100.0;
const MILLIS_THRESHOLD: i64 = 100_000_000_000;
const BLOCKED_STATUSES: [&str; 5] = [
    "blocked",
    "rate_limited",
    "rate-limited",
    "limited",
    "exceeded",
];
const WARNING_STATUSES: [&str; 2] = ["warning", "warn"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageSeverity {
    Normal,
    Warning,
    Blocked,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageWindow {
    pub label: String,
    pub percent: f64,
    pub resets_at: Option<SystemTime>,
    pub severity: UsageSeverity,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AccountUsage {
    Ok {
        plan: String,
        windows: Vec<UsageWindow>,
    },
    Unavailable(String),
    Link(String),
}

pub async fn account_usage(
    kind: AgentKind,
    spec: &AgentSpec,
    consent_opencode_go: bool,
) -> AccountUsage {
    match kind {
        AgentKind::ClaudeCode => claude::read(spec).await,
        AgentKind::Codex => codex::read(spec).await,
        AgentKind::OpenCode => opencode_go::read(consent_opencode_go).await,
        AgentKind::CommandCode => AccountUsage::Link(COMMAND_CODE_USAGE_URL.to_owned()),
        AgentKind::Antigravity => AccountUsage::Unavailable(NOT_SUPPORTED.to_owned()),
    }
}

pub(crate) fn severity_for(percent: f64, status: Option<&str>) -> UsageSeverity {
    let status = status.map(str::to_ascii_lowercase);
    let status = status.as_deref().unwrap_or_default();
    if percent >= BLOCKED_PERCENT || BLOCKED_STATUSES.contains(&status) {
        UsageSeverity::Blocked
    } else if percent >= WARNING_PERCENT || WARNING_STATUSES.contains(&status) {
        UsageSeverity::Warning
    } else {
        UsageSeverity::Normal
    }
}

pub(crate) fn epoch_time(seconds: i64) -> Option<SystemTime> {
    let seconds = u64::try_from(seconds).ok()?;
    UNIX_EPOCH.checked_add(Duration::from_secs(seconds))
}

pub(crate) fn parse_time(value: &Value) -> Option<SystemTime> {
    match value {
        Value::String(text) => chrono::DateTime::parse_from_rfc3339(text)
            .ok()
            .map(SystemTime::from),
        Value::Number(number) => {
            let raw = number.as_i64()?;
            if raw >= MILLIS_THRESHOLD {
                epoch_time(raw / 1000)
            } else {
                epoch_time(raw)
            }
        }
        _ => None,
    }
}

pub(crate) fn title_case(text: &str) -> String {
    let spaced = text.replace(['_', '-'], " ");
    spaced
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first
                    .to_uppercase()
                    .chain(chars.flat_map(char::to_lowercase))
                    .collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

async fn stop(child: &mut KillOnDropChild) {
    if let Err(error) = child.kill_tree() {
        tracing::debug!(%error, "usage probe process already gone");
    }
    let _ = child.wait().await;
}

pub(crate) async fn with_probe<T, Fut>(
    spec: &AgentSpec,
    extra_args: &[&str],
    exchange: impl FnOnce(ChildStdin, ChildStdout) -> Fut,
) -> Result<T, String>
where
    Fut: Future<Output = Result<T, String>>,
{
    let cwd = dirs::home_dir().ok_or_else(|| HOME_MISSING.to_owned())?;
    let mut args = spec.args.clone();
    args.extend(extra_args.iter().map(|arg| (*arg).to_owned()));
    let launch = AgentSpec {
        args,
        ..spec.clone()
    };
    let (mut child, stdin, stdout) =
        start_agent(&launch, &cwd, Arc::default()).map_err(|error| error.to_string())?;
    let outcome = tokio::time::timeout(PROBE_BOUND, exchange(stdin, stdout)).await;
    stop(&mut child).await;
    outcome.unwrap_or_else(|_| Err(format!("sem resposta em {}s", PROBE_BOUND.as_secs())))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
