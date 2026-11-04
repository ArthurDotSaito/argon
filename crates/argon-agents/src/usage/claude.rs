use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};

use argon_core::AgentKind;

use super::{AccountUsage, UsageWindow, parse_time, title_case, usage_window, with_probe};
use crate::registry::AgentSpec;
use crate::util::read_line_lossy;

const PROBE_ARGS: [&str; 10] = [
    "-p",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--no-session-persistence",
    "--setting-sources",
    "user",
    "--strict-mcp-config",
];
const USAGE_REQUEST_ID: &str = "u1";
const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;
const API_KEY_ACCOUNT: &str = "Conta por API key: o Claude não informa limites de plano";
const CLOSED: &str = "o Claude encerrou sem informar o uso";
const REFUSED: &str = "o Claude recusou a consulta de uso";
const SESSION_LABEL: &str = "Sessão atual";
const WEEKLY_ALL_LABEL: &str = "Semanal · todos os modelos";
const WEEKLY_LABEL: &str = "Semanal";

fn initialize_request() -> String {
    json!({
        "type": "control_request",
        "request_id": "i1",
        "request": {"subtype": "initialize"},
    })
    .to_string()
}

fn usage_request() -> String {
    json!({
        "type": "control_request",
        "request_id": USAGE_REQUEST_ID,
        "request": {"subtype": "get_usage", "skip_behaviors": true},
    })
    .to_string()
}

pub(super) async fn read(spec: &AgentSpec) -> AccountUsage {
    match with_probe(AgentKind::ClaudeCode, spec, &PROBE_ARGS, exchange).await {
        Ok(usage) => usage,
        Err(reason) => AccountUsage::Unavailable(reason),
    }
}

async fn exchange(mut stdin: ChildStdin, stdout: ChildStdout) -> Result<AccountUsage, String> {
    for request in [initialize_request(), usage_request()] {
        write_line(&mut stdin, &request)
            .await
            .map_err(|error| format!("falha ao falar com o Claude: {error}"))?;
    }
    let mut reader = BufReader::new(stdout);
    loop {
        let line = read_line_lossy(&mut reader, MAX_LINE_BYTES)
            .await
            .map_err(|error| format!("falha ao ler a resposta do Claude: {error}"))?
            .ok_or_else(|| CLOSED.to_owned())?;
        if let Some(response) = usage_response(&line) {
            return response.map(|usage| parse(&usage));
        }
    }
}

async fn write_line(stdin: &mut ChildStdin, line: &str) -> std::io::Result<()> {
    stdin.write_all(line.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await
}

fn usage_response(line: &str) -> Option<Result<Value, String>> {
    let message: Value = serde_json::from_str(line.trim()).ok()?;
    if message.get("type").and_then(Value::as_str) != Some("control_response") {
        return None;
    }
    let response = message.get("response")?;
    if response.get("request_id").and_then(Value::as_str) != Some(USAGE_REQUEST_ID) {
        return None;
    }
    if response.get("subtype").and_then(Value::as_str) == Some("success") {
        return Some(Ok(response.get("response").cloned().unwrap_or(Value::Null)));
    }
    let reason = response
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or(REFUSED);
    Some(Err(reason.to_owned()))
}

fn window_label(limit: &Value, kind: &str) -> String {
    match kind {
        "session" => SESSION_LABEL.to_owned(),
        "weekly_all" => WEEKLY_ALL_LABEL.to_owned(),
        "weekly_scoped" => {
            let model = limit
                .pointer("/scope/model/display_name")
                .and_then(Value::as_str);
            match model {
                Some(model) => format!("{WEEKLY_LABEL} · {model}"),
                None => WEEKLY_LABEL.to_owned(),
            }
        }
        other => format!("Limite {other}"),
    }
}

fn window(limit: &Value) -> Option<UsageWindow> {
    let kind = limit.get("kind").and_then(Value::as_str)?;
    let percent = limit.get("percent").and_then(Value::as_f64)?;
    Some(usage_window(
        window_label(limit, kind),
        percent,
        limit.get("resets_at").and_then(parse_time),
        limit.get("severity").and_then(Value::as_str),
    ))
}

fn parse(usage: &Value) -> AccountUsage {
    let plan = usage.get("subscription_type").and_then(Value::as_str);
    let available = usage.get("rate_limits_available").and_then(Value::as_bool) != Some(false);
    let Some(plan) = plan.filter(|_| available) else {
        return AccountUsage::Unavailable(API_KEY_ACCOUNT.to_owned());
    };
    let windows = usage
        .pointer("/rate_limits/limits")
        .and_then(Value::as_array)
        .map(|limits| limits.iter().filter_map(window).collect())
        .unwrap_or_default();
    AccountUsage::Ok {
        plan: title_case(plan),
        windows,
    }
}

#[cfg(test)]
#[path = "claude_tests.rs"]
mod tests;
