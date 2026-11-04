use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Value, json};

use super::*;
use crate::usage::UsageSeverity;

fn team_usage() -> Value {
    json!({
        "subscription_type": "team",
        "rate_limits_available": true,
        "rate_limits": {"limits": [
            {
                "kind": "session",
                "percent": 89,
                "severity": "warning",
                "resets_at": "2026-09-24T03:10:00.419321+00:00",
                "scope": null
            },
            {
                "kind": "weekly_scoped",
                "percent": 56,
                "scope": {"model": {"display_name": "Fable"}}
            },
            {"kind": "weekly_all", "percent": 12.5, "resets_at": null},
            {"kind": "overage", "percent": 100}
        ]},
        "extra_usage": {"enabled": false}
    })
}

#[test]
fn parses_the_plan_and_every_window() {
    let AccountUsage::Ok { plan, windows } = parse(&team_usage()) else {
        panic!("expected usage");
    };

    assert_eq!(plan, "Team");
    assert_eq!(
        windows,
        vec![
            UsageWindow {
                label: "Sessão atual".to_owned(),
                percent: 89.0,
                resets_at: Some(UNIX_EPOCH + Duration::new(1_790_219_400, 419_321_000)),
                severity: UsageSeverity::Warning,
            },
            UsageWindow {
                label: "Semanal · Fable".to_owned(),
                percent: 56.0,
                resets_at: None,
                severity: UsageSeverity::Normal,
            },
            UsageWindow {
                label: "Semanal · todos os modelos".to_owned(),
                percent: 12.5,
                resets_at: None,
                severity: UsageSeverity::Normal,
            },
            UsageWindow {
                label: "overage".to_owned(),
                percent: 100.0,
                resets_at: None,
                severity: UsageSeverity::Blocked,
            },
        ]
    );
}

#[test]
fn scoped_window_without_a_model_is_just_weekly() {
    let usage = json!({
        "subscription_type": "max",
        "rate_limits_available": true,
        "rate_limits": {"limits": [{"kind": "weekly_scoped", "percent": 1, "scope": null}]}
    });

    let AccountUsage::Ok { plan, windows } = parse(&usage) else {
        panic!("expected usage");
    };
    assert_eq!(plan, "Max");
    assert_eq!(windows[0].label, "Semanal");
}

#[test]
fn api_key_accounts_have_no_plan_limits() {
    let message = "Conta por API key: o Claude não informa limites de plano";
    let unavailable = AccountUsage::Unavailable(message.to_owned());

    assert_eq!(
        parse(&json!({"subscription_type": null, "rate_limits_available": true})),
        unavailable
    );
    assert_eq!(
        parse(&json!({"subscription_type": "pro", "rate_limits_available": false})),
        unavailable
    );
    assert_eq!(parse(&json!({})), unavailable);
}

#[test]
fn picks_only_the_usage_control_response() {
    let initialize = r#"{"type":"control_response","response":{"subtype":"success","request_id":"i1","response":{}}}"#;
    let assistant = r#"{"type":"assistant","message":{}}"#;
    let usage = r#"{"type":"control_response","response":{"subtype":"success","request_id":"u1","response":{"subscription_type":"pro"}}}"#;

    assert_eq!(usage_response("not json"), None);
    assert_eq!(usage_response(assistant), None);
    assert_eq!(usage_response(initialize), None);
    assert_eq!(
        usage_response(usage),
        Some(Ok(json!({"subscription_type": "pro"})))
    );
}

#[test]
fn error_responses_surface_the_reason() {
    let refused = r#"{"type":"control_response","response":{"subtype":"error","request_id":"u1","error":"not logged in"}}"#;
    let unexplained =
        r#"{"type":"control_response","response":{"subtype":"error","request_id":"u1"}}"#;

    assert_eq!(
        usage_response(refused),
        Some(Err("not logged in".to_owned()))
    );
    assert_eq!(
        usage_response(unexplained),
        Some(Err("o Claude recusou a consulta de uso".to_owned()))
    );
}

#[test]
fn requests_match_the_control_protocol() {
    assert_eq!(
        serde_json::from_str::<Value>(&initialize_request()).unwrap(),
        json!({"type": "control_request", "request_id": "i1", "request": {"subtype": "initialize"}})
    );
    assert_eq!(
        serde_json::from_str::<Value>(&usage_request()).unwrap(),
        json!({
            "type": "control_request",
            "request_id": "u1",
            "request": {"subtype": "get_usage", "skip_behaviors": true}
        })
    );
}
