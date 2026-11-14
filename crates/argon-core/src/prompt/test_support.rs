use std::fs;
use std::path::{Path, PathBuf};

use super::defaults::DEFAULT_TEMPLATES;
use super::template::parse_template;
use super::{PromptTemplate, TemplateKind};

pub(super) struct TempDir(PathBuf);

impl TempDir {
    pub(super) fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("argon-prompt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn embedded_templates(kind: TemplateKind) -> Vec<PromptTemplate> {
    DEFAULT_TEMPLATES
        .into_iter()
        .map(|(id, source)| parse_template(id, source).unwrap())
        .filter(|template| template.kind == kind)
        .collect()
}

pub(super) fn embedded_template(id: &str) -> PromptTemplate {
    let (_, source) = DEFAULT_TEMPLATES
        .into_iter()
        .find(|(candidate, _)| *candidate == id)
        .unwrap();
    parse_template(id, source).unwrap()
}
