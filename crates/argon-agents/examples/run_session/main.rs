use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use argon_agents::{
    AgentEvent, AgentSessionHandle, AgentSpec, SessionContext, SessionOptions, SessionWriter,
    Support, Transport, WorktreeManager, agent_spec, agent_spec_for, list_models, lists_models,
    new_session_id, start_session_with, support_for,
};
use argon_core::{AgentKind, Config, Paths, PermissionMode, PrKey, RelatedRepo, SessionKind};
use argon_github::{GithubClient, PrDetail, discover_accounts, token_for};
use argon_store::Store;
use futures::StreamExt;
use tracing_subscriber::EnvFilter;

mod prompt;

use prompt::{node_id, render, render_summary};

const HOST: &str = "github.com";
const TEMPLATE_ID: &str = "review-padrao";
const SUMMARY_TEMPLATE_ID: &str = "resumo-padrao";
const RUN_TIMEOUT: Duration = Duration::from_secs(25 * 60);
const PREVIEW_CHARS: usize = 160;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

struct Args {
    kind: AgentKind,
    repo: String,
    number: u64,
    model: Option<String>,
    cancel_after: Option<Duration>,
    startup_timeout: Option<Duration>,
    transport: Option<Transport>,
    prompt: Option<String>,
    mode: Option<PermissionMode>,
    session_kind: SessionKind,
    related: Vec<String>,
    command: Option<Vec<String>>,
}

fn usage() -> String {
    "usage: run_session <claude|codex|opencode|command-code> <owner/repo#n> [model] \
     [--cancel-after secs] [--startup-timeout secs] \
     [--transport acp|claude-stream|codex-app-server|opencode-serve] \
     [--prompt text] [--mode supervised|accept-edits|full-access] \
     [--kind review|summary] [--related owner/repo]... [--command program args...]"
        .to_owned()
}

fn parse_kind(value: Option<String>) -> Result<SessionKind, BoxError> {
    match value.as_deref() {
        Some("review") => Ok(SessionKind::Review),
        Some("summary") => Ok(SessionKind::Summary),
        _ => Err(usage().into()),
    }
}

fn parse_mode(value: Option<String>) -> Result<PermissionMode, BoxError> {
    match value.as_deref() {
        Some("supervised") => Ok(PermissionMode::Supervised),
        Some("accept-edits") => Ok(PermissionMode::AcceptEdits),
        Some("full-access") => Ok(PermissionMode::FullAccess),
        _ => Err(usage().into()),
    }
}

fn seconds(value: Option<String>) -> Result<Duration, BoxError> {
    Ok(Duration::from_secs(value.ok_or_else(usage)?.parse()?))
}

fn parse_transport(value: Option<String>) -> Result<Transport, BoxError> {
    value
        .as_deref()
        .and_then(Transport::parse)
        .filter(|transport| *transport != Transport::Headless)
        .ok_or_else(|| usage().into())
}

fn parse_args() -> Result<Args, BoxError> {
    let mut positional = Vec::new();
    let mut cancel_after = None;
    let mut startup_timeout = None;
    let mut transport = None;
    let mut prompt = None;
    let mut mode = None;
    let mut session_kind = SessionKind::Review;
    let mut related = Vec::new();
    let mut command = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cancel-after" => cancel_after = Some(seconds(args.next())?),
            "--startup-timeout" => startup_timeout = Some(seconds(args.next())?),
            "--transport" => transport = Some(parse_transport(args.next())?),
            "--prompt" => prompt = Some(args.next().ok_or_else(usage)?),
            "--mode" => mode = Some(parse_mode(args.next())?),
            "--kind" => session_kind = parse_kind(args.next())?,
            "--related" => related.push(args.next().ok_or_else(usage)?),
            "--command" => command = Some(args.by_ref().collect()),
            _ => positional.push(arg),
        }
    }
    let kind = match positional.first().map(String::as_str) {
        Some("claude" | "claude-code") => AgentKind::ClaudeCode,
        Some("codex") => AgentKind::Codex,
        Some("opencode") => AgentKind::OpenCode,
        Some("command-code" | "commandcode") => AgentKind::CommandCode,
        _ => return Err(usage().into()),
    };
    let target = positional.get(1).ok_or_else(usage)?;
    let (repo, number) = target.split_once('#').ok_or_else(usage)?;
    Ok(Args {
        kind,
        repo: repo.to_owned(),
        number: number.parse()?,
        model: positional.get(2).cloned(),
        cancel_after,
        startup_timeout,
        transport,
        prompt,
        mode,
        session_kind,
        related,
        command,
    })
}

