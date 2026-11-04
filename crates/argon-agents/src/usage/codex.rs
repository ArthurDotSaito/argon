use std::time::SystemTime;

use argon_core::AgentKind;
use serde_json::{Value, json};
use tokio::process::{ChildStdin, ChildStdout};

use super::{
    AccountUsage, PROBE_BOUND, UsageWindow, duration_label, epoch_time, title_case, usage_window,
    with_probe,
};
use crate::codex::protocol::initialize_params;
use crate::jsonrpc::RpcClient;
use crate::registry::AgentSpec;

const APP_SERVER_ARGS: [&str; 1] = ["app-server"];
const LOGIN_REQUIRED: &str = "Faça login no Codex";
const API_KEY_ACCOUNT: &str = "Conta por API key: sem limites de plano";
const NO_PLAN_LIMITS: &str = "Conta sem limites de plano";
const UNKNOWN_WINDOW: &str = "Limite";
const BLOCKED_STATUS: &str = "blocked";
const DEFAULT_BUCKET: &str = "codex";
const PLAN_PREFIX: &str = "ChatGPT";

enum Gate {
    Chatgpt { plan: Option<String> },
    Stop(AccountUsage),
}

pub(super) async fn read(spec: &AgentSpec) -> AccountUsage {
    match with_probe(AgentKind::Codex, spec, &APP_SERVER_ARGS, exchange).await {
        Ok(usage) => usage,
        Err(reason) => AccountUsage::Unavailable(reason),
    }
}

async fn exchange(stdin: ChildStdin, stdout: ChildStdout) -> Result<AccountUsage, String> {
    let (rpc, _incoming) = RpcClient::with_timeout(stdin, stdout, PROBE_BOUND);
    rpc.call("initialize", initialize_params(env!("CARGO_PKG_VERSION")))
        .await?;
    rpc.notify("initialized", None)?;
    let account = rpc
        .call("account/read", json!({"refreshToken": false}))
        .await?;
    let plan = match gate(&account) {
        Gate::Chatgpt { plan } => plan,
        Gate::Stop(usage) => return Ok(usage),
    };
    let limits = rpc
        .call(
            "account/rateLimits/read",
            json!({"excludeResetCreditDetails": true}),
        )
        .await?;
    Ok(parse_limits(&limits, plan.as_deref(), SystemTime::now()))
}

fn gate(response: &Value) -> Gate {
    let Some(account) = response.get("account").filter(|account| !account.is_null()) else {
        return Gate::Stop(AccountUsage::Unavailable(LOGIN_REQUIRED.to_owned()));
    };
    match account.get("type").and_then(Value::as_str) {
        Some("chatgpt") => Gate::Chatgpt {
            plan: account
                .get("planType")
                .and_then(Value::as_str)
                .map(str::to_owned),
        },
        Some("apiKey") => Gate::Stop(AccountUsage::Unavailable(API_KEY_ACCOUNT.to_owned())),
        _ => Gate::Stop(AccountUsage::Unavailable(NO_PLAN_LIMITS.to_owned())),
    }
}

fn plan_name(plan_type: &str) -> &str {
    match plan_type {
        "ent26" => "enterprise",
        other if other.starts_with("self_serve_business") => "business",
        other if other.starts_with("enterprise") => "enterprise",
        "edu_plus" | "edu_pro" => "edu",
        other => other,
    }
}

fn plan_label(plan_type: &str) -> String {
    match plan_type {
        "unknown" => PLAN_PREFIX.to_owned(),
        other => format!("{PLAN_PREFIX} {}", title_case(plan_name(other))),
    }
}

fn window(
    raw: &Value,
    prefix: Option<&str>,
    reached: bool,
    now: SystemTime,
) -> Option<UsageWindow> {
    let used = raw.get("usedPercent").and_then(Value::as_f64)?;
    let label = raw
        .get("windowDurationMins")
        .and_then(Value::as_i64)
        .map_or_else(|| UNKNOWN_WINDOW.to_owned(), duration_label);
    let resets_at = raw
        .get("resetsAt")
        .and_then(Value::as_i64)
        .and_then(epoch_time);
    let (used, resets_at) = match resets_at {
        Some(at) if at <= now => (0.0, None),
        other => (used, other),
    };
    let label = match prefix {
        Some(prefix) => format!("{prefix} · {label}"),
        None => label,
    };
    let status = (reached && used > 0.0).then_some(BLOCKED_STATUS);
    Some(usage_window(label, used, resets_at, status))
}

fn snapshots(limits: &Value) -> Vec<&Value> {
    let by_id = limits
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
        .filter(|buckets| !buckets.is_empty());
    match by_id {
        Some(buckets) => {
            let mut ordered: Vec<(&String, &Value)> = buckets.iter().collect();
            ordered.sort_by_key(|(id, _)| id.as_str() != DEFAULT_BUCKET);
            ordered.into_iter().map(|(_, snapshot)| snapshot).collect()
        }
        None => limits.get("rateLimits").into_iter().collect(),
    }
}

fn bucket_prefix(snapshot: &Value, bucket_count: usize) -> Option<String> {
    let id = snapshot.get("limitId").and_then(Value::as_str);
    if bucket_count < 2 || id.is_none_or(|id| id == DEFAULT_BUCKET) {
        return None;
    }
    snapshot
        .get("limitName")
        .and_then(Value::as_str)
        .or(id)
        .map(str::to_owned)
}

fn limit_reached(snapshot: &Value, ordinary_usage_blocked: bool) -> bool {
    ordinary_usage_blocked
        || snapshot
            .get("rateLimitReachedType")
            .is_some_and(|reached| !reached.is_null())
}

fn parse_limits(limits: &Value, account_plan: Option<&str>, now: SystemTime) -> AccountUsage {
    let snapshots = snapshots(limits);
    let ordinary_usage_blocked =
        limits.get("ordinaryUsageAllowed").and_then(Value::as_bool) == Some(false);
    let plan_type = account_plan.or_else(|| {
        snapshots
            .iter()
            .find_map(|snapshot| snapshot.get("planType").and_then(Value::as_str))
    });
    let windows = snapshots
        .iter()
        .flat_map(|snapshot| {
            let prefix = bucket_prefix(snapshot, snapshots.len());
            let reached = limit_reached(snapshot, ordinary_usage_blocked);
            ["primary", "secondary"]
                .into_iter()
                .filter_map(|slot| snapshot.get(slot))
                .filter_map(|raw| window(raw, prefix.as_deref(), reached, now))
                .collect::<Vec<UsageWindow>>()
        })
        .collect();
    AccountUsage::Ok {
        plan: plan_type.map_or_else(|| PLAN_PREFIX.to_owned(), plan_label),
        windows,
    }
}

#[cfg(test)]
#[path = "codex_tests.rs"]
mod tests;
