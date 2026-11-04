use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use argon_core::Paths;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use tokio::process::{ChildStderr, ChildStdout};

use crate::http::LocalApi;
use crate::process::{KillOnDropChild, spawn_in_job};
use crate::registry::AgentSpec;
use crate::session::StderrTail;
use crate::util::{Resolved, json_text, lock, resolve_command};

const SERVE_ARGS: [&str; 6] = ["serve", "--stdio", "--port", "0", "--hostname", "127.0.0.1"];
const STARTUP_TIMEOUT: Duration = Duration::from_secs(90);
const PROBE_INTERVAL: Duration = Duration::from_millis(500);
const EXIT_GRACE: Duration = Duration::from_millis(500);
const STDERR_LINES_IN_ERROR: usize = 10;
const MAX_LINE_BYTES: usize = 64 * 1024;
const REDACTED: &str = "[redacted]";
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
    let startup = Startup {
        password: &password,
        stderr: &stderr,
        last_probe_error: Mutex::new(None),
    };
    let ready =
        tokio::time::timeout(STARTUP_TIMEOUT, startup.connect(&mut child, stdout, cwd)).await;
    match ready {
        Ok(Ok(api)) => Ok(OpencodeServer { api, child }),
        Ok(Err(message)) => {
            kill(&mut child).await;
            Err(message)
        }
        Err(_) => {
            kill(&mut child).await;
            Err(startup.timeout_message())
        }
    }
}

struct Startup<'a> {
    password: &'a str,
    stderr: &'a StderrTail,
    last_probe_error: Mutex<Option<String>>,
}

impl Startup<'_> {
    async fn connect(
        &self,
        child: &mut KillOnDropChild,
        stdout: ChildStdout,
        cwd: &Path,
    ) -> Result<LocalApi, String> {
        let mut reader = BufReader::new(stdout);
        let port = self.read_port(&mut reader, child).await?;
        drain_stdout(reader);
        let api = LocalApi::new(
            format!("http://127.0.0.1:{port}"),
            self.password,
            cwd.display().to_string(),
        )?;
        self.probe(&api, child).await?;
        Ok(api)
    }

    async fn read_port(
        &self,
        reader: &mut BufReader<ChildStdout>,
        child: &mut KillOnDropChild,
    ) -> Result<u16, String> {
        loop {
            match read_line_lossy(reader).await {
                Ok(Some(line)) => {
                    if let Some(port) = port_from_line(&line) {
                        return Ok(port);
                    }
                }
                Ok(None) | Err(_) => return Err(self.exited_early(child).await),
            }
        }
    }

    async fn probe(&self, api: &LocalApi, child: &mut KillOnDropChild) -> Result<(), String> {
        loop {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return Err(self.exited_early(child).await);
            }
            match api.get("/api/info").await {
                Ok(info) => {
                    if let Some(version) =
                        json_text(&info, "version").filter(|text| !text.is_empty())
                    {
                        return check_version(&version);
                    }
                    self.note_probe_error("resposta sem versão".to_owned());
                }
                Err(error) => self.note_probe_error(error),
            }
            tokio::time::sleep(PROBE_INTERVAL).await;
        }
    }

    fn note_probe_error(&self, error: String) {
        *lock(&self.last_probe_error) = Some(error);
    }

    fn timeout_message(&self) -> String {
        let base = format!("o OpenCode não iniciou em {}s", STARTUP_TIMEOUT.as_secs());
        match lock(&self.last_probe_error).as_deref() {
            Some(error) => format!("{base} (último erro: {})", self.scrub(error)),
            None => base,
        }
    }

    async fn exited_early(&self, child: &mut KillOnDropChild) -> String {
        let _ = tokio::time::timeout(EXIT_GRACE, child.wait()).await;
        let tail = self.scrub(&self.stderr.last(STDERR_LINES_IN_ERROR).join("\n"));
        if tail.is_empty() {
            "o OpenCode encerrou antes de ficar pronto".to_owned()
        } else {
            format!("o OpenCode encerrou antes de ficar pronto:\n{tail}")
        }
    }

    fn scrub(&self, text: &str) -> String {
        text.replace(self.password, REDACTED)
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

async fn kill(child: &mut KillOnDropChild) {
    if let Err(error) = child.kill_tree() {
        tracing::debug!(%error, "OpenCode server process tree already gone");
    }
    let _ = child.wait().await;
}

async fn read_line_lossy<R: AsyncBufRead + Unpin>(reader: &mut R) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    let mut any_bytes = false;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            break;
        }
        any_bytes = true;
        let (consumed, complete) = match chunk.iter().position(|byte| *byte == b'\n') {
            Some(index) => (index + 1, true),
            None => (chunk.len(), false),
        };
        let room = MAX_LINE_BYTES.saturating_sub(line.len());
        line.extend_from_slice(&chunk[..consumed.min(room)]);
        reader.consume(consumed);
        if complete {
            break;
        }
    }
    if !any_bytes {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&line);
    Ok(Some(text.trim_end_matches(['\r', '\n']).to_owned()))
}

fn drain_stdout(mut reader: BufReader<ChildStdout>) {
    tokio::spawn(async move {
        let _ = tokio::io::copy(&mut reader, &mut tokio::io::sink()).await;
    });
}

fn drain_stderr(stderr: ChildStderr, tail: Arc<StderrTail>) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        while let Ok(Some(line)) = read_line_lossy(&mut reader).await {
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

fn has_env_key(env: &BTreeMap<String, String>, key: &str) -> bool {
    env.keys()
        .any(|existing| existing.eq_ignore_ascii_case(key))
}

fn server_env(
    spec_env: &BTreeMap<String, String>,
    config_dir: &Path,
    password: &str,
) -> BTreeMap<String, String> {
    let mut env = spec_env.clone();
    env.retain(|key, _| !key.eq_ignore_ascii_case(PASSWORD_ENV));
    let defaults = [
        (CONFIG_DIR_ENV, config_dir.display().to_string()),
        (DISABLE_PROJECT_CONFIG_ENV, "true".to_owned()),
        (DISABLE_AUTOUPDATE_ENV, "true".to_owned()),
    ];
    for (key, value) in defaults {
        if !has_env_key(&env, key) {
            env.insert(key.to_owned(), value);
        }
    }
    env.insert(PASSWORD_ENV.to_owned(), password.to_owned());
    env
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