fn preview(text: &str) -> String {
    let flat = text.replace(['\n', '\r'], " ");
    let cut: String = flat.chars().take(PREVIEW_CHARS).collect();
    if flat.chars().count() > PREVIEW_CHARS {
        format!("{cut}…")
    } else {
        cut
    }
}

fn print_event(started: Instant, event: &AgentEvent) {
    let at = format!("{:>7.1}s", started.elapsed().as_secs_f64());
    match event {
        AgentEvent::MessageChunk(text) => println!("{at} message  {}", preview(text)),
        AgentEvent::ThoughtChunk(text) => println!("{at} thought  {}", preview(text)),
        AgentEvent::UserMessage(text) => println!("{at} user     {} chars", text.chars().count()),
        AgentEvent::ModelsAvailable(models) => println!("{at} models   {} available", models.len()),
        AgentEvent::Findings { findings, skipped } => {
            println!(
                "{at} findings {} (skipped {})",
                findings.len(),
                skipped.len()
            );
            for finding in findings {
                println!(
                    "          [{:?}] {}:{} {}",
                    finding.finding.severity,
                    finding.finding.file,
                    finding.finding.line,
                    finding.finding.title
                );
            }
            for reason in skipped {
                println!("          skipped: {reason}");
            }
        }
        other => println!(
            "{at} {}",
            preview(&serde_json::to_string(other).unwrap_or_default())
        ),
    }
}

fn deny(handle: &AgentSessionHandle, event: &AgentEvent) {
    let AgentEvent::PermissionRequest {
        request_id,
        options,
        ..
    } = event
    else {
        return;
    };
    let reject = options
        .iter()
        .find(|option| option.kind == "reject_once")
        .or_else(|| {
            options
                .iter()
                .find(|option| option.kind.starts_with("reject"))
        });
    let result = match reject {
        Some(option) => handle.respond_permission(request_id, &option.id),
        None => handle.cancel(),
    };
    println!("          denied {request_id}: {result:?}");
}

async fn drive(
    handle: &AgentSessionHandle,
    prompt: String,
    cancel_after: Option<Duration>,
) -> Result<(), BoxError> {
    let started = Instant::now();
    let (snapshot, events) = handle.subscribe();
    println!(
        "snapshot: status={:?} model={} models={} last_seq={}",
        snapshot.status,
        snapshot.model,
        snapshot.models.len(),
        snapshot.last_seq
    );
    let mut events = Box::pin(events);
    handle.prompt(prompt)?;
    if let Some(delay) = cancel_after {
        let canceller = handle.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            println!("--- cancelling after {delay:?}");
            let _ = canceller.cancel();
        });
    }
    let mut ended = false;
    loop {
        let next = tokio::time::timeout(RUN_TIMEOUT, events.next()).await?;
        let Some((_, event)) = next else { break };
        print_event(started, &event);
        deny(handle, &event);
        match &event {
            AgentEvent::TurnEnded { stop_reason } => {
                ended = true;
                if stop_reason != "end_turn" {
                    break;
                }
            }
            AgentEvent::Findings { .. } | AgentEvent::Summary { .. } => break,
            AgentEvent::Error(_) if ended || handle.is_finished() => break,
            _ => {}
        }
    }
    Ok(())
}

fn summarize(paths: &Paths, id: &str, transactions: u64) -> Result<(), BoxError> {
    let store = Store::open(&paths.db_file())?;
    let session = store.session(id)?.ok_or("session not persisted")?;
    let events: Vec<AgentEvent> = store.events(id)?;
    let findings = store.findings(id)?;
    let summaries = store.summaries_for_session(id)?;
    println!(
        "persisted: status={:?} model={} acp_session={:?} events={} findings={} summaries={} related={} turn_transactions={transactions}",
        session.status,
        session.model,
        session.acp_session_id,
        events.len(),
        findings.len(),
        summaries.len(),
        session.related_worktrees.len()
    );
    Ok(())
}

