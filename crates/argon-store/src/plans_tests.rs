use argon_core::{PrKey, SessionKind};
use chrono::{TimeZone, Utc};
use rusqlite::params;

use super::*;
use crate::agents::tests::new_session;

const HOST: &str = "github.com";
const ISSUE: &str = "ISSUE_1";

fn backdate(store: &Store, id: i64) {
    store
        .conn
        .execute(
            "UPDATE plan_drafts SET created_at = 1, updated_at = 1 WHERE id = ?1",
            params![id],
        )
        .unwrap();
}

fn record(store: &mut Store, session: &str, body: &str, sha: &str) -> PlanDraft {
    store.record_plan(HOST, ISSUE, session, body, sha).unwrap()
}

#[test]
fn first_plan_creates_an_open_draft() {
    let mut store = Store::open_in_memory().unwrap();
    let plan = record(&mut store, "s1", "# Plan v1", "sha1");
    assert_eq!(plan.host, HOST);
    assert_eq!(plan.issue_id, ISSUE);
    assert_eq!(plan.session_id, "s1");
    assert_eq!(plan.body, "# Plan v1");
    assert_eq!(plan.base_sha, "sha1");
    assert_eq!(plan.agent_body, None);
    assert!(!plan.edited);
    assert_eq!(plan.created_at, plan.updated_at);
    assert_eq!(plan.published_at, None);
    assert_eq!(plan.comment_id, None);
    assert_eq!(plan.comment_url, None);
    assert_eq!(store.plans_for_session("s1").unwrap(), [plan]);
}

#[test]
fn unedited_draft_is_replaced_by_the_next_agent_plan() {
    let mut store = Store::open_in_memory().unwrap();
    let first = record(&mut store, "s1", "v1", "sha1");
    backdate(&store, first.id);

    let second = record(&mut store, "s1", "v2", "sha2");

    assert_eq!(second.id, first.id);
    assert_eq!(second.body, "v2");
    assert_eq!(second.base_sha, "sha2");
    assert_eq!(second.agent_body, None);
    assert!(!second.edited);
    assert_eq!(second.created_at, Utc.timestamp_millis_opt(1).unwrap());
    assert!(second.updated_at > second.created_at);
    assert_eq!(store.plans_for_session("s1").unwrap().len(), 1);
}

#[test]
fn edited_draft_keeps_the_user_text_and_parks_the_agent_version() {
    let mut store = Store::open_in_memory().unwrap();
    let first = record(&mut store, "s1", "v1", "sha1");
    store.edit_plan(first.id, "my version").unwrap();

    let second = record(&mut store, "s1", "agent v2", "sha2");
    assert_eq!(second.id, first.id);
    assert_eq!(second.body, "my version");
    assert_eq!(second.base_sha, "sha1");
    assert_eq!(second.agent_body.as_deref(), Some("agent v2"));
    assert!(second.edited);

    let third = record(&mut store, "s1", "agent v3", "sha3");
    assert_eq!(third.body, "my version");
    assert_eq!(third.agent_body.as_deref(), Some("agent v3"));
    assert_eq!(store.plans_for_session("s1").unwrap().len(), 1);
}

#[test]
fn published_draft_is_locked_and_the_next_plan_is_a_new_row() {
    let mut store = Store::open_in_memory().unwrap();
    let first = record(&mut store, "s1", "v1", "sha1");
    let at = Utc.timestamp_millis_opt(5_000).unwrap();
    store
        .mark_plan_published(first.id, "IC_1", "https://github.com/o/r/issues/1#c1", at)
        .unwrap();

    let error = store.edit_plan(first.id, "late edit").unwrap_err();
    assert!(matches!(error, StoreError::PlanPublished(id) if id == first.id));
    let error = store.resolve_agent_version(first.id, false).unwrap_err();
    assert!(matches!(error, StoreError::PlanPublished(id) if id == first.id));

    let second = record(&mut store, "s1", "v2", "sha2");
    assert_ne!(second.id, first.id);
    assert_eq!(second.body, "v2");
    assert_eq!(second.published_at, None);

    let plans = store.plans_for_session("s1").unwrap();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].id, second.id);
    assert_eq!(plans[1].body, "v1");
    assert_eq!(plans[1].published_at, Some(at));
    assert_eq!(plans[1].comment_id.as_deref(), Some("IC_1"));
    assert_eq!(
        plans[1].comment_url.as_deref(),
        Some("https://github.com/o/r/issues/1#c1")
    );
}

