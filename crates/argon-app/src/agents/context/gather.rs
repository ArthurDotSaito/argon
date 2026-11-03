use argon_core::PullRequest;
use argon_github::{PrDetail, PrFile};
use gpui_kit::{App, Task};

use super::{ci_failures, failed_jobs};
use crate::services::{ServiceError, Services};

#[derive(Debug, Clone)]
pub struct Gathered {
    pub detail: PrDetail,
    pub files: Vec<PrFile>,
}

pub fn gather(
    services: &Services,
    pr: &PullRequest,
    detail: Option<PrDetail>,
    cx: &App,
) -> Task<Result<Gathered, ServiceError>> {
    let (id, repo, number) = (pr.id.clone(), pr.repo.clone(), pr.number);
    services.spawn_github(cx, pr.host.clone(), move |client| async move {
        let detail = match detail {
            Some(detail) => detail,
            None => client.pr_detail(&id).await?,
        };
        let files = client.pr_files(&repo, number).await?;
        Ok(Gathered { detail, files })
    })
}

pub fn gather_ci(
    services: &Services,
    detail: &PrDetail,
    cx: &App,
) -> Task<Result<String, ServiceError>> {
    let repo = detail.pr.repo.clone();
    let head = detail.head_oid.clone();
    services.spawn_github(cx, detail.pr.host.clone(), move |client| async move {
        let runs = client.workflow_runs(&repo, &head).await?;
        let jobs = failed_jobs(&runs);
        let log = match jobs.first() {
            Some(job) => Some(
                client
                    .job_log(&repo, job.id)
                    .await
                    .map(|log| log.text)
                    .map_err(|error| error.to_string()),
            ),
            None => None,
        };
        Ok(ci_failures(&jobs, log))
    })
}
