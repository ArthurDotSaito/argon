use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::v1::schema::{
    ClientCapabilities, ContentBlock, Implementation, InitializeRequest, PermissionOptionKind,
    PromptRequest, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionId, SessionNotification, TextContent,
};
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, Client, LineDirection, UntypedMessage,
};
use argon_agents::defaults::{AgentDefaults, ModelSelection, Transport, agent_defaults};
use argon_core::AgentKind;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};

const PROMPT: &str = "Liste os arquivos do diretório atual e responda em uma frase.";
const TIMEOUT: Duration = Duration::from_secs(120);
const PREVIEW_CHARS: usize = 600;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

struct Options {
    kind: AgentKind,
    model: Option<String>,
    mode: Option<String>,
    extra: Vec<String>,
    load: Option<String>,
    cwd: PathBuf,
    headless: bool,
    prompt: bool,
}

fn scratch_dir() -> PathBuf {
    std::env::temp_dir().join("agents-scratchpad")
}

fn parse_kind(name: &str) -> Option<AgentKind> {
    match name {
        "claude" | "claude-code" => Some(AgentKind::ClaudeCode),
        "codex" => Some(AgentKind::Codex),
        "opencode" => Some(AgentKind::OpenCode),
        "command-code" | "commandcode" => Some(AgentKind::CommandCode),
        "agy" | "antigravity" => Some(AgentKind::Antigravity),
        _ => None,
    }
}

fn parse_options() -> Result<Options, BoxError> {
    let mut args = std::env::args().skip(1);
    let mut kind = None;
    let mut model = None;
    let mut mode = None;
    let mut extra = Vec::new();
    let mut load = None;
    let mut cwd = scratch_dir().join("acp-probe-cwd");
    let mut headless = false;
    let mut prompt = true;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--model" => model = args.next(),
            "--mode" => mode = args.next(),
            "--extra" => extra.push(args.next().ok_or("--extra needs a value")?),
            "--load" => load = args.next(),
            "--cwd" => cwd = PathBuf::from(args.next().ok_or("--cwd needs a value")?),
            "--headless" => headless = true,
            "--no-prompt" => prompt = false,
            other => {
                kind = Some(parse_kind(other).ok_or_else(|| format!("unknown agent {other}"))?)
            }
        }
    }
    let kind = kind.ok_or(
        "usage: acp_probe <claude|codex|opencode|command-code|agy> [--headless] [--model id] [--mode id] [--extra arg] [--load session-id] [--no-prompt] [--cwd dir]",
    )?;
    std::fs::create_dir_all(&cwd)?;
    Ok(Options {
        kind,
        model,
        mode,
        extra,
        load,
        cwd: std::fs::canonicalize(&cwd).map(strip_verbatim)?,
        headless,
        prompt,
    })
}

fn strip_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path,
    }
}

fn preview(value: &impl serde::Serialize) -> String {
    let text = serde_json::to_string(value).unwrap_or_else(|error| format!("<{error}>"));
    if text.chars().count() > PREVIEW_CHARS {
        let cut: String = text.chars().take(PREVIEW_CHARS).collect();
        format!("{cut}…")
    } else {
        text
    }
}

fn log_path(name: &str, extension: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    scratch_dir().join(format!("acp-probe-{name}-{stamp}.{extension}"))
}

fn elapsed(start: Instant) -> String {
    format!("{:>6.2}s", start.elapsed().as_secs_f64())
}

fn acp_error(message: impl Into<String>) -> agent_client_protocol::Error {
    agent_client_protocol::Error::internal_error().data(message.into())
}

fn config_option(new_session: &Value, category: &str) -> Option<(String, Vec<String>)> {
    let options = new_session.get("configOptions")?.as_array()?;
    let option = options.iter().find(|option| {
        option.get("category").and_then(Value::as_str) == Some(category)
            || option.get("id").and_then(Value::as_str) == Some(category)
    })?;
    let id = option.get("id")?.as_str()?.to_string();
    let mut values = Vec::new();
    collect_values(option.get("options"), &mut values);
    Some((id, values))
}

fn collect_values(options: Option<&Value>, values: &mut Vec<String>) {
    let Some(items) = options.and_then(Value::as_array) else {
        return;
    };
    for item in items {
        if let Some(value) = item.get("value").and_then(Value::as_str) {
            values.push(value.to_string());
        }
        collect_values(item.get("options"), values);
    }
}

fn legacy_models(new_session: &Value) -> Option<(String, Vec<String>)> {
    let models = new_session.get("models")?;
    let current = models
        .get("currentModelId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let available = models
        .get("availableModels")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("modelId").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Some((current, available))
}

fn deny_permission(request: &RequestPermissionRequest) -> RequestPermissionResponse {
    let reject = [
        PermissionOptionKind::RejectOnce,
        PermissionOptionKind::RejectAlways,
    ]
    .iter()
    .find_map(|kind| {
        request
            .options
            .iter()
            .find(|option| option.kind == *kind)
            .map(|option| option.option_id.clone())
    });
    match reject {
        Some(id) => RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new(id),
        )),
        None => RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled),
    }
}

