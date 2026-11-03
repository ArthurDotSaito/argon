use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use argon_agents::{
    AgentEvent, AgentSessionHandle, SessionContext, StoreSink, Support, WorktreeManager,
    agent_spec, new_session_id, start_session, support_for,
};
use argon_core::{
    AgentKind, Config, Paths, PrId, PromptContext, RenderedPrompt, ReviewCfg,
    ensure_default_templates, load_templates, render_prompt,
};
use argon_github::{GithubClient, PrDetail, PrFile, discover_accounts, token_for};
use argon_store::Store;
use futures::StreamExt;
use tokio::process::Command;
use tracing_subscriber::EnvFilter;

const HOST: &str = "github.com";
const TEMPLATE_ID: &str = "review-padrao";
const RUN_TIMEOUT: Duration = Duration::from_secs(25 * 60);
const PREVIEW_CHARS: usize = 160;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

struct Args {
    kind: AgentKind,
    repo: String,
    number: u64,
    model: Option<String>,
    cancel_after: Option<Duration>,
}

fn usage() -> String {
    "usage: run_session <claude|codex|opencode|command-code> <owner/repo#n> [model] [--cancel-after secs]"
        .to_owned()
}

fn parse_args() -> Result<Args, BoxError> {
    let mut positional = Vec::new();
    let mut cancel_after = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--cancel-after" {
            let secs: u64 = args.next().ok_or_else(usage)?.parse()?;
            cancel_after = Some(Duration::from_secs(secs));
        } else {
            positional.push(arg);
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
    })
}

async fn node_id(repo: &str, number: u64) -> Result<PrId, BoxError> {
    let output = Command::new("gh")
        .args([
            "api",
            &format!("repos/{repo}/pulls/{number}"),
            "--jq",
            ".node_id",
        ])
        .output()
        .await?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr)
            .trim()
            .to_owned()
            .into());
    }
    Ok(PrId(String::from_utf8(output.stdout)?.trim().to_owned()))
}

fn unified_diff(files: &[PrFile]) -> String {
    files
        .iter()
        .filter_map(|file| {
            let patch = file.patch.as_ref()?;
            let old = file.previous_path.as_deref().unwrap_or(&file.path);
            Some(format!(
                "diff --git a/{old} b/{path}\n--- a/{old}\n+++ b/{path}\n{patch}\n",
                path = file.path
            ))
        })
        .collect()
}

fn render(
    detail: &PrDetail,
    files: &[PrFile],
    templates_dir: &Path,
) -> Result<RenderedPrompt, BoxError> {
    ensure_default_templates(templates_dir)?;
    let template = load_templates(templates_dir)?
        .into_iter()
        .find(|template| template.id == TEMPLATE_ID)
        .ok_or("template review-padrao not found")?;
    let ctx = PromptContext {
        pr_title: detail.pr.title.clone(),
        pr_number: detail.pr.number,
        pr_url: detail.pr.url.clone(),
        pr_body: detail.body.clone(),
        repo: detail.pr.repo.clone(),
        base: detail.pr.base.clone(),
        base_sha: None,
        head: detail.pr.head.clone(),
        diff: unified_diff(files),
        files: files.iter().map(|file| file.path.clone()).collect(),
        previous_reviews: String::new(),
        ci_failures: String::new(),
    };
    Ok(render_prompt(
        &template,
        &ctx,
        ReviewCfg::default().max_diff_chars,
    )?)
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
                    finding.severity, finding.file, finding.line, finding.title
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

async fn deny(handle: &AgentSessionHandle, event: &AgentEvent) {
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
        Some(option) => handle.respond_permission(request_id, &option.id).await,
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
    let mut events = Box::pin(handle.events());
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
        let Some(event) = next else { break };
        print_event(started, &event);
        deny(handle, &event).await;
        match &event {
            AgentEvent::TurnEnded { stop_reason } => {
                ended = true;
                if stop_reason != "end_turn" {
                    break;
                }
            }
            AgentEvent::Findings { .. } => break,
            AgentEvent::Error(_) if ended || handle.is_finished() => break,
            _ => {}
        }
    }
    Ok(())
}

fn summarize(store: &Mutex<Store>, id: &str) -> Result<(), BoxError> {
    let store = store.lock().map_err(|_| "store poisoned")?;
    let session = store.session(id)?.ok_or("session not persisted")?;
    let events: Vec<AgentEvent> = store.events(id)?;
    let findings = store.findings(id)?;
    println!(
        "persisted: status={:?} model={} acp_session={:?} events={} findings={}",
        session.status,
        session.model,
        session.acp_session_id,
        events.len(),
        findings.len()
    );
    Ok(())
}

async fn run() -> Result<(), BoxError> {
    let args = parse_args()?;
    if let Support::NotYet { reason } = support_for(args.kind) {
        return Err(reason.into());
    }
    let paths = Paths::resolve();
    std::fs::create_dir_all(&paths.data_dir)?;
    let config = Config::load(&paths.config_file()).unwrap_or_default();
    let spec = agent_spec(args.kind, &config);

    let accounts = discover_accounts()?;
    let account = accounts
        .iter()
        .find(|account| account.host == HOST)
        .ok_or("no gh account for github.com")?;
    let client = GithubClient::new(&account.host, token_for(&account.host).await?);
    let detail = client
        .pr_detail(&node_id(&args.repo, args.number).await?)
        .await?;
    let files = client.pr_files(&args.repo, args.number).await?;
    let rendered = render(&detail, &files, &paths.data_dir.join("prompts"))?;
    for warning in &rendered.warnings {
        println!("prompt warning: {warning}");
    }

    let id = new_session_id()?;
    let manager = WorktreeManager::new(&paths);
    let worktree: PathBuf = manager.prepare(HOST, &args.repo, args.number, &id).await?;
    println!("worktree: {}", worktree.display());
    let store = Arc::new(Mutex::new(Store::open(&paths.db_file())?));
    let sink: Arc<dyn StoreSink> = store.clone();
    let ctx = SessionContext {
        id: id.clone(),
        pr: detail.pr.key(),
        template_id: TEMPLATE_ID.to_owned(),
        nonce: rendered.nonce.clone(),
        allowed_files: files.iter().map(|file| file.path.clone()).collect(),
        cwd: worktree.clone(),
        model: args
            .model
            .clone()
            .unwrap_or_else(|| spec.default_model.clone()),
    };

    let outcome = match start_session(&spec, ctx, sink).await {
        Ok((session, handle)) => {
            println!("session: {} (acp {})", session.id, session.acp_session_id);
            let driven = drive(&handle, rendered.text, args.cancel_after).await;
            handle.shutdown().await;
            println!("shutdown: finished={}", handle.is_finished());
            driven.and_then(|()| summarize(&store, &id))
        }
        Err(error) => Err(error.into()),
    };
    manager.remove(&worktree).await?;
    println!("worktree removed: {}", !worktree.exists());
    outcome
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
