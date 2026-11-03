use std::fs::{self, DirEntry, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;

use serde::Deserialize;

use super::PromptError;

const HEADER_FENCE: &str = "+++";
const TEMPLATE_EXTENSION: &str = "md";

const DEFAULT_TEMPLATES: [(&str, &str); 3] = [
    (
        "review-padrao",
        include_str!("../../templates/review-padrao.md"),
    ),
    ("seguranca", include_str!("../../templates/seguranca.md")),
    (
        "performance",
        include_str!("../../templates/performance.md"),
    ),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTemplate {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub body: String,
}

#[derive(Debug, Default, Deserialize)]
struct Header {
    name: Option<String>,
    description: Option<String>,
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> PromptError {
    let path = path.to_path_buf();
    move |source| PromptError::Io { path, source }
}

fn split_header(source: &str) -> Result<(Header, &str), String> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut lines = source.split_inclusive('\n');
    let Some(first) = lines.next().filter(|line| line.trim_end() == HEADER_FENCE) else {
        return Ok((Header::default(), source));
    };
    let header_start = first.len();
    let mut offset = header_start;
    for line in lines {
        if line.trim_end() == HEADER_FENCE {
            let header =
                toml::from_str(&source[header_start..offset]).map_err(|error| error.to_string())?;
            return Ok((header, &source[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(format!("missing closing `{HEADER_FENCE}` line"))
}

fn parse_template(id: &str, source: &str) -> Result<PromptTemplate, PromptError> {
    let (header, body) = split_header(source).map_err(|reason| PromptError::InvalidHeader {
        id: id.to_owned(),
        reason,
    })?;
    let name = header
        .name
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.to_owned());
    Ok(PromptTemplate {
        id: id.to_owned(),
        name,
        description: header.description,
        body: body.trim_start_matches(['\r', '\n']).to_owned(),
    })
}

pub fn ensure_default_templates(dir: &Path) -> Result<(), PromptError> {
    fs::create_dir_all(dir).map_err(io_error(dir))?;
    for (id, content) in DEFAULT_TEMPLATES {
        let path = dir.join(format!("{id}.{TEMPLATE_EXTENSION}"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => file
                .write_all(content.as_bytes())
                .map_err(io_error(&path))?,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_error(&path)(error)),
        }
    }
    Ok(())
}

fn load_entry(entry: std::io::Result<DirEntry>) -> Option<PromptTemplate> {
    let entry = entry
        .inspect_err(|error| tracing::warn!(%error, "skipping unreadable prompt directory entry"))
        .ok()?;
    let path = entry.path();
    let is_template = path.is_file()
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(TEMPLATE_EXTENSION));
    if !is_template {
        return None;
    }
    let id = path.file_stem()?.to_string_lossy().into_owned();
    let source = fs::read_to_string(&path)
        .inspect_err(|error| {
            tracing::warn!(path = %path.display(), %error, "skipping unreadable prompt template");
        })
        .ok()?;
    parse_template(&id, &source)
        .inspect_err(|error| {
            tracing::warn!(path = %path.display(), %error, "skipping invalid prompt template");
        })
        .ok()
}

pub fn load_templates(dir: &Path) -> Result<Vec<PromptTemplate>, PromptError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(dir)(error)),
    };
    let mut templates: Vec<PromptTemplate> = entries.filter_map(load_entry).collect();
    templates.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(templates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("argon-prompt-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn parses_header_with_name_and_description() {
        let source =
            "+++\nname = \"Review padrão\"\ndescription = \"Geral\"\n+++\nCorpo {{repo}}\n";

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
        let template =
            parse_template("meu-prompt", "+++\ndescription = \"x\"\n+++\nCorpo").unwrap();

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
    fn ensure_default_templates_writes_the_embedded_templates() {
        let dir = TempDir::new("ensure-writes");
        let prompts = dir.path().join("prompts");

        ensure_default_templates(&prompts).unwrap();

        let templates = load_templates(&prompts).unwrap();
        let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["performance", "review-padrao", "seguranca"]);
        assert!(
            templates
                .iter()
                .all(|t| !t.name.is_empty() && !t.body.is_empty())
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
    fn load_templates_returns_empty_when_directory_is_missing() {
        let dir = TempDir::new("load-missing");

        let templates = load_templates(&dir.path().join("nao-existe")).unwrap();

        assert!(templates.is_empty());
    }
}
