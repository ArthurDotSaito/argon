use std::process::ExitCode;
use std::time::Instant;

use argon_agents::WorktreeManager;
use argon_core::Paths;
use tracing_subscriber::EnvFilter;

const DEFAULT_HOST: &str = "github.com";
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
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (Some(repo), Some(number)) = (args.next(), args.next()) else {
        return Err("usage: worktree <owner/repo> <pr-number>".to_string());
    };
    let number: u64 = number
        .parse()
        .map_err(|_| format!("invalid PR number `{number}`"))?;
    let host = std::env::var("GH_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());

    let manager = WorktreeManager::new(&Paths::resolve());
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
    println!("head_oid: {head}");

    manager
        .remove(&path)
        .await
        .map_err(|error| error.to_string())?;
    println!("removed, worktree dir exists: {}", path.exists());
    Ok(())
}
