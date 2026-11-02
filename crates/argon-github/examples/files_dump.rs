use std::collections::BTreeMap;
use std::process::Command;

use argon_core::PrId;
use argon_github::{FileStatus, GithubClient, ViewedState, discover_accounts, token_for};

fn main_usage() -> &'static str {
    "usage: files_dump <owner/repo#number>"
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

fn status_name(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Added => "added",
        FileStatus::Modified => "modified",
        FileStatus::Removed => "removed",
        FileStatus::Renamed => "renamed",
        FileStatus::Copied => "copied",
        FileStatus::Changed => "changed",
        FileStatus::Unchanged => "unchanged",
    }
}

fn viewed_name(state: ViewedState) -> &'static str {
    match state {
        ViewedState::Viewed => "viewed",
        ViewedState::Unviewed => "unviewed",
        ViewedState::Dismissed => "dismissed",
    }
}

fn print_counts<'a>(title: &str, names: impl Iterator<Item = &'a str>) {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for name in names {
        *counts.entry(name).or_default() += 1;
    }
    println!("{title}: {counts:?}");
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
    let files = client.pr_files(repo, number).await?;
    println!("== {repo}#{number}");
    println!("files: {}", files.len());
    print_counts(
        "files by status",
        files.iter().map(|f| status_name(f.status)),
    );
    let additions: u64 = files.iter().map(|file| u64::from(file.additions)).sum();
    let deletions: u64 = files.iter().map(|file| u64::from(file.deletions)).sum();
    println!("additions: +{additions} deletions: -{deletions}");
    let without_patch = files.iter().filter(|file| file.patch.is_none()).count();
    println!("without patch: {without_patch}");
    let renamed_without_previous = files
        .iter()
        .filter(|file| file.status == FileStatus::Renamed && file.previous_path.is_none())
        .count();
    println!("renamed without previous path: {renamed_without_previous}");

    let viewed = client.viewed_files(&id).await?;
    println!("viewed entries: {}", viewed.len());
    print_counts(
        "files by viewed state",
        viewed.values().map(|state| viewed_name(*state)),
    );
    let unmatched = files
        .iter()
        .filter(|file| !viewed.contains_key(&file.path))
        .count();
    println!("rest files missing from viewed map: {unmatched}");

    let detail = client.pr_detail(&id).await?;
    let head_files = client.commit_files(repo, &detail.head_oid).await?;
    println!("head commit files: {}", head_files.len());
    Ok(())
}
