use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use minijinja::value::{Enumerator, Object, ObjectRepr, Value};
use minijinja::{Environment, Template, UndefinedBehavior, context};

use super::{PromptError, PromptTemplate};

#[cfg(test)]
mod tests;

const KNOWN_VARIABLES: [&str; 15] = [
    "pr",
    "pr.title",
    "pr.number",
    "pr.url",
    "pr.body",
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
struct FileList(Vec<String>);

impl Object for FileList {
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

fn fence(value: Value) -> String {
    let longest = longest_backtick_run(&value.to_string());
    "`".repeat((longest + 1).max(MIN_FENCE_LEN))
}

fn truncate_with_notice(text: &str, max_chars: usize, notice: &str) -> String {
    let Some((cut, _)) = text.char_indices().nth(max_chars) else {
        return text.to_owned();
    };
    let head = &text[..cut];
    let kept = if text[cut..].starts_with('\n') {
        head
    } else {
        head.rfind('\n').map_or("", |newline| &head[..newline])
    };
    if kept.is_empty() {
        notice.to_owned()
    } else {
        format!("{kept}\n{notice}")
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

fn random_nonce() -> Result<String, PromptError> {
    let mut bytes = [0u8; NONCE_BYTES];
    getrandom::fill(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn fresh_nonce(
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

fn unknown_variable(template: &Template<'_, '_>) -> Option<String> {
    let known: HashSet<&str> = KNOWN_VARIABLES.into_iter().collect();
    template
        .undeclared_variables(true)
        .into_iter()
        .filter(|name| !known.contains(name.as_str()))
        .min()
}

pub fn render_prompt(
    template: &PromptTemplate,
    ctx: &PromptContext,
    max_diff_chars: usize,
) -> Result<RenderedPrompt, PromptError> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_keep_trailing_newline(true);
    env.add_filter("fence", fence);
    let compiled = env.template_from_str(&template.body)?;
    if let Some(name) = unknown_variable(&compiled) {
        return Err(PromptError::UnknownVariable { name });
    }
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
    let values = context! {
        nonce => nonce,
        pr => context! {
            title => ctx.pr_title,
            number => ctx.pr_number,
            url => ctx.pr_url,
            body => ctx.pr_body,
        },
        repo => ctx.repo,
        base => ctx.base,
        base_sha => ctx.base_sha.as_deref().filter(|sha| !sha.is_empty()),
        head => ctx.head,
        diff => if inline { ctx.diff.as_str() } else { "" },
        diff_mode => if inline { DIFF_MODE_INLINE } else { DIFF_MODE_WORKTREE },
        files => Value::from_object(FileList(files)),
        previous_reviews => truncate_with_notice(
            &ctx.previous_reviews,
            MAX_PREVIOUS_REVIEWS_CHARS,
            PREVIOUS_REVIEWS_NOTICE,
        ),
        ci_failures => truncate_with_notice(
            &ctx.ci_failures,
            MAX_CI_FAILURES_CHARS,
            CI_FAILURES_NOTICE,
        ),
    };
    let text = compiled.render(values)?;
    let warnings = if template.uses_nonce() {
        Vec::new()
    } else {
        vec![NO_NONCE_WARNING.to_owned()]
    };
    Ok(RenderedPrompt {
        text,
        nonce,
        warnings,
    })
}
