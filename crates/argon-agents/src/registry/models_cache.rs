use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use argon_core::AgentKind;
use serde::{Deserialize, Serialize};

use super::ModelInfo;

const TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    at: u64,
    models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelsCache {
    entries: BTreeMap<AgentKind, Entry>,
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

    pub fn get(&self, kind: AgentKind) -> Option<(SystemTime, Vec<ModelInfo>)> {
        self.entries.get(&kind).map(|entry| {
            (
                UNIX_EPOCH + Duration::from_secs(entry.at),
                entry.models.clone(),
            )
        })
    }

    pub fn put(&mut self, kind: AgentKind, at: SystemTime, models: Vec<ModelInfo>) {
        let at = at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        self.entries.insert(kind, Entry { at, models });
    }

    pub fn is_fresh(&self, kind: AgentKind, now: SystemTime) -> bool {
        self.get(kind)
            .and_then(|(at, _)| now.duration_since(at).ok())
            .is_some_and(|age| age < TTL)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        std::fs::write(&temp, bytes)?;
        std::fs::rename(&temp, path).inspect_err(|_| _ = std::fs::remove_file(&temp))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

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

    #[test]
    fn entries_round_trip_through_the_file() {
        let dir = TempDir::new("round-trip");
        let mut cache = ModelsCache::default();
        cache.put(
            AgentKind::OpenCode,
            at(1_000),
            vec![model("opencode-go/glm")],
        );
        cache.put(
            AgentKind::CommandCode,
            at(2_000),
            vec![model("deepseek/v4")],
        );
        cache.save(&dir.file()).unwrap();
        let loaded = ModelsCache::load(&dir.file());
        assert_eq!(loaded, cache);
        assert_eq!(
            loaded.get(AgentKind::OpenCode),
            Some((at(1_000), vec![model("opencode-go/glm")]))
        );
        assert_eq!(loaded.get(AgentKind::Codex), None);
    }

    #[test]
    fn entries_expire_after_ten_minutes() {
        let mut cache = ModelsCache::default();
        cache.put(AgentKind::OpenCode, at(1_000), vec![model("a/b")]);
        assert!(cache.is_fresh(AgentKind::OpenCode, at(1_000)));
        assert!(cache.is_fresh(AgentKind::OpenCode, at(1_599)));
        assert!(!cache.is_fresh(AgentKind::OpenCode, at(1_600)));
        assert!(!cache.is_fresh(AgentKind::CommandCode, at(1_000)));
    }

    #[test]
    fn entries_from_the_future_are_stale() {
        let mut cache = ModelsCache::default();
        cache.put(AgentKind::OpenCode, at(5_000), vec![model("a/b")]);
        assert!(!cache.is_fresh(AgentKind::OpenCode, at(1_000)));
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
        cache.put(AgentKind::OpenCode, at(1), vec![model("a/b")]);
        cache.save(&dir.file()).unwrap();
        cache.put(AgentKind::OpenCode, at(2), vec![model("c/d")]);
        cache.save(&dir.file()).unwrap();
        assert_eq!(ModelsCache::load(&dir.file()), cache);
        let names: Vec<_> = std::fs::read_dir(dir.file().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["models.json"]);
    }
}
