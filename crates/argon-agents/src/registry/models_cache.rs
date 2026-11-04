use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use argon_core::AgentKind;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{AgentSpec, ModelInfo};

const TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    at: u64,
    #[serde(default)]
    fingerprint: String,
    models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelsCache {
    entries: BTreeMap<AgentKind, Entry>,
}

pub fn spec_fingerprint(spec: &AgentSpec) -> String {
    let mut hasher = Sha256::new();
    hasher.update(spec.command.to_string_lossy().as_bytes());
    hasher.update([0]);
    for arg in &spec.args {
        hasher.update(arg.as_bytes());
        hasher.update([0]);
    }
    hasher.update([1]);
    for (key, value) in &spec.env {
        hasher.update(key.as_bytes());
        hasher.update(b"=");
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl ModelsCache {
    pub fn load(path: &Path) -> ModelsCache {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    tracing::warn!(path = %path.display(), %error, "failed to read the models cache");
                }
                return ModelsCache::default();
            }
        };
        serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            tracing::warn!(path = %path.display(), %error, "ignoring a corrupt models cache");
            ModelsCache::default()
        })
    }

    pub fn get(&self, kind: AgentKind, fingerprint: &str) -> Option<(SystemTime, Vec<ModelInfo>)> {
        self.entries
            .get(&kind)
            .filter(|entry| entry.fingerprint == fingerprint)
            .map(|entry| {
                (
                    UNIX_EPOCH + Duration::from_secs(entry.at),
                    entry.models.clone(),
                )
            })
    }

    pub fn put(
        &mut self,
        kind: AgentKind,
        fingerprint: &str,
        at: SystemTime,
        models: Vec<ModelInfo>,
    ) {
        let at = at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let fingerprint = fingerprint.to_owned();
        let entry = Entry {
            at,
            fingerprint,
            models,
        };
        self.entries.insert(kind, entry);
    }

    pub fn is_fresh(&self, kind: AgentKind, fingerprint: &str, now: SystemTime) -> bool {
        self.get(kind, fingerprint)
            .and_then(|(at, _)| now.duration_since(at).ok())
            .is_some_and(|age| age < TTL)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        std::fs::write(&temp, bytes)?;
        std::fs::rename(&temp, path).inspect_err(|_| _ = std::fs::remove_file(&temp))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use argon_core::Config;

    use super::*;
    use crate::registry::agent_spec;

    const FP: &str = "fp-a";

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("argon-models-{name}-{}", std::process::id()));
            _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn file(&self) -> PathBuf {
            self.0.join("agents").join("models.json")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn model(id: &str) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            label: format!("{id} label"),
        }
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn spec() -> AgentSpec {
        agent_spec(AgentKind::OpenCode, &Config::default())
    }

    #[test]
    fn entries_round_trip_through_the_file() {
        let dir = TempDir::new("round-trip");
        let mut cache = ModelsCache::default();
        cache.put(
            AgentKind::OpenCode,
            FP,
            at(1_000),
            vec![model("opencode-go/glm")],
        );
        cache.put(
            AgentKind::CommandCode,
            "fp-b",
            at(2_000),
            vec![model("deepseek/v4")],
        );
        cache.save(&dir.file()).unwrap();
        let loaded = ModelsCache::load(&dir.file());
        assert_eq!(loaded, cache);
        assert_eq!(
            loaded.get(AgentKind::OpenCode, FP),
            Some((at(1_000), vec![model("opencode-go/glm")]))
        );
        assert_eq!(loaded.get(AgentKind::Codex, FP), None);
    }

    #[test]
    fn entries_expire_after_ten_minutes() {
        let mut cache = ModelsCache::default();
        cache.put(AgentKind::OpenCode, FP, at(1_000), vec![model("a/b")]);
        assert!(cache.is_fresh(AgentKind::OpenCode, FP, at(1_000)));
        assert!(cache.is_fresh(AgentKind::OpenCode, FP, at(1_599)));
        assert!(!cache.is_fresh(AgentKind::OpenCode, FP, at(1_600)));
        assert!(!cache.is_fresh(AgentKind::CommandCode, FP, at(1_000)));
    }

    #[test]
    fn entries_from_the_future_are_stale() {
        let mut cache = ModelsCache::default();
        cache.put(AgentKind::OpenCode, FP, at(5_000), vec![model("a/b")]);
        assert!(!cache.is_fresh(AgentKind::OpenCode, FP, at(1_000)));
    }

    #[test]
    fn a_different_fingerprint_neither_hits_nor_counts_as_fresh() {
        let mut cache = ModelsCache::default();
        cache.put(AgentKind::OpenCode, FP, at(1_000), vec![model("a/b")]);
        assert!(cache.get(AgentKind::OpenCode, "fp-other").is_none());
        assert!(!cache.is_fresh(AgentKind::OpenCode, "fp-other", at(1_000)));
    }

    #[test]
    fn entries_saved_without_a_fingerprint_never_match_a_real_one() {
        let dir = TempDir::new("legacy");
        std::fs::create_dir_all(dir.file().parent().unwrap()).unwrap();
        std::fs::write(
            dir.file(),
            r#"{"opencode":{"at":1000,"models":[{"id":"a/b","label":"a/b"}]}}"#,
        )
        .unwrap();
        let loaded = ModelsCache::load(&dir.file());
        assert!(loaded.get(AgentKind::OpenCode, FP).is_none());
        assert!(loaded.get(AgentKind::OpenCode, "").is_some());
    }

    #[test]
    fn fingerprint_changes_with_command_args_and_env_values() {
        let base = spec();
        let fingerprint = spec_fingerprint(&base);
        assert_eq!(fingerprint, spec_fingerprint(&base.clone()));
        assert_eq!(fingerprint.len(), 64);

        let mut command = base.clone();
        command.command = PathBuf::from("other-opencode");
        let mut args = base.clone();
        args.args.push("--flag".into());
        let mut env = base.clone();
        env.env.insert("KEY".into(), "one".into());
        let mut env_value = env.clone();
        env_value.env.insert("KEY".into(), "two".into());
        let mut env_key = base.clone();
        env_key.env.insert("KEY2".into(), "one".into());
        let all = [&base, &command, &args, &env, &env_value, &env_key];
        let distinct: std::collections::BTreeSet<String> =
            all.iter().map(|spec| spec_fingerprint(spec)).collect();
        assert_eq!(distinct.len(), all.len());
    }

    #[test]
    fn env_values_never_reach_the_saved_file() {
        let dir = TempDir::new("secret");
        let mut spec = spec();
        spec.env
            .insert("API_KEY".into(), "super-secret-value".into());
        let mut cache = ModelsCache::default();
        cache.put(
            AgentKind::OpenCode,
            &spec_fingerprint(&spec),
            at(1),
            vec![model("a/b")],
        );
        cache.save(&dir.file()).unwrap();
        let saved = std::fs::read_to_string(dir.file()).unwrap();
        assert!(!saved.contains("super-secret-value"));
        assert!(!saved.contains("API_KEY"));
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = TempDir::new("missing");
        assert_eq!(ModelsCache::load(&dir.file()), ModelsCache::default());
    }

    #[test]
    fn corrupt_file_loads_empty() {
        let dir = TempDir::new("corrupt");
        std::fs::create_dir_all(dir.file().parent().unwrap()).unwrap();
        for content in ["{not json", "[]", r#"{"opencode":{"at":"soon"}}"#, ""] {
            std::fs::write(dir.file(), content).unwrap();
            assert_eq!(ModelsCache::load(&dir.file()), ModelsCache::default());
        }
    }

    #[test]
    fn save_replaces_the_file_and_leaves_no_temp_file() {
        let dir = TempDir::new("atomic");
        let mut cache = ModelsCache::default();
        cache.put(AgentKind::OpenCode, FP, at(1), vec![model("a/b")]);
        cache.save(&dir.file()).unwrap();
        cache.put(AgentKind::OpenCode, FP, at(2), vec![model("c/d")]);
        cache.save(&dir.file()).unwrap();
        assert_eq!(ModelsCache::load(&dir.file()), cache);
        let names: Vec<_> = std::fs::read_dir(dir.file().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["models.json"]);
    }
}
