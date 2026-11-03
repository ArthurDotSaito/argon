mod render;
mod template;

use std::path::PathBuf;

pub use render::{PromptContext, render_prompt};
pub use template::{PromptTemplate, ensure_default_templates, load_templates};

#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    #[error("failed to access prompt templates at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid header in prompt template `{id}`: {reason}")]
    InvalidHeader { id: String, reason: String },
    #[error("unknown variable `{name}` in prompt template")]
    UnknownVariable { name: String },
    #[error("failed to render prompt template: {0}")]
    Render(#[from] minijinja::Error),
}
