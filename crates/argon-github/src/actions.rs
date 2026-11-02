use std::future::Future;

use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt, TryStreamExt};

use crate::actions_wire::{JobsResponse, RunsResponse, WireJob, WireRun, WireStep};
use crate::client::GithubClient;
use crate::error::{GithubError, Result};
use crate::rest::repo_path;

const JOBS_CONCURRENCY: usize = 3;
pub(crate) const PER_PAGE: usize = 100;
const MAX_PAGES: u32 = 3;
const UNNAMED_RUN: &str = "workflow";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunStatus {
    Queued,
    InProgress,
    Success,
    Failure,
    Cancelled,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub name: String,
    pub status: RunStatus,
    pub number: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: u64,
    pub name: String,
    pub status: RunStatus,
    pub html_url: String,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRun {
    pub id: u64,
    pub name: String,
    pub status: RunStatus,
    pub html_url: String,
    pub run_attempt: u32,
    pub jobs: Vec<Job>,
    pub jobs_error: Option<String>,
}

pub(crate) fn run_status(status: Option<&str>, conclusion: Option<&str>) -> RunStatus {
    match status {
        Some("in_progress") => RunStatus::InProgress,
        Some("completed") => completed_status(conclusion),
        _ => RunStatus::Queued,
    }
}

fn completed_status(conclusion: Option<&str>) -> RunStatus {
    match conclusion {
        Some("success") => RunStatus::Success,
        Some("failure" | "timed_out" | "action_required" | "startup_failure") => RunStatus::Failure,
        Some("cancelled") => RunStatus::Cancelled,
        _ => RunStatus::Skipped,
    }
}

fn into_step(step: WireStep) -> Step {
    Step {
        status: run_status(step.status.as_deref(), step.conclusion.as_deref()),
        name: step.name,
        number: step.number,
    }
}

fn into_job(job: WireJob, run_url: &str) -> Job {
    Job {
        id: job.id,
        status: run_status(job.status.as_deref(), job.conclusion.as_deref()),
        name: job.name,
        html_url: job.html_url.unwrap_or_else(|| run_url.to_string()),
        started_at: job.started_at,
        completed_at: job.completed_at,
        steps: job
            .steps
            .unwrap_or_default()
            .into_iter()
            .map(into_step)
            .collect(),
    }
}

fn into_run(run: WireRun, jobs: Result<Vec<WireJob>>) -> WorkflowRun {
    let (jobs, jobs_error) = match jobs {
        Ok(jobs) => (
            jobs.into_iter()
                .map(|job| into_job(job, &run.html_url))
                .collect(),
            None,
        ),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    WorkflowRun {
        id: run.id,
        status: run_status(run.status.as_deref(), run.conclusion.as_deref()),
        name: run
            .name
            .or(run.display_title)
            .unwrap_or_else(|| UNNAMED_RUN.to_string()),
        html_url: run.html_url,
        run_attempt: run.run_attempt.unwrap_or(1),
        jobs,
        jobs_error,
    }
}

pub(crate) fn is_sha(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_hexdigit())
}

async fn collect_pages<T, F, Fut>(what: &str, fetch: F) -> Result<Vec<T>>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<(Vec<T>, Option<u64>)>>,
{
    collect_pages_capped(what, MAX_PAGES, fetch).await
}

pub(crate) async fn collect_pages_capped<T, F, Fut>(
    what: &str,
    max_pages: u32,
    mut fetch: F,
) -> Result<Vec<T>>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<(Vec<T>, Option<u64>)>>,
{
    let mut items = Vec::new();
    for page in 1..=max_pages {
        let (batch, total) = fetch(page).await?;
        let batch_len = batch.len();
        items.extend(batch);
        let complete = match total {
            Some(total) => items.len() as u64 >= total,
            None => batch_len < PER_PAGE,
        };
        if complete || batch_len == 0 {
            return Ok(items);
        }
    }
    tracing::warn!(
        what,
        kept = items.len(),
        "github returned more items than the page cap, results truncated"
    );
    Ok(items)
}

fn keeps_going(error: &GithubError) -> bool {
    !matches!(
        error,
        GithubError::AuthMissing { .. } | GithubError::RateLimited { .. }
    )
}

impl GithubClient {
    pub async fn workflow_runs(&self, repo: &str, head_sha: &str) -> Result<Vec<WorkflowRun>> {
        let repo = repo_path(repo)?;
        if !is_sha(head_sha) {
            return Err(GithubError::Decode(format!("invalid head sha: {head_sha}")));
        }
        let runs = collect_pages("workflow runs", |page| async move {
            let page = page.to_string();
            let response: RunsResponse = self
                .rest_get(
                    &format!("/repos/{repo}/actions/runs"),
                    &[
                        ("head_sha", head_sha),
                        ("per_page", &PER_PAGE.to_string()),
                        ("page", &page),
                    ],
                )
                .await?;
            Ok((response.workflow_runs, response.total_count))
        })
        .await?;
        stream::iter(runs)
            .map(|run| async move {
                let jobs = self.run_jobs(repo, run.id).await;
                match jobs {
                    Err(error) if !keeps_going(&error) => Err(error),
                    jobs => {
                        if let Err(error) = &jobs {
                            tracing::warn!(run_id = run.id, %error, "failed to fetch jobs of a workflow run");
                        }
                        Ok::<_, GithubError>(into_run(run, jobs))
                    }
                }
            })
            .buffered(JOBS_CONCURRENCY)
            .try_collect()
            .await
    }

    async fn run_jobs(&self, repo: &str, run_id: u64) -> Result<Vec<WireJob>> {
        collect_pages("workflow jobs", |page| async move {
            let page = page.to_string();
            let response: JobsResponse = self
                .rest_get(
                    &format!("/repos/{repo}/actions/runs/{run_id}/jobs"),
                    &[("per_page", &PER_PAGE.to_string()), ("page", &page)],
                )
                .await?;
            Ok((response.jobs, response.total_count))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNS: &str = include_str!("../tests/fixtures/runs.json");
    const JOBS: &str = include_str!("../tests/fixtures/jobs.json");

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }

    fn runs() -> Vec<WorkflowRun> {
        let runs: RunsResponse = serde_json::from_str(RUNS).unwrap();
        let mut jobs = Some(serde_json::from_str::<JobsResponse>(JOBS).unwrap());
        runs.workflow_runs
            .into_iter()
            .map(|run| {
                let jobs = jobs.take().map_or_else(Vec::new, |jobs| jobs.jobs);
                into_run(run, Ok(jobs))
            })
            .collect()
    }

    #[test]
    fn status_and_conclusion_combinations_map_to_run_status() {
        let cases = [
            (Some("queued"), None, RunStatus::Queued),
            (Some("waiting"), None, RunStatus::Queued),
            (Some("pending"), None, RunStatus::Queued),
            (None, None, RunStatus::Queued),
            (Some("in_progress"), None, RunStatus::InProgress),
            (Some("completed"), Some("success"), RunStatus::Success),
            (Some("completed"), Some("failure"), RunStatus::Failure),
            (Some("completed"), Some("timed_out"), RunStatus::Failure),
            (
                Some("completed"),
                Some("action_required"),
                RunStatus::Failure,
            ),
            (
                Some("completed"),
                Some("startup_failure"),
                RunStatus::Failure,
            ),
            (Some("completed"), Some("cancelled"), RunStatus::Cancelled),
            (Some("completed"), Some("skipped"), RunStatus::Skipped),
            (Some("completed"), Some("neutral"), RunStatus::Skipped),
            (Some("completed"), Some("stale"), RunStatus::Skipped),
            (Some("completed"), None, RunStatus::Skipped),
        ];
        for (status, conclusion, expected) in cases {
            assert_eq!(
                run_status(status, conclusion),
                expected,
                "{status:?} / {conclusion:?}"
            );
        }
    }

    #[test]
    fn runs_fixture_maps_names_status_and_attempts() {
        let runs = runs();
        let summary: Vec<(u64, &str, RunStatus, u32)> = runs
            .iter()
            .map(|run| (run.id, run.name.as_str(), run.status, run.run_attempt))
            .collect();
        assert_eq!(
            summary,
            vec![
                (9100000101, "CI", RunStatus::Failure, 2),
                (9100000102, "Lint", RunStatus::InProgress, 1),
                (9100000103, "Deploy preview", RunStatus::Queued, 1),
                (9100000104, "Docs check", RunStatus::Cancelled, 1),
                (9100000105, "Release gate", RunStatus::Skipped, 1),
                (9100000106, "Nightly audit", RunStatus::Success, 1),
                (9100000107, "Hotfix pipeline", RunStatus::Failure, 1),
                (9100000108, "workflow", RunStatus::Queued, 1),
            ]
        );
        assert_eq!(
            runs[0].html_url,
            "https://github.com/orbita-health/atlas/actions/runs/9100000101"
        );
    }

    #[test]
    fn jobs_fixture_maps_jobs_steps_and_timestamps() {
        let runs = runs();
        let jobs = &runs[0].jobs;
        let names: Vec<(&str, RunStatus)> = jobs
            .iter()
            .map(|job| (job.name.as_str(), job.status))
            .collect();
        assert_eq!(
            names,
            vec![
                ("build", RunStatus::Success),
                ("test (unit)", RunStatus::Failure),
                ("test (integration)", RunStatus::InProgress),
                ("package", RunStatus::Queued),
                ("publish", RunStatus::Skipped),
                ("sanity", RunStatus::Cancelled),
                ("coverage", RunStatus::Failure),
            ]
        );
        let failing = &jobs[1];
        assert_eq!(failing.id, 7300000002);
        assert_eq!(
            failing.html_url,
            "https://github.com/orbita-health/atlas/actions/runs/9100000101/job/7300000002"
        );
        assert_eq!(failing.started_at, Some(at("2026-11-01T20:15:10Z")));
        assert_eq!(failing.completed_at, Some(at("2026-11-01T20:19:44Z")));
        let steps: Vec<(u32, &str, RunStatus)> = failing
            .steps
            .iter()
            .map(|step| (step.number, step.name.as_str(), step.status))
            .collect();
        assert_eq!(
            steps,
            vec![
                (1, "Set up job", RunStatus::Success),
                (2, "Run tests", RunStatus::Failure),
                (3, "Upload report", RunStatus::Skipped),
                (4, "Post checkout", RunStatus::Queued),
            ]
        );
        assert_eq!(jobs[2].completed_at, None);
        assert_eq!(jobs[3].started_at, None);
    }

    #[test]
    fn job_without_html_url_falls_back_to_the_run_url() {
        let runs = runs();
        let coverage = &runs[0].jobs[6];
        assert_eq!(coverage.html_url, runs[0].html_url);
        assert!(coverage.steps.is_empty());
    }

    #[test]
    fn run_whose_jobs_failed_to_load_keeps_the_error_and_no_jobs() {
        let run: RunsResponse = serde_json::from_str(RUNS).unwrap();
        let run = run.workflow_runs.into_iter().next().unwrap();
        let run = into_run(run, Err(GithubError::Decode("jobs unavailable".into())));
        assert!(run.jobs.is_empty());
        assert_eq!(
            run.jobs_error.as_deref(),
            Some("decode error: jobs unavailable")
        );
        assert_eq!(run.status, RunStatus::Failure);
    }

    fn page_of(count: usize) -> Vec<usize> {
        (0..count).collect()
    }

    #[tokio::test]
    async fn pages_are_fetched_until_the_total_is_reached() {
        let calls = std::cell::Cell::new(0);
        let items = collect_pages("test", |page| {
            calls.set(calls.get() + 1);
            let count = if page < 3 { PER_PAGE } else { 50 };
            async move { Ok((page_of(count), Some(250))) }
        })
        .await
        .unwrap();
        assert_eq!(items.len(), 250);
        assert_eq!(calls.get(), 3);
    }

    #[tokio::test]
    async fn pages_stop_at_the_cap_even_when_more_exist() {
        let calls = std::cell::Cell::new(0);
        let items = collect_pages("test", |_| {
            calls.set(calls.get() + 1);
            async { Ok((page_of(PER_PAGE), Some(450))) }
        })
        .await
        .unwrap();
        assert_eq!(items.len(), 300);
        assert_eq!(calls.get(), 3);
    }

    #[tokio::test]
    async fn single_short_page_needs_one_request() {
        let calls = std::cell::Cell::new(0);
        let items = collect_pages("test", |_| {
            calls.set(calls.get() + 1);
            async { Ok((page_of(7), None)) }
        })
        .await
        .unwrap();
        assert_eq!(items.len(), 7);
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test]
    async fn empty_page_stops_even_when_the_total_claims_more() {
        let calls = std::cell::Cell::new(0);
        let items = collect_pages("test", |_| {
            calls.set(calls.get() + 1);
            async { Ok((Vec::<usize>::new(), Some(10))) }
        })
        .await
        .unwrap();
        assert!(items.is_empty());
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test]
    async fn page_errors_propagate() {
        let result = collect_pages("test", |_| async {
            Err::<(Vec<usize>, Option<u64>), _>(GithubError::Decode("boom".into()))
        })
        .await;
        assert!(result.is_err());
    }

    fn client_for(port: u16) -> GithubClient {
        let mut client = GithubClient::new("github.com", "test-token-not-real".to_string());
        client.rest_url = format!("http://127.0.0.1:{port}");
        client
    }

    fn two_runs_body() -> String {
        let run = |id: u64| {
            format!(
                r#"{{"id":{id},"name":"CI","status":"completed","conclusion":"success","html_url":"https://github.com/orbita-health/atlas/actions/runs/{id}","run_attempt":1}}"#
            )
        };
        format!(
            r#"{{"total_count":2,"workflow_runs":[{},{}]}}"#,
            run(1),
            run(2)
        )
    }

    #[tokio::test]
    async fn failing_jobs_request_of_one_run_does_not_drop_the_others() {
        use crate::test_server::{json_response, serve};

        let port = serve(|request_line| {
            if request_line.contains("/actions/runs/1/jobs") {
                json_response("200 OK", include_str!("../tests/fixtures/jobs.json"))
            } else if request_line.contains("/actions/runs/2/jobs") {
                json_response("404 Not Found", r#"{"message":"Not Found"}"#)
            } else if request_line.contains("/actions/runs?head_sha=3fa91c0de4&per_page=100&page=1")
            {
                json_response("200 OK", &two_runs_body())
            } else {
                json_response("500 Internal Server Error", "{}")
            }
        })
        .await;

        let runs = client_for(port)
            .workflow_runs("orbita-health/atlas", "3fa91c0de4")
            .await
            .unwrap();

        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].jobs.len(), 7);
        assert_eq!(runs[0].jobs_error, None);
        assert!(runs[1].jobs.is_empty());
        let error = runs[1].jobs_error.as_deref().unwrap();
        assert!(error.contains("404"), "{error}");
        assert!(!error.contains("127.0.0.1"), "{error}");
    }

    #[tokio::test]
    async fn auth_failure_while_fetching_jobs_still_fails_the_call() {
        use crate::test_server::{json_response, serve};

        let port = serve(|request_line| {
            if request_line.contains("/jobs") {
                json_response("401 Unauthorized", r#"{"message":"Bad credentials"}"#)
            } else {
                json_response("200 OK", &two_runs_body())
            }
        })
        .await;

        let result = client_for(port)
            .workflow_runs("orbita-health/atlas", "3fa91c0de4")
            .await;

        assert!(matches!(result, Err(GithubError::AuthMissing { .. })));
    }

    #[test]
    fn head_sha_must_be_hexadecimal() {
        assert!(is_sha("3fa91c0de4"));
        assert!(!is_sha(""));
        assert!(!is_sha("abc&per_page=1"));
    }
}
