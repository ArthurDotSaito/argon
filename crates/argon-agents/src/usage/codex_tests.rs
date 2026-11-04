use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Value, json};

use super::*;
use crate::usage::UsageSeverity;

const NOW_SECS: u64 = 1_790_000_000;

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(NOW_SECS)
}

fn windows_of(usage: AccountUsage) -> Vec<UsageWindow> {
    let AccountUsage::Ok { windows, .. } = usage else {
        panic!("expected usage");
    };
    windows
}

fn snapshot() -> Value {
    json!({
        "limitId": "codex",
        "limitName": null,
        "planType": "plus",
        "primary": {"usedPercent": 42, "windowDurationMins": 300, "resetsAt": 1_790_003_600},
        "secondary": {"usedPercent": 85, "windowDurationMins": 10_080, "resetsAt": 1_790_400_000},
        "credits": null,
        "rateLimitReachedType": null
    })
}

#[test]
fn reads_the_single_bucket_view() {
    let limits = json!({"rateLimits": snapshot(), "rateLimitsByLimitId": null});

    let AccountUsage::Ok { plan, windows } = parse_limits(&limits, None, now()) else {
        panic!("expected usage");
    };

    assert_eq!(plan, "ChatGPT Plus");
    assert_eq!(
        windows,
        vec![
            UsageWindow {
                label: "5 horas".to_owned(),
                percent: 42.0,
                resets_at: Some(UNIX_EPOCH + Duration::from_secs(1_790_003_600)),
                severity: UsageSeverity::Normal,
            },
            UsageWindow {
                label: "Semanal".to_owned(),
                percent: 85.0,
                resets_at: Some(UNIX_EPOCH + Duration::from_secs(1_790_400_000)),
                severity: UsageSeverity::Warning,
            },
        ]
    );
}

#[test]
fn prefers_the_buckets_keyed_by_limit_id() {
    let mut other = snapshot();
    other["limitId"] = json!("codex_bengalfox");
    other["limitName"] = json!("GPT-5.3-Codex-Spark");
    other["primary"] = json!({"usedPercent": 100, "windowDurationMins": 60, "resetsAt": null});
    other["secondary"] = Value::Null;
    let limits = json!({
        "rateLimits": {"primary": {"usedPercent": 1, "windowDurationMins": 300}},
        "rateLimitsByLimitId": {"codex": snapshot(), "codex_bengalfox": other}
    });

    let windows = windows_of(parse_limits(&limits, None, now()));

    let labels: Vec<&str> = windows.iter().map(|window| window.label.as_str()).collect();
    assert_eq!(labels, ["5 horas", "Semanal", "GPT-5.3-Codex-Spark · 1 h"]);
    assert_eq!(windows[2].percent, 100.0);
    assert_eq!(windows[2].severity, UsageSeverity::Blocked);
}

#[test]
fn falls_back_to_the_single_bucket_when_the_map_is_empty() {
    let limits = json!({"rateLimits": snapshot(), "rateLimitsByLimitId": {}});

    assert_eq!(windows_of(parse_limits(&limits, None, now())).len(), 2);
}

#[test]
fn a_window_already_reset_shows_zero_percent() {
    let mut stale = snapshot();
    stale["primary"] =
        json!({"usedPercent": 97, "windowDurationMins": 300, "resetsAt": NOW_SECS - 60});
    let limits = json!({"rateLimits": stale});

    let windows = windows_of(parse_limits(&limits, None, now()));

    assert_eq!(windows[0].percent, 0.0);
    assert_eq!(windows[0].resets_at, None);
    assert_eq!(windows[0].severity, UsageSeverity::Normal);
    assert_eq!(windows[1].percent, 85.0);
}

#[test]
fn window_without_a_reset_time_keeps_its_percent() {
    let limits = json!({"rateLimits": {"primary": {"usedPercent": 30}}});

    let windows = windows_of(parse_limits(&limits, None, now()));

    assert_eq!(
        windows,
        vec![UsageWindow {
            label: "Limite".to_owned(),
            percent: 30.0,
            resets_at: None,
            severity: UsageSeverity::Normal,
        }]
    );
}

#[test]
fn account_plan_wins_over_the_snapshot_plan() {
    let limits = json!({"rateLimits": snapshot()});

    let AccountUsage::Ok { plan, .. } = parse_limits(&limits, Some("team"), now()) else {
        panic!("expected usage");
    };
    assert_eq!(plan, "ChatGPT Team");

    let AccountUsage::Ok { plan, .. } = parse_limits(&json!({"rateLimits": {}}), None, now())
    else {
        panic!("expected usage");
    };
    assert_eq!(plan, "ChatGPT");
}

#[test]
fn maps_every_plan_type_in_the_schema() {
    let expected = [
        ("free", "ChatGPT Free"),
        ("go", "ChatGPT Go"),
        ("plus", "ChatGPT Plus"),
        ("pro", "ChatGPT Pro"),
        ("prolite", "ChatGPT Prolite"),
        ("team", "ChatGPT Team"),
        ("self_serve_business_prolite", "ChatGPT Business"),
        ("self_serve_business_usage_based", "ChatGPT Business"),
        ("business", "ChatGPT Business"),
        ("ent26", "ChatGPT Enterprise"),
        ("enterprise_cbp_automation", "ChatGPT Enterprise"),
        ("enterprise_cbp_usage_based", "ChatGPT Enterprise"),
        ("enterprise", "ChatGPT Enterprise"),
        ("edu", "ChatGPT Edu"),
        ("edu_plus", "ChatGPT Edu"),
        ("edu_pro", "ChatGPT Edu"),
        ("unknown", "ChatGPT"),
    ];

    for (plan_type, label) in expected {
        assert_eq!(plan_label(plan_type), label, "{plan_type}");
    }
}

