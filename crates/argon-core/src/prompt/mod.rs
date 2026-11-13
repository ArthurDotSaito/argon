mod defaults;
mod plan;
mod render;
mod template;
#[cfg(test)]
mod test_support;

use std::path::PathBuf;

pub use defaults::ensure_default_templates;
pub use plan::{PlanComment, PlanContext, render_plan_prompt};
pub use render::{PromptContext, RenderedPrompt, render_prompt};
pub use template::{
    InvalidTemplate, PromptTemplate, TemplateKind, TemplateSet, load_templates, scan_templates,
};

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
    #[error("failed to generate prompt nonce: {0}")]
    Nonce(#[from] getrandom::Error),
}
