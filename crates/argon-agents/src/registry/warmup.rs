use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use argon_core::{AgentKind, Config};
use futures::future::join_all;
use tokio::process::Command;

use super::spec::{agent_spec, is_command_overridden};
use crate::defaults::{AGENT_DEFAULTS, agent_defaults};
use crate::process::spawn_in_job;
use crate::util::{Resolved, lock, resolve_command};

const WARMUP_TIMEOUT: Duration = Duration::from_secs(60);
const WARMUP_COOLDOWN: Duration = Duration::from_secs(120);

static THROTTLE: LazyLock<Mutex<Throttle>> = LazyLock::new(Mutex::default);

enum Slot {
    Running,
    Finished(Instant),
}

#[derive(Default)]
struct Throttle {
    slots: HashMap<AgentKind, Slot>,
}

impl Throttle {
    fn try_begin(&mut self, kind: AgentKind, now: Instant) -> bool {
        let busy = match self.slots.get(&kind) {
            Some(Slot::Running) => true,
            Some(Slot::Finished(at)) => now.saturating_duration_since(*at) < WARMUP_COOLDOWN,
            None => false,
        };
        if !busy {
            self.slots.insert(kind, Slot::Running);
        }
        !busy
    }

    fn finish(&mut self, kind: AgentKind, now: Instant) {
        self.slots.insert(kind, Slot::Finished(now));
    }
}

struct FinishOnDrop(AgentKind);

impl Drop for FinishOnDrop {
    fn drop(&mut self) {
        lock(&THROTTLE).finish(self.0, Instant::now());
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Warmup {
    kind: AgentKind,
    label: &'static str,
    command: PathBuf,
    args: Vec<String>,
    env: BTreeMap<String, String>,
}

fn warmup_for(kind: AgentKind, cfg: &Config) -> Option<Warmup> {
    let defaults = agent_defaults(kind);
    let extra = defaults.warmup_args?;
    let spec = agent_spec(kind, cfg);
    if is_command_overridden(kind, cfg) || spec.args != defaults.args {
        return None;
    }
    let mut args = spec.args;
    args.extend(extra.iter().map(ToString::to_string));
    Some(Warmup {
        kind,
        label: spec.label,
        command: spec.command,
        args,
        env: spec.env,
    })
}

pub async fn warm_up_agents(cfg: Config) {
    let warmups = AGENT_DEFAULTS
        .iter()
        .filter_map(|defaults| warmup_for(defaults.kind, &cfg));
    join_all(warmups.map(run)).await;
}

async fn run(warmup: Warmup) {
    if !lock(&THROTTLE).try_begin(warmup.kind, Instant::now()) {
        return;
    }
    let _finish = FinishOnDrop(warmup.kind);
    let program = match resolve_command(&warmup.command) {
        Resolved::Path(path) => path,
        Resolved::Missing => return,
        Resolved::Failed(error) => {
            tracing::warn!(agent = warmup.label, %error, "agent warm-up skipped");
            return;
        }
    };
    let mut command = Command::new(program);
    command
        .args(&warmup.args)
        .envs(&warmup.env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let started = Instant::now();
    let mut child = match spawn_in_job(&mut command) {
        Ok(child) => child,
        Err(error) => {
            tracing::warn!(agent = warmup.label, %error, "agent warm-up failed to start");
            return;
        }
    };
    let elapsed_ms = || started.elapsed().as_millis();
    match tokio::time::timeout(WARMUP_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) if status.success() => {
            tracing::info!(
                agent = warmup.label,
                elapsed_ms = elapsed_ms(),
                "agent warm-up finished"
            );
        }
        Ok(Ok(status)) => {
            tracing::warn!(agent = warmup.label, %status, elapsed_ms = elapsed_ms(), "agent warm-up exited with failure");
        }
        Ok(Err(error)) => {
            tracing::warn!(agent = warmup.label, %error, "agent warm-up failed");
        }
        Err(_) => {
            tracing::warn!(
                agent = warmup.label,
                timeout_s = WARMUP_TIMEOUT.as_secs(),
                "agent warm-up timed out"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use argon_core::AgentCfg;

    use super::*;

    fn config_with(kind: AgentKind, agent: AgentCfg) -> Config {
        Config {
            agents: BTreeMap::from([(kind, agent)]),
            ..Config::default()
        }
    }

    #[test]
    fn npx_adapters_warm_up_through_the_session_command() {
        for (kind, extra) in [
            (AgentKind::ClaudeCode, &["--cli", "--version"][..]),
            (AgentKind::Codex, &["--version"][..]),
        ] {
            let warmup = warmup_for(kind, &Config::default()).unwrap();
            let defaults = agent_defaults(kind);
            assert_eq!(warmup.command, PathBuf::from(defaults.command), "{kind:?}");
            assert_eq!(
                &warmup.args[..defaults.args.len()],
                defaults.args,
                "{kind:?}"
            );
            assert_eq!(&warmup.args[defaults.args.len()..], extra, "{kind:?}");
        }
    }

    #[test]
    fn agents_without_warmup_args_are_skipped() {
        for kind in [
            AgentKind::OpenCode,
            AgentKind::CommandCode,
            AgentKind::Antigravity,
        ] {
            assert_eq!(warmup_for(kind, &Config::default()), None, "{kind:?}");
        }
    }

    #[test]
    fn overridden_command_or_args_skip_warmup() {
        let command = config_with(
            AgentKind::ClaudeCode,
            AgentCfg {
                command: Some(PathBuf::from("claude-agent-acp")),
                ..AgentCfg::default()
            },
        );
        assert_eq!(warmup_for(AgentKind::ClaudeCode, &command), None);
        let args = config_with(
            AgentKind::ClaudeCode,
            AgentCfg {
                args: vec!["--verbose".into()],
                ..AgentCfg::default()
            },
        );
        assert_eq!(warmup_for(AgentKind::ClaudeCode, &args), None);
    }

    #[test]
    fn configured_env_reaches_the_warmup() {
        let cfg = config_with(
            AgentKind::ClaudeCode,
            AgentCfg {
                env: BTreeMap::from([("CLAUDE_CODE_EXECUTABLE".into(), "claude.exe".into())]),
                ..AgentCfg::default()
            },
        );
        let warmup = warmup_for(AgentKind::ClaudeCode, &cfg).unwrap();
        assert_eq!(warmup.env["CLAUDE_CODE_EXECUTABLE"], "claude.exe");
    }

    #[test]
    fn throttle_skips_while_running_and_during_cooldown() {
        let mut throttle = Throttle::default();
        let at = Instant::now();
        assert!(throttle.try_begin(AgentKind::ClaudeCode, at));
        assert!(!throttle.try_begin(AgentKind::ClaudeCode, at + WARMUP_COOLDOWN * 10));
        assert!(throttle.try_begin(AgentKind::OpenCode, at));
        throttle.finish(AgentKind::ClaudeCode, at);
        assert!(!throttle.try_begin(AgentKind::ClaudeCode, at + WARMUP_COOLDOWN / 2));
        assert!(throttle.try_begin(AgentKind::ClaudeCode, at + WARMUP_COOLDOWN));
    }
}
