mod gather;
mod plan;
#[cfg(test)]
mod tests;

use argon_core::{PromptContext, ReviewState, redact_text};
use argon_github::{Job, PrDetail, PrFile, RunStatus, TimelineItem, WorkflowRun};

use super::labels;
use crate::bottom::clean_line;
use crate::pr::labels::{line_ref, review_state};

pub use gather::{Gathered, gather, gather_ci};
pub use plan::plan_context;

pub const CI_LOG_LINES: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextOptions {
    pub include_reviews: bool,
    pub include_ci: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedJob {
    pub id: u64,
    pub name: String,
}

pub fn prompt_context(
    gathered: &Gathered,
    ci_failures: Option<&str>,
    options: ContextOptions,
    max_diff_chars: usize,
) -> PromptContext {
    let detail = &gathered.detail;
    let pr = &detail.pr;
    PromptContext {
        pr_title: pr.title.clone(),
        pr_number: pr.number,
        pr_url: pr.url.clone(),
        pr_body: detail.body.clone(),
        repo: pr.repo.clone(),
        base: pr.base.clone(),
        base_sha: detail.base_oid.clone(),
        head: pr.head.clone(),
        head_sha: detail.head_oid.clone(),
        diff: if max_diff_chars == 0 {
            String::new()
        } else {
            unified_diff(&gathered.files)
        },
        files: gathered
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect(),
        file_stats: gathered
            .files
            .iter()
            .map(|file| (file.path.clone(), file.additions, file.deletions))
            .collect(),
        previous_reviews: if options.include_reviews {
            previous_reviews(detail)
        } else {
            String::new()
        },
        ci_failures: if options.include_ci {
            ci_failures.unwrap_or_default().to_owned()
        } else {
            String::new()
        },
    }
}

pub fn diff_inline(diff: &str, max_diff_chars: usize) -> bool {
    max_diff_chars > 0 && diff.chars().nth(max_diff_chars).is_none()
}

pub fn unified_diff(files: &[PrFile]) -> String {
    files
        .iter()
        .filter_map(|file| {
            let patch = file.patch.as_ref()?;
            let old = file.previous_path.as_deref().unwrap_or(&file.path);
            Some(format!(
                "diff --git a/{old} b/{path}\n--- a/{old}\n+++ b/{path}\n{patch}\n",
                path = file.path
            ))
        })
        .collect()
}

pub fn previous_reviews(detail: &PrDetail) -> String {
    let text = detail
        .timeline
        .iter()
        .filter_map(review_block)
        .collect::<Vec<_>>()
        .join("\n");
    redact_text(&text)
}

fn review_block(item: &TimelineItem) -> Option<String> {
    let TimelineItem::Review {
        author,
        state,
        body,
        submitted_at,
        comments,
        ..
    } = item
    else {
        return None;
    };
    if *state == ReviewState::Pending || (body.trim().is_empty() && comments.is_empty()) {
        return None;
    }
    let mut block = format!(
        "### {author} — {} ({})\n",
        review_state(*state),
        submitted_at.format("%Y-%m-%d")
    );
    if !body.trim().is_empty() {
        block.push_str(body.trim());
        block.push('\n');
    }
    for comment in comments {
        let location = line_ref(&comment.path, comment.line);
        block.push_str(&format!(
            "- `{location}`: {}\n",
            indent_continuation(comment.body.trim())
        ));
    }
    Some(block)
}

fn indent_continuation(text: &str) -> String {
    text.lines().collect::<Vec<_>>().join("\n  ")
}

pub fn failed_jobs(runs: &[WorkflowRun]) -> Vec<FailedJob> {
    runs.iter()
        .flat_map(|run| {
            run.jobs
                .iter()
                .filter(|job| job.status == RunStatus::Failure)
                .map(move |job| failed_job(run, job))
        })
        .collect()
}

fn failed_job(run: &WorkflowRun, job: &Job) -> FailedJob {
    FailedJob {
        id: job.id,
        name: format!("{} / {}", run.name, job.name),
    }
}

pub fn tail_lines(text: &str, count: usize) -> String {
    let lines: Vec<String> = text.lines().map(clean_line).collect();
    let start = lines.len().saturating_sub(count);
    lines[start..].join("\n")
}

pub fn ci_failures(jobs: &[FailedJob], first_log: Option<Result<String, String>>) -> String {
    let Some(first) = jobs.first() else {
        return String::new();
    };
    let mut text = String::from(labels::CI_HEADING);
    text.push('\n');
    for job in jobs {
        text.push_str(&format!("- {}\n", job.name));
    }
    text.push('\n');
    match first_log {
        Some(Ok(log)) => {
            let tail = tail_lines(&log, CI_LOG_LINES);
            text.push_str(&labels::ci_log_heading(&first.name, CI_LOG_LINES));
            text.push('\n');
            text.push_str(if tail.trim().is_empty() {
                labels::NO_CI_LOG
            } else {
                &tail
            });
            text.push('\n');
        }
        Some(Err(error)) => {
            text.push_str(&labels::ci_log_unavailable(&first.name, &error));
            text.push('\n');
        }
        None => {}
    }
    redact_text(&text)
}
