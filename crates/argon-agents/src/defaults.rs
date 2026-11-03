use argon_core::AgentKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Acp,
    Headless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSelection {
    AcpConfigOption { config_id: &'static str },
    CliFlag { flag: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownModel {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadlessFlags {
    pub output: &'static [&'static str],
    pub prompt: &'static str,
    pub resume: &'static str,
    pub mode: &'static str,
    pub skip_permissions: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentDefaults {
    pub kind: AgentKind,
    pub label: &'static str,
    pub transport: Transport,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub env: &'static [(&'static str, &'static str)],
    pub cli_command: &'static str,
    pub version_args: &'static [&'static str],
    pub install_hint: &'static str,
    pub login_command: &'static str,
    pub login_check: &'static str,
    pub model_selection: ModelSelection,
    pub models_from_session: bool,
    pub models: &'static [KnownModel],
    pub default_model: &'static str,
    pub permission_mode: Option<&'static str>,
    pub load_session: bool,
    pub headless: Option<HeadlessFlags>,
    pub tested_version: &'static str,
    pub turn_verified: bool,
}

const MODEL_OPTION: ModelSelection = ModelSelection::AcpConfigOption { config_id: "model" };

const CLAUDE_MODELS: &[KnownModel] = &[
    KnownModel {
        id: "default",
        label: "Default (Opus 5.5)",
    },
    KnownModel {
        id: "opus",
        label: "Opus 5.5",
    },
    KnownModel {
        id: "fable",
        label: "Fable 5.1",
    },
    KnownModel {
        id: "sonnet",
        label: "Sonnet 5.5",
    },
    KnownModel {
        id: "haiku",
        label: "Haiku 5.5",
    },
    KnownModel {
        id: "claude-haiku-4-5-20251001",
        label: "Haiku 4.5",
    },
    KnownModel {
        id: "claude-sonnet-5",
        label: "Sonnet 5",
    },
    KnownModel {
        id: "claude-opus-5",
        label: "Opus 5",
    },
    KnownModel {
        id: "claude-fable-5",
        label: "Fable 5",
    },
    KnownModel {
        id: "claude-opus-4-8",
        label: "Opus 4.8",
    },
    KnownModel {
        id: "claude-opus-4-7",
        label: "Opus 4.7",
    },
    KnownModel {
        id: "claude-opus-4-6",
        label: "Opus 4.6",
    },
    KnownModel {
        id: "claude-sonnet-4-6",
        label: "Sonnet 4.6",
    },
    KnownModel {
        id: "opusplan",
        label: "Opus Plan Mode",
    },
];

const COMMAND_CODE_MODELS: &[KnownModel] = &[
    KnownModel {
        id: "claude-sonnet-5-5",
        label: "Claude Sonnet 5.5",
    },
    KnownModel {
        id: "claude-opus-5-5",
        label: "Claude Opus 5.5",
    },
    KnownModel {
        id: "claude-fable-5-1",
        label: "Claude Fable 5.1",
    },
    KnownModel {
        id: "claude-haiku-5-5",
        label: "Claude Haiku 5.5",
    },
    KnownModel {
        id: "gpt-6-sol",
        label: "GPT-6 Sol",
    },
    KnownModel {
        id: "gpt-5.3-codex",
        label: "GPT-5.3 Codex",
    },
    KnownModel {
        id: "google/gemini-4-argon",
        label: "Gemini 4 Argon",
    },
    KnownModel {
        id: "deepseek/deepseek-v4-flash",
        label: "DeepSeek V4 Flash",
    },
    KnownModel {
        id: "deepseek/deepseek-v4-pro",
        label: "DeepSeek V4 Pro",
    },
    KnownModel {
        id: "z-ai/glm-5.3-flash",
        label: "GLM 5.3 Flash",
    },
    KnownModel {
        id: "moonshotai/kimi-k3.1-pro",
        label: "Kimi K3.1 Pro",
    },
];

const ANTIGRAVITY_MODELS: &[KnownModel] = &[
    KnownModel {
        id: "gemini-4-argon-medium",
        label: "Gemini 4 Argon (Medium)",
    },
    KnownModel {
        id: "gemini-4-argon-high",
        label: "Gemini 4 Argon (High)",
    },
    KnownModel {
        id: "gemini-4-argon-low",
        label: "Gemini 4 Argon (Low)",
    },
    KnownModel {
        id: "gemini-3.8-flash-high",
        label: "Gemini 3.8 Flash (High)",
    },
    KnownModel {
        id: "gemini-3.8-flash-medium",
        label: "Gemini 3.8 Flash (Medium)",
    },
    KnownModel {
        id: "gemini-3.8-flash-low",
        label: "Gemini 3.8 Flash (Low)",
    },
    KnownModel {
        id: "claude-opus-5-5-low",
        label: "Claude Opus 5.5 (Low)",
    },
    KnownModel {
        id: "claude-opus-5-5-medium",
        label: "Claude Opus 5.5 (Medium)",
    },
    KnownModel {
        id: "claude-opus-5-5-high",
        label: "Claude Opus 5.5 (High)",
    },
    KnownModel {
        id: "claude-sonnet-5-5-low",
        label: "Claude Sonnet 5.5 (Low)",
    },
    KnownModel {
        id: "claude-sonnet-5-5-medium",
        label: "Claude Sonnet 5.5 (Medium)",
    },
    KnownModel {
        id: "claude-sonnet-5-5-high",
        label: "Claude Sonnet 5.5 (High)",
    },
    KnownModel {
        id: "gpt-oss-120b-medium",
        label: "GPT-OSS 120B (Medium)",
    },
];

pub const AGENT_DEFAULTS: [AgentDefaults; 5] = [
    AgentDefaults {
        kind: AgentKind::ClaudeCode,
        label: "Claude Code",
        transport: Transport::Acp,
        command: "npx",
        args: &["-y", "@agentclientprotocol/claude-agent-acp@0.100.0"],
        env: &[],
        cli_command: "claude",
        version_args: &["--version"],
        install_hint: "Claude Code native installer (claude.exe in %USERPROFILE%\\.local\\bin); adapter runs through npx",
        login_command: "claude auth login",
        login_check: "claude auth status",
        model_selection: MODEL_OPTION,
        models_from_session: true,
        models: CLAUDE_MODELS,
        default_model: "default",
        permission_mode: Some("default"),
        load_session: true,
        headless: None,
        tested_version: "claude-agent-acp 0.100.0 / claude 2.1.316",
        turn_verified: true,
    },
    AgentDefaults {
        kind: AgentKind::Codex,
        label: "Codex",
        transport: Transport::Acp,
        command: "npx",
        args: &["-y", "@agentclientprotocol/codex-acp@2.5.0"],
        env: &[],
        cli_command: "codex",
        version_args: &["--version"],
        install_hint: "npm i -g @openai/codex; adapter runs through npx and bundles its own codex",
        login_command: "codex login",
        login_check: "codex login status",
        model_selection: MODEL_OPTION,
        models_from_session: true,
        models: &[],
        default_model: "",
        permission_mode: None,
        load_session: true,
        headless: None,
        tested_version: "codex-acp 2.5.0 / codex 0.160.0",
        turn_verified: false,
    },
    AgentDefaults {
        kind: AgentKind::OpenCode,
        label: "OpenCode",
        transport: Transport::Acp,
        command: "opencode",
        args: &["acp"],
        env: &[],
        cli_command: "opencode",
        version_args: &["--version"],
        install_hint: "npm i -g @opencode/cli",
        login_command: "opencode auth login",
        login_check: "opencode auth list",
        model_selection: MODEL_OPTION,
        models_from_session: true,
        models: &[],
        default_model: "",
        permission_mode: None,
        load_session: true,
        headless: None,
        tested_version: "opencode 2.2.5",
        turn_verified: true,
    },
    AgentDefaults {
        kind: AgentKind::CommandCode,
        label: "Command Code",
        transport: Transport::Acp,
        command: "command-code",
        args: &["acp"],
        env: &[],
        cli_command: "command-code",
        version_args: &["--version"],
        install_hint: "npm i -g command-code",
        login_command: "command-code login",
        login_check: "command-code status",
        model_selection: MODEL_OPTION,
        models_from_session: true,
        models: COMMAND_CODE_MODELS,
        default_model: "",
        permission_mode: Some("default"),
        load_session: true,
        headless: None,
        tested_version: "command-code 1.88.2",
        turn_verified: false,
    },
    AgentDefaults {
        kind: AgentKind::Antigravity,
        label: "Antigravity",
        transport: Transport::Headless,
        command: "agy",
        args: &["--output-format", "stream-json", "--print"],
        env: &[],
        cli_command: "agy",
        version_args: &["--version"],
        install_hint: "Antigravity CLI installer (agy.exe in %LOCALAPPDATA%\\agy\\bin)",
        login_command: "agy",
        login_check: "agy models",
        model_selection: ModelSelection::CliFlag { flag: "--model" },
        models_from_session: false,
        models: ANTIGRAVITY_MODELS,
        default_model: "",
        permission_mode: None,
        load_session: true,
        headless: Some(HeadlessFlags {
            output: &["--output-format", "stream-json"],
            prompt: "--print",
            resume: "--conversation",
            mode: "--mode",
            skip_permissions: "--dangerously-skip-permissions",
        }),
        tested_version: "agy 1.4.7",
        turn_verified: false,
    },
];

pub fn agent_defaults(kind: AgentKind) -> &'static AgentDefaults {
    let index = match kind {
        AgentKind::ClaudeCode => 0,
        AgentKind::Codex => 1,
        AgentKind::OpenCode => 2,
        AgentKind::CommandCode => 3,
        AgentKind::Antigravity => 4,
    };
    &AGENT_DEFAULTS[index]
}
