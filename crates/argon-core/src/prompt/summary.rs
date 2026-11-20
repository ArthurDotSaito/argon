use minijinja::context;

use super::render::{compile, environment, fence_for, fresh_nonce, nonce_warnings, random_nonce};
use super::{PromptError, PromptTemplate, RenderedPrompt};
use crate::redact::redact_text;

#[cfg(test)]
mod tests;

const KNOWN_SUMMARY_VARIABLES: [&str; 15] = [
    "pr",
    "pr.title",
    "pr.number",
    "pr.url",
    "pr.body",
    "pr.repo",
    "base",
    "base_sha",
    "head",
    "head_sha",
    "related",
    "project_overview",
    "refresh_project",
    "has_nodes",
    "nonce",
];

const NO_NONCE_WARNING: &str =
    "Este template não pede o bloco do resumo com nonce — nenhum resumo será lido";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedRepo {
    pub repo: String,
    pub path: Option<String>,
    pub branch: String,
    pub error: Option<String>,
    pub local: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryContext {
    pub pr_title: String,
    pub pr_number: u64,
    pub pr_url: String,
    pub pr_body: String,
    pub repo: String,
    pub base: String,
    pub base_sha: String,
    pub head: String,
    pub head_sha: String,
    pub related: Vec<RelatedRepo>,
    pub project_overview: Option<String>,
    pub project_nodes: Vec<(String, String)>,
    pub refresh_project: bool,
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn related_line(related: &RelatedRepo) -> String {
    let repo = &related.repo;
    match (&related.path, &related.error) {
        (Some(path), None) if related.local => {
            format!("- `{repo}` (pasta local, pode ter mudanças não commitadas): `{path}`")
        }
        (Some(path), None) => format!("- `{repo}` (branch `{}`): `{path}`", related.branch),
        (_, Some(error)) => format!(
            "- `{repo}`: indisponível ({})",
            single_line(&redact_text(error))
        ),
        (None, None) => format!("- `{repo}`: indisponível"),
    }
}

fn related_lines(related: &[RelatedRepo]) -> String {
    related
        .iter()
        .map(related_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn overview_block(overview: &str, nonce: &str) -> String {
    let ticks = fence_for(overview);
    format!(
        "<untrusted kind=\"project_overview\" nonce=\"{nonce}\">\n{ticks}markdown\n{overview}\n{ticks}\n</untrusted nonce=\"{nonce}\">"
    )
}

fn nodes_block(nodes: &[(String, String)], nonce: &str) -> String {
    let lines = nodes
        .iter()
        .map(|(id, label)| format!("{id}: {}", single_line(label)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<untrusted kind=\"project_diagram_nodes\" nonce=\"{nonce}\">\n{lines}\n</untrusted nonce=\"{nonce}\">"
    )
}

fn project_text(overview: Option<&str>, nodes: &[(String, String)], nonce: &str) -> String {
    let Some(overview) = overview else {
        return String::new();
    };
    let mut sections = vec![overview_block(overview, nonce)];
    if !nodes.is_empty() {
        sections.push(nodes_block(nodes, nonce));
    }
    sections.join("\n\n")
}

pub fn render_summary_prompt(
    template: &PromptTemplate,
    context: &SummaryContext,
) -> Result<RenderedPrompt, PromptError> {
    render_summary_with(template, context, random_nonce)
}

fn render_summary_with(
    template: &PromptTemplate,
    context: &SummaryContext,
    generate: impl FnMut() -> Result<String, PromptError>,
) -> Result<RenderedPrompt, PromptError> {
    let env = environment();
    let compiled = compile(&env, template, &KNOWN_SUMMARY_VARIABLES)?;
    let body = redact_text(&context.pr_body);
    let overview = context
        .project_overview
        .as_deref()
        .map(redact_text)
        .filter(|text| !text.trim().is_empty());
    let node_text = context
        .project_nodes
        .iter()
        .flat_map(|(id, label)| [id.as_str(), label.as_str()])
        .collect::<Vec<_>>()
        .join("\n");
    let nonce = fresh_nonce(
        &[
            &context.pr_title,
            &body,
            &context.head,
            overview.as_deref().unwrap_or_default(),
            &node_text,
        ],
        generate,
    )?;
    let nodes: &[(String, String)] = if overview.is_some() {
        &context.project_nodes
    } else {
        &[]
    };
    let values = context! {
        nonce => nonce,
        pr => context! {
            title => context.pr_title,
            number => context.pr_number,
            url => context.pr_url,
            body => body,
            repo => context.repo,
        },
        base => context.base,
        base_sha => context.base_sha,
        head => context.head,
        head_sha => context.head_sha,
        related => related_lines(&context.related),
        project_overview => project_text(overview.as_deref(), nodes, &nonce),
        refresh_project => context.refresh_project || overview.is_none(),
        has_nodes => !nodes.is_empty(),
    };
    Ok(RenderedPrompt {
        text: compiled.render(values)?,
        nonce,
        warnings: nonce_warnings(template, NO_NONCE_WARNING),
    })
}
