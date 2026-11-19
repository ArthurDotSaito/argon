use std::process::{ExitCode, Stdio};
use std::time::Instant;

use argon_agents::WorktreeManager;
use argon_core::Paths;
use tokio::process::Command;
use tracing_subscriber::EnvFilter;

const DEFAULT_HOST: &str = "github.com";
const RELATED_FLAG: &str = "--related";
const USAGE: &str = "usage: worktree <owner/repo> <pr-number> | worktree --related <owner/repo>";
const EXAMPLE_SESSION: &str = "example0-worktree";

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("argon_agents=debug")),
        )
        .with_target(false)
        .init();
    match run().await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<bool, String> {
    let mut args = std::env::args().skip(1);
    let host = std::env::var("GH_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());
    let manager = WorktreeManager::new(&Paths::resolve());
    let Some(first) = args.next() else {
        return Err(USAGE.to_string());
    };
    if first == RELATED_FLAG {
        let repo = args.next().ok_or_else(|| USAGE.to_string())?;
        return run_related(&manager, &host, &repo).await;
    }
    let repo = first;
    let number = args.next().ok_or_else(|| USAGE.to_string())?;
    let number: u64 = number
        .parse()
        .map_err(|_| format!("invalid PR number `{number}`"))?;

    let started = Instant::now();
    let path = manager
        .prepare(&host, &repo, number, EXAMPLE_SESSION)
        .await
        .map_err(|error| error.to_string())?;
    println!("prepared in {:.1?}: {}", started.elapsed(), path.display());

    let head = manager
        .head_oid(&path)
        .await
        .map_err(|error| error.to_string())?;
    let expected = expected_head(&repo, number).await?;
    let matches = head == expected;
    println!(
        "head_oid: {head} (gh headRefOid: {expected}) {}",
        if matches { "OK" } else { "FALHA" }
    );

    manager
        .remove(&path)
        .await
        .map_err(|error| error.to_string())?;
    let cleaned = !path.exists();
    println!("removed, worktree dir gone: {cleaned}");
    Ok(matches && cleaned)
}

async fn run_related(manager: &WorktreeManager, host: &str, repo: &str) -> Result<bool, String> {
    let planned = manager
        .planned_related_path(host, repo, EXAMPLE_SESSION)
        .map_err(|error| error.to_string())?;
    let started = Instant::now();
    let prepared = manager
        .prepare_related(host, repo, EXAMPLE_SESSION)
        .await
        .map_err(|error| error.to_string())?;
    println!(
        "prepared in {:.1?}: {} @ {} ({})",
        started.elapsed(),
        prepared.path.display(),
        prepared.base_sha,
        prepared.branch
    );
    let planned_matches = prepared.path == planned;
    println!(
        "planned path matches: {}",
        if planned_matches { "OK" } else { "FALHA" }
    );

    manager
        .remove(&prepared.path)
        .await
        .map_err(|error| error.to_string())?;
    let cleaned = !prepared.path.exists();
    println!("removed, worktree dir gone: {cleaned}");
    Ok(planned_matches && cleaned)
}

async fn expected_head(repo: &str, number: u64) -> Result<String, String> {
    let output = Command::new("gh")
        .args(["pr", "view", &number.to_string(), "-R", repo])
        .args(["--json", "headRefOid", "--jq", ".headRefOid"])
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