fn native_command(transport: Transport) -> Option<&'static str> {
    match transport {
        Transport::ClaudeStream => Some("claude"),
        Transport::CodexAppServer => Some("codex"),
        Transport::OpencodeServe => Some("opencode"),
        Transport::Acp | Transport::Headless => None,
    }
}

fn with_transport(mut spec: AgentSpec, args: &Args, config: &Config) -> AgentSpec {
    let Some(transport) = args
        .transport
        .filter(|transport| *transport != spec.transport)
    else {
        return spec;
    };
    if let Some(declared) = agent_spec_for(args.kind, config, transport) {
        return declared;
    }
    spec.transport = transport;
    if args.command.is_none()
        && let Some(program) = native_command(transport)
    {
        spec.command = PathBuf::from(program);
        spec.args = Vec::new();
    }
    spec
}

fn options(args: &Args) -> SessionOptions {
    let mut options = SessionOptions::default();
    if let Some(timeout) = args.startup_timeout {
        options.startup_timeout = timeout;
    }
    options
}

async fn startup_probe(
    args: &Args,
    mut spec: AgentSpec,
    paths: &Paths,
    command: &[String],
) -> Result<(), BoxError> {
    let (program, rest) = command.split_first().ok_or_else(usage)?;
    spec.command = PathBuf::from(program);
    spec.args = rest.to_vec();
    let cwd = paths.data_dir.join("startup-probe");
    std::fs::create_dir_all(&cwd)?;
    let writer = SessionWriter::open(&paths.db_file())?;
    let ctx = SessionContext {
        id: new_session_id()?,
        pr: PrKey::new(HOST, "startup-probe"),
        template_id: TEMPLATE_ID.to_owned(),
        nonce: String::new(),
        allowed_files: Vec::new(),
        cwd,
        read_roots: Vec::new(),
        local_roots: Vec::new(),
        repo: String::new(),
        number: 0,
        refresh_project: false,
        model: String::new(),
        permission_mode: PermissionMode::Supervised,
        kind: SessionKind::Review,
        base_sha: None,
    };
    let started = Instant::now();
    let result = start_session_with(&spec, ctx, writer, options(args)).await;
    println!(
        "startup result after {:.1?}: {:?}",
        started.elapsed(),
        result.as_ref().map(|(session, _)| session.id.clone())
    );
    Ok(())
}

async fn first_listed_model(spec: &AgentSpec) -> Option<String> {
    if !lists_models(spec.kind) {
        return None;
    }
    match list_models(spec).await? {
        Ok(models) => {
            let first = models.into_iter().next()?;
            println!("model: {} (first listed)", first.id);
            Some(first.id)
        }
        Err(error) => {
            println!("model listing failed: {error}");
            None
        }
    }
}

