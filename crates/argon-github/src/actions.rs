use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt, TryStreamExt};

use crate::actions_wire::{JobsResponse, RunsResponse, WireJob, WireRun, WireStep};
use crate::client::GithubClient;
use crate::error::{GithubError, Result};
use crate::rest::repo_path;

const JOBS_CONCURRENCY: usize = 3;
const PER_PAGE: &str = "100";
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

fn into_run(run: WireRun, jobs: JobsResponse) -> WorkflowRun {
    let jobs = jobs
        .jobs
        .into_iter()
        .map(|job| into_job(job, &run.html_url))
        .collect();
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
    }
}

fn is_sha(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_hexdigit())
}

impl GithubClient {
    pub async fn workflow_runs(&self, repo: &str, head_sha: &str) -> Result<Vec<WorkflowRun>> {
        let repo = repo_path(repo)?;
        if !is_sha(head_sha) {
            return Err(GithubError::Decode(format!("invalid head sha: {head_sha}")));
        }
        let runs: RunsResponse = self
            .rest_get(
                &format!("/repos/{repo}/actions/runs"),
                &[("head_sha", head_sha), ("per_page", PER_PAGE)],
            )
            .await?;
        stream::iter(runs.workflow_runs)
            .map(|run| async move {
                let jobs: JobsResponse = self
                    .rest_get(
                        &format!("/repos/{repo}/actions/runs/{}/jobs", run.id),
                        &[("per_page", PER_PAGE)],
                    )
                    .await?;
                Ok::<_, GithubError>(into_run(run, jobs))
            })
            .buffered(JOBS_CONCURRENCY)
            .try_collect()
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
                let jobs = jobs.take().unwrap_or(JobsResponse { jobs: Vec::new() });
                into_run(run, jobs)
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
    fn head_sha_must_be_hexadecimal() {
        assert!(is_sha("3fa91c0de4"));
        assert!(!is_sha(""));
        assert!(!is_sha("abc&per_page=1"));
    }
}
