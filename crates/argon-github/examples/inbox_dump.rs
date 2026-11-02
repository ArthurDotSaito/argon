use argon_core::{PullRequest, ViewSet, classify};
use argon_github::{GithubClient, discover_accounts, token_for};
use chrono::Utc;

const CLOSED_WINDOW_DAYS: i64 = 7;

const VIEWS: [(&str, ViewSet); 7] = [
    ("inbox", ViewSet::INBOX),
    ("needs_you", ViewSet::NEEDS_YOU),
    ("your_prs", ViewSet::YOUR_PRS),
    ("waiting_on_others", ViewSet::WAITING_ON_OTHERS),
    ("changes_requested", ViewSet::CHANGES_REQUESTED),
    ("ready_to_merge", ViewSet::READY_TO_MERGE),
    ("already_done", ViewSet::ALREADY_DONE),
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let accounts = discover_accounts()?;
    if accounts.is_empty() {
        println!("no gh accounts found");
    }
    for account in accounts {
        println!("== {} ({})", account.host, account.login);
        let client = GithubClient::new(&account.host, token_for(&account.host).await?);
        let viewer = client.viewer().await?;
        let admin_orgs: Vec<String> = viewer
            .orgs
            .iter()
            .filter(|org| org.can_admin)
            .map(|org| org.login.clone())
            .collect();
        let prs = client
            .fetch_inbox(&viewer.login, &admin_orgs, CLOSED_WINDOW_DAYS)
            .await?;
        print_summary(&prs, &viewer.login);
        println!("dropped nodes: {}", client.dropped_nodes());
        for pr in &prs {
            println!("{}#{} {}", pr.repo, pr.number, pr.title);
        }
    }
    Ok(())
}

fn print_summary(prs: &[PullRequest], me: &str) {
    let count = |predicate: fn(&PullRequest) -> bool| prs.iter().filter(|pr| predicate(pr)).count();
    println!("total: {}", prs.len());
    println!("requested: {}", count(|pr| pr.involvement.requested));
    println!("assigned: {}", count(|pr| pr.involvement.assigned));
    println!("authored: {}", count(|pr| pr.involvement.authored));
    println!("reviewed: {}", count(|pr| pr.involvement.reviewed));
    let now = Utc::now();
    for (name, view) in VIEWS {
        let in_view = prs
            .iter()
            .filter(|pr| classify(pr, me, now, CLOSED_WINDOW_DAYS).contains(view))
            .count();
        println!("view {name}: {in_view}");
    }
}
