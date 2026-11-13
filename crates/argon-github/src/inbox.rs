use std::collections::{HashMap, HashSet};

use argon_core::{Involvement, Issue, PullRequest};
use chrono::{DateTime, Duration, Utc};
use futures::stream::{self, StreamExt, TryStreamExt};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::client::GithubClient;
use crate::convert::{into_issues, into_pull_requests};
use crate::error::{GithubError, Result};
use crate::wire::{HydrateData, IdsData, IssueHydrateData, RateLimit};

const IDS_QUERY: &str = include_str!("../queries/inbox_ids.graphql");
const HYDRATE_QUERY: &str = concat!(
    include_str!("../queries/inbox_hydrate.graphql"),
    include_str!("../queries/pull_request_fields.graphql")
);
const ISSUE_HYDRATE_QUERY: &str = concat!(
    include_str!("../queries/issue_hydrate.graphql"),
    include_str!("../queries/issue_fields.graphql")
);
const PAGE_SIZE: usize = 100;
const ORG_PAGE_SIZE: usize = 50;
const MIN_BATCH: usize = 10;
const HYDRATE_CHUNK: usize = 40;
const SEARCH_RESULT_LIMIT: usize = 1000;
const RATE_LIMIT_FLOOR: u32 = 200;
const SEARCH_CONCURRENCY: usize = 3;
const HYDRATE_CONCURRENCY: usize = 3;

#[derive(Clone, Copy)]
enum Kind {
    Pr,
    Issue,
}

impl Kind {
    fn qualifier(self) -> &'static str {
        match self {
            Self::Pr => "is:pr",
            Self::Issue => "is:issue",
        }
    }
}

struct Search {
    query: String,
    involvement: Involvement,
    page_size: usize,
    limit: usize,
    org: Option<String>,
}

struct Found {
    involvement: Involvement,
    ids: Vec<String>,
    has_more: bool,
    org: Option<String>,
}

trait Hydrated {
    fn id(&self) -> &str;
    fn updated_at(&self) -> DateTime<Utc>;
    fn set_involvement(&mut self, involvement: Involvement);
}

impl Hydrated for PullRequest {
    fn id(&self) -> &str {
        &self.id.0
    }

    fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    fn set_involvement(&mut self, involvement: Involvement) {
        self.involvement = involvement;
    }
}

impl Hydrated for Issue {
    fn id(&self) -> &str {
        &self.id.0
    }

    fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    fn set_involvement(&mut self, involvement: Involvement) {
        self.involvement = involvement;
    }
}

trait HydratePage: DeserializeOwned {
    type Item: Hydrated;
    const OPERATION: &'static str;
    const QUERY: &'static str;

    fn rate_limit(&self) -> &RateLimit;
    fn into_items(self, host: &str) -> Vec<Self::Item>;
}

impl HydratePage for HydrateData {
    type Item = PullRequest;
    const OPERATION: &'static str = "hydrate";
    const QUERY: &'static str = HYDRATE_QUERY;

    fn rate_limit(&self) -> &RateLimit {
        &self.rate_limit
    }

    fn into_items(self, host: &str) -> Vec<PullRequest> {
        into_pull_requests(self.nodes, host)
    }
}

impl HydratePage for IssueHydrateData {
    type Item = Issue;
    const OPERATION: &'static str = "issue_hydrate";
    const QUERY: &'static str = ISSUE_HYDRATE_QUERY;

    fn rate_limit(&self) -> &RateLimit {
        &self.rate_limit
    }

    fn into_items(self, host: &str) -> Vec<Issue> {
        into_issues(self.nodes, host)
    }
}

pub struct OrgFetch {
    pub prs: Vec<PullRequest>,
    pub orgs_with_more: Vec<String>,
}

pub struct IssueOrgFetch {
    pub issues: Vec<Issue>,
    pub orgs_with_more: Vec<String>,
}

impl GithubClient {
    pub async fn fetch_important(&self, closed_window_days: i64) -> Result<Vec<PullRequest>> {
        let found = self
            .run_searches(important_searches(&since(closed_window_days)))
            .await?;
        self.hydrate_all::<HydrateData>(merge_ids(into_batches(found)))
            .await
    }