async fn run() -> Result<(), BoxError> {
    let args = parse_args()?;
    if let Support::NotYet { reason } = support_for(args.kind) {
        return Err(reason.into());
    }
    let paths = Paths::resolve();
    std::fs::create_dir_all(&paths.data_dir)?;
    let config = Config::load(&paths.config_file()).unwrap_or_default();
    let spec = with_transport(agent_spec(args.kind, &config), &args, &config);
    if let Some(command) = &args.command {
        return startup_probe(&args, spec, &paths, command).await;
    }

    let accounts = discover_accounts()?;
    let account = accounts
        .iter()
        .find(|account| account.host == HOST)
        .ok_or("no gh account for github.com")?;
    let client = GithubClient::new(&account.host, token_for(&account.host).await?);
    let detail = client
        .pr_detail(&node_id(&args.repo, args.number).await?)
        .await?;

    let model = match args.model.clone() {
        Some(model) => model,
        None => first_listed_model(&spec)
            .await
            .unwrap_or_else(|| spec.default_model.clone()),
    };
    let id = new_session_id()?;
    let manager = WorktreeManager::new(&paths);
    let worktree: PathBuf = manager.prepare(HOST, &args.repo, args.number, &id).await?;
    println!("worktree: {}", worktree.display());
    let mut created = vec![worktree.clone()];
    let outcome = async {
        let launch = match args.session_kind {
            SessionKind::Summary => {
                summary_launch(&manager, &detail, &worktree, &id, &args.related, &paths).await?
            }
            SessionKind::Review | SessionKind::Plan => {
                let files = client.pr_files(&args.repo, args.number).await?;
                Launch {
                    rendered: render(&detail, &files, &paths.data_dir.join("prompts"))?,
                    template_id: TEMPLATE_ID,
                    allowed_files: files.iter().map(|file| file.path.clone()).collect(),
                    read_roots: Vec::new(),
                    base_sha: None,
                }
            }
        };
        created.extend(launch.read_roots.iter().cloned());
        for warning in &launch.rendered.warnings {
            println!("prompt warning: {warning}");
        }
        let writer = SessionWriter::open(&paths.db_file())?;
        let ctx = SessionContext {
            id: id.clone(),
            pr: detail.pr.key(),
            template_id: launch.template_id.to_owned(),
            nonce: launch.rendered.nonce.clone(),
            allowed_files: launch.allowed_files,
            cwd: worktree.clone(),
            read_roots: launch.read_roots,
            local_roots: Vec::new(),
            repo: args.repo.clone(),
            number: args.number,
            refresh_project: args.session_kind == SessionKind::Summary,
            model,
            permission_mode: args.mode.unwrap_or_else(|| {
                config
                    .agents
                    .get(&args.kind)
                    .map(|agent| agent.permission_mode())
                    .unwrap_or_default()
            }),
            kind: args.session_kind,
            base_sha: launch.base_sha,
        };
        let prompt = args.prompt.clone().unwrap_or(launch.rendered.text);
        match start_session_with(&spec, ctx, writer.clone(), options(&args)).await {
            Ok((session, handle)) => {
                println!("session: {} (acp {})", session.id, session.agent_session_id);
                let before = writer.transactions();
                let driven = drive(&handle, prompt, args.cancel_after).await;
                writer.flush().await;
                let transactions = writer.transactions() - before;
                handle.shutdown().await;
                println!("shutdown: finished={}", handle.is_finished());
                driven.and_then(|()| summarize(&paths, &id, transactions))
            }
            Err(error) => Err(error.into()),
        }
    }
    .await;
    for path in &created {
        if let Err(error) = manager.remove(path).await {
            println!("failed to remove {}: {error}", path.display());
        }
    }
    println!(
        "worktrees removed: {}",
        created.iter().all(|path| !path.exists())
    );
    outcome
}

struct Launch {
    rendered: argon_core::RenderedPrompt,
    template_id: &'static str,
    allowed_files: Vec<String>,
    read_roots: Vec<PathBuf>,
    base_sha: Option<String>,
}

async fn summary_launch(
    manager: &WorktreeManager,
    detail: &PrDetail,
    worktree: &std::path::Path,
    session: &str,
    repos: &[String],
    paths: &Paths,
) -> Result<Launch, BoxError> {
    let head = manager.head_oid(worktree).await?;
    let mut related = Vec::new();
    let mut read_roots = Vec::new();
    for repo in repos {
        match manager.prepare_related(HOST, repo, session).await {
            Ok(prepared) => {
                println!("related: {repo} -> {}", prepared.path.display());
                related.push(RelatedRepo {
                    repo: repo.clone(),
                    path: Some(prepared.path.to_string_lossy().into_owned()),
                    branch: prepared.branch,
                    error: None,
                    local: false,
                });
                read_roots.push(prepared.path);
            }
            Err(error) => {
                println!("related {repo} unavailable: {error}");
                related.push(RelatedRepo {
                    repo: repo.clone(),
                    path: None,
                    branch: String::new(),
                    error: Some(error.to_string()),
                    local: false,
                });
            }
        }
    }
    let rendered = match render_summary(detail, &head, related, &paths.data_dir.join("prompts")) {
        Ok(rendered) => rendered,
        Err(error) => {
            for root in &read_roots {
                let _ = manager.remove(root).await;
            }
            return Err(error);
        }
    };
    Ok(Launch {
        rendered,
        template_id: SUMMARY_TEMPLATE_ID,
        allowed_files: Vec::new(),
        read_roots,
        base_sha: Some(head),
    })
}
#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("argon_agents=info")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
