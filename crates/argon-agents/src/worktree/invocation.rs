use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::Command;

use super::error::{Result, WorktreeError};
use crate::process::spawn_in_job;

const MAX_STDERR_CHARS: usize = 4000;
const NULL_DEVICE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };
const GH_CREDENTIAL_HELPER: &str = "credential.helper=!gh auth git-credential";

pub(super) const CLONE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub(super) const FETCH_TIMEOUT: Duration = Duration::from_secs(3 * 60);
pub(super) const WORKTREE_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) struct Invocation {
    program: &'static str,
    config: Vec<OsString>,
    args: Vec<OsString>,
    env: Vec<(&'static str, String)>,
}

impl Invocation {
    pub fn git(repo_dir: &Path) -> Invocation {
        Invocation {
            program: "git",
            config: ["core.fsmonitor=false".to_string(), hooks_path_config()]
                .into_iter()
                .flat_map(|setting| [OsString::from("-c"), OsString::from(setting)])
                .collect(),
            args: Vec::new(),
            env: Vec::new(),
        }
        .arg("-C")
        .arg(repo_dir)
    }

    pub fn git_authenticated(repo_dir: &Path, host: &str) -> Invocation {
        let mut invocation = Invocation::git(repo_dir);
        invocation
            .config
            .extend(["-c", "credential.helper=", "-c", GH_CREDENTIAL_HELPER].map(OsString::from));
        invocation.env.push(("GH_HOST", host.to_string()));
        invocation
    }

    pub fn gh(host: &str) -> Invocation {
        Invocation {
            program: "gh",
            config: Vec::new(),
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

    fn command_args(&self) -> impl Iterator<Item = &OsString> {
        self.config.iter().chain(&self.args)
    }

    pub async fn run(self, timeout: Duration) -> Result<String> {
        let cmd = self.describe();
        let mut command = Command::new(self.program);
        command
            .args(self.command_args())
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();
        let child = spawn_in_job(&mut command)?;
        let output = tokio::time::timeout(timeout, child.wait_with_output())
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

fn hooks_path_config() -> String {
    format!("core.hooksPath={NULL_DEVICE}")
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

    fn rendered(invocation: &Invocation) -> Vec<String> {
        invocation
            .command_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn describes_git_invocation_with_repo_dir() {
        let invocation = Invocation::git(Path::new("mirror")).args(["fetch", "origin"]);
        assert_eq!(invocation.describe(), "git -C mirror fetch origin");
    }

    #[test]
    fn git_always_disables_fsmonitor_and_hooks() {
        let args = rendered(&Invocation::git(Path::new("mirror")).arg("status"));
        let joined = args.join(" ");
        assert!(joined.contains("-c core.fsmonitor=false"));
        assert!(joined.contains(&format!("-c core.hooksPath={NULL_DEVICE}")));
        assert!(joined.ends_with("-C mirror status"));
    }

    #[test]
    fn authenticated_git_resets_helpers_before_using_gh() {
        let invocation =
            Invocation::git_authenticated(Path::new("mirror"), "ghe.acme.io").arg("fetch");
        let args = rendered(&invocation);
        let reset = args.iter().position(|arg| arg == "credential.helper=");
        let gh = args.iter().position(|arg| arg == GH_CREDENTIAL_HELPER);
        assert!(reset.is_some() && gh.is_some() && reset < gh);
        assert!(
            invocation
                .env
                .contains(&("GH_HOST", "ghe.acme.io".to_string()))
        );
        assert_eq!(invocation.describe(), "git -C mirror fetch");
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