fn update_text(notification: &SessionNotification, kind: &str) -> Option<String> {
    let value = serde_json::to_value(&notification.update).ok()?;
    if value.get("sessionUpdate").and_then(Value::as_str) != Some(kind) {
        return None;
    }
    let content = value.get("content")?;
    content
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_string)
}

async fn run_acp(options: Options, defaults: &'static AgentDefaults) -> Result<(), BoxError> {
    let command = which::which(defaults.command)?;
    let raw_log_path = log_path(defaults.command, "log");
    let raw_log = Arc::new(Mutex::new(File::create(&raw_log_path)?));
    println!("agent      : {}", defaults.label);
    println!(
        "command    : {} {}",
        command.display(),
        defaults.args.join(" ")
    );
    println!("cwd        : {}", options.cwd.display());
    println!("raw log    : {}", raw_log_path.display());

    let start = Instant::now();
    let log_for_debug = Arc::clone(&raw_log);
    let agent = AcpAgent::new(
        AcpAgentConfig::new(&command)
            .args(defaults.args.iter().copied())
            .envs(defaults.env.iter().copied()),
    )
    .with_debug(move |line, direction| {
        let tag = match direction {
            LineDirection::Stdin => ">>",
            LineDirection::Stdout => "<<",
            LineDirection::Stderr => "!!",
        };
        if let Ok(mut file) = log_for_debug.lock() {
            let _ = writeln!(file, "{tag} {line}");
        }
        if direction == LineDirection::Stderr {
            eprintln!("[stderr] {line}");
        }
    });

    let answer = Arc::new(Mutex::new(String::new()));
    let answer_for_updates = Arc::clone(&answer);
    let cwd = options.cwd.clone();
    let model = options.model.clone();
    let mode = options.mode.clone();
    let load = options.load.clone();
    let send_prompt = options.prompt;

    let connection = Client
        .v1()
        .name("argon-acp-probe")
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                let update = serde_json::to_value(&notification.update).unwrap_or_default();
                let kind = update
                    .get("sessionUpdate")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_string();
                if let Some(text) = update_text(&notification, "agent_message_chunk")
                    && let Ok(mut answer) = answer_for_updates.lock()
                {
                    answer.push_str(&text);
                }
                println!("{} update   {kind}: {}", elapsed(start), preview(&update));
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _cx| {
                println!(
                    "{} PERMISSION request: {}",
                    elapsed(start),
                    preview(&request)
                );
                let response = deny_permission(&request);
                println!(
                    "{} PERMISSION reply  : {}",
                    elapsed(start),
                    preview(&response)
                );
                responder.respond(response)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |cx: ConnectionTo<Agent>| async move {
            let initialize = cx
                .send_request(
                    InitializeRequest::new(ProtocolVersion::V1)
                        .client_capabilities(ClientCapabilities::default())
                        .client_info(Implementation::new(
                            "argon-acp-probe",
                            env!("CARGO_PKG_VERSION"),
                        )),
                )
                .block_task()
                .await?;
            println!(
                "{} initialize agentInfo: {}",
                elapsed(start),
                preview(&initialize.agent_info)
            );
            println!(
                "{} initialize capabilities: {}",
                elapsed(start),
                preview(&initialize.agent_capabilities)
            );
            println!(
                "{} initialize authMethods: {}",
                elapsed(start),
                preview(&initialize.auth_methods)
            );

            let setup_params = json!({ "cwd": cwd, "mcpServers": [] });
            let (method, params) = match &load {
                Some(id) => {
                    let mut params = setup_params;
                    params["sessionId"] = json!(id);
                    ("session/load", params)
                }
                None => ("session/new", setup_params),
            };
            let session = cx
                .send_request(UntypedMessage::new(method, params)?)
                .block_task()
                .await?;
            let session_id = match &load {
                Some(id) => id.clone(),
                None => session
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| acp_error("session/new without sessionId"))?
                    .to_string(),
            };
            println!("{} {method} sessionId: {session_id}", elapsed(start));
            println!(
                "{} {method} modes: {}",
                elapsed(start),
                preview(&session.get("modes"))
            );
            println!(
                "{} {method} models (legacy): {}",
                elapsed(start),
                preview(&session.get("models"))
            );
            println!(
                "{} {method} configOptions: {}",
                elapsed(start),
                preview(&session.get("configOptions"))
            );
            if let Some((id, values)) = config_option(&session, "model") {
                println!("{} model config option '{id}': {values:?}", elapsed(start));
            }
            if let Some((current, values)) = legacy_models(&session) {
                println!(
                    "{} legacy models current={current}: {values:?}",
                    elapsed(start)
                );
            }

            let selections = [
                ("mode", &mode, "session/set_mode", "modeId"),
                ("model", &model, "session/set_model", "modelId"),
            ];
            for (category, wanted, legacy_method, legacy_field) in selections {
                let Some(wanted) = wanted else {
                    continue;
                };
                let (method, params) = match config_option(&session, category) {
                    Some((config_id, _)) => (
                        "session/set_config_option",
                        json!({ "sessionId": session_id, "configId": config_id, "value": wanted }),
                    ),
                    None => (
                        legacy_method,
                        json!({ "sessionId": session_id, legacy_field: wanted }),
                    ),
                };
                let reply = cx
                    .send_request(UntypedMessage::new(method, params)?)
                    .block_task()
                    .await;
                match reply {
                    Ok(value) => println!(
                        "{} {method} {category}={wanted} -> {}",
                        elapsed(start),
                        preview(&value)
                    ),
                    Err(error) => println!(
                        "{} {method} {category}={wanted} failed: {error}",
                        elapsed(start)
                    ),
                }
            }

            if send_prompt {
                println!("{} prompt: {PROMPT}", elapsed(start));
                let response = cx
                    .send_request(PromptRequest::new(
                        SessionId::new(session_id.as_str()),
                        vec![ContentBlock::Text(TextContent::new(PROMPT))],
                    ))
                    .block_task()
                    .await?;
                println!(
                    "{} prompt stopReason: {:?}",
                    elapsed(start),
                    response.stop_reason
                );
                println!("{} prompt response: {}", elapsed(start), preview(&response));
            }
            Ok(())
        });

    let outcome = tokio::time::timeout(TIMEOUT, connection).await;
    if let Ok(mut file) = raw_log.lock() {
        let _ = file.flush();
    }
    if let Ok(answer) = answer.lock()
        && !answer.is_empty()
    {
        println!("answer     : {}", answer.trim());
    }
    match outcome {
        Err(_) => Err(format!("timed out after {}s", TIMEOUT.as_secs()).into()),
        Ok(Err(error)) => Err(format!("acp error: {error}").into()),
        Ok(Ok(())) => {
            println!("{} done", elapsed(start));
            Ok(())
        }
    }
}

