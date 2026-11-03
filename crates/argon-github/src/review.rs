use argon_core::{PrId, ReviewSubmission};
use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::{Value, json};

use crate::client::GithubClient;
use crate::error::{GithubError, Result};
use crate::mutations::require_payload;

const SUBMIT_REVIEW_QUERY: &str = include_str!("../queries/add_pull_request_review.graphql");
const REPLY_THREAD_QUERY: &str = include_str!("../queries/add_review_thread_reply.graphql");
const RESOLVE_THREAD_QUERY: &str = include_str!("../queries/resolve_review_thread.graphql");
const UNRESOLVE_THREAD_QUERY: &str = include_str!("../queries/unresolve_review_thread.graphql");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmitReviewData {
    add_pull_request_review: Option<SubmitReviewPayload>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmitReviewPayload {
    pull_request_review: Option<ReviewNode>,
}

#[derive(Deserialize)]
struct ReviewNode {
    id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplyThreadData {
    add_pull_request_review_thread_reply: Option<IgnoredAny>,
}

#[derive(Deserialize)]
struct ResolveThreadData {
    #[serde(alias = "resolveReviewThread", alias = "unresolveReviewThread")]
    payload: Option<IgnoredAny>,
}

fn submit_review_variables(pr: &PrId, head_oid: &str, submission: &ReviewSubmission) -> Value {
    let mut input = json!({
        "pullRequestId": pr.0,
        "commitOID": head_oid,
        "event": submission.event.as_graphql(),
        "threads": submission.threads,
    });
    if !submission.body.is_empty() {
        input["body"] = Value::String(submission.body.clone());
    }
    json!({ "input": input })
}

fn reply_thread_variables(thread_id: &str, body: &str) -> Value {
    json!({ "input": { "pullRequestReviewThreadId": thread_id, "body": body } })
}

fn thread_resolution_variables(thread_id: &str) -> Value {
    json!({ "input": { "threadId": thread_id } })
}

fn thread_resolution_mutation(resolved: bool) -> (&'static str, &'static str) {
    if resolved {
        ("resolveReviewThread", RESOLVE_THREAD_QUERY)
    } else {
        ("unresolveReviewThread", UNRESOLVE_THREAD_QUERY)
    }
}

fn review_id(data: SubmitReviewData) -> Result<String> {
    let payload = data.add_pull_request_review.ok_or_else(|| {
        GithubError::GraphQl(vec!["addPullRequestReview returned no payload".to_string()])
    })?;
    payload
        .pull_request_review
        .and_then(|review| review.id)
        .ok_or_else(|| {
            GithubError::GraphQl(vec![
                "addPullRequestReview returned no review id".to_string(),
            ])
        })
}

impl GithubClient {
    pub async fn submit_review(
        &self,
        pr: &PrId,
        head_oid: &str,
        submission: &ReviewSubmission,
    ) -> Result<String> {
        let data: SubmitReviewData = self
            .graphql(
                "submit_review",
                SUBMIT_REVIEW_QUERY,
                submit_review_variables(pr, head_oid, submission),
            )
            .await?;
        review_id(data)
    }

    pub async fn reply_thread(&self, thread_id: &str, body: &str) -> Result<()> {
        let data: ReplyThreadData = self
            .graphql(
                "reply_thread",
                REPLY_THREAD_QUERY,
                reply_thread_variables(thread_id, body),
            )
            .await?;
        require_payload(
            data.add_pull_request_review_thread_reply,
            "addPullRequestReviewThreadReply",
        )
    }

    pub async fn set_thread_resolved(&self, thread_id: &str, resolved: bool) -> Result<()> {
        let (mutation, query) = thread_resolution_mutation(resolved);
        let data: ResolveThreadData = self
            .graphql(mutation, query, thread_resolution_variables(thread_id))
            .await?;
        require_payload(data.payload, mutation)
    }
}

#[cfg(test)]
mod tests {
    use argon_core::{
        DraftComment, DraftSource, PrKey, ReviewEvent, Side, build_submission,
        single_comment_submission,
    };
    use chrono::Utc;

    use super::*;
    use crate::wire::Envelope;

    const HEAD_OID: &str = "3fa91c0de4b27a85d6e1f09c4b7a2d83e5f61a90";
    const LINE_NOT_IN_DIFF: &str = "pull_request_review_thread.line must be part of the diff";

    fn pr() -> PrId {
        PrId("PR_kwDOOrbita0412".to_string())
    }

    fn draft(id: i64, path: &str, line: u32, side: Side) -> DraftComment {
        DraftComment {
            id,
            pr: PrKey::new("github.com", "PR_kwDOOrbita0412"),
            path: path.to_string(),
            line,
            side,
            start_line: None,
            start_side: None,
            body: format!("comment {id}"),
            source: DraftSource::User,
            session_id: None,
            head_oid: None,
        }
    }

    fn envelope<T: serde::de::DeserializeOwned>(json: &str) -> Result<T> {
        let envelope: Envelope<T> = serde_json::from_str(json).unwrap();
        Ok(envelope.into_data(Utc::now())?.data)
    }

    #[test]
    fn submit_variables_carry_commit_event_and_threads() {
        let ranged = DraftComment {
            start_line: Some(10),
            start_side: Some(Side::Right),
            ..draft(1, "src/a.ts", 14, Side::Right)
        };
        let removed = draft(2, "src/old.ts", 3, Side::Left);
        let submission =
            build_submission("Looks good", ReviewEvent::Approve, &[ranged, removed]).unwrap();
        assert_eq!(
            submit_review_variables(&pr(), HEAD_OID, &submission),
            json!({ "input": {
                "pullRequestId": "PR_kwDOOrbita0412",
                "commitOID": HEAD_OID,
                "body": "Looks good",
                "event": "APPROVE",
                "threads": [
                    { "path": "src/a.ts", "line": 14, "side": "RIGHT",
                      "startLine": 10, "startSide": "RIGHT", "body": "comment 1" },
                    { "path": "src/old.ts", "line": 3, "side": "LEFT", "body": "comment 2" }
                ]
            } })
        );
    }

    #[test]
    fn submit_variables_omit_an_empty_body() {
        let submission = single_comment_submission(&draft(1, "a.ts", 2, Side::Right));
        assert_eq!(
            submit_review_variables(&pr(), HEAD_OID, &submission),
            json!({ "input": {
                "pullRequestId": "PR_kwDOOrbita0412",
                "commitOID": HEAD_OID,
                "event": "COMMENT",
                "threads": [{ "path": "a.ts", "line": 2, "side": "RIGHT", "body": "comment 1" }]
            } })
        );
    }

    #[test]
    fn submit_variables_map_every_event() {
        for (event, expected) in [
            (ReviewEvent::Comment, "COMMENT"),
            (ReviewEvent::Approve, "APPROVE"),
            (ReviewEvent::RequestChanges, "REQUEST_CHANGES"),
        ] {
            let submission = build_submission("body", event, &[]).unwrap();
            let variables = submit_review_variables(&pr(), HEAD_OID, &submission);
            assert_eq!(variables["input"]["event"], json!(expected));
            assert_eq!(variables["input"]["threads"], json!([]));
        }
    }

    #[test]
    fn reply_variables_carry_thread_and_body() {
        let body = "Done in `abc123`.\n\"quoted\" \\ backslash";
        assert_eq!(
            reply_thread_variables("PRRT_kwDOOrbita01", body),
            json!({ "input": { "pullRequestReviewThreadId": "PRRT_kwDOOrbita01", "body": body } })
        );
    }

    #[test]
    fn resolution_variables_carry_the_thread_id() {
        assert_eq!(
            thread_resolution_variables("PRRT_kwDOOrbita01"),
            json!({ "input": { "threadId": "PRRT_kwDOOrbita01" } })
        );
    }

    #[test]
    fn resolved_flag_selects_the_matching_mutation() {
        let (resolve, resolve_query) = thread_resolution_mutation(true);
        assert_eq!(resolve, "resolveReviewThread");
        assert!(resolve_query.contains("resolveReviewThread(input: $input)"));
        assert!(resolve_query.contains("$input: ResolveReviewThreadInput!"));
        let (unresolve, unresolve_query) = thread_resolution_mutation(false);
        assert_eq!(unresolve, "unresolveReviewThread");
        assert!(unresolve_query.contains("unresolveReviewThread(input: $input)"));
        assert!(unresolve_query.contains("$input: UnresolveReviewThreadInput!"));
    }

    #[test]
    fn review_documents_match_their_variables() {
        assert!(SUBMIT_REVIEW_QUERY.contains("addPullRequestReview(input: $input)"));
        assert!(SUBMIT_REVIEW_QUERY.contains("$input: AddPullRequestReviewInput!"));
        assert!(SUBMIT_REVIEW_QUERY.contains("pullRequestReview"));
        assert!(REPLY_THREAD_QUERY.contains("addPullRequestReviewThreadReply(input: $input)"));
        assert!(REPLY_THREAD_QUERY.contains("$input: AddPullRequestReviewThreadReplyInput!"));
    }

    #[test]
    fn submit_payload_yields_the_review_id() {
        let data: SubmitReviewData = envelope(
            r#"{ "data": { "addPullRequestReview": { "pullRequestReview": { "id": "PRR_kwDOOrbita77" } } } }"#,
        )
        .unwrap();
        assert_eq!(review_id(data).unwrap(), "PRR_kwDOOrbita77");
    }

    #[test]
    fn submit_null_payload_or_missing_review_is_an_error() {
        for json in [
            r#"{ "data": { "addPullRequestReview": null } }"#,
            r#"{ "data": { "addPullRequestReview": { "pullRequestReview": null } } }"#,
            r#"{ "data": { "addPullRequestReview": { "pullRequestReview": { "id": null } } } }"#,
        ] {
            let data: SubmitReviewData = envelope(json).unwrap();
            assert!(matches!(review_id(data), Err(GithubError::GraphQl(_))));
        }
    }

    #[test]
    fn reply_null_payload_is_an_error_and_payload_is_ok() {
        let null: ReplyThreadData =
            envelope(r#"{ "data": { "addPullRequestReviewThreadReply": null } }"#).unwrap();
        assert!(
            require_payload(
                null.add_pull_request_review_thread_reply,
                "addPullRequestReviewThreadReply"
            )
            .is_err()
        );
        let ok: ReplyThreadData = envelope(
            r#"{ "data": { "addPullRequestReviewThreadReply": { "comment": { "id": "PRRC_1" } } } }"#,
        )
        .unwrap();
        assert!(
            require_payload(
                ok.add_pull_request_review_thread_reply,
                "addPullRequestReviewThreadReply"
            )
            .is_ok()
        );
    }

    #[test]
    fn resolution_payloads_deserialize_under_either_field_name() {
        for json in [
            r#"{ "resolveReviewThread": { "clientMutationId": null } }"#,
            r#"{ "unresolveReviewThread": { "clientMutationId": null } }"#,
        ] {
            let data: ResolveThreadData = serde_json::from_str(json).unwrap();
            assert!(require_payload(data.payload, "resolution").is_ok());
        }
    }

    #[test]
    fn resolution_null_payload_is_an_error() {
        for json in [
            r#"{ "resolveReviewThread": null }"#,
            r#"{ "unresolveReviewThread": null }"#,
        ] {
            let data: ResolveThreadData = serde_json::from_str(json).unwrap();
            assert!(matches!(
                require_payload(data.payload, "resolution"),
                Err(GithubError::GraphQl(_))
            ));
        }
    }

    #[test]
    fn line_outside_the_diff_keeps_the_github_message() {
        let json = format!(
            r#"{{
                "data": {{ "addPullRequestReview": null }},
                "errors": [{{
                    "type": "UNPROCESSABLE",
                    "path": ["addPullRequestReview"],
                    "message": "{LINE_NOT_IN_DIFF}"
                }}]
            }}"#
        );
        match envelope::<SubmitReviewData>(&json) {
            Err(GithubError::GraphQl(messages)) => {
                assert_eq!(
                    messages,
                    vec![format!("[UNPROCESSABLE] {LINE_NOT_IN_DIFF}")]
                );
                assert!(
                    GithubError::GraphQl(messages)
                        .to_string()
                        .contains(LINE_NOT_IN_DIFF)
                );
            }
            other => panic!("unexpected result: {:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn line_error_without_data_keeps_the_message() {
        let json = format!(r#"{{ "errors": [{{ "message": "{LINE_NOT_IN_DIFF}" }}] }}"#);
        match envelope::<SubmitReviewData>(&json) {
            Err(GithubError::GraphQl(messages)) => assert_eq!(messages, vec![LINE_NOT_IN_DIFF]),
            other => panic!("unexpected result: {:?}", other.map(|_| ())),
        }
    }
}
