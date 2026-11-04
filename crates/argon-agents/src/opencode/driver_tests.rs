use serde_json::json;

use super::state::*;
use super::*;

fn model(id: &str) -> ModelInfo {
    ModelInfo {
        id: id.to_owned(),
        label: id.to_owned(),
    }
}

fn route(session_id: &str, request_id: &str) -> Route {
    Route {
        session_id: session_id.to_owned(),
        request_id: request_id.to_owned(),
        kind: "edit".to_owned(),
    }
}

#[test]
fn family_starts_with_the_root_and_tracks_children() {
    let mut family = Family::new("ses_root".to_owned());

    assert!(family.contains("ses_root"));
    assert!(!family.contains("ses_child"));
    assert!(family.insert("ses_child".to_owned()));
    assert!(!family.insert("ses_child".to_owned()));
    assert!(family.contains("ses_child"));
    assert_eq!(family.children().collect::<Vec<_>>(), vec!["ses_child"]);
    assert_eq!(family.root(), "ses_root");
}

#[test]
fn family_rejects_ids_unsafe_for_url_paths() {
    let mut family = Family::new("ses_root".to_owned());

    for unsafe_id in ["", "../model", "ses?x=1", "ses/child", "ses child"] {
        assert!(!family.insert(unsafe_id.to_owned()), "{unsafe_id:?}");
    }
    assert_eq!(family.children().count(), 0);
}

#[test]
fn reply_route_targets_the_asking_session() {
    assert_eq!(
        route("ses_child", "per_1").reply_path().as_deref(),
        Some("/api/session/ses_child/permission/per_1/reply")
    );
    assert_eq!(route("ses_x", "../../model").reply_path(), None);
    assert_eq!(route("", "per_1").reply_path(), None);
}

#[test]
fn session_path_appends_the_action() {
    assert_eq!(
        session_path("ses_1", "interrupt"),
        "/api/session/ses_1/interrupt"
    );
}

#[test]
fn answers_map_to_decisions_and_never_to_always() {
    let selected = |id: &str| PermissionAnswer::Selected(id.to_owned());

    assert_eq!(decision(&selected("allow_once")), "allow_once");
    assert_eq!(decision(&PermissionAnswer::Cancelled), "reject_once");
    for answer in [
        selected("allow_once"),
        selected("allow_always"),
        selected("reject_once"),
        PermissionAnswer::Cancelled,
    ] {
        let body = permission_body(decision(&answer));
        assert_ne!(body["decision"], "always", "{answer:?}");
    }
    assert_eq!(
        permission_body(decision(&PermissionAnswer::Cancelled)),
        json!({"decision": "reject"})
    );
}

#[test]
fn permission_outside_a_live_turn_is_refused_without_asking() {
    let safe = route("ses_root", "per_1");
    let mut turns = Turns::default();

    assert_eq!(permission_intake(&safe, &turns), Intake::Refuse);
    turns.start();
    assert_eq!(permission_intake(&safe, &turns), Intake::Ask);
    turns.cancel();
    assert_eq!(permission_intake(&safe, &turns), Intake::Refuse);
    turns.clear();
    assert_eq!(permission_intake(&safe, &turns), Intake::Refuse);
}

#[test]
fn refusal_replies_with_a_reject_decision_on_the_asking_session() {
    let asked = route("ses_child", "per_1");

    assert_eq!(
        asked.reply_path().as_deref(),
        Some("/api/session/ses_child/permission/per_1/reply")
    );
    assert_eq!(permission_body(REJECT), json!({"decision": "reject"}));
}

#[test]
fn unsafe_permission_route_is_unanswerable_in_any_turn_state() {
    let unsafe_route = route("ses_root", "per/../x");
    let mut turns = Turns::default();

    assert_eq!(
        permission_intake(&unsafe_route, &turns),
        Intake::Unanswerable
    );
    turns.start();
    assert_eq!(
        permission_intake(&unsafe_route, &turns),
        Intake::Unanswerable
    );
}

#[test]
fn only_allow_always_grants_for_the_session() {
    assert!(grants_session(&PermissionAnswer::Selected(
        "allow_always".to_owned()
    )));
    assert!(!grants_session(&PermissionAnswer::Selected(
        "allow_once".to_owned()
    )));
    assert!(!grants_session(&PermissionAnswer::Cancelled));
}

