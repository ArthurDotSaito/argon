use std::time::Duration;

use argon_core::AgentKind;

use super::probe::capture;
use super::{AgentSpec, ModelInfo};

const LIST_TIMEOUT: Duration = Duration::from_secs(5);
const LIST_ATTEMPTS: usize = 2;
const OPENCODE_ARGS: &[&str] = &["models"];
const COMMAND_CODE_ARGS: &[&str] = &["--list-models"];
const COMMAND_CODE_FOOTER: [&str; 2] = ["Pass the full id", "Decision models"];

pub fn listing_args(kind: AgentKind) -> Option<&'static [&'static str]> {
    match kind {
        AgentKind::OpenCode => Some(OPENCODE_ARGS),
        AgentKind::CommandCode => Some(COMMAND_CODE_ARGS),
        AgentKind::ClaudeCode | AgentKind::Codex | AgentKind::Antigravity => None,
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

pub fn parse_opencode_models(text: &str) -> Vec<ModelInfo> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.contains('/') && is_model_id(line))
        .map(model)
        .collect()
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

fn parse(kind: AgentKind, text: &str) -> Vec<ModelInfo> {
    match kind {
        AgentKind::CommandCode => parse_command_code_models(text),
        _ => parse_opencode_models(text),
    }
}

pub async fn list_models(spec: &AgentSpec) -> Option<Result<Vec<ModelInfo>, String>> {
    let args = listing_args(spec.kind)?;
    let mut last_error = String::new();
    for _ in 0..LIST_ATTEMPTS {
        match capture(&spec.command, args, LIST_TIMEOUT).await {
            Ok(captured) if captured.success => return Some(Ok(parse(spec.kind, &captured.text))),
            Ok(captured) => last_error = captured.text.trim().to_owned(),
            Err(error) => last_error = error,
        }
    }
    Some(Err(last_error))
}

pub fn apply_listed_models(spec: &mut AgentSpec, models: Vec<ModelInfo>) {
    if !models.iter().any(|model| model.id == spec.default_model) {
        spec.default_model = models
            .first()
            .map(|model| model.id.clone())
            .unwrap_or_default();
    }
    spec.models = models;
}

#[cfg(test)]
mod tests {
    use argon_core::Config;

    use super::*;
    use crate::registry::agent_spec;

    const OPENCODE_OUTPUT: &str = "claude-agent/claude-opus-5\ncommandcode/deepseek/deepseek-v4-pro\n\
                                   opencode-go/glm-5.3\n\nServiceUnavailableError: oops\n";
    const COMMAND_CODE_OUTPUT: &str = "Available models  · 3 models\n\nOpen Source\n\n\
        deepseek/deepseek-v4-flash             fast hybrid-attention reasoning (default)\n\
        reflection/beam:free                   FREE reasoning model\n\nAnthropic\n\n\
        claude-sonnet-5-5                      best combo of speed & intelligence (recommended)\n\n\
        Pass the full id, or just the short name after the last \"/\":\n\
        cmdc --model moonshotai/kimi-k2.5\n\nDecision models (headless only)\n\
        typesafe/jev  typed questions in\n";

    #[test]
    fn parses_opencode_model_list() {
        let ids: Vec<String> = parse_opencode_models(OPENCODE_OUTPUT)
            .into_iter()
            .map(|model| model.id)
            .collect();
        assert_eq!(
            ids,
            [
                "claude-agent/claude-opus-5",
                "commandcode/deepseek/deepseek-v4-pro",
                "opencode-go/glm-5.3"
            ]
        );
    }

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
    fn only_opencode_and_command_code_list_models() {
        assert!(listing_args(AgentKind::OpenCode).is_some());
        assert!(listing_args(AgentKind::CommandCode).is_some());
        assert!(listing_args(AgentKind::ClaudeCode).is_none());
    }

    #[test]
    fn listed_models_keep_configured_default_or_pick_first() {
        let mut spec = agent_spec(AgentKind::OpenCode, &Config::default());
        spec.default_model = "opencode-go/glm-5.3".into();
        apply_listed_models(&mut spec, parse_opencode_models(OPENCODE_OUTPUT));
        assert_eq!(spec.default_model, "opencode-go/glm-5.3");
        spec.default_model = "missing/model".into();
        apply_listed_models(&mut spec, parse_opencode_models(OPENCODE_OUTPUT));
        assert_eq!(spec.default_model, "claude-agent/claude-opus-5");
        apply_listed_models(&mut spec, Vec::new());
        assert!(spec.default_model.is_empty());
        assert!(spec.models.is_empty());
    }
}
