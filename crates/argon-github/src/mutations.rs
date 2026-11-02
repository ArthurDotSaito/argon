use argon_core::PrId;
use reqwest::Method;
use serde::de::IgnoredAny;
use serde_json::{Value, json};

use crate::client::GithubClient;
use crate::error::Result;
use crate::rest::repo_path;

const ADD_COMMENT_QUERY: &str = include_str!("../queries/add_comment.graphql");
const MERGE_QUERY: &str = include_str!("../queries/merge_pull_request.graphql");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    fn as_graphql(self) -> &'static str {
        match self {
            Self::Merge => "MERGE",
            Self::Squash => "SQUASH",
            Self::Rebase => "REBASE",
        }
    }
}

fn add_comment_variables(pr: &PrId, body: &str) -> Value {
    json!({ "input": { "subjectId": pr.0, "body": body } })
}

fn merge_variables(pr: &PrId, method: MergeMethod) -> Value {
    json!({ "input": { "pullRequestId": pr.0, "mergeMethod": method.as_graphql() } })
}

fn rerun_failed_path(repo: &str, run_id: u64) -> Result<String> {
    let repo = repo_path(repo)?;
    Ok(format!(
        "/repos/{repo}/actions/runs/{run_id}/rerun-failed-jobs"
    ))
}

impl GithubClient {
    pub async fn add_comment(&self, pr: &PrId, body: &str) -> Result<()> {
        let _: IgnoredAny = self
            .graphql(
                "add_comment",
                ADD_COMMENT_QUERY,
                add_comment_variables(pr, body),
            )
            .await?;
        Ok(())
    }

    pub async fn merge(&self, pr: &PrId, method: MergeMethod) -> Result<()> {
        let _: IgnoredAny = self
            .graphql("merge", MERGE_QUERY, merge_variables(pr, method))
            .await?;
        Ok(())
    }

    pub async fn rerun_failed(&self, repo: &str, run_id: u64) -> Result<()> {
        let url = self.rest_url_for(&rerun_failed_path(repo, run_id)?, &[])?;
        let response = self.rest_request(Method::POST, url).send().await?;
        self.check_status(response)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr() -> PrId {
        PrId("PR_kwDOOrbita0412".to_string())
    }

    #[test]
    fn add_comment_variables_carry_subject_and_body() {
        let body = "Looks good.\n\n```rust\nlet x = 1;\n```\n\"quoted\" \\ backslash";
        assert_eq!(
            add_comment_variables(&pr(), body),
            json!({ "input": { "subjectId": "PR_kwDOOrbita0412", "body": body } })
        );
    }

    #[test]
    fn merge_variables_map_every_method() {
        for (method, expected) in [
            (MergeMethod::Merge, "MERGE"),
            (MergeMethod::Squash, "SQUASH"),
            (MergeMethod::Rebase, "REBASE"),
        ] {
            assert_eq!(
                merge_variables(&pr(), method),
                json!({ "input": { "pullRequestId": "PR_kwDOOrbita0412", "mergeMethod": expected } })
            );
        }
    }

    #[test]
    fn mutation_documents_match_their_variables() {
        assert!(ADD_COMMENT_QUERY.contains("addComment(input: $input)"));
        assert!(ADD_COMMENT_QUERY.contains("$input: AddCommentInput!"));
        assert!(MERGE_QUERY.contains("mergePullRequest(input: $input)"));
        assert!(MERGE_QUERY.contains("$input: MergePullRequestInput!"));
    }

    #[test]
    fn rerun_failed_path_targets_the_run() {
        assert_eq!(
            rerun_failed_path("orbita-health/atlas", 9100000101).unwrap(),
            "/repos/orbita-health/atlas/actions/runs/9100000101/rerun-failed-jobs"
        );
        assert!(rerun_failed_path("../atlas", 1).is_err());
    }
}
