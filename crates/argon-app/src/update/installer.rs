use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;

const UPDATE_DIR: &str = "argon-update";
const ARGS: [&str; 3] = ["/P", "/R", "/NS"];
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x0000_0008;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

fn dir() -> PathBuf {
    std::env::temp_dir().join(UPDATE_DIR)
}

pub fn path_for(version: &Version) -> PathBuf {
    dir().join(format!("argon-{version}-setup.exe"))
}

pub fn reset_dir() -> io::Result<()> {
    if let Err(error) = std::fs::remove_dir_all(dir())
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(%error, "failed to clean the previous update downloads");
    }
    std::fs::create_dir_all(dir())
}

pub fn launch(path: &Path) -> io::Result<()> {
    let mut command = Command::new(path);
    command.args(ARGS);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command.spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_is_named_after_the_version_inside_the_update_dir() {
        let path = path_for(&Version::new(0, 2, 0));
        assert_eq!(path.parent().unwrap(), dir());
        assert_eq!(path.file_name().unwrap(), "argon-0.2.0-setup.exe");
    }
}
