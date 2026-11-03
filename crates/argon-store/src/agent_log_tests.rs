use serde::{Deserialize, Serialize};

use super::*;
use crate::agents::tests::new_session;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum Event {
    Chunk(String),
    Ended { reason: String },
}

fn finding(title: &str, severity: Severity, side: Side) -> Finding {
    Finding {
        severity,
        file: "src/a.rs".to_owned(),
        line: 12,
        side,
        title: title.to_owned(),
        body: "corpo".to_owned(),
        suggestion: Some("let x = 1;".to_owned()),
    }
}

fn json(event: &Event) -> String {
    serde_json::to_string(event).unwrap()
}

#[test]
fn events_round_trip_in_order_per_session() {
    let mut store = Store::open_in_memory().unwrap();
    store.create_session(&new_session("s1")).unwrap();
    store.create_session(&new_session("s2")).unwrap();
    let first = Event::Chunk("olá".to_owned());
    let second = Event::Ended {
        reason: "end_turn".to_owned(),
    };
    assert_eq!(store.append_event("s1", &first).unwrap(), 1);
    assert_eq!(store.append_event("s2", &first).unwrap(), 1);
    assert_eq!(store.append_event("s1", &second).unwrap(), 2);
    assert_eq!(
        store.events::<Event>("s1").unwrap(),
        [first.clone(), second]
    );
    assert_eq!(store.events::<Event>("s2").unwrap(), [first]);
}

#[test]
fn batch_writes_events_status_and_model_in_one_transaction() {
    let mut store = Store::open_in_memory().unwrap();
    store.tune_for_background_writes().unwrap();
    store.create_session(&new_session("s1")).unwrap();
    store.create_session(&new_session("s2")).unwrap();
    let a = Event::Chunk("a".into());
    let b = Event::Chunk("b".into());
    store
        .append_batch(&[
            SessionWrite {
                session_id: "s1".into(),
                events: vec![(1, json(&a)), (2, json(&b))],
                status: Some(SessionStatus::AwaitingUser),
                model: Some("opus".into()),
            },
            SessionWrite {
                session_id: "s2".into(),
                events: vec![(7, json(&b))],
                status: None,
                model: None,
            },
        ])
        .unwrap();
    let s1 = store.session("s1").unwrap().unwrap();
    assert_eq!(s1.status, SessionStatus::AwaitingUser);
    assert_eq!(s1.model, "opus");
    assert_eq!(store.last_event_seq("s1").unwrap(), 2);
    assert_eq!(store.last_event_seq("s2").unwrap(), 7);
    assert_eq!(store.last_event_seq("none").unwrap(), 0);
    assert_eq!(
        store.events_since::<Event>("s1", 1).unwrap(),
        [(2, b.clone())]
    );
    assert_eq!(store.events_since::<Event>("s2", 0).unwrap(), [(7, b)]);
}

#[test]
fn failed_batch_rolls_back_everything() {
    let mut store = Store::open_in_memory().unwrap();
    store.create_session(&new_session("s1")).unwrap();
    let a = json(&Event::Chunk("a".into()));
    store
        .append_batch(&[SessionWrite {
            session_id: "s1".into(),
            events: vec![(1, a.clone())],
            ..SessionWrite::default()
        }])
        .unwrap();
    let duplicate = store.append_batch(&[SessionWrite {
        session_id: "s1".into(),
        events: vec![(2, a.clone()), (1, a)],
        status: Some(SessionStatus::Failed),
        model: None,
    }]);
    assert!(duplicate.is_err());
    assert_eq!(store.last_event_seq("s1").unwrap(), 1);
    assert_eq!(
        store.session("s1").unwrap().unwrap().status,
        SessionStatus::Running
    );
}

#[test]
fn findings_are_saved_listed_and_change_state() {
    let mut store = Store::open_in_memory().unwrap();
    store.create_session(&new_session("s1")).unwrap();
    let saved = [
        finding("a", Severity::High, Side::Right),
        finding("b", Severity::Low, Side::Left),
    ];
    let stored = store.save_findings("s1", &saved).unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(store.findings("s1").unwrap(), stored);
    store
        .set_finding_state(stored[1].id, FindingState::Ignored)
        .unwrap();
    store
        .set_finding_state(stored[0].id, FindingState::Added)
        .unwrap();
    let states: Vec<_> = store
        .findings("s1")
        .unwrap()
        .into_iter()
        .map(|f| f.state)
        .collect();
    assert_eq!(states, [FindingState::Added, FindingState::Ignored]);
}

#[test]
fn identical_findings_are_not_duplicated_across_turns() {
    let mut store = Store::open_in_memory().unwrap();
    store.create_session(&new_session("s1")).unwrap();
    let first = store
        .save_findings("s1", &[finding("a", Severity::High, Side::Right)])
        .unwrap();
    store
        .set_finding_state(first[0].id, FindingState::Added)
        .unwrap();
    let mut changed_body = finding("a", Severity::Medium, Side::Right);
    changed_body.body = "outro".into();
    let second = store
        .save_findings(
            "s1",
            &[changed_body, finding("novo", Severity::Low, Side::Right)],
        )
        .unwrap();
    assert_eq!(second[0].id, first[0].id);
    assert_eq!(second[0].state, FindingState::Added);
    assert_ne!(second[1].id, first[0].id);
    assert_eq!(store.findings("s1").unwrap().len(), 2);
}

#[test]
fn deleting_a_session_cascades() {
    let mut store = Store::open_in_memory().unwrap();
    store.create_session(&new_session("s1")).unwrap();
    store.append_event("s1", &Event::Chunk("x".into())).unwrap();
    store
        .save_findings("s1", &[finding("a", Severity::Medium, Side::Right)])
        .unwrap();
    store
        .conn
        .execute("DELETE FROM agent_sessions WHERE id = 's1'", [])
        .unwrap();
    assert!(store.events::<Event>("s1").unwrap().is_empty());
    assert!(store.findings("s1").unwrap().is_empty());
}
