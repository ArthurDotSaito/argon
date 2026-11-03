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
