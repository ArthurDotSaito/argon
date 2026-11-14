use super::*;
use crate::prompt::test_support::TempDir;

#[test]
fn parses_header_with_name_and_description() {
    let source = "+++\nname = \"Review padrão\"\ndescription = \"Geral\"\n+++\nCorpo {{repo}}\n";

    let template = parse_template("review-padrao", source).unwrap();

    assert_eq!(template.id, "review-padrao");
    assert_eq!(template.name, "Review padrão");
    assert_eq!(template.description.as_deref(), Some("Geral"));
    assert_eq!(template.body, "Corpo {{repo}}\n");
}

#[test]
fn header_without_kind_is_a_review_template() {
    let template = parse_template("r", "+++\nname = \"R\"\n+++\nCorpo").unwrap();
    let bare = parse_template("s", "Só corpo").unwrap();

    assert_eq!(template.kind, TemplateKind::Review);
    assert_eq!(template.version, None);
    assert_eq!(bare.kind, TemplateKind::Review);
}

#[test]
fn parses_kind_and_template_version() {
    let plan = parse_template(
        "plano",
        "+++\nname = \"Plano\"\nkind = \"plan\"\nargon_template_version = 1\n+++\nCorpo",
    )
    .unwrap();
    let review = parse_template("r", "+++\nkind = \"review\"\n+++\nCorpo").unwrap();

    assert_eq!(plan.kind, TemplateKind::Plan);
    assert_eq!(plan.version, Some(1));
    assert_eq!(plan.body, "Corpo");
    assert_eq!(review.kind, TemplateKind::Review);
}

#[test]
fn odd_template_versions_are_ignored_without_invalidating_the_template() {
    for value in ["\"3\"", "-1", "4294967296", "1.5", "true", "[1]"] {
        let source = format!("+++\nname = \"V\"\nargon_template_version = {value}\n+++\nCorpo");

        let template = parse_template("v", &source)
            .unwrap_or_else(|error| panic!("{value} invalidated the template: {error}"));

        assert_eq!(template.version, None, "{value}");
        assert_eq!(template.name, "V", "{value}");
    }
    let max = parse_template("m", "+++\nargon_template_version = 4294967295\n+++\nx").unwrap();
    assert_eq!(max.version, Some(u32::MAX));
}

#[test]
fn odd_template_version_is_still_listed_as_a_valid_template() {
    let dir = TempDir::new("scan-version");
    fs::write(
        dir.path().join("texto.md"),
        "+++\nargon_template_version = \"3\"\n+++\nx",
    )
    .unwrap();
    fs::write(
        dir.path().join("negativo.md"),
        "+++\nargon_template_version = -1\n+++\nx",
    )
    .unwrap();

    let set = scan_templates(dir.path()).unwrap();

    assert!(set.invalid.is_empty());
    assert_eq!(set.templates.len(), 2);
    assert!(set.templates.iter().all(|t| t.version.is_none()));
}

#[test]
fn unknown_kind_is_an_invalid_header() {
    let error = parse_template("x", "+++\nkind = \"triage\"\n+++\nCorpo").unwrap_err();

    assert!(matches!(&error, PromptError::InvalidHeader { id, .. } if id == "x"));
    let message = error.to_string();
    assert!(
        message.contains("unknown template kind `triage`"),
        "{message}"
    );
    assert!(message.contains("`review` or `plan`"), "{message}");
}

#[test]
fn unknown_kind_is_listed_as_invalid_by_the_scan() {
    let dir = TempDir::new("scan-kind");
    fs::write(dir.path().join("ok.md"), "+++\nkind = \"plan\"\n+++\nx").unwrap();
    fs::write(
        dir.path().join("estranho.md"),
        "+++\nkind = \"Plan\"\n+++\nx",
    )
    .unwrap();

    let set = scan_templates(dir.path()).unwrap();

    let ids: Vec<&str> = set.templates.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["ok"]);
    assert_eq!(set.invalid.len(), 1);
    assert_eq!(set.invalid[0].id, "estranho");
    assert!(
        set.invalid[0]
            .error
            .contains("unknown template kind `Plan`")
    );
}

#[test]
fn template_kind_text_round_trips() {
    for kind in [TemplateKind::Review, TemplateKind::Plan] {
        assert_eq!(TemplateKind::parse(kind.as_str()), Some(kind));
    }
    assert_eq!(TemplateKind::parse("plano"), None);
    assert_eq!(TemplateKind::default(), TemplateKind::Review);
}

#[test]
fn parses_header_with_crlf_line_endings() {
    let source = "+++\r\nname = \"Segurança\"\r\n+++\r\n\r\nCorpo\r\n";

    let template = parse_template("seguranca", source).unwrap();

    assert_eq!(template.name, "Segurança");
    assert_eq!(template.body, "Corpo\r\n");
}

#[test]
fn missing_header_uses_id_as_name() {
    let template = parse_template("meu-prompt", "Só corpo\n").unwrap();

    assert_eq!(template.name, "meu-prompt");
    assert_eq!(template.description, None);
    assert_eq!(template.body, "Só corpo\n");
}

#[test]
fn header_without_name_uses_id_as_name() {
    let template = parse_template("meu-prompt", "+++\ndescription = \"x\"\n+++\nCorpo").unwrap();

    assert_eq!(template.name, "meu-prompt");
}

#[test]
fn invalid_toml_header_is_an_error() {
    let error = parse_template("quebrado", "+++\nname = \n+++\nCorpo").unwrap_err();

    assert!(matches!(error, PromptError::InvalidHeader { ref id, .. } if id == "quebrado"));
}

#[test]
fn unclosed_header_is_an_error() {
    let error = parse_template("aberto", "+++\nname = \"x\"\nCorpo").unwrap_err();

    assert!(matches!(error, PromptError::InvalidHeader { .. }));
}

#[test]
fn load_templates_skips_invalid_files_and_sorts_by_name() {
    let dir = TempDir::new("load-skips");
    fs::write(dir.path().join("b.md"), "+++\nname = \"Zeta\"\n+++\nz").unwrap();
    fs::write(dir.path().join("a.md"), "+++\nname = \"alfa\"\n+++\na").unwrap();
    fs::write(dir.path().join("c.md"), "sem cabeçalho").unwrap();
    fs::write(dir.path().join("ruim.md"), "+++\nname = \n+++\nx").unwrap();
    fs::write(dir.path().join("notas.txt"), "ignorado").unwrap();

    let templates = load_templates(dir.path()).unwrap();

    let names: Vec<&str> = templates.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["alfa", "c", "Zeta"]);

    let set = scan_templates(dir.path()).unwrap();
    assert_eq!(set.templates.len(), 3);
    let invalid: Vec<&str> = set.invalid.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(invalid, ["ruim"]);
    assert!(set.invalid[0].error.contains("ruim"));
}

#[test]
fn uses_nonce_detects_the_variable_in_the_body() {
    let with = parse_template("a", "Bloco {{ nonce }} fim").unwrap();
    let inside_logic = parse_template("b", "{% if repo %}{{nonce}}{% endif %}").unwrap();
    let without = parse_template("c", "Sem variável, só texto nonce").unwrap();
    let broken = parse_template("d", "{{nonce").unwrap();

    assert!(with.uses_nonce());
    assert!(inside_logic.uses_nonce());
    assert!(!without.uses_nonce());
    assert!(!broken.uses_nonce());
}

#[test]
fn load_templates_returns_empty_when_directory_is_missing() {
    let dir = TempDir::new("load-missing");

    let templates = load_templates(&dir.path().join("nao-existe")).unwrap();

    assert!(templates.is_empty());
}
