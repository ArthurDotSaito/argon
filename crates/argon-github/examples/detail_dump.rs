use std::collections::BTreeMap;
use std::process::Command;

use argon_core::PrId;
use argon_github::{
    GithubClient, Job, PrDetail, RunStatus, TimelineItem, WorkflowRun, discover_accounts, token_for,
};

fn main_usage() -> &'static str {
    "usage: detail_dump <owner/repo#number>"
}

fn parse_target(target: &str) -> Option<(&str, u64)> {
    let (repo, number) = target.split_once('#')?;
    repo.split_once('/')?;
    Some((repo, number.parse().ok()?))
}

fn node_id(repo: &str, number: u64) -> Result<PrId, Box<dyn std::error::Error>> {
    let output = Command::new("gh")
        .args([
            "api",
            &format!("repos/{repo}/pulls/{number}"),
            "--jq",
            ".node_id",
        ])
        .output()?;
    if !output.status.success() {
        return Err(format!("gh api failed for {repo}#{number}").into());
    }
    Ok(PrId(String::from_utf8(output.stdout)?.trim().to_string()))
}

fn timeline_kind(item: &TimelineItem) -> &'static str {
    match item {
        TimelineItem::Comment { .. } => "comment",
        TimelineItem::Review { .. } => "review",
        TimelineItem::Commit { .. } => "commit",
        TimelineItem::ReviewRequested { .. } => "review_requested",
        TimelineItem::Labeled { .. } => "labeled",
        TimelineItem::HeadRefForcePushed { .. } => "force_pushed",
        TimelineItem::Merged { .. } => "merged",
        TimelineItem::Closed { .. } => "closed",
        TimelineItem::ReadyForReview { .. } => "ready_for_review",
    }
}

fn print_counts<'a>(title: &str, names: impl Iterator<Item = &'a str>) {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for name in names {
        *counts.entry(name).or_default() += 1;
    }
    println!("{title}: {counts:?}");
}

fn print_detail(detail: &PrDetail) {
    println!("labels: {}", detail.labels.len());
    println!("reviewers: {}", detail.reviewers.len());
    println!("timeline items: {}", detail.timeline.len());
    print_counts(
        "timeline by type",
        detail.timeline.iter().map(timeline_kind),
    );
    println!("timeline truncated: {}", detail.truncated_timeline);
    let resolved = detail
        .threads
        .iter()
        .filter(|thread| thread.resolved)
        .count();
    println!("threads: {} ({resolved} resolved)", detail.threads.len());
    println!("commits: {}", detail.commits.len());
}

fn status_name(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Queued => "queued",
        RunStatus::InProgress => "in_progress",
        RunStatus::Success => "success",
        RunStatus::Failure => "failure",
        RunStatus::Cancelled => "cancelled",
        RunStatus::Skipped => "skipped",
    }
}

fn first_failed_job(runs: &[WorkflowRun]) -> Option<&Job> {
    runs.iter()
        .flat_map(|run| &run.jobs)
        .find(|job| job.status == RunStatus::Failure)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = std::env::args().nth(1).ok_or(main_usage())?;
    let (repo, number) = parse_target(&target).ok_or(main_usage())?;
    let accounts = discover_accounts()?;
    let account = accounts
        .iter()
        .find(|account| account.host == "github.com")
        .or(accounts.first())
        .ok_or("no gh accounts found")?;
    let client = GithubClient::new(&account.host, token_for(&account.host).await?);

    let id = node_id(repo, number)?;
    let detail = client.pr_detail(&id).await?;
    println!("== {repo}#{number} ({:?})", detail.pr.state);
    print_detail(&detail);

    let runs = client.workflow_runs(repo, &detail.head_oid).await?;
    println!("runs: {}", runs.len());
    print_counts(
        "runs by status",
        runs.iter().map(|run| status_name(run.status)),
    );
    let jobs: Vec<&Job> = runs.iter().flat_map(|run| &run.jobs).collect();
    println!("jobs: {}", jobs.len());
    print_counts(
        "jobs by status",
        jobs.iter().map(|job| status_name(job.status)),
    );
    match first_failed_job(&runs) {
        Some(job) => {
            let log = client.job_log(repo, job.id).await?;
            println!("first failed job log: {} bytes", log.len());
        }
        None => println!("no failed job"),
    }
    Ok(())
}
