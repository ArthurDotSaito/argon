use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use argon_core::{AgentCfg, AgentKind, Config};

use crate::defaults::{AgentDefaults, Launch, ModelSelection, Transport, agent_defaults};

const ANTIGRAVITY_REASON: &str =
    "Ainda não suportado — o modo headless exige liberar ferramentas sem confirmação";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSpec {
    pub kind: AgentKind,
    pub label: &'static str,
    pub transport: Transport,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub models: Vec<ModelInfo>,
    pub default_model: String,
    pub model_selection: ModelSelection,
    pub supports_load: bool,
    pub requires_login_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Support {
    Supported,
    NotYet { reason: String },
}

pub fn support_for(kind: AgentKind) -> Support {
    match kind {
        AgentKind::Antigravity => Support::NotYet {
            reason: ANTIGRAVITY_REASON.to_string(),
        },
        AgentKind::ClaudeCode | AgentKind::Codex | AgentKind::OpenCode | AgentKind::CommandCode => {
            Support::Supported
        }
    }
}

pub fn agent_spec(kind: AgentKind, cfg: &Config) -> AgentSpec {
    spec_from_defaults(agent_defaults(kind), cfg)
}

pub fn agent_spec_for(kind: AgentKind, cfg: &Config, transport: Transport) -> Option<AgentSpec> {
    spec_for_transport(agent_defaults(kind), cfg, transport)
}

pub(super) fn spec_from_defaults(defaults: &AgentDefaults, cfg: &Config) -> AgentSpec {
    build_spec(defaults, effective_launch(defaults, cfg), cfg)
}

pub(super) fn effective_launch(defaults: &AgentDefaults, cfg: &Config) -> Launch {
    let use_acp = cfg
        .agents
        .get(&defaults.kind)
        .is_some_and(|agent| agent.use_acp);
    match defaults.acp_fallback {
        Some(fallback) if use_acp => fallback,
        _ => defaults.launch,
    }
}

fn spec_for_transport(
    defaults: &AgentDefaults,
    cfg: &Config,
    transport: Transport,
) -> Option<AgentSpec> {
    [Some(defaults.launch), defaults.acp_fallback]
        .into_iter()
        .flatten()
        .find(|launch| launch.transport == transport)
        .map(|launch| build_spec(defaults, launch, cfg))
}

fn build_spec(defaults: &AgentDefaults, launch: Launch, cfg: &Config) -> AgentSpec {
    let kind = defaults.kind;
    let overrides = cfg.agents.get(&kind);
    AgentSpec {
        kind,
        label: defaults.label,
        transport: launch.transport,
        command: command_for(kind, launch, overrides),
        args: args_for(kind, launch, overrides),
        env: env_for(defaults, overrides),
        models: defaults
            .models
            .iter()
            .map(|model| ModelInfo {
                id: model.id.to_string(),
                label: model.label.to_string(),
            })
            .collect(),
        default_model: overrides
            .and_then(|agent| agent.default_model.clone())
            .unwrap_or_else(|| defaults.default_model.to_string()),
        model_selection: defaults.model_selection,
        supports_load: defaults.load_session,
        requires_login_hint: requires_login(kind).then(|| login_hint(defaults)),
    }
}

pub(crate) fn login_hint(defaults: &AgentDefaults) -> String {
    format!(
        "Execute `{}` no terminal para entrar.",
        defaults.login_command
    )
}

pub(super) fn is_command_overridden(kind: AgentKind, cfg: &Config) -> bool {
    command_override(kind, cfg.agents.get(&kind)).is_some()
}

fn requires_login(kind: AgentKind) -> bool {
    matches!(kind, AgentKind::Codex | AgentKind::CommandCode)
}

fn is_system_cmd(command: &Path) -> bool {
    command
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.eq_ignore_ascii_case("cmd"))
}

fn command_override(kind: AgentKind, overrides: Option<&AgentCfg>) -> Option<PathBuf> {
    let command = overrides.and_then(|agent| agent.command.clone())?;
    if kind == AgentKind::CommandCode && is_system_cmd(&command) {
        tracing::warn!(
            command = %command.display(),
            "ignoring Command Code override that points to cmd.exe"
        );
        return None;
    }
    Some(command)
}

fn command_for(kind: AgentKind, launch: Launch, overrides: Option<&AgentCfg>) -> PathBuf {
    command_override(kind, overrides).unwrap_or_else(|| PathBuf::from(launch.command))
}

fn args_for(kind: AgentKind, launch: Launch, overrides: Option<&AgentCfg>) -> Vec<String> {
    match overrides {
        Some(agent) if command_override(kind, overrides).is_some() || !agent.args.is_empty() => {
            agent.args.clone()
        }
        _ => launch.args.iter().map(ToString::to_string).collect(),
    }
}

