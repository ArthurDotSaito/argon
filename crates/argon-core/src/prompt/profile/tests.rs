use super::super::render::render_prompt;
use super::super::template::parse_template;
use super::super::test_support::{embedded_template, embedded_templates};
use super::super::{PromptContext, TemplateKind};
use super::*;

const HEAD: &str = "Review {{pr.number}} nonce {{nonce}}.\n\nRead the diff.";
const TAIL: &str = "Answer in JSON.\n";

fn template(id: &str, body: &str) -> PromptTemplate {
    PromptTemplate {
        id: id.into(),
        name: id.into(),
        body: body.into(),
        ..PromptTemplate::default()
    }
}

fn base_with_slot() -> PromptTemplate {
    template("base", &base_body(HEAD, TAIL))
}

fn profile(text: &str) -> PromptTemplate {
    PromptTemplate {
        base: Some("base".into()),
        ..template("profile", text)
    }
}

fn detached_profile(parts: &ProfileParts) -> PromptTemplate {
    PromptTemplate {
        detached: true,
        ..profile(&detached_body(parts))
    }
}

fn own_parts() -> ProfileParts {
    ProfileParts {
        head: HEAD.into(),
        middle: "Own text.".into(),
        tail: TAIL.into(),
    }
}

fn context() -> PromptContext {
    PromptContext {
        pr_title: "Fix login".into(),
        pr_number: 42,
        pr_url: "https://github.com/acme/api/pull/42".into(),
        pr_body: "Corrige o login".into(),
        repo: "acme/api".into(),
        base: "main".into(),
        base_sha: None,
        head: "fix/login".into(),
        head_sha: "9b1d7a3c5e2f4086".into(),
        diff: "+a\n-b\n".into(),
        files: vec!["src/a.rs".into()],
        file_stats: Vec::new(),
        previous_reviews: String::new(),
        ci_failures: String::new(),
    }
}

#[test]
fn resolving_without_base_and_without_slot_is_the_identity() {
    let plain = template("plain", "Texto {{nonce}}\n\n\nsem marcador");

    assert_eq!(resolve_template(&plain, &[]).unwrap(), plain);
}

#[test]
fn resolving_a_base_with_the_slot_drops_the_marker_without_a_triple_blank_line() {
    let resolved = resolve_template(&base_with_slot(), &[]).unwrap();

    assert_eq!(resolved.body, format!("{HEAD}\n\n{TAIL}"));
    assert!(!resolved.body.contains(PROFILE_SLOT));
    assert!(!resolved.body.contains("\n\n\n"));
}

#[test]
fn a_following_profile_resolves_to_head_then_text_then_tail() {
    let resolved =
        resolve_template(&profile("  Check migrations.\n"), &[base_with_slot()]).unwrap();

    assert_eq!(
        resolved.body,
        format!("{HEAD}\n\nCheck migrations.\n\n{TAIL}")
    );
    assert_eq!(resolved.id, "profile");
    assert_eq!(resolved.base, None);
    assert!(!resolved.detached);
}

#[test]
fn a_base_without_the_slot_gets_the_profile_text_at_the_end() {
    let base = template("base", "Review {{nonce}}.\n\nAnswer in JSON.\n");

    let resolved = resolve_template(&profile("Extra rule."), &[base]).unwrap();

    assert_eq!(
        resolved.body,
        "Review {{nonce}}.\n\nAnswer in JSON.\n\nExtra rule.\n"
    );
}

#[test]
fn an_empty_profile_text_equals_resolving_the_base_alone() {
    let base = base_with_slot();
    let alone = resolve_template(&base, &[]).unwrap().body;

    for text in ["", "  \n\n "] {
        let resolved = resolve_template(&profile(text), std::slice::from_ref(&base)).unwrap();
        assert_eq!(resolved.body, alone);
    }
}

#[test]
fn a_detached_profile_resolves_to_head_middle_and_tail_without_markers() {
    let resolved = resolve_template(&detached_profile(&own_parts()), &[]).unwrap();

    assert_eq!(resolved.body, format!("{HEAD}\n\nOwn text.\n\n{TAIL}"));
    assert!(!resolved.body.contains("argon:"));
}

#[test]
fn profile_parts_round_trips_between_following_and_detached() {
    let base = base_with_slot();
    let parts = profile_parts(&profile("Check migrations."), &[base]).unwrap();

    assert_eq!(parts.head, HEAD);
    assert_eq!(parts.middle, "Check migrations.");
    assert_eq!(parts.tail, TAIL);
    assert_eq!(
        profile_parts(&detached_profile(&parts), &[]).unwrap(),
        parts
    );
}

