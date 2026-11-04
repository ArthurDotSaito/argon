use std::time::Duration;

use argon_core::{AgentKind, Config};

use super::probe::capture_with_env;
use super::spec::agent_spec;
use super::{AgentSpec, ModelInfo};
use crate::defaults::Transport;
use crate::util::{Resolved, resolve_command};
use crate::{codex, opencode};

const COMMAND_CODE_TIMEOUT: Duration = Duration::from_secs(30);
const COMMAND_CODE_ATTEMPTS: usize = 2;
const COMMAND_CODE_ARGS: &[&str] = &["--list-models", "--no-auto-update"];
const COMMAND_CODE_FOOTER: [&str; 2] = ["Pass the full id", "Decision models"];

pub fn lists_models(kind: AgentKind) -> bool {
    matches!(
        kind,
        AgentKind::OpenCode | AgentKind::CommandCode | AgentKind::Codex
    )
}

pub fn spec_lists_models(spec: &AgentSpec) -> bool {
    match spec.kind {
        AgentKind::Codex => spec.transport == Transport::CodexAppServer,
        kind => lists_models(kind),
    }
}

pub fn listing_spec(kind: AgentKind, cfg: &Config) -> Option<AgentSpec> {
    let mut spec = agent_spec(kind, cfg);
    if !spec_lists_models(&spec) {
        return None;
    }
    match resolve_command(&spec.command) {
        Resolved::Path(path) => {
            spec.command = path;
            Some(spec)
        }
        Resolved::Missing | Resolved::Failed(_) => None,
    }
}

fn is_model_id(id: &str) -> bool {
    id.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "./:_-[]".contains(ch))
}

fn model(id: &str) -> ModelInfo {
    ModelInfo {
        id: id.to_owned(),
        label: id.to_owned(),
    }
}

pub fn parse_command_code_models(text: &str) -> Vec<ModelInfo> {
    text.lines()
        .take_while(|line| {
            !COMMAND_CODE_FOOTER
                .iter()
                .any(|footer| line.trim_start().starts_with(footer))
        })
        .filter_map(|line| line.trim().split_once("  "))
        .map(|(id, _)| id.trim())
        .filter(|id| is_model_id(id))
        .map(model)
        .collect()
}

pub async fn list_models(spec: &AgentSpec) -> Option<Result<Vec<ModelInfo>, String>> {
    match spec.kind {
        AgentKind::OpenCode => Some(opencode::list_models(spec).await),
        AgentKind::CommandCode => Some(list_command_code_models(spec).await),
        AgentKind::Codex if spec_lists_models(spec) => Some(codex::driver::list_models(spec).await),
        AgentKind::ClaudeCode | AgentKind::Codex | AgentKind::Antigravity => None,
    }
}