#[test]
fn editing_an_unknown_draft_fails() {
    let mut store = Store::open_in_memory().unwrap();
    assert!(store.edit_plan(99, "x").is_err());
    assert!(store.resolve_agent_version(99, true).is_err());
    assert!(store.mark_plan_published(99, "c", "u", Utc::now()).is_err());
}

#[test]
fn using_the_agent_version_replaces_the_body_and_clears_the_edit() {
    let mut store = Store::open_in_memory().unwrap();
    let first = record(&mut store, "s1", "v1", "sha1");
    store.edit_plan(first.id, "mine").unwrap();
    record(&mut store, "s1", "agent v2", "sha2");

    store.resolve_agent_version(first.id, true).unwrap();

    let plan = &store.plans_for_session("s1").unwrap()[0];
    assert_eq!(plan.body, "agent v2");
    assert_eq!(plan.agent_body, None);
    assert!(!plan.edited);
}

#[test]
fn keeping_my_version_only_drops_the_agent_version() {
    let mut store = Store::open_in_memory().unwrap();
    let first = record(&mut store, "s1", "v1", "sha1");
    store.edit_plan(first.id, "mine").unwrap();
    record(&mut store, "s1", "agent v2", "sha2");

    store.resolve_agent_version(first.id, false).unwrap();

    let plan = &store.plans_for_session("s1").unwrap()[0];
    assert_eq!(plan.body, "mine");
    assert_eq!(plan.agent_body, None);
    assert!(plan.edited);
}

#[test]
fn using_the_agent_version_without_one_pending_changes_nothing() {
    let mut store = Store::open_in_memory().unwrap();
    let first = record(&mut store, "s1", "v1", "sha1");
    store.edit_plan(first.id, "mine").unwrap();

    store.resolve_agent_version(first.id, true).unwrap();

    let plan = &store.plans_for_session("s1").unwrap()[0];
    assert_eq!(plan.body, "mine");
    assert!(plan.edited);
}

#[test]
fn plans_are_listed_newest_first_per_issue_and_per_session() {
    let mut store = Store::open_in_memory().unwrap();
    let a = record(&mut store, "s1", "a", "sha");
    let b = record(&mut store, "s2", "b", "sha");
    store
        .record_plan(HOST, "ISSUE_2", "s3", "other", "sha")
        .unwrap();
    store
        .record_plan("ghe.acme.io", ISSUE, "s4", "other host", "sha")
        .unwrap();

    let ids: Vec<i64> = store
        .plans_for_issue(HOST, ISSUE)
        .unwrap()
        .iter()
        .map(|plan| plan.id)
        .collect();
    assert_eq!(ids, [b.id, a.id]);
    assert_eq!(store.plans_for_session("s2").unwrap().len(), 1);
    assert!(store.plans_for_session("missing").unwrap().is_empty());
}

#[test]
fn session_kind_and_base_sha_round_trip() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .create_session(&crate::NewAgentSession {
            kind: SessionKind::Plan,
            base_sha: Some("abc123".to_owned()),
            pr: PrKey::new(HOST, ISSUE),
            ..new_session("plan1")
        })
        .unwrap();
    store.create_session(&new_session("review1")).unwrap();

    let plan = store.session("plan1").unwrap().unwrap();
    assert_eq!(plan.kind, SessionKind::Plan);
    assert_eq!(plan.base_sha.as_deref(), Some("abc123"));
    let review = store.session("review1").unwrap().unwrap();
    assert_eq!(review.kind, SessionKind::Review);
    assert_eq!(review.base_sha, None);
}
