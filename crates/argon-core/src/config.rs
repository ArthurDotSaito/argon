use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

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
    pub env: BTreeMap<String, String>,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewCfg {
    pub default_template: String,
    pub max_diff_chars: usize,
    pub use_worktree: bool,
    pub include_previous_reviews: bool,
    pub include_ci_failures: bool,
}

impl Default for ReviewCfg {
    fn default() -> Self {
        Self {
            default_template: "review-padrao".into(),
            max_diff_chars: 0,
            use_worktree: true,
            include_previous_reviews: true,
            include_ci_failures: false,
        }
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
                env: BTreeMap::from([("ANTHROPIC_LOG".into(), "debug".into())]),
                default_model: Some("opus".into()),
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
                max_diff_chars: 50_000,
                use_worktree: false,
                include_previous_reviews: false,
                include_ci_failures: true,
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
