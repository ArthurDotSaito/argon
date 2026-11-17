use super::*;
use crate::prompt::TemplateKind;
use crate::prompt::profile::PROFILE_SLOT;
use crate::prompt::template::{load_templates, parse_template};
use crate::prompt::test_support::TempDir;

#[test]
fn ensure_default_templates_writes_the_embedded_templates() {
    let dir = TempDir::new("ensure-writes");
    let prompts = dir.path().join("prompts");

    ensure_default_templates(&prompts).unwrap();

    let templates = load_templates(&prompts).unwrap();
    let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        ["performance", "plan-padrao", "review-padrao", "seguranca"]
    );
    assert!(
        templates
            .iter()
            .all(|t| !t.name.is_empty() && !t.body.is_empty())
    );
}

#[test]
fn embedded_templates_declare_their_kind() {
    for (id, source) in DEFAULT_TEMPLATES {
        let expected = match id {
            "plan-padrao" => TemplateKind::Plan,
            _ => TemplateKind::Review,
        };
        let template = parse_template(id, source).unwrap();
        assert_eq!(template.kind, expected, "{id}");
        assert_eq!(template.version, Some(expected_version(id)), "{id}");
    }
}

#[test]
fn plan_padrao_header_matches_the_spec() {
    let template = parse_template("plan-padrao", embedded_source("plan-padrao")).unwrap();

    assert_eq!(template.name, "Plano padrão");
    assert!(
        template
            .description
            .as_deref()
            .is_some_and(|text| !text.is_empty())
    );
}

#[test]
fn ensure_default_templates_does_not_overwrite_user_edits() {
    let dir = TempDir::new("ensure-keeps");
    let edited = dir.path().join("seguranca.md");
    fs::write(&edited, "+++\nname = \"Meu\"\n+++\neditado").unwrap();

    ensure_default_templates(dir.path()).unwrap();

    assert_eq!(
        fs::read_to_string(&edited).unwrap(),
        "+++\nname = \"Meu\"\n+++\neditado"
    );
    assert!(dir.path().join("review-padrao.md").is_file());
    assert!(dir.path().join("performance.md").is_file());
    assert!(dir.path().join("plan-padrao.md").is_file());
}

#[test]
fn embedded_templates_use_the_nonce() {
    for (id, source) in DEFAULT_TEMPLATES {
        assert!(parse_template(id, source).unwrap().uses_nonce(), "{id}");
    }
}

const V1_TEMPLATES: [(&str, &str); 3] = [
    (
        "review-padrao",
        include_str!("../testdata/review-padrao.v1.md"),
    ),
    ("seguranca", include_str!("../testdata/seguranca.v1.md")),
    ("performance", include_str!("../testdata/performance.v1.md")),
];

fn embedded_source(id: &str) -> &'static str {
    DEFAULT_TEMPLATES
        .into_iter()
        .find(|(candidate, _)| *candidate == id)
        .unwrap()
        .1
}

const V2_REVIEW_PADRAO: &str = include_str!("../testdata/review-padrao.v2.md");
const V3_REVIEW_PADRAO: &str = include_str!("../testdata/review-padrao.v3.md");
const V2_SEGURANCA: &str = include_str!("../testdata/seguranca.v2.md");
const V2_PERFORMANCE: &str = include_str!("../testdata/performance.v2.md");

fn expected_version(id: &str) -> u32 {
    match id {
        "review-padrao" => 4,
        "plan-padrao" => 1,
        _ => 3,
    }
}

#[test]
fn embedded_templates_carry_the_version_header() {
    for (id, source) in DEFAULT_TEMPLATES {
        let header = format!("argon_template_version = {}", expected_version(id));
        assert!(source.lines().take(6).any(|line| line == header), "{id}");
    }
}

#[test]
fn v2_review_padrao_hash_is_registered_as_a_previous_version() {
    assert!(is_previous_version(
        "review-padrao",
        &content_hash(V2_REVIEW_PADRAO)
    ));
}

#[test]
fn unedited_v2_review_padrao_is_upgraded_to_the_current_template() {
    let dir = TempDir::new("ensure-upgrades-v2");
    let path = dir.path().join("review-padrao.md");
    fs::write(&path, V2_REVIEW_PADRAO).unwrap();

    ensure_default_templates(dir.path()).unwrap();

    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        embedded_source("review-padrao")
    );
}

