use std::time::SystemTime;

use serde_json::{Value, json};
use tokio::process::{ChildStdin, ChildStdout};

use super::{
    AccountUsage, PROBE_BOUND, UsageWindow, epoch_time, severity_for, title_case, with_probe,
};
use crate::codex::protocol::initialize_params;
use crate::jsonrpc::RpcClient;
use crate::registry::AgentSpec;

const APP_SERVER_ARGS: [&str; 1] = ["app-server"];
const LOGIN_REQUIRED: &str = "Faça login no Codex";
const API_KEY_ACCOUNT: &str = "Conta por API key: sem limites de plano";
const NO_PLAN_LIMITS: &str = "Conta sem limites de plano";
const DEFAULT_BUCKET: &str = "codex";
const PLAN_PREFIX: &str = "ChatGPT";
const FIVE_HOURS_MINS: i64 = 300;
const WEEK_MINS: i64 = 10_080;

enum Gate {
    Chatgpt { plan: Option<String> },
    Stop(AccountUsage),
}

pub(super) async fn read(spec: &AgentSpec) -> AccountUsage {
    match with_probe(spec, &APP_SERVER_ARGS, exchange).await {
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

fn duration_label(minutes: Option<i64>) -> String {
    match minutes {
        Some(FIVE_HOURS_MINS) => "Janela de 5 horas".to_owned(),
        Some(WEEK_MINS) => "Semanal".to_owned(),
        Some(minutes) => format!("{minutes} min"),
        None => "Limite".to_owned(),
    }
}

fn window(raw: &Value, prefix: Option<&str>, now: SystemTime) -> Option<UsageWindow> {
    let used = raw.get("usedPercent").and_then(Value::as_f64)?;
    let label = duration_label(raw.get("windowDurationMins").and_then(Value::as_i64));
    let resets_at = raw
        .get("resetsAt")
        .and_then(Value::as_i64)
        .and_then(epoch_time);
    let (percent, resets_at) = match resets_at {
        Some(at) if at <= now => (0.0, None),
        other => (used, other),
    };
    Some(UsageWindow {
        label: match prefix {
            Some(prefix) => format!("{prefix} · {label}"),
            None => label,
        },
        percent,
        resets_at,
        severity: severity_for(percent, None),
    })
}

fn snapshots(limits: &Value) -> Vec<&Value> {
    let by_id = limits
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
        .filter(|buckets| !buckets.is_empty());
    match by_id {
        Some(buckets) => buckets.values().collect(),
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

fn parse_limits(limits: &Value, account_plan: Option<&str>, now: SystemTime) -> AccountUsage {
    let snapshots = snapshots(limits);
    let plan_type = account_plan.or_else(|| {
        snapshots
            .iter()
            .find_map(|snapshot| snapshot.get("planType").and_then(Value::as_str))
    });
    let windows = snapshots
        .iter()
        .flat_map(|snapshot| {
            let prefix = bucket_prefix(snapshot, snapshots.len());
            ["primary", "secondary"]
                .into_iter()
                .filter_map(|slot| snapshot.get(slot))
                .filter_map(|raw| window(raw, prefix.as_deref(), now))
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
