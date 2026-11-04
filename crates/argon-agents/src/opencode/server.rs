use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use argon_core::Paths;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{ChildStderr, ChildStdout};

use crate::http::LocalApi;
use crate::process::{KillOnDropChild, spawn_in_job};
use crate::registry::AgentSpec;
use crate::session::StderrTail;
use crate::util::{Resolved, json_text, resolve_command};

const SERVE_ARGS: [&str; 6] = ["serve", "--stdio", "--port", "0", "--hostname", "127.0.0.1"];
const STARTUP_TIMEOUT: Duration = Duration::from_secs(90);
const PROBE_INTERVAL: Duration = Duration::from_millis(500);
const EXIT_GRACE: Duration = Duration::from_millis(500);
const STDERR_LINES_IN_ERROR: usize = 10;
const PASSWORD_BYTES: usize = 24;
const REQUIRED_MAJOR: u64 = 2;
const PASSWORD_ENV: &str = "OPENCODE_PASSWORD";
const CONFIG_DIR_ENV: &str = "OPENCODE_CONFIG_DIR";
const DISABLE_PROJECT_CONFIG_ENV: &str = "OPENCODE_DISABLE_PROJECT_CONFIG";
const DISABLE_AUTOUPDATE_ENV: &str = "OPENCODE_DISABLE_AUTOUPDATE";

pub(crate) struct OpencodeServer {
    pub api: LocalApi,
    child: KillOnDropChild,
}

impl OpencodeServer {
    pub async fn stop(mut self) {
        drop(self.child.stdin.take());
        if let Err(error) = self.child.kill_tree() {
            tracing::debug!(%error, "OpenCode server process tree already gone");
        }
        let _ = self.child.wait().await;
    }
}

pub(crate) fn config_dir() -> PathBuf {
    Paths::resolve().data_dir.join("agents").join("opencode")
}

pub(crate) async fn start(
    spec: &AgentSpec,
    cwd: &Path,
    stderr: Arc<StderrTail>,
) -> Result<OpencodeServer, String> {
    let program = match resolve_command(&spec.command) {
        Resolved::Path(path) => path,
        Resolved::Missing => return Err("opencode não encontrado no PATH".to_owned()),
        Resolved::Failed(message) => return Err(message),
    };
    let config_dir = config_dir();
    tokio::fs::create_dir_all(&config_dir)
        .await
        .map_err(|error| {
            format!(
                "falha ao criar a pasta de configuração do OpenCode ({}): {error}",
                config_dir.display()
            )
        })?;
    let password = new_password()?;
    let mut command = tokio::process::Command::new(program);
    command
        .args(&spec.args)
        .args(SERVE_ARGS)
        .envs(server_env(&spec.env, &config_dir, &password))
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn_in_job(&mut command)
        .map_err(|error| format!("falha ao iniciar o OpenCode: {error}"))?;
    let stdout = child.stdout.take().ok_or("o OpenCode não expôs o stdout")?;
    if let Some(child_stderr) = child.stderr.take() {
        drain_stderr(child_stderr, Arc::clone(&stderr));
    }
    let ready = tokio::time::timeout(
        STARTUP_TIMEOUT,
        connect(&mut child, stdout, &password, cwd, &stderr),
    )
    .await;
    match ready {
        Ok(Ok(api)) => Ok(OpencodeServer { api, child }),
        Ok(Err(message)) => {
            kill(&mut child).await;
            Err(message)
        }
        Err(_) => {
            kill(&mut child).await;
            Err(format!(
                "o OpenCode não iniciou em {}s",
                STARTUP_TIMEOUT.as_secs()
            ))
        }
    }
}

async fn connect(
    child: &mut KillOnDropChild,
    stdout: ChildStdout,
    password: &str,
    cwd: &Path,
    stderr: &StderrTail,
) -> Result<LocalApi, String> {
    let mut lines = BufReader::new(stdout).lines();
    let port = read_port(&mut lines, child, stderr).await?;
    drain_stdout(lines);
    let api = LocalApi::new(
        format!("http://127.0.0.1:{port}"),
        password,
        cwd.display().to_string(),
    )?;
    probe(&api, child, stderr).await?;
    Ok(api)
}

async fn read_port(
    lines: &mut Lines<BufReader<ChildStdout>>,
    child: &mut KillOnDropChild,
    stderr: &StderrTail,
) -> Result<u16, String> {
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if let Some(port) = port_from_line(&line) {
                    return Ok(port);
                }
            }
            Ok(None) | Err(_) => return Err(exited_early(child, stderr).await),
        }
    }
}

async fn probe(
    api: &LocalApi,
    child: &mut KillOnDropChild,
    stderr: &StderrTail,
) -> Result<(), String> {
    loop {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return Err(exited_early(child, stderr).await);
        }
        if let Ok(info) = api.get("/api/info").await
            && let Some(version) = json_text(&info, "version").filter(|text| !text.is_empty())
        {
            return check_version(&version);
        }
        tokio::time::sleep(PROBE_INTERVAL).await;
    }
}

fn check_version(version: &str) -> Result<(), String> {
    match major_version(version) {
        Some(major) if major >= REQUIRED_MAJOR => Ok(()),
        _ => Err(format!(
            "OpenCode {version} não suportado; o Argon requer a {REQUIRED_MAJOR}.x"
        )),
    }
}

async fn exited_early(child: &mut KillOnDropChild, stderr: &StderrTail) -> String {
    let _ = tokio::time::timeout(EXIT_GRACE, child.wait()).await;
    let tail = stderr.last(STDERR_LINES_IN_ERROR).join("\n");
    if tail.is_empty() {
        "o OpenCode encerrou antes de ficar pronto".to_owned()
    } else {
        format!("o OpenCode encerrou antes de ficar pronto:\n{tail}")
    }
}

async fn kill(child: &mut KillOnDropChild) {
    if let Err(error) = child.kill_tree() {
        tracing::debug!(%error, "OpenCode server process tree already gone");
    }
    let _ = child.wait().await;
}

fn drain_stdout(mut lines: Lines<BufReader<ChildStdout>>) {
    tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
}

fn drain_stderr(stderr: ChildStderr, tail: Arc<StderrTail>) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(target: "argon_agents::agent_stderr", "{line}");
            tail.push(line);
        }
    });
}

fn new_password() -> Result<String, String> {
    let mut bytes = [0u8; PASSWORD_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|error| format!("falha ao gerar a senha do servidor: {error}"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn port_from_line(line: &str) -> Option<u16> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    let url = reqwest::Url::parse(&json_text(&value, "url")?).ok()?;
    url.port().filter(|port| *port > 0)
}

fn major_version(version: &str) -> Option<u64> {
    let trimmed = version.trim().trim_start_matches(['v', 'V']);
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn server_env(
    spec_env: &BTreeMap<String, String>,
    config_dir: &Path,
    password: &str,
) -> BTreeMap<String, String> {
    let mut env = spec_env.clone();
    env.entry(CONFIG_DIR_ENV.to_owned())
        .or_insert_with(|| config_dir.display().to_string());
    for key in [DISABLE_PROJECT_CONFIG_ENV, DISABLE_AUTOUPDATE_ENV] {
        env.entry(key.to_owned())
            .or_insert_with(|| "true".to_owned());
    }
    env.insert(PASSWORD_ENV.to_owned(), password.to_owned());
    env
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
