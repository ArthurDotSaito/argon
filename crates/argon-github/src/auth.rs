use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tokio::process::Command;

use crate::error::{GithubError, Result};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const TOKEN_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub host: String,
    pub login: String,
}

#[derive(Deserialize)]
struct HostEntry {
    user: Option<String>,
}

pub fn discover_accounts() -> Result<Vec<Account>> {
    let Some(path) = hosts_file() else {
        return Ok(Vec::new());
    };
    match std::fs::read_to_string(path) {
        Ok(text) => parse_hosts(&text),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

pub async fn token_for(host: &str) -> Result<String> {
    let mut command = Command::new("gh");
    command
        .args(["auth", "token", "--hostname", host])
        .stdin(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = tokio::time::timeout(TOKEN_TIMEOUT, command.output())
        .await
        .map_err(|_| std::io::Error::new(ErrorKind::TimedOut, "gh auth token timed out"))??;
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || token.is_empty() {
        return Err(GithubError::AuthMissing {
            host: host.to_string(),
        });
    }
    Ok(token)
}

fn hosts_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("hosts.yml"))
}

fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("GH_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|appdata| PathBuf::from(appdata).join("GitHub CLI"))
    } else {
        dirs::home_dir().map(|home| home.join(".config").join("gh"))
    }
}

fn parse_hosts(text: &str) -> Result<Vec<Account>> {
    let hosts: Option<BTreeMap<String, HostEntry>> = serde_yaml_ng::from_str(text)?;
    Ok(hosts
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(host, entry)| entry.user.map(|login| Account { host, login }))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hosts_reads_user_per_top_level_host() {
        let text = "github.com:\n    users:\n        mcouto:\n    git_protocol: https\n    user: mcouto\nghe.orbita.example:\n    user: lfarias\n    oauth_token: not-read\n";
        let accounts = parse_hosts(text).unwrap();
        assert_eq!(
            accounts,
            vec![
                Account {
                    host: "ghe.orbita.example".into(),
                    login: "lfarias".into()
                },
                Account {
                    host: "github.com".into(),
                    login: "mcouto".into()
                },
            ]
        );
    }

    #[test]
    fn parse_hosts_accepts_an_empty_file() {
        assert_eq!(parse_hosts("").unwrap(), Vec::new());
    }
}
