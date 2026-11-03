use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::Command;

use super::error::{Result, WorktreeError};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_STDERR_CHARS: usize = 4000;

pub(super) const CLONE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub(super) const FETCH_TIMEOUT: Duration = Duration::from_secs(3 * 60);
pub(super) const WORKTREE_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) struct Invocation {
    program: &'static str,
    args: Vec<OsString>,
    env: Vec<(&'static str, String)>,
}

impl Invocation {
    pub fn git(repo_dir: &Path) -> Invocation {
        Invocation {
            program: "git",
            args: Vec::new(),
            env: Vec::new(),
        }
        .arg("-C")
        .arg(repo_dir)
    }

    pub fn gh(host: &str) -> Invocation {
        Invocation {
            program: "gh",
            args: Vec::new(),
            env: vec![("GH_HOST", host.to_string())],
        }
    }

    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Invocation {
        self.args.push(arg.as_ref().to_os_string());
        self
    }

    pub fn args<I>(mut self, args: I) -> Invocation
    where
        I: IntoIterator,
        I::Item: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|arg| arg.as_ref().to_os_string()));
        self
    }

    pub fn describe(&self) -> String {
        let args: Vec<String> = self
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        format!("{} {}", self.program, args.join(" "))
    }

    pub async fn run(self, timeout: Duration) -> Result<String> {
        let cmd = self.describe();
        let mut command = Command::new(self.program);
        command
            .args(&self.args)
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        let started = Instant::now();
        let output = tokio::time::timeout(timeout, command.output())
            .await
            .map_err(|_| WorktreeError::Timeout { cmd: cmd.clone() })??;
        tracing::debug!(
            cmd = %cmd,
            elapsed_ms = started.elapsed().as_millis() as u64,
            success = output.status.success(),
            "worktree command finished"
        );
        if !output.status.success() {
            return Err(WorktreeError::Git {
                cmd,
                status: output.status,
                stderr: tail(String::from_utf8_lossy(&output.stderr).trim()),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

fn tail(text: &str) -> String {
    let count = text.chars().count();
    if count <= MAX_STDERR_CHARS {
        return text.to_string();
    }
    text.chars().skip(count - MAX_STDERR_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_git_invocation_with_repo_dir() {
        let invocation = Invocation::git(Path::new("mirror")).args(["fetch", "origin"]);
        assert_eq!(invocation.describe(), "git -C mirror fetch origin");
    }

    #[test]
    fn tail_keeps_the_last_chars() {
        let long = "a".repeat(MAX_STDERR_CHARS) + "end";
        let kept = tail(&long);
        assert_eq!(kept.chars().count(), MAX_STDERR_CHARS);
        assert!(kept.ends_with("end"));
        assert_eq!(tail("short"), "short");
    }
}
