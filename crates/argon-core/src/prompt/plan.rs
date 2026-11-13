use std::cmp::Reverse;

use chrono::{DateTime, Utc};
use minijinja::context;
use minijinja::value::Value;

use super::render::{
    TextList, compile, cut_at_line, environment, fence_for, fresh_nonce, nonce_warnings,
    random_nonce,
};
use super::{PromptError, PromptTemplate, RenderedPrompt};
use crate::redact::redact_text;

#[cfg(test)]
mod tests;

const KNOWN_PLAN_VARIABLES: [&str; 13] = [
    "issue",
    "issue.title",
    "issue.number",
    "issue.url",
    "issue.body",
    "issue.repo",
    "issue.labels",
    "issue.assignees",
    "issue.milestone",
    "issue.comments",
    "base",
    "base_sha",
    "nonce",
];

const NO_NONCE_WARNING: &str =
    "Este template não pede o bloco do plano com nonce — nenhum plano será lido";
const MAX_COMMENTS_CHARS: usize = 40_000;
const COMMENTS_NOTICE: &str =
    "[comentários truncados — consulte a issue no GitHub para ver os mais antigos]";
const COMMENT_DATE_FORMAT: &str = "%Y-%m-%d %H:%M UTC";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanComment {
    pub author: String,
    pub created_at: DateTime<Utc>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanContext {
    pub issue_title: String,
    pub issue_number: u64,
    pub issue_url: String,
    pub issue_body: String,
    pub repo: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestone: Option<String>,
    pub comments: Vec<PlanComment>,
    pub base: String,
    pub base_sha: String,
}

struct KeptComments {
    comments: Vec<PlanComment>,
    truncated: bool,
}

fn with_body(comment: &PlanComment, body: String) -> PlanComment {
    PlanComment {
        author: comment.author.clone(),
        created_at: comment.created_at,
        body,
    }
}

fn kept_comments(comments: &[PlanComment]) -> KeptComments {
    let mut newest_first: Vec<&PlanComment> = comments.iter().collect();
    newest_first.sort_by_key(|comment| Reverse(comment.created_at));
    let mut budget = MAX_COMMENTS_CHARS;
    let mut kept = KeptComments {
        comments: Vec::new(),
        truncated: false,
    };
    for comment in newest_first {
        let body = redact_text(&comment.body);
        if let Some(head) = cut_at_line(&body, budget) {
            if !head.is_empty() {
                kept.comments.push(with_body(comment, head.to_owned()));
            }
            kept.truncated = true;
            break;
        }
        budget -= body.chars().count();
        kept.comments.push(with_body(comment, body));
    }
    kept
}

fn comment_block(comment: &PlanComment, nonce: &str) -> String {
    let ticks = fence_for(&comment.body);
    format!(
        "<untrusted kind=\"issue_comment\" nonce=\"{nonce}\">\n{author} · {date}\n{ticks}markdown\n{body}\n{ticks}\n</untrusted nonce=\"{nonce}\">",
        author = comment.author,
        date = comment.created_at.format(COMMENT_DATE_FORMAT),
        body = comment.body,
    )
}

fn comment_blocks(kept: &KeptComments, nonce: &str) -> String {
    let mut sections: Vec<String> = kept
        .comments
        .iter()
        .map(|comment| comment_block(comment, nonce))
        .collect();
    if kept.truncated {
        sections.push(COMMENTS_NOTICE.to_owned());
    }
    sections.join("\n\n")
}

pub fn render_plan_prompt(
    template: &PromptTemplate,
    context: &PlanContext,
) -> Result<RenderedPrompt, PromptError> {
    render_plan_with(template, context, random_nonce)
}

fn render_plan_with(
    template: &PromptTemplate,
    context: &PlanContext,
    generate: impl FnMut() -> Result<String, PromptError>,
) -> Result<RenderedPrompt, PromptError> {
    let env = environment();
    let compiled = compile(&env, template, &KNOWN_PLAN_VARIABLES)?;
    let body = redact_text(&context.issue_body);
    let kept = kept_comments(&context.comments);
    let milestone = context.milestone.as_deref().filter(|name| !name.is_empty());
    let labels = context.labels.join("\n");
    let assignees = context.assignees.join("\n");
    let comments_text = kept
        .comments
        .iter()
        .flat_map(|comment| [comment.author.as_str(), comment.body.as_str()])
        .collect::<Vec<_>>()
        .join("\n");
    let nonce = fresh_nonce(
        &[
            &context.issue_title,
            &body,
            &labels,
            &assignees,
            milestone.unwrap_or_default(),
            &comments_text,
        ],
        generate,
    )?;
    let values = context! {
        nonce => nonce,
        issue => context! {
            title => context.issue_title,
            number => context.issue_number,
            url => context.issue_url,
            body => body,
            repo => context.repo,
            labels => Value::from_object(TextList(context.labels.clone())),
            assignees => Value::from_object(TextList(context.assignees.clone())),
            milestone => milestone,
            comments => comment_blocks(&kept, &nonce),
        },
        base => context.base,
        base_sha => context.base_sha,
    };
    Ok(RenderedPrompt {
        text: compiled.render(values)?,
        nonce,
        warnings: nonce_warnings(template, NO_NONCE_WARNING),
    })
}