#[test]
fn detached_parts_without_the_slot_keep_everything_in_the_head() {
    let loose = PromptTemplate {
        detached: true,
        ..profile("Only text.\n")
    };

    let parts = profile_parts(&loose, &[]).unwrap();

    assert_eq!(parts.head, "Only text.");
    assert!(parts.middle.is_empty() && parts.tail.is_empty());
}

#[test]
fn detached_parts_without_the_end_marker_take_the_rest_as_the_middle() {
    let loose = PromptTemplate {
        detached: true,
        ..profile(&format!("Head\n\n{PROFILE_SLOT}\nMiddle\nmore\n"))
    };

    let parts = profile_parts(&loose, &[]).unwrap();

    assert_eq!(parts.head, "Head");
    assert_eq!(parts.middle, "Middle\nmore");
    assert!(parts.tail.is_empty());
}

#[test]
fn base_parts_round_trips_through_base_body() {
    for (head, tail) in [(HEAD, TAIL), (HEAD, ""), ("Only head", "Tail\nlines\n")] {
        let body = base_body(head, tail);
        assert_eq!(base_parts(&body), (head.to_owned(), tail.to_owned()));
    }
}

#[test]
fn base_parts_without_the_slot_keeps_everything_in_the_head() {
    assert_eq!(
        base_parts("Whole body.\n\n"),
        ("Whole body.".to_owned(), String::new())
    );
}

#[test]
fn an_unknown_base_is_an_error() {
    let error = resolve_template(&profile("x"), &[]).unwrap_err();

    assert!(matches!(
        error,
        PromptError::UnknownBase { ref id, ref base } if id == "profile" && base == "base"
    ));
}

#[test]
fn a_base_that_is_itself_a_profile_is_an_error() {
    let middle = PromptTemplate {
        base: Some("root".into()),
        ..template("base", "text")
    };
    let root = template("root", "root body");

    let error = resolve_template(&profile("x"), &[middle, root]).unwrap_err();

    assert!(matches!(error, PromptError::NestedBase { .. }));
}

#[test]
fn a_base_of_a_different_kind_is_an_error() {
    let plan_base = PromptTemplate {
        kind: TemplateKind::Plan,
        ..base_with_slot()
    };

    let error = resolve_template(&profile("x"), &[plan_base]).unwrap_err();

    assert!(matches!(error, PromptError::BaseKindMismatch { .. }));
}

#[test]
fn a_detached_profile_still_resolves_when_its_base_is_gone() {
    assert!(resolve_template(&detached_profile(&own_parts()), &[]).is_ok());
}

#[test]
fn variables_in_the_profile_text_render_after_resolving() {
    let resolved = resolve_template(
        &profile("Look at {{ pr.repo }} closely."),
        &[base_with_slot()],
    )
    .unwrap();

    let rendered = render_prompt(&resolved, &context(), 10_000).unwrap();

    assert!(rendered.text.contains("Look at acme/api closely."));
    assert!(rendered.text.contains(&rendered.nonce));
}

#[test]
fn a_following_profile_uses_the_nonce_of_its_base() {
    let own = profile("No variables here.");
    let resolved = resolve_template(&own, &[base_with_slot()]).unwrap();

    assert!(!own.uses_nonce());
    assert!(resolved.uses_nonce());
}

#[test]
fn embedded_review_templates_render_without_the_marker() {
    for review in embedded_templates(TemplateKind::Review) {
        let resolved = resolve_template(&review, &[]).unwrap();
        let rendered = render_prompt(&resolved, &context(), 10_000).unwrap();

        assert!(!resolved.body.contains("argon:"), "{}", review.id);
        assert!(!rendered.text.contains("argon:perfil"), "{}", review.id);
        assert!(!rendered.text.contains(PROFILE_SLOT), "{}", review.id);
    }
}

#[test]
fn a_profile_on_the_embedded_review_lands_before_the_answer_instructions() {
    let base = embedded_template("review-padrao");
    let own = parse_template(
        "perfil",
        "+++\nbase = \"review-padrao\"\n+++\nWatch the migrations.\n",
    )
    .unwrap();

    let resolved = resolve_template(&own, &[base]).unwrap();

    let text = resolved.body.find("Watch the migrations.").unwrap();
    let answer = resolved.body.find("Read the diff with").unwrap();
    assert!(text < answer);
}
