use argon_core::Config;
use serde_json::json;

use super::*;
use crate::agent_spec;

#[test]
fn severity_follows_percent_thresholds() {
    assert_eq!(severity_for(0.0, None), UsageSeverity::Normal);
    assert_eq!(severity_for(79.9, None), UsageSeverity::Normal);
    assert_eq!(severity_for(80.0, None), UsageSeverity::Warning);
    assert_eq!(severity_for(99.0, None), UsageSeverity::Warning);
    assert_eq!(severity_for(100.0, None), UsageSeverity::Blocked);
    assert_eq!(severity_for(120.0, None), UsageSeverity::Blocked);
}

#[test]
fn severity_honors_the_provider_status() {
    assert_eq!(severity_for(10.0, Some("warning")), UsageSeverity::Warning);
    assert_eq!(severity_for(10.0, Some("Blocked")), UsageSeverity::Blocked);
    assert_eq!(
        severity_for(10.0, Some("rate-limited")),
        UsageSeverity::Blocked
    );
    assert_eq!(severity_for(10.0, Some("ok")), UsageSeverity::Normal);
    assert_eq!(severity_for(90.0, Some("ok")), UsageSeverity::Warning);
}

#[test]
fn parse_time_reads_rfc3339_seconds_and_milliseconds() {
    let expected = UNIX_EPOCH + Duration::new(1_790_219_400, 419_321_000);

    assert_eq!(
        parse_time(&json!("2026-09-24T03:10:00.419321+00:00")),
        Some(expected)
    );
    assert_eq!(
        parse_time(&json!(1_790_219_400)),
        Some(UNIX_EPOCH + Duration::from_secs(1_790_219_400))
    );
    assert_eq!(
        parse_time(&json!(1_790_219_400_000_i64)),
        Some(UNIX_EPOCH + Duration::from_secs(1_790_219_400))
    );
    assert_eq!(parse_time(&json!("not a date")), None);
    assert_eq!(parse_time(&json!(null)), None);
}

#[test]
fn epoch_time_rejects_negative_seconds() {
    assert_eq!(epoch_time(-1), None);
    assert_eq!(epoch_time(0), Some(UNIX_EPOCH));
}

#[test]
fn title_case_normalizes_words() {
    assert_eq!(title_case("team"), "Team");
    assert_eq!(title_case("MAX"), "Max");
    assert_eq!(title_case("self_serve_business"), "Self Serve Business");
}

#[tokio::test]
async fn command_code_links_to_the_usage_page() {
    let spec = agent_spec(AgentKind::CommandCode, &Config::default());

    assert_eq!(
        account_usage(AgentKind::CommandCode, &spec, false).await,
        AccountUsage::Link("https://commandcode.ai/usage".to_owned())
    );
}

#[tokio::test]
async fn antigravity_is_unsupported() {
    let spec = agent_spec(AgentKind::Antigravity, &Config::default());

    assert_eq!(
        account_usage(AgentKind::Antigravity, &spec, true).await,
        AccountUsage::Unavailable("Sem suporte".to_owned())
    );
}

#[tokio::test]
async fn opencode_without_consent_is_unavailable() {
    let spec = agent_spec(AgentKind::OpenCode, &Config::default());

    assert_eq!(
        account_usage(AgentKind::OpenCode, &spec, false).await,
        AccountUsage::Unavailable(
            "Autorize a leitura do login do OpenCode para ver o uso do Go".to_owned()
        )
    );
}

#[tokio::test]
#[ignore = "runs the installed claude and codex against the logged-in accounts"]
async fn smoke_reads_usage_from_the_installed_agents() {
    for kind in [AgentKind::ClaudeCode, AgentKind::Codex] {
        let spec = agent_spec(kind, &Config::default());
        let started = std::time::Instant::now();
        let usage = account_usage(kind, &spec, false).await;
        println!(
            "{kind:?} ({:.1}s): {usage:#?}",
            started.elapsed().as_secs_f32()
        );
    }
}

#[test]
fn duration_labels_are_unified_in_portuguese() {
    assert_eq!(duration_label(300), "5 horas");
    assert_eq!(duration_label(1_440), "Diário");
    assert_eq!(duration_label(10_080), "Semanal");
    assert_eq!(duration_label(43_200), "Mensal");
    assert_eq!(duration_label(60), "1 h");
    assert_eq!(duration_label(120), "2 h");
    assert_eq!(duration_label(45), "45 min");
    assert_eq!(duration_label(90), "90 min");
    assert_eq!(duration_label(0), "0 min");
}

#[test]
fn clamp_percent_keeps_values_in_range() {
    assert_eq!(clamp_percent(f64::NAN), 0.0);
    assert_eq!(clamp_percent(-1.0), 0.0);
    assert_eq!(clamp_percent(55.5), 55.5);
    assert_eq!(clamp_percent(101.0), 100.0);
    assert_eq!(clamp_percent(f64::INFINITY), 100.0);
}

#[test]
fn usage_window_clamps_before_computing_the_severity() {
    let window = usage_window("x".to_owned(), 130.0, None, None);

    assert_eq!(window.percent, 100.0);
    assert_eq!(window.severity, UsageSeverity::Blocked);
    assert_eq!(
        usage_window("x".to_owned(), f64::NAN, None, None).percent,
        0.0
    );
}

#[test]
fn epoch_millis_normalizes_seconds_and_milliseconds() {
    assert_eq!(epoch_millis(1_790_219_400), 1_790_219_400_000);
    assert_eq!(epoch_millis(1_790_219_400_000), 1_790_219_400_000);
}

#[test]
fn probe_ignores_the_spec_command_and_args_but_keeps_its_env() {
    let mut acp_spec = agent_spec(AgentKind::Codex, &Config::default());
    acp_spec.env.insert("KEY".to_owned(), "value".to_owned());

    let probe = probe_command(AgentKind::Codex, &acp_spec, &["app-server"]);

    assert_eq!(probe.command, std::path::Path::new("codex"));
    assert_eq!(probe.args, ["app-server"]);
    assert_eq!(probe.env["KEY"], "value");
}
