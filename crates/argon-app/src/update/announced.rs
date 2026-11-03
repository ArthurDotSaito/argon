use std::io;
use std::path::{Path, PathBuf};

use argon_core::Paths;
use semver::Version;

const ANNOUNCED_FILE: &str = "update_announced";

pub fn file() -> PathBuf {
    Paths::resolve().data_dir.join(ANNOUNCED_FILE)
}

pub fn is_new(path: &Path, version: &Version) -> bool {
    read(path).as_ref() != Some(version)
}

pub fn write(path: &Path, version: &Version) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, version.to_string())
}

fn read(path: &Path) -> Option<Version> {
    Version::parse(std::fs::read_to_string(path).ok()?.trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_file(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("argon-announced-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("update_announced")
    }

    #[test]
    fn every_version_is_new_until_it_is_written() {
        let path = scratch_file("new");
        let version = Version::new(0, 2, 0);
        assert!(is_new(&path, &version));
        write(&path, &version).unwrap();
        assert!(!is_new(&path, &version));
        assert!(is_new(&path, &Version::new(0, 3, 0)));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn corrupt_content_counts_as_never_announced() {
        let path = scratch_file("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not a version").unwrap();
        assert!(is_new(&path, &Version::new(0, 2, 0)));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