    pub async fn fetch_orgs(
        &self,
        orgs: &[(String, usize)],
        known: &HashSet<String>,
        closed_window_days: i64,
    ) -> Result<OrgFetch> {
        let found = self
            .run_searches(org_searches(Kind::Pr, orgs, &since(closed_window_days)))
            .await?;
        let orgs_with_more = orgs_with_more(&found);
        let prs = self
            .hydrate_all::<HydrateData>(unknown_ids(merge_ids(into_batches(found)), known))
            .await?;
        Ok(OrgFetch {
            prs,
            orgs_with_more,
        })
    }

    pub async fn fetch_important_issues(&self, closed_window_days: i64) -> Result<Vec<Issue>> {
        let found = self
            .run_searches(important_issue_searches(&since(closed_window_days)))
            .await?;
        self.hydrate_all::<IssueHydrateData>(merge_ids(into_batches(found)))
            .await
    }

    pub async fn fetch_org_issues(
        &self,
        orgs: &[(String, usize)],
        known: &HashSet<String>,
        closed_window_days: i64,
    ) -> Result<IssueOrgFetch> {
        let found = self
            .run_searches(org_searches(Kind::Issue, orgs, &since(closed_window_days)))
            .await?;
        let orgs_with_more = orgs_with_more(&found);
        let issues = self
            .hydrate_all::<IssueHydrateData>(unknown_ids(merge_ids(into_batches(found)), known))
            .await?;
        Ok(IssueOrgFetch {
            issues,
            orgs_with_more,
        })
    }

    async fn run_searches(&self, searches: Vec<Search>) -> Result<Vec<Found>> {
        stream::iter(searches)
            .map(|search| async move { self.search_ids(&search).await })
            .buffered(SEARCH_CONCURRENCY)
            .try_collect()
            .await
    }

    async fn hydrate_all<P: HydratePage>(
        &self,
        merged: Vec<(String, Involvement)>,
    ) -> Result<Vec<P::Item>> {
        let chunks: Vec<Vec<String>> = merged
            .chunks(HYDRATE_CHUNK)
            .map(|chunk| chunk.iter().map(|(id, _)| id.clone()).collect())
            .collect();
        let hydrated: Vec<Vec<P::Item>> = stream::iter(chunks)
            .map(|chunk| async move { self.hydrate::<P>(&chunk).await })
            .buffered(HYDRATE_CONCURRENCY)
            .try_collect()
            .await?;
        Ok(apply_involvement(
            hydrated.into_iter().flatten().collect(),
            merged,
        ))
    }

    async fn search_ids(&self, search: &Search) -> Result<Found> {
        let mut ids = Vec::new();
        let mut fetched = 0;
        let mut after: Option<String> = None;
        let mut page_size = search.page_size;
        let mut has_next_page = false;
        while fetched < search.limit {
            let first = page_size.min(search.limit - fetched);
            let variables = json!({ "query": search.query, "after": after, "first": first });
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
            page_size = (page_size * 2).min(search.page_size);
            fetched += data.search.nodes.len();
            ids.extend(
                data.search
                    .nodes
                    .into_iter()
                    .flatten()
                    .filter_map(|node| node.id),
            );
            has_next_page = data.search.page_info.has_next_page;
            if !has_next_page {
                break;
            }
            after = data.search.page_info.end_cursor;
        }
        if fetched >= SEARCH_RESULT_LIMIT {
            tracing::warn!(
                query = search.query,
                "search reached the result limit, results may be truncated"
            );
        }
        Ok(Found {
            involvement: search.involvement,
            ids,
            has_more: has_next_page && fetched < SEARCH_RESULT_LIMIT,
            org: search.org.clone(),
        })
    }

