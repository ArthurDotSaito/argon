use std::path::PathBuf;

const APP_DIR: &str = "Argon";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn resolve() -> Paths {
        let base = |dir: Option<PathBuf>| dir.unwrap_or_else(|| PathBuf::from(".")).join(APP_DIR);
        Paths {
            config_dir: base(dirs::config_dir()),
            data_dir: base(dirs::data_local_dir()),
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn prompts_dir(&self) -> PathBuf {
        self.config_dir.join("prompts")
    }

    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join("argon.db")
    }

    pub fn repos_dir(&self) -> PathBuf {
        self.data_dir.join("repos")
    }

    pub fn worktrees_dir(&self) -> PathBuf {
        self.data_dir.join("worktrees")
    }

    pub fn layout_file(&self) -> PathBuf {
        self.data_dir.join("layout.json")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Paths {
        Paths {
            config_dir: PathBuf::from("cfg").join("Argon"),
            data_dir: PathBuf::from("data").join("Argon"),
        }
    }

    #[test]
    fn config_files_live_under_config_dir() {
        let paths = paths();
        assert_eq!(paths.config_file(), paths.config_dir.join("config.toml"));
        assert_eq!(paths.prompts_dir(), paths.config_dir.join("prompts"));
    }

    #[test]
    fn data_files_live_under_data_dir() {
        let paths = paths();
        assert_eq!(paths.db_file(), paths.data_dir.join("argon.db"));
        assert_eq!(paths.repos_dir(), paths.data_dir.join("repos"));
        assert_eq!(paths.worktrees_dir(), paths.data_dir.join("worktrees"));
        assert_eq!(paths.layout_file(), paths.data_dir.join("layout.json"));
        assert_eq!(paths.log_dir(), paths.data_dir.join("logs"));
    }

    #[test]
    fn resolve_scopes_both_directories_to_argon() {
        let paths = Paths::resolve();
        assert_eq!(paths.config_dir.file_name().unwrap(), "Argon");
        assert_eq!(paths.data_dir.file_name().unwrap(), "Argon");
    }
}
