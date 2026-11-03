use std::fs::{self, DirEntry};
use std::io::ErrorKind;
use std::path::Path;

use minijinja::Environment;
use serde::Deserialize;

use super::PromptError;

const HEADER_FENCE: &str = "+++";
pub(super) const TEMPLATE_EXTENSION: &str = "md";
const NONCE_VARIABLE: &str = "nonce";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTemplate {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub body: String,
}

impl PromptTemplate {
    pub fn uses_nonce(&self) -> bool {
        let env = Environment::new();
        env.template_from_str(&self.body).is_ok_and(|compiled| {
            compiled
                .undeclared_variables(false)
                .contains(NONCE_VARIABLE)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidTemplate {
    pub id: String,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateSet {
    pub templates: Vec<PromptTemplate>,
    pub invalid: Vec<InvalidTemplate>,
}

#[derive(Debug, Default, Deserialize)]
struct Header {
    name: Option<String>,
    description: Option<String>,
}

pub(super) fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> PromptError {
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

pub(super) fn parse_template(id: &str, source: &str) -> Result<PromptTemplate, PromptError> {
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

fn load_entry(entry: std::io::Result<DirEntry>) -> Option<Result<PromptTemplate, InvalidTemplate>> {
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
    let invalid = |error: String| InvalidTemplate {
        id: id.clone(),
        error,
    };
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "skipping unreadable prompt template");
            return Some(Err(invalid(error.to_string())));
        }
    };
    let template = match parse_template(&id, &source) {
        Ok(template) => template,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "skipping invalid prompt template");
            return Some(Err(invalid(error.to_string())));
        }
    };
    if !template.uses_nonce() {
        tracing::warn!(
            path = %path.display(),
            "prompt template does not use the nonce variable, so no findings will be read"
        );
    }
    Some(Ok(template))
}

pub fn scan_templates(dir: &Path) -> Result<TemplateSet, PromptError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(TemplateSet::default()),
        Err(error) => return Err(io_error(dir)(error)),
    };
    let mut set = TemplateSet::default();
    for loaded in entries.filter_map(load_entry) {
        match loaded {
            Ok(template) => set.templates.push(template),
            Err(invalid) => set.invalid.push(invalid),
        }
    }
    set.templates.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    set.invalid.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(set)
}

pub fn load_templates(dir: &Path) -> Result<Vec<PromptTemplate>, PromptError> {
    scan_templates(dir).map(|set| set.templates)
}

#[cfg(test)]
mod tests;