#[test]
fn windows_are_labeled_by_their_duration() {
    let limits = json!({"rateLimits": {
        "primary": {"usedPercent": 1, "windowDurationMins": 43_200},
        "secondary": {"usedPercent": 2}
    }});

    let labels: Vec<String> = windows_of(parse_limits(&limits, None, now()))
        .into_iter()
        .map(|window| window.label)
        .collect();

    assert_eq!(labels, ["Mensal", "Limite"]);
}

#[test]
fn the_default_bucket_comes_first() {
    let mut early = snapshot();
    early["limitId"] = json!("alpha");
    early["limitName"] = json!("Alpha");
    let limits = json!({
        "rateLimits": snapshot(),
        "rateLimitsByLimitId": {"alpha": early, "codex": snapshot()}
    });

    let windows = windows_of(parse_limits(&limits, None, now()));

    let labels: Vec<&str> = windows.iter().map(|window| window.label.as_str()).collect();
    assert_eq!(
        labels,
        ["5 horas", "Semanal", "Alpha · 5 horas", "Alpha · Semanal"]
    );
}

#[test]
fn a_reached_limit_blocks_the_windows_still_in_use() {
    let mut reached = snapshot();
    reached["rateLimitReachedType"] = json!("rate_limit_reached");
    reached["primary"] =
        json!({"usedPercent": 40, "windowDurationMins": 300, "resetsAt": NOW_SECS - 1});

    let windows = windows_of(parse_limits(&json!({"rateLimits": reached}), None, now()));

    assert_eq!(windows[0].percent, 0.0);
    assert_eq!(windows[0].severity, UsageSeverity::Normal);
    assert_eq!(windows[1].percent, 85.0);
    assert_eq!(windows[1].severity, UsageSeverity::Blocked);
}

#[test]
fn ordinary_usage_not_allowed_blocks_every_bucket() {
    let limits = json!({"rateLimits": snapshot(), "ordinaryUsageAllowed": false});

    let windows = windows_of(parse_limits(&limits, None, now()));

    assert!(
        windows
            .iter()
            .all(|window| window.severity == UsageSeverity::Blocked)
    );
}

#[test]
fn allowed_usage_keeps_the_percent_severity() {
    let limits = json!({"rateLimits": snapshot(), "ordinaryUsageAllowed": true});

    let severities: Vec<UsageSeverity> = windows_of(parse_limits(&limits, None, now()))
        .into_iter()
        .map(|window| window.severity)
        .collect();

    assert_eq!(severities, [UsageSeverity::Normal, UsageSeverity::Warning]);
}

#[test]
fn percent_is_clamped_into_range() {
    let limits = json!({"rateLimits": {
        "primary": {"usedPercent": 180},
        "secondary": {"usedPercent": -3}
    }});

    let windows = windows_of(parse_limits(&limits, None, now()));

    assert_eq!(windows[0].percent, 100.0);
    assert_eq!(windows[0].severity, UsageSeverity::Blocked);
    assert_eq!(windows[1].percent, 0.0);
}

#[test]
fn the_probe_launches_the_installed_cli_not_the_acp_adapter() {
    let acp_spec = crate::agent_spec_for(
        AgentKind::Codex,
        &argon_core::Config::default(),
        crate::Transport::Acp,
    )
    .unwrap();
    assert_eq!(acp_spec.command, Path::new("npx"));

    let probe = crate::usage::probe_command(AgentKind::Codex, &acp_spec, &APP_SERVER_ARGS);

    assert_eq!(probe.command, Path::new("codex"));
    assert_eq!(probe.args, ["app-server"]);
}

#[test]
fn missing_account_asks_for_login() {
    for response in [
        json!({"account": null, "requiresOpenaiAuth": true}),
        json!({"requiresOpenaiAuth": true}),
    ] {
        assert!(matches!(
            gate(&response),
            Gate::Stop(AccountUsage::Unavailable(message)) if message == "Faça login no Codex"
        ));
    }
}

#[test]
fn api_key_accounts_have_no_plan_limits() {
    let response = json!({"account": {"type": "apiKey"}, "requiresOpenaiAuth": false});

    assert!(matches!(
        gate(&response),
        Gate::Stop(AccountUsage::Unavailable(message))
            if message == "Conta por API key: sem limites de plano"
    ));
}

#[test]
fn other_account_types_have_no_plan_limits() {
    let response = json!({
        "account": {"type": "amazonBedrock", "usesCodexManagedCredentials": false},
        "requiresOpenaiAuth": false
    });

    assert!(matches!(
        gate(&response),
        Gate::Stop(AccountUsage::Unavailable(message)) if message == "Conta sem limites de plano"
    ));
}

#[test]
fn chatgpt_accounts_continue_with_their_plan() {
    let response = json!({
        "account": {"type": "chatgpt", "email": "dev@example.com", "planType": "pro"},
        "requiresOpenaiAuth": false
    });

    assert!(matches!(
        gate(&response),
        Gate::Chatgpt { plan: Some(plan) } if plan == "pro"
    ));
}
