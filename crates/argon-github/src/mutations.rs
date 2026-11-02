use argon_core::PrId;
use reqwest::Method;
use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::{Value, json};

use crate::client::GithubClient;
use crate::error::{GithubError, Result};
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddCommentData {
    add_comment: Option<IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergeData {
    merge_pull_request: Option<IgnoredAny>,
}

pub(crate) fn require_payload(payload: Option<IgnoredAny>, mutation: &str) -> Result<()> {
    payload
        .map(|_| ())
        .ok_or_else(|| GithubError::GraphQl(vec![format!("{mutation} returned no payload")]))
}

fn add_comment_variables(pr: &PrId, body: &str) -> Value {
    json!({ "input": { "subjectId": pr.0, "body": body } })
}

fn merge_variables(pr: &PrId, method: MergeMethod, expected_head_oid: &str) -> Value {
    json!({ "input": { "pullRequestId": pr.0, "mergeMethod": method.as_graphql(), "expectedHeadOid": expected_head_oid } })
}

fn rerun_failed_path(repo: &str, run_id: u64) -> Result<String> {
    let repo = repo_path(repo)?;
    Ok(format!(
        "/repos/{repo}/actions/runs/{run_id}/rerun-failed-jobs"
    ))
}

impl GithubClient {
    pub async fn add_comment(&self, pr: &PrId, body: &str) -> Result<()> {
        let data: AddCommentData = self
            .graphql(
                "add_comment",
                ADD_COMMENT_QUERY,
                add_comment_variables(pr, body),
            )
            .await?;
        require_payload(data.add_comment, "addComment")
    }

    pub async fn merge(
        &self,
        pr: &PrId,
        method: MergeMethod,
        expected_head_oid: &str,
    ) -> Result<()> {
        let data: MergeData = self
            .graphql(
                "merge",
                MERGE_QUERY,
                merge_variables(pr, method, expected_head_oid),
            )
            .await?;
        require_payload(data.merge_pull_request, "mergePullRequest")
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
    use chrono::Utc;

    use super::*;
    use crate::wire::Envelope;

    const HEAD_OID: &str = "3fa91c0de4b27a85d6e1f09c4b7a2d83e5f61a90";

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
                merge_variables(&pr(), method, HEAD_OID),
                json!({ "input": {
                    "pullRequestId": "PR_kwDOOrbita0412",
                    "mergeMethod": expected,
                    "expectedHeadOid": HEAD_OID
                } })
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

    fn merge_outcome(json: &str) -> Result<()> {
        let envelope: Envelope<MergeData> = serde_json::from_str(json).unwrap();
        let data = envelope.into_data(Utc::now())?.data;
        require_payload(data.merge_pull_request, "mergePullRequest")
    }

    #[test]
    fn merge_envelope_with_errors_and_null_payload_is_an_error() {
        let json = r#"{
            "data": { "mergePullRequest": null },
            "errors": [{
                "type": "UNPROCESSABLE",
                "path": ["mergePullRequest"],
                "message": "Pull request is not mergeable"
            }]
        }"#;
        match merge_outcome(json) {
            Err(GithubError::GraphQl(messages)) => {
                assert_eq!(
                    messages,
                    vec!["[UNPROCESSABLE] Pull request is not mergeable"]
                );
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn merge_envelope_with_null_payload_and_no_errors_is_an_error() {
        let json = r#"{ "data": { "mergePullRequest": null } }"#;
        assert!(matches!(merge_outcome(json), Err(GithubError::GraphQl(_))));
    }

    #[test]
    fn merge_envelope_with_payload_is_ok() {
        let json = r#"{ "data": { "mergePullRequest": { "clientMutationId": null } } }"#;
        assert!(merge_outcome(json).is_ok());
    }

    #[test]
    fn add_comment_null_payload_is_an_error() {
        let envelope: Envelope<AddCommentData> =
            serde_json::from_str(r#"{ "data": { "addComment": null } }"#).unwrap();
        let data = envelope.into_data(Utc::now()).unwrap().data;
        assert!(require_payload(data.add_comment, "addComment").is_err());
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
