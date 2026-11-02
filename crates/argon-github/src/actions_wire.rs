use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct RunsResponse {
    pub(crate) total_count: Option<u64>,
    pub(crate) workflow_runs: Vec<WireRun>,
}

#[derive(Deserialize)]
pub(crate) struct WireRun {
    pub(crate) id: u64,
    pub(crate) name: Option<String>,
    pub(crate) display_title: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) conclusion: Option<String>,
    pub(crate) html_url: String,
    pub(crate) run_attempt: Option<u32>,
}

#[derive(Deserialize)]
pub(crate) struct JobsResponse {
    pub(crate) total_count: Option<u64>,
    pub(crate) jobs: Vec<WireJob>,
}

#[derive(Deserialize)]
pub(crate) struct WireJob {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) status: Option<String>,
    pub(crate) conclusion: Option<String>,
    pub(crate) html_url: Option<String>,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) completed_at: Option<DateTime<Utc>>,
    pub(crate) steps: Option<Vec<WireStep>>,
}

#[derive(Deserialize)]
pub(crate) struct WireStep {
    pub(crate) name: String,
    pub(crate) status: Option<String>,
    pub(crate) conclusion: Option<String>,
    pub(crate) number: u32,
}