#[test]
fn not_found_is_detected_from_the_status_prefix() {
    assert!(is_not_found("HTTP 404: {\"name\":\"PermissionNotFound\"}"));
    assert!(!is_not_found("HTTP 400: bad"));
    assert!(!is_not_found("error sending request"));
}

#[test]
fn context_model_is_applied_before_the_first_prompt_only_when_set() {
    assert_eq!(initial_model(""), None);
    assert_eq!(
        initial_model("opencode-go/glm-5.3").as_deref(),
        Some("opencode-go/glm-5.3")
    );
}

#[test]
fn model_ref_requires_provider_and_model() {
    assert_eq!(
        model_ref("opencode-go/glm-5.3"),
        Ok(("opencode-go", "glm-5.3"))
    );
    assert!(model_ref("glm-5.3").unwrap_err().contains("glm-5.3"));
}

#[test]
fn set_model_checks_against_known_models() {
    let models = vec![model("opencode-go/glm-5.3")];

    assert!(check_model(&models, "opencode-go/glm-5.3").is_ok());
    assert!(matches!(
        check_model(&models, "x/y"),
        Err(SessionError::ModelUnavailable { model }) if model == "x/y"
    ));
    assert!(check_model(&[], "x/y").is_ok());
}

#[test]
fn plans_from_child_sessions_are_dropped() {
    let events = vec![
        AgentEvent::Plan(vec!["a".to_owned()]),
        AgentEvent::MessageChunk("oi".to_owned()),
    ];

    assert_eq!(without_child_plans(events.clone(), false), events);
    assert_eq!(
        without_child_plans(events, true),
        vec![AgentEvent::MessageChunk("oi".to_owned())]
    );
}

#[test]
fn event_session_reads_the_data_session_id() {
    assert_eq!(
        event_session(&json!({"data": {"sessionID": "ses_1"}})).as_deref(),
        Some("ses_1")
    );
    assert_eq!(event_session(&json!({"data": {}})), None);
}

#[test]
fn active_sessions_are_keyed_by_id() {
    let body = json!({"data": {"ses_1": {"type": "running"}}});

    assert!(is_active(&body, "ses_1"));
    assert!(!is_active(&body, "ses_2"));
    assert!(!is_active(&json!({"data": {}}), "ses_1"));
}

#[test]
fn created_session_reads_id_and_directory() {
    let body = json!({"data": {"id": "ses_1", "location": {"directory": "C:\\wt"}}});

    assert_eq!(
        created_session(&body),
        Some(("ses_1".to_owned(), Some("C:\\wt".to_owned())))
    );
    assert_eq!(
        created_session(&json!({"data": {"id": "ses_1"}})),
        Some(("ses_1".to_owned(), None))
    );
    assert_eq!(created_session(&json!({"data": {}})), None);
    assert_eq!(created_session(&json!({"data": {"id": "a/b"}})), None);
}

#[test]
fn directory_match_ignores_separators_and_trailing_slash() {
    let cwd = if cfg!(windows) {
        PathBuf::from(r"C:\Work\Repo")
    } else {
        PathBuf::from("/work/repo")
    };
    let reported = if cfg!(windows) {
        "c:/work/repo/"
    } else {
        "/work/repo/"
    };

    assert!(same_directory(reported, &cwd));
    assert!(!same_directory("/elsewhere", &cwd));
}

#[test]
fn resubscription_gives_up_after_three_consecutive_failures() {
    let mut reconnect = Reconnect::default();

    assert!(!reconnect.failed());
    assert!(!reconnect.failed());
    reconnect.succeeded();
    assert!(!reconnect.failed());
    assert!(!reconnect.failed());
    assert!(reconnect.failed());
}

#[test]
fn turn_end_closes_by_stop_reason() {
    let mut turns = Turns::default();
    turns.start();
    assert_eq!(turns.finish("end_turn", None), Some(Close::Completed));

    turns.start();
    assert_eq!(turns.finish("cancelled", None), Some(Close::Cancelled));

    turns.start();
    assert_eq!(
        turns.finish("error", Some("sem crédito".to_owned())),
        Some(Close::Failed("sem crédito".to_owned()))
    );
    assert!(!turns.in_turn());
    assert_eq!(turns.finish("end_turn", None), None);
}

