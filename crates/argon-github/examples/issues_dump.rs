use std::collections::HashMap;

use argon_github::{GithubClient, discover_accounts, token_for};

const CLOSED_WINDOW_DAYS: i64 = 7;
const SAMPLE: usize = 5;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let accounts = discover_accounts()?;
    if accounts.is_empty() {
        println!("no gh accounts found");
    }
    for account in accounts {
        println!("== {} ({})", account.host, account.login);
        let client = GithubClient::new(&account.host, token_for(&account.host).await?);
        let issues = client
            .fetch_important_issues(&HashMap::new(), CLOSED_WINDOW_DAYS)
            .await?
            .items;
        println!("total: {}", issues.len());
        println!(
            "assigned: {}",
            issues
                .iter()
                .filter(|issue| issue.involvement.assigned)
                .count()
        );
        println!(
            "authored: {}",
            issues
                .iter()
                .filter(|issue| issue.involvement.authored)
                .count()
        );
        println!(
            "mentioned: {}",
            issues
                .iter()
                .filter(|issue| issue.involvement.mentioned)
                .count()
        );
        println!(
            "commented: {}",
            issues
                .iter()
                .filter(|issue| issue.involvement.commented)
                .count()
        );
        println!("dropped nodes: {}", client.dropped_nodes());
        for issue in issues.iter().take(SAMPLE) {
            let flags = issue.involvement;
            println!(
                "{}#{} {} [assigned={} authored={} mentioned={} commented={}]",
                issue.repo,
                issue.number,
                issue.title,
                flags.assigned,
                flags.authored,
                flags.mentioned,
                flags.commented
            );
        }
    }
    Ok(())
}