async fn list_command_code_models(spec: &AgentSpec) -> Result<Vec<ModelInfo>, String> {
    let mut last_error = String::new();
    for _ in 0..COMMAND_CODE_ATTEMPTS {
        match capture_with_env(
            &spec.command,
            COMMAND_CODE_ARGS,
            &spec.env,
            COMMAND_CODE_TIMEOUT,
        )
        .await
        {
            Ok(captured) if captured.success => {
                return Ok(parse_command_code_models(&captured.text));
            }
            Ok(captured) => last_error = captured.text.trim().to_owned(),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMAND_CODE_OUTPUT: &str = "Available models  · 3 models\n\nOpen Source\n\n\
        deepseek/deepseek-v4-flash             fast hybrid-attention reasoning (default)\n\
        reflection/beam:free                   FREE reasoning model\n\nAnthropic\n\n\
        claude-sonnet-5-5                      best combo of speed & intelligence (recommended)\n\n\
        Pass the full id, or just the short name after the last \"/\":\n\
        cmdc --model moonshotai/kimi-k2.5\n\nDecision models (headless only)\n\
        typesafe/jev  typed questions in\n";

    #[test]
    fn parses_command_code_model_table() {
        let ids: Vec<String> = parse_command_code_models(COMMAND_CODE_OUTPUT)
            .into_iter()
            .map(|model| model.id)
            .collect();
        assert_eq!(
            ids,
            [
                "deepseek/deepseek-v4-flash",
                "reflection/beam:free",
                "claude-sonnet-5-5"
            ]
        );
    }

    #[test]
    fn opencode_command_code_and_codex_list_models() {
        assert!(lists_models(AgentKind::OpenCode));
        assert!(lists_models(AgentKind::CommandCode));
        assert!(lists_models(AgentKind::Codex));
        assert!(!lists_models(AgentKind::ClaudeCode));
        assert!(!lists_models(AgentKind::Antigravity));
    }

    fn codex_on(transport: Transport) -> AgentSpec {
        let mut spec = agent_spec(AgentKind::Codex, &Config::default());
        spec.transport = transport;
        spec
    }

    #[test]
    fn codex_lists_models_only_through_the_app_server() {
        assert!(spec_lists_models(&codex_on(Transport::CodexAppServer)));
        assert!(!spec_lists_models(&codex_on(Transport::Acp)));
        for kind in [AgentKind::OpenCode, AgentKind::CommandCode] {
            assert!(spec_lists_models(&agent_spec(kind, &Config::default())));
        }
        assert!(!spec_lists_models(&agent_spec(
            AgentKind::ClaudeCode,
            &Config::default()
        )));
    }

    #[test]
    fn listing_spec_is_only_offered_to_listing_agents() {
        assert!(listing_spec(AgentKind::ClaudeCode, &Config::default()).is_none());
        let codex = agent_spec(AgentKind::Codex, &Config::default());
        if codex.transport != Transport::CodexAppServer {
            assert!(listing_spec(AgentKind::Codex, &Config::default()).is_none());
        }
    }

    #[tokio::test]
    async fn agents_without_listing_return_none() {
        let spec = agent_spec(AgentKind::ClaudeCode, &Config::default());
        assert!(list_models(&spec).await.is_none());
        assert!(list_models(&codex_on(Transport::Acp)).await.is_none());
    }

    #[tokio::test]
    async fn codex_listing_reports_a_missing_command() {
        let mut spec = codex_on(Transport::CodexAppServer);
        spec.command = std::path::PathBuf::from("argon-missing-codex-binary");
        let error = list_models(&spec).await.unwrap().unwrap_err();
        assert!(error.contains("argon-missing-codex-binary"), "{error}");
    }

    #[tokio::test]
    #[ignore = "runs the installed codex app-server (initialize + model/list only)"]
    async fn smoke_codex_listing_against_the_installed_cli() {
        let mut spec = codex_on(Transport::CodexAppServer);
        spec.command = std::path::PathBuf::from("codex");
        spec.args.clear();
        let started = std::time::Instant::now();
        let models = list_models(&spec).await.unwrap().unwrap();
        println!(
            "Codex: {} models in {:.1}s: {:?}",
            models.len(),
            started.elapsed().as_secs_f32(),
            models
                .iter()
                .map(|model| format!("{} ({})", model.id, model.label))
                .collect::<Vec<_>>()
        );
        assert!(!models.is_empty());
    }

    #[tokio::test]
    #[ignore = "runs the installed opencode and command-code listings"]
    async fn smoke_listings_against_the_installed_agents() {
        for kind in [AgentKind::OpenCode, AgentKind::CommandCode] {
            let spec = listing_spec(kind, &Config::default()).expect("agent not installed");
            let started = std::time::Instant::now();
            let models = list_models(&spec).await.unwrap().unwrap();
            let opencode_go = models
                .iter()
                .filter(|model| model.id.starts_with("opencode-go/"))
                .count();
            println!(
                "{kind:?}: {} models, {opencode_go} opencode-go, {:.1}s",
                models.len(),
                started.elapsed().as_secs_f32()
            );
            assert!(!models.is_empty());
        }
    }
}
