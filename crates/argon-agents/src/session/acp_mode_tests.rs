use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::*;

const TIMEOUT: Duration = Duration::from_secs(5);

fn offered(ids: &[&str]) -> Vec<ModelInfo> {
    ids.iter()
        .map(|id| ModelInfo {
            id: (*id).to_owned(),
            label: (*id).to_owned(),
        })
        .collect()
}

fn claude_modes() -> Vec<ModelInfo> {
    offered(&["default", "acceptEdits", "plan", "bypassPermissions"])
}

type Log<T> = Arc<Mutex<Vec<T>>>;

fn committer() -> (
    impl Fn(PermissionMode) + Send + 'static,
    Log<PermissionMode>,
) {
    let log = Log::default();
    let sink = Arc::clone(&log);
    (move |mode| sink.lock().unwrap().push(mode), log)
}

fn request(
    applier: &mpsc::UnboundedSender<ModeRequest>,
    mode: PermissionMode,
) -> oneshot::Receiver<Result<()>> {
    let (reply, response) = oneshot::channel();
    applier.send((mode, reply)).unwrap();
    response
}

#[test]
fn claude_style_modes_map_to_their_values() {
    let modes = claude_modes();
    assert_eq!(
        acp_mode_value(PermissionMode::Supervised, &modes).as_deref(),
        Some("default")
    );
    assert_eq!(
        acp_mode_value(PermissionMode::AcceptEdits, &modes).as_deref(),
        Some("default")
    );
    assert_eq!(
        acp_mode_value(PermissionMode::FullAccess, &modes).as_deref(),
        Some("bypassPermissions")
    );
}

#[test]
fn accept_edits_never_delegates_to_the_agent_accept_mode() {
    let modes = offered(&["acceptEdits", "auto-accept", "bypass"]);
    assert_eq!(acp_mode_value(PermissionMode::AcceptEdits, &modes), None);
    assert_eq!(
        acp_mode_value(PermissionMode::FullAccess, &modes).as_deref(),
        Some("bypass")
    );
}

#[test]
fn first_candidate_wins_when_several_are_offered() {
    let modes = offered(&["bypass", "bypassPermissions"]);
    assert_eq!(
        acp_mode_value(PermissionMode::FullAccess, &modes).as_deref(),
        Some("bypassPermissions")
    );
}

#[test]
fn no_matching_value_yields_none() {
    let modes = offered(&["read-only", "auto", "full-access"]);
    for mode in [
        PermissionMode::Supervised,
        PermissionMode::AcceptEdits,
        PermissionMode::FullAccess,
    ] {
        assert_eq!(acp_mode_value(mode, &modes), None, "{mode:?}");
    }
    assert_eq!(acp_mode_value(PermissionMode::Supervised, &[]), None);
}

#[tokio::test]
async fn confirmed_change_commits_after_the_agent_call() {
    let sent = Log::<String>::default();
    let calls = Arc::clone(&sent);
    let (commit, committed) = committer();
    let applier = spawn_mode_applier(
        claude_modes(),
        move |value| {
            calls.lock().unwrap().push(value);
            async { Ok(()) }
        },
        commit,
        TIMEOUT,
    );
    let response = request(&applier, PermissionMode::FullAccess);
    assert!(response.await.unwrap().is_ok());
    assert_eq!(*sent.lock().unwrap(), ["bypassPermissions"]);
    assert_eq!(*committed.lock().unwrap(), [PermissionMode::FullAccess]);
}

#[tokio::test]
async fn failed_change_returns_error_and_commits_nothing() {
    let (commit, committed) = committer();
    let applier = spawn_mode_applier(
        claude_modes(),
        |_| async { Err("recusado".to_owned()) },
        commit,
        TIMEOUT,
    );
    let response = request(&applier, PermissionMode::FullAccess);
    let error = response.await.unwrap().unwrap_err();
    assert!(
        matches!(&error, SessionError::SetPermissionMode { mode, reason }
            if mode == "bypassPermissions" && reason == "recusado"),
        "{error:?}"
    );
    assert!(committed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn timed_out_change_returns_error_and_commits_nothing() {
    let (commit, committed) = committer();
    let applier = spawn_mode_applier(
        claude_modes(),
        |_| std::future::pending(),
        commit,
        Duration::from_millis(20),
    );
    let response = request(&applier, PermissionMode::Supervised);
    assert!(matches!(
        response.await.unwrap(),
        Err(SessionError::SetPermissionMode { .. })
    ));
    assert!(committed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unmapped_mode_commits_without_calling_the_agent() {
    let (commit, committed) = committer();
    let applier = spawn_mode_applier(
        offered(&["auto"]),
        |_| async { Err("unexpected agent call".to_owned()) },
        commit,
        TIMEOUT,
    );
    let response = request(&applier, PermissionMode::AcceptEdits);
    assert!(response.await.unwrap().is_ok());
    assert_eq!(*committed.lock().unwrap(), [PermissionMode::AcceptEdits]);
}

#[tokio::test]
async fn changes_are_applied_one_at_a_time_in_request_order() {
    let in_flight = Arc::new(AtomicUsize::new(0));
    let overlaps = Arc::new(AtomicUsize::new(0));
    let sent = Log::<String>::default();
    let (commit, committed) = committer();
    let applier = {
        let in_flight = Arc::clone(&in_flight);
        let overlaps = Arc::clone(&overlaps);
        let sent = Arc::clone(&sent);
        spawn_mode_applier(
            claude_modes(),
            move |value| {
                let in_flight = Arc::clone(&in_flight);
                let overlaps = Arc::clone(&overlaps);
                let sent = Arc::clone(&sent);
                async move {
                    if in_flight.fetch_add(1, Ordering::SeqCst) > 0 {
                        overlaps.fetch_add(1, Ordering::SeqCst);
                    }
                    let delay = if value == "bypassPermissions" { 30 } else { 1 };
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    sent.lock().unwrap().push(value);
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                }
            },
            commit,
            TIMEOUT,
        )
    };
    let first = request(&applier, PermissionMode::FullAccess);
    let second = request(&applier, PermissionMode::Supervised);
    assert!(second.await.unwrap().is_ok());
    assert!(first.await.unwrap().is_ok());
    assert_eq!(overlaps.load(Ordering::SeqCst), 0);
    assert_eq!(*sent.lock().unwrap(), ["bypassPermissions", "default"]);
    assert_eq!(
        *committed.lock().unwrap(),
        [PermissionMode::FullAccess, PermissionMode::Supervised]
    );
}
