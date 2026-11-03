use std::path::{Path, PathBuf};
use std::time::Duration;

use argon_core::{AgentKind, Config};

use super::login::{LoginState, login_probe, parse_login};
use super::probe::capture;
use super::spec::{agent_spec, is_command_overridden, login_hint, support_for};
use super::version::parse_version;
use super::{AgentSpec, Detection, Support};
use crate::defaults::{AgentDefaults, agent_defaults};

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const UNKNOWN_VERSION: &str = "unknown";

enum Resolved {
    Path(PathBuf),
    Missing,
    Failed(String),
}

pub(super) async fn detect_agent(kind: AgentKind, cfg: &Config) -> (AgentSpec, Support, Detection) {
    let mut spec = agent_spec(kind, cfg);
    let overridden = is_command_overridden(kind, cfg);
    let detection = detect(&mut spec, overridden).await;
    (spec, support_for(kind), detection)
}

fn resolve(name: &Path) -> Resolved {
    match which::which(name) {
        Ok(path) => Resolved::Path(path),
        Err(which::Error::CannotFindBinaryPath) => Resolved::Missing,
        Err(error) => Resolved::Failed(error.to_string()),
    }
}

async fn detect(spec: &mut AgentSpec, overridden: bool) -> Detection {
    let defaults = agent_defaults(spec.kind);
    let base_cli = if overridden || defaults.cli_command == defaults.command {
        None
    } else {
        match resolve(Path::new(defaults.cli_command)) {
            Resolved::Path(path) => Some(path),
            Resolved::Missing => return Detection::NotFound,
            Resolved::Failed(message) => return Detection::Error(message),
        }
    };
    let launcher = match resolve(&spec.command) {
        Resolved::Path(path) => path,
        Resolved::Missing if base_cli.is_some() => {
            return Detection::Error(format!(
                "`{}` not found in PATH (required to run the {} adapter)",
                spec.command.display(),
                spec.label
            ));
        }
        Resolved::Missing => return Detection::NotFound,
        Resolved::Failed(message) => return Detection::Error(message),
    };
    spec.command.clone_from(&launcher);
    let cli = base_cli.unwrap_or(launcher);
    inspect(spec.kind, defaults, &cli, !overridden).await
}

async fn inspect(
    kind: AgentKind,
    defaults: &AgentDefaults,
    cli: &Path,
    check_login: bool,
) -> Detection {
    let (version, login) = tokio::join!(
        capture(cli, defaults.version_args, PROBE_TIMEOUT),
        login_state(kind, cli, check_login)
    );
    let version = match version {
        Ok(captured) if captured.success => {
            parse_version(&captured.text).unwrap_or_else(|| UNKNOWN_VERSION.to_string())
        }
        Ok(captured) => {
            return Detection::Error(format!(
                "`{} {}` failed: {}",
                cli.display(),
                defaults.version_args.join(" "),
                captured.text.trim()
            ));
        }
        Err(message) => return Detection::Error(message),
    };
    match login {
        LoginState::LoggedOut => Detection::NeedsLogin {
            hint: login_hint(defaults),
        },
        LoginState::LoggedIn | LoginState::Unknown => Detection::Found { version },
    }
}

async fn login_state(kind: AgentKind, cli: &Path, enabled: bool) -> LoginState {
    let Some(args) = login_probe(kind).filter(|_| enabled) else {
        return LoginState::Unknown;
    };
    match capture(cli, args, PROBE_TIMEOUT).await {
        Ok(captured) => parse_login(kind, captured.success, &captured.text),
        Err(_) => LoginState::Unknown,
    }
}