fn env_for(defaults: &AgentDefaults, overrides: Option<&AgentCfg>) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = defaults
        .env
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    if let Some(agent) = overrides {
        env.extend(agent.env.clone());
    }
    env
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::defaults::AGENT_DEFAULTS;

    fn config_with(kind: AgentKind, agent: AgentCfg) -> Config {
        Config {
            agents: BTreeMap::from([(kind, agent)]),
            ..Config::default()
        }
    }

    #[test]
    fn config_overrides_defaults() {
        let cfg = config_with(
            AgentKind::ClaudeCode,
            AgentCfg {
                command: Some(PathBuf::from("claude-agent-acp")),
                args: vec!["--verbose".into()],
                env: BTreeMap::from([("ANTHROPIC_LOG".into(), "debug".into())]),
                default_model: Some("opus".into()),
                use_acp: false,
                ..AgentCfg::default()
            },
        );
        let spec = agent_spec(AgentKind::ClaudeCode, &cfg);
        assert_eq!(spec.command, Path::new("claude-agent-acp"));
        assert_eq!(spec.args, ["--verbose"]);
        assert_eq!(spec.env["ANTHROPIC_LOG"], "debug");
        assert_eq!(spec.default_model, "opus");
        assert_eq!(spec.transport, Transport::Acp);
    }

    #[test]
    fn empty_config_keeps_defaults() {
        let spec = agent_spec(AgentKind::ClaudeCode, &Config::default());
        let defaults = agent_defaults(AgentKind::ClaudeCode);
        assert_eq!(spec.command, Path::new(defaults.launch.command));
        assert_eq!(spec.args, defaults.launch.args);
        assert_eq!(spec.default_model, defaults.default_model);
        assert_eq!(spec.models.len(), defaults.models.len());
        assert!(
            spec.models
                .iter()
                .any(|model| model.id == spec.default_model)
        );
        assert!(spec.supports_load);
    }

    #[test]
    fn default_agent_cfg_changes_nothing() {
        let cfg = config_with(AgentKind::Codex, AgentCfg::default());
        assert_eq!(
            agent_spec(AgentKind::Codex, &cfg),
            agent_spec(AgentKind::Codex, &Config::default())
        );
    }

    #[test]
    fn args_only_override_keeps_default_command() {
        let cfg = config_with(
            AgentKind::OpenCode,
            AgentCfg {
                args: vec!["acp".into(), "--port=0".into()],
                ..AgentCfg::default()
            },
        );
        let spec = agent_spec(AgentKind::OpenCode, &cfg);
        assert_eq!(spec.command, Path::new("opencode"));
        assert_eq!(spec.args, ["acp", "--port=0"]);
    }

    #[test]
    fn command_override_drops_default_args() {
        let cfg = config_with(
            AgentKind::Codex,
            AgentCfg {
                command: Some(PathBuf::from("codex-acp")),
                ..AgentCfg::default()
            },
        );
        let spec = agent_spec(AgentKind::Codex, &cfg);
        assert_eq!(spec.command, Path::new("codex-acp"));
        assert!(spec.args.is_empty());
        assert!(is_command_overridden(AgentKind::Codex, &cfg));
        assert!(!is_command_overridden(AgentKind::Codex, &Config::default()));
    }

    #[test]
    fn config_only_affects_its_own_agent() {
        let cfg = config_with(
            AgentKind::ClaudeCode,
            AgentCfg {
                default_model: Some("opus".into()),
                ..AgentCfg::default()
            },
        );
        assert_eq!(
            agent_spec(AgentKind::OpenCode, &cfg),
            agent_spec(AgentKind::OpenCode, &Config::default())
        );
    }

    #[test]
    fn antigravity_is_never_supported() {
        assert!(matches!(
            support_for(AgentKind::Antigravity),
            Support::NotYet { .. }
        ));
        for kind in [
            AgentKind::ClaudeCode,
            AgentKind::Codex,
            AgentKind::OpenCode,
            AgentKind::CommandCode,
        ] {
            assert_eq!(support_for(kind), Support::Supported);
        }
    }

    #[test]
    fn command_code_never_resolves_to_cmd() {
        let spec = agent_spec(AgentKind::CommandCode, &Config::default());
        assert_eq!(spec.command, Path::new("command-code"));
        for defaults in &AGENT_DEFAULTS {
            let spec = agent_spec(defaults.kind, &Config::default());
            assert_ne!(
                spec.command.file_stem().and_then(|stem| stem.to_str()),
                Some("cmd"),
                "{:?}",
                defaults.kind
            );
        }
    }

    #[test]
    fn command_code_override_to_cmd_is_ignored() {
        for command in ["cmd", "cmd.exe", r"C:\Windows\System32\CMD.EXE"] {
            let cfg = config_with(
                AgentKind::CommandCode,
                AgentCfg {
                    command: Some(PathBuf::from(command)),
                    args: vec!["acp".into()],
                    ..AgentCfg::default()
                },
            );
            let spec = agent_spec(AgentKind::CommandCode, &cfg);
            assert_eq!(spec.command, Path::new("command-code"), "{command}");
            assert_eq!(spec.args, ["acp"]);
            assert!(!is_command_overridden(AgentKind::CommandCode, &cfg));
        }
        let cfg = config_with(
            AgentKind::OpenCode,
            AgentCfg {
                command: Some(PathBuf::from("cmd")),
                ..AgentCfg::default()
            },
        );
        assert_eq!(
            agent_spec(AgentKind::OpenCode, &cfg).command,
            Path::new("cmd")
        );
    }

    const NATIVE_LAUNCH: Launch = Launch {
        transport: Transport::ClaudeStream,
        command: "claude",
        args: &["--stream"],
        warmup_args: None,
    };
    const ACP_LAUNCH: Launch = Launch {
        transport: Transport::Acp,
        command: "npx",
        args: &["-y", "adapter"],
        warmup_args: None,
    };

    fn defaults_with(launch: Launch, acp_fallback: Option<Launch>) -> AgentDefaults {
        AgentDefaults {
            launch,
            acp_fallback,
            ..*agent_defaults(AgentKind::ClaudeCode)
        }
    }

    fn use_acp_config(use_acp: bool) -> Config {
        config_with(
            AgentKind::ClaudeCode,
            AgentCfg {
                use_acp,
                ..AgentCfg::default()
            },
        )
    }

    #[test]
    fn default_config_resolves_the_default_launch() {
        let defaults = defaults_with(NATIVE_LAUNCH, Some(ACP_LAUNCH));
        let spec = spec_from_defaults(&defaults, &use_acp_config(false));
        assert_eq!(spec.transport, Transport::ClaudeStream);
        assert_eq!(spec.command, Path::new("claude"));
        assert_eq!(spec.args, ["--stream"]);
    }

    #[test]
    fn use_acp_without_fallback_keeps_the_default_launch() {
        let defaults = defaults_with(NATIVE_LAUNCH, None);
        let spec = spec_from_defaults(&defaults, &use_acp_config(true));
        assert_eq!(spec.transport, Transport::ClaudeStream);
        assert_eq!(spec.command, Path::new("claude"));
        assert_eq!(spec.args, ["--stream"]);
    }

    #[test]
    fn use_acp_with_fallback_resolves_the_fallback_launch() {
        let defaults = defaults_with(NATIVE_LAUNCH, Some(ACP_LAUNCH));
        let spec = spec_from_defaults(&defaults, &use_acp_config(true));
        assert_eq!(spec.transport, Transport::Acp);
        assert_eq!(spec.command, Path::new("npx"));
        assert_eq!(spec.args, ["-y", "adapter"]);
    }

    #[test]
    fn overrides_apply_on_top_of_the_fallback_launch() {
        let defaults = defaults_with(NATIVE_LAUNCH, Some(ACP_LAUNCH));
        let cfg = config_with(
            AgentKind::ClaudeCode,
            AgentCfg {
                use_acp: true,
                args: vec!["--verbose".into()],
                env: BTreeMap::from([("KEY".into(), "value".into())]),
                ..AgentCfg::default()
            },
        );
        let spec = spec_from_defaults(&defaults, &cfg);
        assert_eq!(spec.transport, Transport::Acp);
        assert_eq!(spec.command, Path::new("npx"));
        assert_eq!(spec.args, ["--verbose"]);
        assert_eq!(spec.env["KEY"], "value");
    }

    #[test]
    fn spec_for_transport_picks_the_matching_launch() {
        let defaults = defaults_with(NATIVE_LAUNCH, Some(ACP_LAUNCH));
        let cfg = use_acp_config(false);
        let native = spec_for_transport(&defaults, &cfg, Transport::ClaudeStream).unwrap();
        assert_eq!(native.command, Path::new("claude"));
        let acp = spec_for_transport(&defaults, &cfg, Transport::Acp).unwrap();
        assert_eq!(acp.transport, Transport::Acp);
        assert_eq!(acp.command, Path::new("npx"));
        assert_eq!(acp.args, ["-y", "adapter"]);
    }

    #[test]
    fn spec_for_transport_is_none_without_a_matching_launch() {
        let defaults = defaults_with(NATIVE_LAUNCH, None);
        let cfg = use_acp_config(true);
        assert_eq!(spec_for_transport(&defaults, &cfg, Transport::Acp), None);
        assert_eq!(
            spec_for_transport(&defaults, &cfg, Transport::CodexAppServer),
            None
        );
    }

    #[test]
    fn agent_spec_for_matches_the_current_acp_defaults() {
        let cfg = Config::default();
        for defaults in &AGENT_DEFAULTS {
            let kind = defaults.kind;
            assert_eq!(
                agent_spec_for(kind, &cfg, defaults.launch.transport),
                Some(agent_spec(kind, &cfg)),
                "{kind:?}"
            );
            assert_eq!(
                agent_spec_for(kind, &cfg, Transport::OpencodeServe),
                None,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn login_hint_only_for_agents_that_require_it() {
        let hint = |kind| agent_spec(kind, &Config::default()).requires_login_hint;
        assert!(hint(AgentKind::Codex).unwrap().contains("codex login"));
        assert!(
            hint(AgentKind::CommandCode)
                .unwrap()
                .contains("command-code login")
        );
        assert!(hint(AgentKind::OpenCode).is_none());
        assert!(hint(AgentKind::ClaudeCode).is_none());
    }
}