    async fn hydrate<P: HydratePage>(&self, ids: &[String]) -> Result<Vec<P::Item>> {
        let mut items = Vec::new();
        let mut pending: Vec<&[String]> = vec![ids];
        while let Some(batch) = pending.pop() {
            let variables = json!({ "ids": batch });
            let data: P = match self.graphql(P::OPERATION, P::QUERY, variables).await {
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
            check_rate_limit(data.rate_limit())?;
            items.extend(data.into_items(&self.host));
        }
        Ok(items)
    }
}

fn apply_involvement<T: Hydrated>(mut items: Vec<T>, merged: Vec<(String, Involvement)>) -> Vec<T> {
    let involvement: HashMap<String, Involvement> = merged.into_iter().collect();
    for item in &mut items {
        if let Some(found) = involvement.get(item.id()) {
            item.set_involvement(*found);
        }
    }
    items.sort_by(|left, right| {
        right
            .updated_at()
            .cmp(&left.updated_at())
            .then_with(|| left.id().cmp(right.id()))
    });
    items
}

fn orgs_with_more(found: &[Found]) -> Vec<String> {
    found
        .iter()
        .filter(|found| found.has_more)
        .filter_map(|found| found.org.clone())
        .collect()
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

fn since(closed_window_days: i64) -> String {
    (Utc::now() - Duration::days(closed_window_days))
        .format("%Y-%m-%d")
        .to_string()
}

fn flagged_search(query: &str, involvement: Involvement) -> Search {
    Search {
        query: query.to_string(),
        involvement,
        page_size: PAGE_SIZE,
        limit: SEARCH_RESULT_LIMIT,
        org: None,
    }
}

fn important_searches(since: &str) -> Vec<Search> {
    vec![
        flagged_search(
            "is:pr is:open review-requested:@me",
            Involvement {
                requested: true,
                ..Involvement::default()
            },
        ),
        flagged_search(
            "is:pr is:open assignee:@me",
            Involvement {
                assigned: true,
                ..Involvement::default()
            },
        ),
        flagged_search(
            &format!("is:pr author:@me updated:>={since}"),
            Involvement {
                authored: true,
                ..Involvement::default()
            },
        ),
        flagged_search(
            "is:pr is:open reviewed-by:@me -author:@me",
            Involvement {
                reviewed: true,
                ..Involvement::default()
            },
        ),
    ]
}

fn important_issue_searches(since: &str) -> Vec<Search> {
    vec![
        flagged_search(
            "is:issue is:open assignee:@me",
            Involvement {
                assigned: true,
                ..Involvement::default()
            },
        ),
        flagged_search(
            &format!("is:issue author:@me updated:>={since}"),
            Involvement {
                authored: true,
                ..Involvement::default()
            },
        ),
        flagged_search(
            &format!("is:issue is:open mentions:@me updated:>={since}"),
            Involvement {
                mentioned: true,
                ..Involvement::default()
            },
        ),
        flagged_search(
            &format!("is:issue is:open commenter:@me -author:@me updated:>={since}"),
            Involvement {
                commented: true,
                ..Involvement::default()
            },
        ),
    ]
}

fn org_searches(kind: Kind, orgs: &[(String, usize)], since: &str) -> Vec<Search> {
    let qualifier = kind.qualifier();
    orgs.iter()
        .map(|(org, depth)| Search {
            query: format!("{qualifier} org:{org} updated:>={since} sort:updated-desc"),
            involvement: Involvement::default(),
            page_size: ORG_PAGE_SIZE,
            limit: ((*depth).max(1) * ORG_PAGE_SIZE).min(SEARCH_RESULT_LIMIT),
            org: Some(org.clone()),
        })
        .collect()
}

fn into_batches(found: Vec<Found>) -> Vec<(Involvement, Vec<String>)> {
    found
        .into_iter()
        .map(|found| (found.involvement, found.ids))
        .collect()
}

fn unknown_ids(
    merged: Vec<(String, Involvement)>,
    known: &HashSet<String>,
) -> Vec<(String, Involvement)> {
    merged
        .into_iter()
        .filter(|(id, _)| !known.contains(id))
        .collect()
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
        mentioned: left.mentioned || right.mentioned,
        commented: left.commented || right.commented,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::tests::{fixture_issues, fixture_prs};
    use crate::wire::Envelope;

    #[test]
    fn hydrate_query_spreads_and_defines_the_shared_fragment() {
        assert!(HYDRATE_QUERY.contains("...PullRequestFields"));
        assert!(HYDRATE_QUERY.contains("fragment PullRequestFields on PullRequest"));
    }

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
                ..Involvement::default()
            }
        );
        assert_eq!(merged[1].1, requested);
        assert_eq!(merged[2].1, Involvement::default());
        assert_eq!(merged[3].1, reviewed);
    }

    #[test]
    fn org_hydration_skips_ids_known_from_the_important_phase() {
        let ids: Vec<String> = fixture_prs().into_iter().map(|pr| pr.id.0).collect();
        let requested = Involvement {
            requested: true,
            ..Involvement::default()
        };
        let important = merge_ids(vec![(requested, vec![ids[0].clone()])]);
        let known: HashSet<String> = important.iter().map(|(id, _)| id.clone()).collect();

        let pending = unknown_ids(
            merge_ids(vec![
                (Involvement::default(), vec![ids[0].clone(), ids[1].clone()]),
                (Involvement::default(), vec![ids[2].clone()]),
            ]),
            &known,
        );

        assert_eq!(
            pending,
            vec![
                (ids[1].clone(), Involvement::default()),
                (ids[2].clone(), Involvement::default()),
            ]
        );
        assert_eq!(important, vec![(ids[0].clone(), requested)]);
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
    fn important_searches_cover_the_four_me_queries_without_a_page_cap() {
        let searches = important_searches("2026-10-26");
        let queries: Vec<&str> = searches
            .iter()
            .map(|search| search.query.as_str())
            .collect();
        assert_eq!(
            queries,
            vec![
                "is:pr is:open review-requested:@me",
                "is:pr is:open assignee:@me",
                "is:pr author:@me updated:>=2026-10-26",
                "is:pr is:open reviewed-by:@me -author:@me",
            ]
        );
        assert!(
            searches
                .iter()
                .all(|search| search.page_size == PAGE_SIZE && search.limit == SEARCH_RESULT_LIMIT)
        );
    }

    #[test]
    fn org_searches_fetch_depth_pages_of_fifty() {
        let orgs = vec![
            ("orbita-health".to_string(), 1),
            ("nimbus".to_string(), 3),
            ("vela".to_string(), 0),
            ("atlas".to_string(), 40),
        ];
        let searches = org_searches(Kind::Pr, &orgs, "2026-10-26");
        let plan: Vec<(&str, usize, usize)> = searches
            .iter()
            .map(|search| (search.query.as_str(), search.page_size, search.limit))
            .collect();
        assert_eq!(
            plan,
            vec![
                (
                    "is:pr org:orbita-health updated:>=2026-10-26 sort:updated-desc",
                    50,
                    50
                ),
                (
                    "is:pr org:nimbus updated:>=2026-10-26 sort:updated-desc",
                    50,
                    150
                ),
                (
                    "is:pr org:vela updated:>=2026-10-26 sort:updated-desc",
                    50,
                    50
                ),
                (
                    "is:pr org:atlas updated:>=2026-10-26 sort:updated-desc",
                    50,
                    1000
                ),
            ]
        );
        assert!(
            searches
                .iter()
                .all(|search| search.involvement == Involvement::default())
        );
    }

    #[test]
    fn issue_hydrate_query_is_separate_and_defines_its_own_fragment() {
        assert!(ISSUE_HYDRATE_QUERY.contains("... on Issue"));
        assert!(ISSUE_HYDRATE_QUERY.contains("...IssueFields"));
        assert!(ISSUE_HYDRATE_QUERY.contains("fragment IssueFields on Issue"));
        assert!(!ISSUE_HYDRATE_QUERY.contains("... on PullRequest"));
        assert!(!HYDRATE_QUERY.contains("... on Issue"));
    }

    #[test]
    fn ids_query_accepts_pull_request_and_issue_nodes() {
        assert!(IDS_QUERY.contains("... on PullRequest"));
        assert!(IDS_QUERY.contains("... on Issue"));
    }

    #[test]
    fn ids_page_fixture_with_issue_nodes_skips_empty_nodes() {
        let envelope: Envelope<IdsData> =
            serde_json::from_str(include_str!("../tests/fixtures/search_page_issues.json"))
                .unwrap();
        let data = envelope.into_data(Utc::now()).unwrap().data;
        let ids: Vec<String> = data
            .search
            .nodes
            .into_iter()
            .flatten()
            .filter_map(|node| node.id)
            .collect();
        assert_eq!(ids, vec!["I_kwDOOrbita0101", "I_kwDOOrbita0087"]);
        assert!(!data.search.page_info.has_next_page);
    }

    #[test]
    fn important_issue_searches_cover_the_four_me_queries_without_a_page_cap() {
        let searches = important_issue_searches("2026-10-26");
        let plan: Vec<(&str, Involvement)> = searches
            .iter()
            .map(|search| (search.query.as_str(), search.involvement))
            .collect();
        assert_eq!(
            plan,
            vec![
                (
                    "is:issue is:open assignee:@me",
                    Involvement {
                        assigned: true,
                        ..Involvement::default()
                    }
                ),
                (
                    "is:issue author:@me updated:>=2026-10-26",
                    Involvement {
                        authored: true,
                        ..Involvement::default()
                    }
                ),
                (
                    "is:issue is:open mentions:@me updated:>=2026-10-26",
                    Involvement {
                        mentioned: true,
                        ..Involvement::default()
                    }
                ),
                (
                    "is:issue is:open commenter:@me -author:@me updated:>=2026-10-26",
                    Involvement {
                        commented: true,
                        ..Involvement::default()
                    }
                ),
            ]
        );
        assert!(searches.iter().all(|search| {
            search.page_size == PAGE_SIZE
                && search.limit == SEARCH_RESULT_LIMIT
                && search.org.is_none()
        }));
    }

    #[test]
    fn org_issue_searches_use_the_same_pages_and_limits_as_pull_requests() {
        let orgs = vec![
            ("orbita-health".to_string(), 1),
            ("nimbus".to_string(), 3),
            ("atlas".to_string(), 40),
        ];
        let searches = org_searches(Kind::Issue, &orgs, "2026-10-26");
        let plan: Vec<(&str, usize, usize, Option<&str>)> = searches
            .iter()
            .map(|search| {
                (
                    search.query.as_str(),
                    search.page_size,
                    search.limit,
                    search.org.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            plan,
            vec![
                (
                    "is:issue org:orbita-health updated:>=2026-10-26 sort:updated-desc",
                    50,
                    50,
                    Some("orbita-health")
                ),
                (
                    "is:issue org:nimbus updated:>=2026-10-26 sort:updated-desc",
                    50,
                    150,
                    Some("nimbus")
                ),
                (
                    "is:issue org:atlas updated:>=2026-10-26 sort:updated-desc",
                    50,
                    1000,
                    Some("atlas")
                ),
            ]
        );
    }

    #[test]
    fn orgs_with_more_names_each_org_by_its_own_search() {
        let found = |org: Option<&str>, has_more: bool| Found {
            involvement: Involvement::default(),
            ids: Vec::new(),
            has_more,
            org: org.map(str::to_string),
        };
        let results = vec![
            found(Some("vela"), false),
            found(Some("nimbus"), true),
            found(None, true),
            found(Some("atlas"), true),
        ];
        assert_eq!(orgs_with_more(&results), vec!["nimbus", "atlas"]);
    }

    #[test]
    fn issue_merge_ors_mentioned_and_commented_flags_and_sorts_by_update() {
        let issues = fixture_issues();
        let id = |number: u64| -> String {
            issues
                .iter()
                .find(|issue| issue.number == number)
                .unwrap()
                .id
                .0
                .clone()
        };
        let flag = |mentioned: bool, commented: bool, assigned: bool| Involvement {
            mentioned,
            commented,
            assigned,
            ..Involvement::default()
        };
        let merged = merge_ids(vec![
            (flag(false, false, true), vec![id(101), id(130)]),
            (flag(true, false, false), vec![id(101), id(87)]),
            (flag(false, true, false), vec![id(101), id(92)]),
        ]);

        let applied = apply_involvement(issues, merged);

        let numbers: Vec<u64> = applied.iter().map(|issue| issue.number).collect();
        assert_eq!(numbers, vec![101, 130, 87, 92, 64]);
        let involvement: Vec<Involvement> = applied.iter().map(|issue| issue.involvement).collect();
        assert_eq!(
            involvement,
            vec![
                flag(true, true, true),
                flag(false, false, true),
                flag(true, false, false),
                flag(false, true, false),
                Involvement::default(),
            ]
        );
    }
}
