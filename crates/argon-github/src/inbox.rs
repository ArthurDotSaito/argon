use std::collections::HashMap;

use argon_core::{Involvement, PullRequest};
use chrono::{Duration, Utc};
use futures::stream::{self, StreamExt, TryStreamExt};
use reqwest::StatusCode;
use serde_json::json;

use crate::client::GithubClient;
use crate::convert::into_pull_requests;
use crate::error::{GithubError, Result};
use crate::wire::{HydrateData, IdsData, RateLimit};

const IDS_QUERY: &str = include_str!("../queries/inbox_ids.graphql");
const HYDRATE_QUERY: &str = include_str!("../queries/inbox_hydrate.graphql");
const PAGE_SIZE: usize = 100;
const MIN_BATCH: usize = 10;
const HYDRATE_CHUNK: usize = 40;
const SEARCH_RESULT_LIMIT: usize = 1000;
const RATE_LIMIT_FLOOR: u32 = 200;
const SEARCH_CONCURRENCY: usize = 3;
const HYDRATE_CONCURRENCY: usize = 3;

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
        let batches: Vec<(Involvement, Vec<String>)> = stream::iter(searches(tracked_orgs, &since))
            .map(|search| async move {
                let ids = self.search_ids(&search.query).await?;
                Ok::<_, GithubError>((search.involvement, ids))
            })
            .buffer_unordered(SEARCH_CONCURRENCY)
            .try_collect()
            .await?;

        let merged = merge_ids(batches);
        let chunks: Vec<Vec<String>> = merged
            .chunks(HYDRATE_CHUNK)
            .map(|chunk| chunk.iter().map(|(id, _)| id.clone()).collect())
            .collect();
        let hydrated: Vec<Vec<PullRequest>> = stream::iter(chunks)
            .map(|chunk| async move { self.hydrate(&chunk).await })
            .buffered(HYDRATE_CONCURRENCY)
            .try_collect()
            .await?;

        let involvement: HashMap<String, Involvement> = merged.into_iter().collect();
        let mut prs: Vec<PullRequest> = hydrated.into_iter().flatten().collect();
        for pr in &mut prs {
            if let Some(found) = involvement.get(&pr.id.0) {
                pr.involvement = *found;
            }
        }
        prs.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.0.cmp(&right.id.0))
        });
        Ok(prs)
    }

    async fn search_ids(&self, query: &str) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        let mut fetched = 0;
        let mut after: Option<String> = None;
        let mut page_size = PAGE_SIZE;
        while fetched < SEARCH_RESULT_LIMIT {
            let variables = json!({ "query": query, "after": after, "first": page_size });
            let data: IdsData = match self.graphql("search_ids", IDS_QUERY, variables).await {
                Err(GithubError::Http(error))
                    if is_gateway_failure(&error) && page_size / 2 >= MIN_BATCH =>
                {
                    page_size /= 2;
                    continue;
                }
                result => result?,
            };
            check_rate_limit(&data.rate_limit)?;
            page_size = (page_size * 2).min(PAGE_SIZE);
            fetched += data.search.nodes.len();
            ids.extend(
                data.search
                    .nodes
                    .into_iter()
                    .flatten()
                    .filter_map(|node| node.id),
            );
            if !data.search.page_info.has_next_page {
                break;
            }
            after = data.search.page_info.end_cursor;
        }
        if fetched >= SEARCH_RESULT_LIMIT {
            tracing::warn!(
                query,
                "search reached the result limit, results may be truncated"
            );
        }
        Ok(ids)
    }

    async fn hydrate(&self, ids: &[String]) -> Result<Vec<PullRequest>> {
        let mut prs = Vec::new();
        let mut pending: Vec<&[String]> = vec![ids];
        while let Some(batch) = pending.pop() {
            let variables = json!({ "ids": batch });
            let data: HydrateData = match self.graphql("hydrate", HYDRATE_QUERY, variables).await {
                Err(GithubError::Http(error))
                    if is_gateway_failure(&error) && batch.len() / 2 >= MIN_BATCH =>
                {
                    let (first, second) = batch.split_at(batch.len() / 2);
                    pending.push(second);
                    pending.push(first);
                    continue;
                }
                result => result?,
            };
            check_rate_limit(&data.rate_limit)?;
            prs.extend(into_pull_requests(data.nodes, &self.host));
        }
        Ok(prs)
    }
}

fn check_rate_limit(rate_limit: &RateLimit) -> Result<()> {
    if rate_limit.remaining < RATE_LIMIT_FLOOR {
        return Err(GithubError::RateLimited {
            reset_at: rate_limit.reset_at,
        });
    }
    Ok(())
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

fn merge_ids(batches: Vec<(Involvement, Vec<String>)>) -> Vec<(String, Involvement)> {
    let mut merged: Vec<(String, Involvement)> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    for (involvement, ids) in batches {
        for id in ids {
            match positions.get(&id) {
                Some(&index) => merged[index].1 = union(merged[index].1, involvement),
                None => {
                    positions.insert(id.clone(), merged.len());
                    merged.push((id, involvement));
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
    use crate::wire::Envelope;

    #[test]
    fn merge_ors_involvement_of_the_same_pr_across_searches() {
        let ids: Vec<String> = fixture_prs().into_iter().map(|pr| pr.id.0).collect();
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
            (requested, vec![ids[0].clone(), ids[1].clone()]),
            (authored, vec![ids[0].clone()]),
            (Involvement::default(), vec![ids[1].clone(), ids[2].clone()]),
            (reviewed, vec![ids[0].clone(), ids[3].clone()]),
        ];

        let merged = merge_ids(batches);

        let merged_ids: Vec<&str> = merged.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(merged_ids, vec![&ids[0], &ids[1], &ids[2], &ids[3]]);
        assert_eq!(
            merged[0].1,
            Involvement {
                requested: true,
                authored: true,
                reviewed: true,
                assigned: false,
            }
        );
        assert_eq!(merged[1].1, requested);
        assert_eq!(merged[2].1, Involvement::default());
        assert_eq!(merged[3].1, reviewed);
    }

    #[test]
    fn ids_page_fixture_skips_non_pull_request_nodes() {
        let envelope: Envelope<IdsData> =
            serde_json::from_str(include_str!("../tests/fixtures/search_page.json")).unwrap();
        let data = envelope.into_data(Utc::now()).unwrap().data;
        let ids: Vec<String> = data
            .search
            .nodes
            .into_iter()
            .flatten()
            .filter_map(|node| node.id)
            .collect();
        assert_eq!(
            ids,
            vec![
                "PR_kwDOOrbita0412",
                "PR_kwDOOrbita0398",
                "PR_kwDOOrbita0080"
            ]
        );
        assert!(data.search.page_info.has_next_page);
        assert_eq!(data.rate_limit.remaining, 4990);
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