#[test]
fn unedited_v1_files_are_upgraded_to_the_current_template() {
    let dir = TempDir::new("ensure-upgrades");
    for (id, v1) in V1_TEMPLATES {
        fs::write(dir.path().join(format!("{id}.md")), v1).unwrap();
    }

    ensure_default_templates(dir.path()).unwrap();

    for (id, _) in V1_TEMPLATES {
        let written = fs::read_to_string(dir.path().join(format!("{id}.md"))).unwrap();
        assert_eq!(written, embedded_source(id), "{id}");
    }
}

#[test]
fn v1_files_with_crlf_line_endings_are_upgraded() {
    let dir = TempDir::new("ensure-upgrades-crlf");
    let path = dir.path().join("seguranca.md");
    fs::write(&path, V1_TEMPLATES[1].1.replace('\n', "\r\n")).unwrap();

    ensure_default_templates(dir.path()).unwrap();

    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        embedded_source("seguranca")
    );
}

#[test]
fn edited_v1_file_is_preserved() {
    let dir = TempDir::new("ensure-preserves-edited-v1");
    let path = dir.path().join("review-padrao.md");
    let edited = format!("{}\nMinha regra extra.\n", V1_TEMPLATES[0].1);
    fs::write(&path, &edited).unwrap();

    ensure_default_templates(dir.path()).unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
}

#[test]
fn v1_hash_of_one_template_does_not_upgrade_another() {
    let dir = TempDir::new("ensure-id-scoped");
    let path = dir.path().join("performance.md");
    fs::write(&path, V1_TEMPLATES[0].1).unwrap();

    ensure_default_templates(dir.path()).unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), V1_TEMPLATES[0].1);
}

#[test]
fn current_templates_are_left_alone_on_the_next_run() {
    let dir = TempDir::new("ensure-idempotent");
    ensure_default_templates(dir.path()).unwrap();
    let path = dir.path().join("performance.md");
    let before = fs::read_to_string(&path).unwrap();

    ensure_default_templates(dir.path()).unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    assert_eq!(before, embedded_source("performance"));
}

#[test]
fn current_templates_are_not_listed_as_previous_versions() {
    for (id, source) in DEFAULT_TEMPLATES {
        assert!(!is_previous_version(id, &content_hash(source)), "{id}");
    }
}

const RETIRED_BEFORE_PROFILE_SLOT: [(&str, &str); 3] = [
    ("review-padrao", V3_REVIEW_PADRAO),
    ("seguranca", V2_SEGURANCA),
    ("performance", V2_PERFORMANCE),
];

#[test]
fn versions_retired_by_the_profile_slot_are_registered_as_previous_versions() {
    for (id, source) in RETIRED_BEFORE_PROFILE_SLOT {
        assert!(is_previous_version(id, &content_hash(source)), "{id}");
    }
}

#[test]
fn unedited_versions_retired_by_the_profile_slot_are_upgraded() {
    let dir = TempDir::new("ensure-upgrades-profile-slot");
    for (id, source) in RETIRED_BEFORE_PROFILE_SLOT {
        fs::write(dir.path().join(format!("{id}.md")), source).unwrap();
    }

    ensure_default_templates(dir.path()).unwrap();

    for (id, _) in RETIRED_BEFORE_PROFILE_SLOT {
        let written = fs::read_to_string(dir.path().join(format!("{id}.md"))).unwrap();
        assert_eq!(written, embedded_source(id), "{id}");
    }
}

#[test]
fn edited_version_retired_by_the_profile_slot_is_preserved() {
    let dir = TempDir::new("ensure-preserves-edited-v3");
    let path = dir.path().join("review-padrao.md");
    let edited = format!(
        "{V3_REVIEW_PADRAO}
Minha regra extra.
"
    );
    fs::write(&path, &edited).unwrap();

    ensure_default_templates(dir.path()).unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
}

#[test]
fn embedded_review_templates_contain_the_profile_slot_and_the_plan_does_not() {
    for (id, source) in DEFAULT_TEMPLATES {
        let expected = id != "plan-padrao";
        assert_eq!(source.contains(PROFILE_SLOT), expected, "{id}");
        assert_eq!(
            source.matches(PROFILE_SLOT).count(),
            usize::from(expected),
            "{id}"
        );
    }
}