#[test]
fn cancelled_turn_closes_as_cancelled_whatever_the_outcome() {
    let mut turns = Turns::default();
    turns.start();

    assert!(turns.cancel());
    assert!(turns.is_cancelled());
    assert!(!turns.cancel());
    assert_eq!(
        turns.finish("error", Some("x".to_owned())),
        Some(Close::Cancelled)
    );
}

#[test]
fn expire_only_closes_a_cancelled_turn() {
    let mut turns = Turns::default();
    assert_eq!(turns.expire(), None);

    turns.start();
    assert_eq!(turns.expire(), None);
    turns.cancel();
    assert_eq!(turns.expire(), Some(Close::Cancelled));
    assert!(!turns.in_turn());
}

#[test]
fn clear_drops_the_running_turn() {
    let mut turns = Turns::default();
    turns.start();
    turns.clear();

    assert!(!turns.in_turn());
}

fn family_with_child() -> Family {
    let mut family = Family::new("ses_root".to_owned());
    family.insert("ses_child".to_owned());
    family
}

#[test]
fn restart_after_grace_when_any_family_session_is_still_active() {
    let family = family_with_child();

    for active in ["ses_root", "ses_child"] {
        let body = json!({"data": {active: {"type": "running"}}});
        assert!(needs_restart(Some(&body), &family), "{active}");
    }
}

#[test]
fn no_restart_when_only_foreign_sessions_are_active() {
    let family = family_with_child();

    assert!(!needs_restart(Some(&json!({"data": {}})), &family));
    assert!(!needs_restart(
        Some(&json!({"data": {"ses_other": {"type": "running"}}})),
        &family
    ));
}

#[test]
fn restart_when_the_active_check_fails() {
    assert!(needs_restart(None, &family_with_child()));
}

#[test]
fn family_reset_keeps_only_the_root() {
    let mut family = family_with_child();
    family.reset();

    assert!(family.contains("ses_root"));
    assert_eq!(family.children().count(), 0);
}

#[tokio::test]
async fn bounded_wait_turns_a_hang_into_an_error() {
    let hung = within(
        Duration::from_millis(10),
        std::future::pending::<std::result::Result<(), String>>(),
    )
    .await;
    let quick = within(Duration::from_secs(1), async { Ok::<_, String>(7) }).await;

    assert!(hung.unwrap_err().contains("sem resposta"));
    assert_eq!(quick, Ok(7));
}

#[tokio::test]
async fn subscription_timeouts_count_towards_giving_up() {
    let mut reconnect = Reconnect::default();
    let mut gave_up = false;
    for _ in 0..3 {
        let attempt = within(
            Duration::from_millis(5),
            std::future::pending::<std::result::Result<(), String>>(),
        )
        .await;
        gave_up = attempt.is_err() && reconnect.failed();
    }

    assert!(gave_up);
}

#[tokio::test]
async fn unsafe_load_id_is_rejected_before_any_request() {
    let api = LocalApi::new("http://127.0.0.1:1".to_owned(), "secret", String::new()).unwrap();

    for id in ["../model", "ses?x=1", ""] {
        assert_eq!(
            open_session(&api, Path::new("."), Some(id)).await,
            Err(SESSION_NOT_FOUND.to_owned()),
            "{id:?}"
        );
    }
}

#[test]
fn permission_with_unsafe_ids_has_no_reply_route() {
    let event = json!({
        "type": "permission.asked",
        "data": {"id": "per/../x", "sessionID": "ses_root", "permission": "bash"},
    });
    let OpencodeEvent::Permission {
        id,
        session_id,
        ask,
    } = decode(&event, "ses_root", &|id| id == "ses_root")
    else {
        panic!("expected a permission");
    };

    let route = Route {
        session_id,
        request_id: id,
        kind: ask.kind,
    };
    assert_eq!(route.reply_path(), None);
}

#[test]
fn directory_match_compares_canonical_paths_when_both_exist() {
    let temp = std::env::temp_dir();
    let dotted = temp.join(".").display().to_string();

    assert!(same_directory(&dotted, &temp));
}