async fn run_headless(options: Options, defaults: &'static AgentDefaults) -> Result<(), BoxError> {
    let command = which::which(defaults.command)?;
    let flags = defaults.headless.ok_or("agent has no headless mode")?;
    let mut args: Vec<String> = Vec::new();
    if let (Some(model), ModelSelection::CliFlag { flag }) =
        (&options.model, defaults.model_selection)
    {
        args.push(flag.to_string());
        args.push(model.clone());
    }
    if let Some(mode) = &options.mode {
        args.push(flags.mode.to_string());
        args.push(mode.clone());
    }
    if let Some(id) = &options.load {
        args.push(flags.resume.to_string());
        args.push(id.clone());
    }
    args.extend(options.extra.iter().cloned());
    args.extend(defaults.args.iter().map(|arg| arg.to_string()));
    args.push(PROMPT.to_string());
    let output_path = log_path("agy-headless", "jsonl");
    let mut output = File::create(&output_path)?;
    println!("agent      : {}", defaults.label);
    println!("command    : {} {}", command.display(), args.join(" "));
    println!("cwd        : {}", options.cwd.display());
    println!("raw output : {}", output_path.display());

    let start = Instant::now();
    let mut child = tokio::process::Command::new(&command)
        .args(&args)
        .current_dir(&options.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("[stderr] {line}");
        }
    });
    let read = async {
        let mut lines = BufReader::new(stdout).lines();
        while let Some(line) = lines.next_line().await? {
            writeln!(output, "{line}")?;
            let kind = serde_json::from_str::<Value>(&line)
                .ok()
                .and_then(|value| {
                    ["event", "type"]
                        .iter()
                        .find_map(|key| value.get(*key).and_then(Value::as_str))
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "text".to_string());
            let shown: String = line.chars().take(PREVIEW_CHARS).collect();
            println!("{} line {kind}: {shown}", elapsed(start));
        }
        let status = child.wait().await?;
        Ok::<_, BoxError>(status)
    };
    let status = tokio::time::timeout(TIMEOUT, read)
        .await
        .map_err(|_| format!("timed out after {}s", TIMEOUT.as_secs()))??;
    let _ = stderr_task.await;
    println!("{} exit: {status}", elapsed(start));
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let options = parse_options()?;
    let defaults = agent_defaults(options.kind);
    if options.headless || defaults.transport == Transport::Headless {
        run_headless(options, defaults).await
    } else {
        run_acp(options, defaults).await
    }
}
