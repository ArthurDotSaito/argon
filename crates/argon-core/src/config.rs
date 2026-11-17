use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

use crate::TemplateKind;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to access config file {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid config file {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("failed to serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),
}

static SAVE_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub type Result<T> = std::result::Result<T, ConfigError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AgentKind {
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
    #[serde(rename = "command-code")]
    CommandCode,
    #[serde(rename = "antigravity")]
    Antigravity,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    #[default]
    Supervised,
    AcceptEdits,
    FullAccess,
}

impl PermissionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supervised => "supervised",
            Self::AcceptEdits => "accept-edits",
            Self::FullAccess => "full-access",
        }
    }

    pub fn parse(value: &str) -> Option<PermissionMode> {
        match value {
            "supervised" => Some(Self::Supervised),
            "accept-edits" => Some(Self::AcceptEdits),
            "full-access" => Some(Self::FullAccess),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeChoice {
    #[default]
    OneDark,
    OneLight,
    System,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Density {
    #[default]
    Compact,
    Comfortable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncCfg {
    pub interval_secs: u64,
    pub closed_window_days: i64,
}

impl Default for SyncCfg {
    fn default() -> Self {
        Self {
            interval_secs: 120,
            closed_window_days: 7,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OrgsCfg {
    pub tracked: Vec<String>,
    pub initialized: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentCfg {
    pub command: Option<PathBuf>,
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_transport: Option<String>,
    pub env: BTreeMap<String, String>,
    pub default_model: Option<String>,
    #[serde(
        deserialize_with = "lenient_enum",
        skip_serializing_if = "Option::is_none"
    )]
    pub permission_mode: Option<PermissionMode>,
    #[serde(skip_serializing_if = "is_false")]
    pub full_access: bool,
    #[serde(default)]
    pub use_acp: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub read_plan_usage: bool,
}

impl AgentCfg {
    pub fn permission_mode(&self) -> PermissionMode {
        match self.permission_mode {
            Some(mode) => mode,
            None if self.full_access => PermissionMode::FullAccess,
            None => PermissionMode::Supervised,
        }
    }

    pub fn with_permission_mode(mut self, mode: PermissionMode) -> AgentCfg {
        self.permission_mode = Some(mode);
        self.full_access = false;
        self
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewCfg {
    pub default_template: String,
    pub default_plan_template: String,
    pub max_diff_chars: usize,
    pub use_worktree: bool,
    pub include_previous_reviews: bool,
    pub include_ci_failures: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub repo_templates: BTreeMap<String, String>,
}

impl Default for ReviewCfg {
    fn default() -> Self {
        Self {
            default_template: "review-padrao".into(),
            default_plan_template: "plan-padrao".into(),
            max_diff_chars: 0,
            use_worktree: true,
            include_previous_reviews: true,
            include_ci_failures: false,
            repo_templates: BTreeMap::new(),
        }
    }
}

pub fn repo_key(host: &str, repo: &str) -> String {
    format!("{host}/{repo}")
}

impl ReviewCfg {
    pub fn template_candidates(&self, kind: TemplateKind, host: &str, repo: &str) -> Vec<&str> {
        match kind {
            TemplateKind::Review => self
                .repo_templates
                .get(&repo_key(host, repo))
                .map(String::as_str)
                .into_iter()
                .chain([self.default_template.as_str()])
                .collect(),
            TemplateKind::Plan => vec![self.default_plan_template.as_str()],
        }
    }

    pub fn repo_template(&self, host: &str, repo: &str) -> Option<&str> {
        self.repo_templates
            .get(&repo_key(host, repo))
            .map(String::as_str)
    }

    pub fn repos_using(&self, template_id: &str) -> Vec<String> {
        self.repo_templates
            .iter()
            .filter(|(_, id)| id.as_str() == template_id)
            .map(|(repo, _)| repo.clone())
            .collect()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceCfg {
    #[serde(deserialize_with = "lenient_enum")]
    pub theme: ThemeChoice,
    #[serde(deserialize_with = "lenient_enum")]
    pub density: Density,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationsCfg {
    pub needs_you: bool,
    pub agent_events: bool,
}

impl Default for NotificationsCfg {
    fn default() -> Self {
        Self {
            needs_you: true,
            agent_events: true,
        }
    }
}

fn lenient_enum<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = toml::Value::deserialize(deserializer)?;
    Ok(T::deserialize(value).unwrap_or_default())
}

fn known_agents<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<AgentKind, AgentCfg>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = BTreeMap::<String, toml::Value>::deserialize(deserializer)?;
    let mut agents = BTreeMap::new();
    for (key, value) in raw {
        let Ok(kind) = AgentKind::deserialize(toml::Value::String(key)) else {
            continue;
        };
        let cfg = AgentCfg::deserialize(value).map_err(serde::de::Error::custom)?;
        agents.insert(kind, cfg);
    }
    Ok(agents)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub sync: SyncCfg,
    pub orgs: OrgsCfg,
    #[serde(deserialize_with = "known_agents")]
    pub agents: BTreeMap<AgentKind, AgentCfg>,
    pub review: ReviewCfg,
    pub appearance: AppearanceCfg,
    pub notifications: NotificationsCfg,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Config::default()),
            Err(source) => {
                return Err(ConfigError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let io_error = |source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }
        let text = toml::to_string_pretty(self)?;
        let mut temp_name = path.file_name().unwrap_or_default().to_os_string();
        let serial = SAVE_SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        temp_name.push(format!(".{}.{serial}.tmp", std::process::id()));
        let temp_path = path.with_file_name(temp_name);
        let written =
            std::fs::write(&temp_path, text).and_then(|()| std::fs::rename(&temp_path, path));
        if written.is_err() {
            let _ = std::fs::remove_file(&temp_path);
        }
        written.map_err(io_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("argon-core-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn customized() -> Config {
        let mut agents = BTreeMap::new();
        agents.insert(
            AgentKind::ClaudeCode,
            AgentCfg {
                command: Some(PathBuf::from("claude-agent-acp")),
                args: vec!["--verbose".into()],
                override_transport: Some("acp".into()),
                env: BTreeMap::from([("ANTHROPIC_LOG".into(), "debug".into())]),
                default_model: Some("opus".into()),
                permission_mode: Some(PermissionMode::AcceptEdits),
                full_access: false,
                use_acp: true,
                read_plan_usage: false,
            },
        );
        agents.insert(AgentKind::Antigravity, AgentCfg::default());
        Config {
            sync: SyncCfg {
                interval_secs: 30,
                closed_window_days: 14,
            },
            orgs: OrgsCfg {
                tracked: vec!["github.com/acme".into(), "ghe.example.com/platform".into()],
                initialized: true,
            },
            agents,
            review: ReviewCfg {
                default_template: "seguranca".into(),
                default_plan_template: "meu-plano".into(),
                max_diff_chars: 50_000,
                use_worktree: false,
                include_previous_reviews: false,
                include_ci_failures: true,
                repo_templates: BTreeMap::from([(
                    "github.com/acme/api".into(),
                    "perfil-api".into(),
                )]),
            },
            appearance: AppearanceCfg {
                theme: ThemeChoice::System,
                density: Density::Compact,
            },
            notifications: NotificationsCfg {
                needs_you: false,
                agent_events: false,
            },
        }
    }

    #[test]
    fn defaults_match_the_spec() {
        let config = Config::default();
        assert_eq!(config.sync.interval_secs, 120);
        assert_eq!(config.sync.closed_window_days, 7);
        assert!(config.orgs.tracked.is_empty());
        assert!(!config.orgs.initialized);
        assert!(config.agents.is_empty());
        assert_eq!(config.review.default_template, "review-padrao");
        assert_eq!(config.review.default_plan_template, "plan-padrao");
        assert_eq!(config.review.max_diff_chars, 0);
        assert!(config.review.use_worktree);
        assert!(config.review.include_previous_reviews);
        assert!(!config.review.include_ci_failures);
        assert_eq!(config.appearance.theme, ThemeChoice::OneDark);
        assert_eq!(config.appearance.density, Density::Compact);
        assert!(config.notifications.needs_you);
        assert!(config.notifications.agent_events);
    }

    #[test]
    fn toml_round_trip_preserves_every_field() {
        let config = customized();
        let text = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn agent_kinds_use_stable_toml_keys() {
        let text = toml::to_string_pretty(&customized()).unwrap();
        assert!(text.contains("[agents.claude-code]"), "{text}");
        assert!(text.contains("[agents.antigravity]"), "{text}");
        let kinds = [
            (AgentKind::ClaudeCode, "claude-code"),
            (AgentKind::Codex, "codex"),
            (AgentKind::OpenCode, "opencode"),
            (AgentKind::CommandCode, "command-code"),
            (AgentKind::Antigravity, "antigravity"),
        ];
        for (kind, key) in kinds {
            let text = format!("[agents.{key}]\ndefault_model = \"m\"\n");
            let parsed: Config = toml::from_str(&text).unwrap();
            assert_eq!(
                parsed
                    .agents
                    .get(&kind)
                    .and_then(|a| a.default_model.as_deref()),
                Some("m"),
                "{key}"
            );
        }
    }

    #[test]
    fn partial_file_fills_defaults() {
        let text = "[sync]\ninterval_secs = 45\n\n[review]\nuse_worktree = false\n";
        let parsed: Config = toml::from_str(text).unwrap();
        let expected = Config {
            sync: SyncCfg {
                interval_secs: 45,
                ..SyncCfg::default()
            },
            review: ReviewCfg {
                use_worktree: false,
                ..ReviewCfg::default()
            },
            ..Config::default()
        };
        assert_eq!(parsed, expected);
    }

    #[test]
    fn review_section_without_plan_template_keeps_the_default_plan_template() {
        let text = "[review]\ndefault_template = \"seguranca\"\nmax_diff_chars = 1000\n";
        let parsed: Config = toml::from_str(text).unwrap();
        assert_eq!(parsed.review.default_template, "seguranca");
        assert_eq!(parsed.review.default_plan_template, "plan-padrao");
        let text = "[review]\ndefault_plan_template = \"meu-plano\"\n";
        let parsed: Config = toml::from_str(text).unwrap();
        assert_eq!(parsed.review.default_plan_template, "meu-plano");
        assert_eq!(parsed.review.default_template, "review-padrao");
    }

    #[test]
    fn repo_templates_are_omitted_when_empty_and_read_back_when_set() {
        let text = toml::to_string_pretty(&Config::default()).unwrap();
        assert!(!text.contains("repo_templates"), "{text}");
        let text = "[review.repo_templates]\n\"github.com/acme/api\" = \"perfil-api\"\n";
        let parsed: Config = toml::from_str(text).unwrap();
        assert_eq!(
            parsed.review.repo_template("github.com", "acme/api"),
            Some("perfil-api")
        );
        assert_eq!(parsed.review.default_template, "review-padrao");
        assert_eq!(parsed.review.repo_template("github.com", "acme/web"), None);
    }

    #[test]
    fn template_candidates_put_the_repo_template_first() {
        let review = ReviewCfg {
            repo_templates: BTreeMap::from([("github.com/acme/api".into(), "perfil-api".into())]),
            ..ReviewCfg::default()
        };
        assert_eq!(
            review.template_candidates(TemplateKind::Review, "github.com", "acme/api"),
            ["perfil-api", "review-padrao"]
        );
        assert_eq!(
            review.template_candidates(TemplateKind::Review, "github.com", "acme/web"),
            ["review-padrao"]
        );
        assert_eq!(
            review.template_candidates(TemplateKind::Plan, "github.com", "acme/api"),
            ["plan-padrao"]
        );
        assert_eq!(review.repos_using("perfil-api"), ["github.com/acme/api"]);
        assert!(review.repos_using("review-padrao").is_empty());
    }

    #[test]
    fn unknown_agents_are_discarded() {
        let text = "[agents.foo]\ncommand = \"foo\"\n\n[agents.codex]\ndefault_model = \"m\"\n";
        let parsed: Config = toml::from_str(text).unwrap();
        assert_eq!(parsed.agents.len(), 1);
        assert_eq!(
            parsed.agents[&AgentKind::Codex].default_model.as_deref(),
            Some("m")
        );
    }

    #[test]
    fn agent_without_use_acp_defaults_to_false() {
        let text = "[agents.claude-code]\nfull_access = true\n";
        let parsed: Config = toml::from_str(text).unwrap();
        let agent = &parsed.agents[&AgentKind::ClaudeCode];
        assert!(agent.full_access);
        assert!(!agent.use_acp);
    }

    #[test]
    fn override_transport_is_absent_by_default_and_only_written_when_set() {
        assert_eq!(agent("").override_transport, None);
        let marked = agent("override_transport = \"claude-stream\"\n");
        assert_eq!(marked.override_transport.as_deref(), Some("claude-stream"));
        let text = toml::to_string_pretty(&AgentCfg::default()).unwrap();
        assert!(!text.contains("override_transport"), "{text}");
        let text = toml::to_string_pretty(&marked).unwrap();
        assert!(
            text.contains("override_transport = \"claude-stream\""),
            "{text}"
        );
    }

    #[test]
    fn plan_usage_consent_is_off_by_default_and_only_written_when_on() {
        assert!(!agent("").read_plan_usage);
        assert!(agent("read_plan_usage = true\n").read_plan_usage);
        let text = toml::to_string_pretty(&AgentCfg::default()).unwrap();
        assert!(!text.contains("read_plan_usage"), "{text}");
        let granted = AgentCfg {
            read_plan_usage: true,
            ..AgentCfg::default()
        };
        let text = toml::to_string_pretty(&granted).unwrap();
        assert!(text.contains("read_plan_usage = true"), "{text}");
    }

    fn agent(text: &str) -> AgentCfg {
        let parsed: Config = toml::from_str(&format!("[agents.codex]\n{text}")).unwrap();
        parsed.agents[&AgentKind::Codex].clone()
    }

    #[test]
    fn legacy_full_access_maps_to_a_permission_mode() {
        let legacy_on = agent("full_access = true\n");
        assert_eq!(legacy_on.permission_mode(), PermissionMode::FullAccess);
        let legacy_off = agent("full_access = false\n");
        assert_eq!(legacy_off.permission_mode(), PermissionMode::Supervised);
        assert_eq!(agent("").permission_mode(), PermissionMode::Supervised);
    }

    #[test]
    fn explicit_permission_mode_wins_over_legacy_full_access() {
        let cfg = agent("full_access = true\npermission_mode = \"accept-edits\"\n");
        assert_eq!(cfg.permission_mode(), PermissionMode::AcceptEdits);
        let cfg = agent("full_access = false\npermission_mode = \"full-access\"\n");
        assert_eq!(cfg.permission_mode(), PermissionMode::FullAccess);
    }

    #[test]
    fn with_permission_mode_replaces_the_legacy_flag() {
        let legacy = agent("full_access = true\n");
        let migrated = legacy
            .clone()
            .with_permission_mode(legacy.permission_mode());
        assert_eq!(migrated.permission_mode, Some(PermissionMode::FullAccess));
        assert!(!migrated.full_access);
        let text = toml::to_string_pretty(&migrated).unwrap();
        assert!(text.contains("permission_mode = \"full-access\""), "{text}");
        assert!(!text.contains("full_access"), "{text}");
    }

    #[test]
    fn unknown_permission_mode_falls_back_to_legacy_rule() {
        let cfg = agent("permission_mode = \"yolo\"\nfull_access = true\n");
        assert_eq!(cfg.permission_mode, None);
        assert_eq!(cfg.permission_mode(), PermissionMode::FullAccess);
    }

    #[test]
    fn serialization_writes_permission_mode_and_not_full_access() {
        let cfg = AgentCfg {
            permission_mode: Some(PermissionMode::AcceptEdits),
            ..AgentCfg::default()
        };
        let text = toml::to_string_pretty(&cfg).unwrap();
        assert!(
            text.contains("permission_mode = \"accept-edits\""),
            "{text}"
        );
        assert!(!text.contains("full_access"), "{text}");
        let text = toml::to_string_pretty(&AgentCfg::default()).unwrap();
        assert!(!text.contains("full_access"), "{text}");
        assert!(!text.contains("permission_mode"), "{text}");
    }

    #[test]
    fn permission_mode_text_round_trips() {
        for mode in [
            PermissionMode::Supervised,
            PermissionMode::AcceptEdits,
            PermissionMode::FullAccess,
        ] {
            assert_eq!(PermissionMode::parse(mode.as_str()), Some(mode));
            let value = toml::Value::try_from(mode).unwrap();
            assert_eq!(value.as_str(), Some(mode.as_str()));
        }
        assert_eq!(PermissionMode::parse("bogus"), None);
        assert_eq!(PermissionMode::default(), PermissionMode::Supervised);
    }

    #[test]
    fn unknown_enum_values_fall_back_to_defaults() {
        let text = "[appearance]\ntheme = \"solarized\"\ndensity = \"xyz\"\n";
        let parsed: Config = toml::from_str(text).unwrap();
        assert_eq!(parsed.appearance, AppearanceCfg::default());
        let text = "[appearance]\ntheme = \"one-light\"\ndensity = \"xyz\"\n";
        let parsed: Config = toml::from_str(text).unwrap();
        assert_eq!(parsed.appearance.theme, ThemeChoice::OneLight);
        assert_eq!(parsed.appearance.density, Density::Compact);
    }

    #[test]
    fn save_overwrites_existing_file_without_leaving_temp_files() {
        let dir = TempDir::new("atomic");
        let path = dir.path().join("config.toml");
        Config::default().save(&path).unwrap();
        let config = customized();
        config.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), config);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn empty_file_yields_defaults() {
        let parsed: Config = toml::from_str("").unwrap();
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let text = "future_flag = true\n\n[sync]\ninterval_secs = 10\nunknown = \"x\"\n\n[telemetry]\nenabled = true\n";
        let parsed: Config = toml::from_str(text).unwrap();
        assert_eq!(parsed.sync.interval_secs, 10);
        assert_eq!(parsed.sync.closed_window_days, 7);
    }

    #[test]
    fn load_missing_file_returns_defaults() {
        let dir = TempDir::new("missing");
        let config = Config::load(&dir.path().join("config.toml")).unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn save_creates_parent_directory_and_load_reads_it_back() {
        let dir = TempDir::new("roundtrip");
        let path = dir.path().join("nested").join("Argon").join("config.toml");
        let config = customized();
        config.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), config);
    }

    #[test]
    fn load_invalid_toml_reports_the_path() {
        let dir = TempDir::new("invalid");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "sync = [").unwrap();
        let error = Config::load(&path).unwrap_err();
        assert!(matches!(error, ConfigError::Parse { .. }));
        assert!(error.to_string().contains("config.toml"));
    }
}
