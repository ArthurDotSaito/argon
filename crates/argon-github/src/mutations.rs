use argon_core::{IssueStateReason, PrId};
use reqwest::Method;
use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::{Value, json};

use crate::client::GithubClient;
use crate::error::{GithubError, Result};
use crate::rest::repo_path;

const ADD_COMMENT_QUERY: &str = include_str!("../queries/add_comment.graphql");
const MERGE_QUERY: &str = include_str!("../queries/merge_pull_request.graphql");
const CLOSE_ISSUE_QUERY: &str = include_str!("../queries/close_issue.graphql");
const ADD_ASSIGNEES_QUERY: &str = include_str!("../queries/add_assignees.graphql");

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentRef {
    pub id: String,
    pub url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddCommentData {
    add_comment: Option<AddCommentPayload>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddCommentPayload {
    comment_edge: Option<CommentEdge>,
}

#[derive(Deserialize)]
struct CommentEdge {
    node: Option<CommentNode>,
}

#[derive(Deserialize)]
struct CommentNode {
    id: String,
    url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergeData {
    merge_pull_request: Option<IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloseIssueData {
    close_issue: Option<IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddAssigneesData {
    add_assignees_to_assignable: Option<IgnoredAny>,
}

pub(crate) fn require_payload(payload: Option<IgnoredAny>, mutation: &str) -> Result<()> {
    payload
        .map(|_| ())
        .ok_or_else(|| GithubError::GraphQl(vec![format!("{mutation} returned no payload")]))
}

fn comment_ref(payload: Option<AddCommentPayload>) -> Result<CommentRef> {
    let payload = payload
        .ok_or_else(|| GithubError::GraphQl(vec!["addComment returned no payload".to_string()]))?;
    let node = payload
        .comment_edge
        .and_then(|edge| edge.node)
        .ok_or_else(|| GithubError::Unconfirmed("addComment".to_string()))?;
    Ok(CommentRef {
        id: node.id,
        url: node.url,
    })
}

fn add_comment_variables(subject: &PrId, body: &str) -> Value {
    json!({ "input": { "subjectId": subject.0, "body": body } })
}

fn close_issue_state_reason(reason: IssueStateReason) -> Result<&'static str> {
    match reason {
        IssueStateReason::Completed => Ok("COMPLETED"),
        IssueStateReason::NotPlanned => Ok("NOT_PLANNED"),
        IssueStateReason::Duplicate => Ok("DUPLICATE"),
        IssueStateReason::Reopened => Err(GithubError::GraphQl(vec![
            "an issue cannot be closed with the reopened reason".to_string(),
        ])),
    }
}

fn close_issue_variables(issue: &PrId, reason: IssueStateReason) -> Result<Value> {
    Ok(json!({ "input": { "issueId": issue.0, "stateReason": close_issue_state_reason(reason)? } }))
}

fn add_assignees_variables(issue: &PrId, assignee_ids: &[String]) -> Value {
    json!({ "input": { "assignableId": issue.0, "assigneeIds": assignee_ids } })
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
    pub async fn add_comment(&self, subject: &PrId, body: &str) -> Result<CommentRef> {
        let data: AddCommentData = self
            .graphql(
                "add_comment",
                ADD_COMMENT_QUERY,
                add_comment_variables(subject, body),
            )
            .await?;
        comment_ref(data.add_comment)
    }

    pub async fn close_issue(&self, id: &PrId, reason: IssueStateReason) -> Result<()> {
        let variables = close_issue_variables(id, reason)?;
        let data: CloseIssueData = self
            .graphql("close_issue", CLOSE_ISSUE_QUERY, variables)
            .await?;
        require_payload(data.close_issue, "closeIssue")
    }

    pub async fn add_assignees(&self, id: &PrId, assignee_ids: &[String]) -> Result<()> {
        let data: AddAssigneesData = self
            .graphql(
                "add_assignees",
                ADD_ASSIGNEES_QUERY,
                add_assignees_variables(id, assignee_ids),
            )
            .await?;
        require_payload(data.add_assignees_to_assignable, "addAssigneesToAssignable")
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
        assert!(CLOSE_ISSUE_QUERY.contains("closeIssue(input: $input)"));
        assert!(CLOSE_ISSUE_QUERY.contains("$input: CloseIssueInput!"));
        assert!(ADD_ASSIGNEES_QUERY.contains("addAssigneesToAssignable(input: $input)"));
        assert!(ADD_ASSIGNEES_QUERY.contains("$input: AddAssigneesToAssignableInput!"));
    }

    #[test]
    fn close_issue_variables_map_every_closing_reason() {
        let issue = PrId("I_kwDOOrbita0101".to_string());
        for (reason, expected) in [
            (IssueStateReason::Completed, "COMPLETED"),
            (IssueStateReason::NotPlanned, "NOT_PLANNED"),
            (IssueStateReason::Duplicate, "DUPLICATE"),
        ] {
            assert_eq!(
                close_issue_variables(&issue, reason).unwrap(),
                json!({ "input": { "issueId": "I_kwDOOrbita0101", "stateReason": expected } })
            );
        }
    }

    #[test]
    fn close_issue_rejects_the_reopened_reason() {
        let issue = PrId("I_kwDOOrbita0101".to_string());
        assert!(matches!(
            close_issue_variables(&issue, IssueStateReason::Reopened),
            Err(GithubError::GraphQl(_))
        ));
    }

    #[test]
    fn add_assignees_variables_carry_the_issue_and_the_users() {
        let issue = PrId("I_kwDOOrbita0101".to_string());
        let assignees = vec!["U_kgDOOrbita01".to_string(), "U_kgDOOrbita02".to_string()];
        assert_eq!(
            add_assignees_variables(&issue, &assignees),
            json!({ "input": {
                "assignableId": "I_kwDOOrbita0101",
                "assigneeIds": ["U_kgDOOrbita01", "U_kgDOOrbita02"]
            } })
        );
    }

    fn closed_issue(json: &str) -> Result<()> {
        let envelope: Envelope<CloseIssueData> = serde_json::from_str(json).unwrap();
        require_payload(
            envelope.into_data(Utc::now())?.data.close_issue,
            "closeIssue",
        )
    }

    fn added_assignees(json: &str) -> Result<()> {
        let envelope: Envelope<AddAssigneesData> = serde_json::from_str(json).unwrap();
        require_payload(
            envelope
                .into_data(Utc::now())?
                .data
                .add_assignees_to_assignable,
            "addAssigneesToAssignable",
        )
    }

    #[test]
    fn close_issue_without_payload_is_an_error() {
        assert!(matches!(
            closed_issue(r#"{ "data": { "closeIssue": null } }"#),
            Err(GithubError::GraphQl(_))
        ));
    }

    #[test]
    fn close_issue_with_payload_is_ok() {
        assert!(
            closed_issue(r#"{ "data": { "closeIssue": { "clientMutationId": null } } }"#).is_ok()
        );
    }

    #[test]
    fn add_assignees_without_payload_is_an_error() {
        assert!(matches!(
            added_assignees(r#"{ "data": { "addAssigneesToAssignable": null } }"#),
            Err(GithubError::GraphQl(_))
        ));
    }

    #[test]
    fn add_assignees_with_payload_is_ok() {
        assert!(
            added_assignees(
                r#"{ "data": { "addAssigneesToAssignable": { "clientMutationId": null } } }"#
            )
            .is_ok()
        );
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

    fn added_comment(json: &str) -> Result<CommentRef> {
        let envelope: Envelope<AddCommentData> = serde_json::from_str(json).unwrap();
        comment_ref(envelope.into_data(Utc::now())?.data.add_comment)
    }

    #[test]
    fn add_comment_null_payload_is_an_error() {
        assert!(matches!(
            added_comment(r#"{ "data": { "addComment": null } }"#),
            Err(GithubError::GraphQl(_))
        ));
    }

    #[test]
    fn add_comment_payload_without_comment_edge_is_unconfirmed() {
        for json in [
            r#"{ "data": { "addComment": { "commentEdge": null } } }"#,
            r#"{ "data": { "addComment": { "commentEdge": { "node": null } } } }"#,
        ] {
            assert!(
                matches!(added_comment(json), Err(GithubError::Unconfirmed(_))),
                "{json}"
            );
        }
    }

    #[test]
    fn add_comment_reads_the_created_comment_from_the_edge() {
        let json = r#"{ "data": { "addComment": { "commentEdge": { "node": {
            "id": "IC_kwDOOrbita77",
            "url": "https://github.com/orbita-health/atlas/issues/101#issuecomment-77"
        } } } } }"#;
        assert_eq!(
            added_comment(json).unwrap(),
            CommentRef {
                id: "IC_kwDOOrbita77".to_string(),
                url: "https://github.com/orbita-health/atlas/issues/101#issuecomment-77"
                    .to_string(),
            }
        );
    }

    #[test]
    fn add_comment_document_asks_for_the_comment_edge_node() {
        assert!(ADD_COMMENT_QUERY.contains("commentEdge"));
        assert!(ADD_COMMENT_QUERY.contains("node {"));
        assert!(ADD_COMMENT_QUERY.contains("url"));
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
