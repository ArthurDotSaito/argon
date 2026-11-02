use std::collections::HashMap;

use argon_core::{Involvement, PrId, PullRequest};
use chrono::{Duration, Utc};
use reqwest::StatusCode;
use serde_json::json;

use crate::client::GithubClient;
use crate::convert::into_pull_requests;
use crate::error::{GithubError, Result};
use crate::wire::SearchData;

const INBOX_QUERY: &str = include_str!("../queries/inbox.graphql");
const PAGE_SIZE: usize = 100;
const MIN_PAGE_SIZE: usize = 10;
const SEARCH_RESULT_LIMIT: usize = 1000;
const RATE_LIMIT_FLOOR: u32 = 200;

struct Search {
    query: String,
    involvement: Involvement,
}

impl GithubClient {
    pub async fn fetch_inbox(
        &self,
        _me: &str,
        tracked_orgs: &[String],
        closed_window_days: i64,
    ) -> Result<Vec<PullRequest>> {
        let since = (Utc::now() - Duration::days(closed_window_days))
            .format("%Y-%m-%d")
            .to_string();
        let mut batches = Vec::new();
        for search in searches(tracked_orgs, &since) {
            let prs = self.search_all(&search.query).await?;
            batches.push((search.involvement, prs));
        }
        Ok(merge_by_id(batches))
    }

    async fn search_all(&self, query: &str) -> Result<Vec<PullRequest>> {
        let mut prs = Vec::new();
        let mut fetched = 0;
        let mut after: Option<String> = None;
        let mut page_size = PAGE_SIZE;
        while fetched < SEARCH_RESULT_LIMIT {
            let variables = json!({ "query": query, "after": after, "first": page_size });
            let data: SearchData = match self.graphql(INBOX_QUERY, variables).await {
                Err(GithubError::Http(error))
                    if is_gateway_failure(&error) && page_size > MIN_PAGE_SIZE =>
                {
                    page_size = (page_size / 2).max(MIN_PAGE_SIZE);
                    continue;
                }
                result => result?,
            };
            if data.rate_limit.remaining < RATE_LIMIT_FLOOR {
                return Err(GithubError::RateLimited {
                    reset_at: data.rate_limit.reset_at,
                });
            }
            fetched += data.search.nodes.len();
            prs.extend(into_pull_requests(data.search.nodes, &self.host));
            if !data.search.page_info.has_next_page {
                break;
            }
            after = data.search.page_info.end_cursor;
        }
        Ok(prs)
    }
}

fn is_gateway_failure(error: &reqwest::Error) -> bool {
    error.status().is_some_and(|status| {
        matches!(
            status,
            StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT
        )
    })
}

fn searches(tracked_orgs: &[String], since: &str) -> Vec<Search> {
    let flagged = |query: &str, involvement: Involvement| Search {
        query: query.to_string(),
        involvement,
    };
    let mut searches = vec![
        flagged(
            "is:pr is:open review-requested:@me",
            Involvement {
                requested: true,
                ..Involvement::default()
            },
        ),
        flagged(
            "is:pr is:open assignee:@me",
            Involvement {
                assigned: true,
                ..Involvement::default()
            },
        ),
        flagged(
            &format!("is:pr author:@me updated:>={since}"),
            Involvement {
                authored: true,
                ..Involvement::default()
            },
        ),
        flagged(
            "is:pr is:open reviewed-by:@me -author:@me",
            Involvement {
                reviewed: true,
                ..Involvement::default()
            },
        ),
    ];
    searches.extend(tracked_orgs.iter().map(|org| {
        flagged(
            &format!("is:pr org:{org} updated:>={since}"),
            Involvement::default(),
        )
    }));
    searches
}

fn merge_by_id(batches: Vec<(Involvement, Vec<PullRequest>)>) -> Vec<PullRequest> {
    let mut merged: Vec<PullRequest> = Vec::new();
    let mut positions: HashMap<PrId, usize> = HashMap::new();
    for (involvement, prs) in batches {
        for mut pr in prs {
            match positions.get(&pr.id) {
                Some(&index) => {
                    merged[index].involvement = union(merged[index].involvement, involvement);
                }
                None => {
                    pr.involvement = union(pr.involvement, involvement);
                    positions.insert(pr.id.clone(), merged.len());
                    merged.push(pr);
                }
            }
        }
    }
    merged
}

fn union(left: Involvement, right: Involvement) -> Involvement {
    Involvement {
        requested: left.requested || right.requested,
        assigned: left.assigned || right.assigned,
        authored: left.authored || right.authored,
        reviewed: left.reviewed || right.reviewed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::tests::fixture_prs;

    #[test]
    fn merge_ors_involvement_of_the_same_pr_across_searches() {
        let prs = fixture_prs();
        let requested = Involvement {
            requested: true,
            ..Involvement::default()
        };
        let authored = Involvement {
            authored: true,
            ..Involvement::default()
        };
        let reviewed = Involvement {
            reviewed: true,
            ..Involvement::default()
        };
        let batches = vec![
            (requested, vec![prs[0].clone(), prs[1].clone()]),
            (authored, vec![prs[0].clone()]),
            (Involvement::default(), vec![prs[1].clone(), prs[2].clone()]),
            (reviewed, vec![prs[0].clone(), prs[3].clone()]),
        ];

        let merged = merge_by_id(batches);

        let ids: Vec<&str> = merged.iter().map(|pr| pr.id.0.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "PR_kwDOOrbita0412",
                "PR_kwDOOrbita0398",
                "PR_kwDOOrbita0077",
                "PR_kwDOOrbita0061",
            ]
        );
        assert_eq!(
            merged[0].involvement,
            Involvement {
                requested: true,
                authored: true,
                reviewed: true,
                assigned: false,
            }
        );
        assert_eq!(merged[1].involvement, requested);
        assert_eq!(merged[2].involvement, Involvement::default());
        assert_eq!(merged[3].involvement, reviewed);
    }

    #[test]
    fn searches_cover_the_five_inbox_queries() {
        let orgs = vec!["orbita-health".to_string()];
        let queries: Vec<String> = searches(&orgs, "2026-10-26")
            .into_iter()
            .map(|search| search.query)
            .collect();
        assert_eq!(
            queries,
            vec![
                "is:pr is:open review-requested:@me",
                "is:pr is:open assignee:@me",
                "is:pr author:@me updated:>=2026-10-26",
                "is:pr is:open reviewed-by:@me -author:@me",
                "is:pr org:orbita-health updated:>=2026-10-26",
            ]
        );
    }
}
