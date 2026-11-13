use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use minijinja::value::{Enumerator, Object, ObjectRepr, Value};
use minijinja::{Environment, Template, UndefinedBehavior, context};

use super::{PromptError, PromptTemplate};

#[cfg(test)]
mod tests;

const KNOWN_VARIABLES: [&str; 20] = [
    "pr",
    "pr.title",
    "pr.number",
    "pr.url",
    "pr.body",
    "pr.repo",
    "pr.base",
    "pr.head",
    "pr.sha",
    "pr.threads",
    "repo",
    "base",
    "base_sha",
    "head",
    "diff",
    "diff_mode",
    "files",
    "previous_reviews",
    "ci_failures",
    "nonce",
];

const DIFF_MODE_INLINE: &str = "inline";
const DIFF_MODE_WORKTREE: &str = "worktree";
const NO_NONCE_WARNING: &str =
    "Este template não pede o bloco de achados com nonce — nenhum achado será lido";
const NONCE_BYTES: usize = 8;
const MIN_FENCE_LEN: usize = 3;
const MAX_PREVIOUS_REVIEWS_CHARS: usize = 40_000;
const MAX_CI_FAILURES_CHARS: usize = 20_000;
const PREVIOUS_REVIEWS_NOTICE: &str =
    "[reviews anteriores truncadas — consulte o PR no GitHub para ver o resto]";
const CI_FAILURES_NOTICE: &str =
    "[falhas de CI truncadas — consulte a aba Actions do PR para ver o resto]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptContext {
    pub pr_title: String,
    pub pr_number: u64,
    pub pr_url: String,
    pub pr_body: String,
    pub repo: String,
    pub base: String,
    pub base_sha: Option<String>,
    pub head: String,
    pub head_sha: String,
    pub diff: String,
    pub files: Vec<String>,
    pub file_stats: Vec<(String, u32, u32)>,
    pub previous_reviews: String,
    pub ci_failures: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPrompt {
    pub text: String,
    pub nonce: String,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub(super) struct TextList(pub(super) Vec<String>);

impl Object for TextList {
    fn repr(self: &Arc<Self>) -> ObjectRepr {
        ObjectRepr::Seq
    }

    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        let file = self.0.get(key.as_usize()?)?;
        Some(Value::from(file.as_str()))
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Seq(self.0.len())
    }

    fn render(self: &Arc<Self>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("\n"))
    }
}

fn longest_backtick_run(content: &str) -> usize {
    content
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or_default()
}

pub(super) fn fence_for(content: &str) -> String {
    "`".repeat((longest_backtick_run(content) + 1).max(MIN_FENCE_LEN))
}

fn fence(value: Value) -> String {
    fence_for(&value.to_string())
}

pub(super) fn cut_at_line(text: &str, max_chars: usize) -> Option<&str> {
    let (cut, _) = text.char_indices().nth(max_chars)?;
    let head = &text[..cut];
    Some(if text[cut..].starts_with('\n') {
        head
    } else {
        head.rfind('\n').map_or("", |newline| &head[..newline])
    })
}

fn truncate_with_notice(text: &str, max_chars: usize, notice: &str) -> String {
    match cut_at_line(text, max_chars) {
        None => text.to_owned(),
        Some("") => notice.to_owned(),
        Some(kept) => format!("{kept}\n{notice}"),
    }
}

fn diff_fits_inline(diff: &str, max_diff_chars: usize) -> bool {
    max_diff_chars > 0 && diff.chars().nth(max_diff_chars).is_none()
}

fn file_lines(ctx: &PromptContext) -> Vec<String> {
    let stats: HashMap<&str, (u32, u32)> = ctx
        .file_stats
        .iter()
        .map(|(path, added, deleted)| (path.as_str(), (*added, *deleted)))
        .collect();
    ctx.files
        .iter()
        .map(|path| match stats.get(path.as_str()) {
            Some((added, deleted)) => format!("{path}  +{added} −{deleted}"),
            None => path.clone(),
        })
        .collect()
}

pub(super) fn random_nonce() -> Result<String, PromptError> {
    let mut bytes = [0u8; NONCE_BYTES];
    getrandom::fill(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(super) fn fresh_nonce(
    untrusted: &[&str],
    mut generate: impl FnMut() -> Result<String, PromptError>,
) -> Result<String, PromptError> {
    loop {
        let nonce = generate()?;
        if !untrusted.iter().any(|content| content.contains(&nonce)) {
            return Ok(nonce);
        }
    }
}

fn unknown_variable(template: &Template<'_, '_>, known_variables: &[&str]) -> Option<String> {
    let known: HashSet<&str> = known_variables.iter().copied().collect();
    template
        .undeclared_variables(true)
        .into_iter()
        .filter(|name| !known.contains(name.as_str()))
        .min()
}

pub(super) fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_keep_trailing_newline(true);
    env.add_filter("fence", fence);
    env
}

pub(super) fn compile<'env, 'source>(
    env: &'env Environment<'static>,
    template: &'source PromptTemplate,
    known_variables: &[&str],
) -> Result<Template<'env, 'source>, PromptError> {
    let compiled = env.template_from_str(&template.body)?;
    match unknown_variable(&compiled, known_variables) {
        Some(name) => Err(PromptError::UnknownVariable { name }),
        None => Ok(compiled),
    }
}

pub(super) fn nonce_warnings(template: &PromptTemplate, warning: &str) -> Vec<String> {
    if template.uses_nonce() {
        Vec::new()
    } else {
        vec![warning.to_owned()]
    }
}

pub fn render_prompt(
    template: &PromptTemplate,
    ctx: &PromptContext,
    max_diff_chars: usize,
) -> Result<RenderedPrompt, PromptError> {
    let env = environment();
    let compiled = compile(&env, template, &KNOWN_VARIABLES)?;
    let files = file_lines(ctx);
    let joined_files = files.join("\n");
    let nonce = fresh_nonce(
        &[
            &ctx.pr_title,
            &ctx.pr_body,
            &ctx.head,
            &ctx.diff,
            &joined_files,
            &ctx.previous_reviews,
            &ctx.ci_failures,
        ],
        random_nonce,
    )?;
    let inline = diff_fits_inline(&ctx.diff, max_diff_chars);
    let previous_reviews = truncate_with_notice(
        &ctx.previous_reviews,
        MAX_PREVIOUS_REVIEWS_CHARS,
        PREVIOUS_REVIEWS_NOTICE,
    );
    let values = context! {
        nonce => nonce,
        pr => context! {
            title => ctx.pr_title,
            number => ctx.pr_number,
            url => ctx.pr_url,
            body => ctx.pr_body,
            repo => ctx.repo,
            base => ctx.base,
            head => ctx.head,
            sha => ctx.head_sha,
            threads => previous_reviews,
        },
        repo => ctx.repo,
        base => ctx.base,
        base_sha => ctx.base_sha.as_deref().filter(|sha| !sha.is_empty()),
        head => ctx.head,
        diff => if inline { ctx.diff.as_str() } else { "" },
        diff_mode => if inline { DIFF_MODE_INLINE } else { DIFF_MODE_WORKTREE },
        files => Value::from_object(TextList(files)),
        previous_reviews => previous_reviews,
        ci_failures => truncate_with_notice(
            &ctx.ci_failures,
            MAX_CI_FAILURES_CHARS,
            CI_FAILURES_NOTICE,
        ),
    };
    Ok(RenderedPrompt {
        text: compiled.render(values)?,
        nonce,
        warnings: nonce_warnings(template, NO_NONCE_WARNING),
    })
}
