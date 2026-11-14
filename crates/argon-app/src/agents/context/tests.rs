use argon_core::fixtures;
use argon_github::{FileStatus, ReviewComment};

use super::*;

fn file(path: &str, previous: Option<&str>, patch: Option<&str>) -> PrFile {
    PrFile {
        path: path.into(),
        previous_path: previous.map(Into::into),
        status: FileStatus::Modified,
        additions: 1,
        deletions: 0,
        patch: patch.map(Into::into),
        sha: None,
    }
}

fn review(
    author: &str,
    state: ReviewState,
    body: &str,
    comments: Vec<ReviewComment>,
) -> TimelineItem {
    TimelineItem::Review {
        id: format!("r-{author}"),
        author: author.into(),
        state,
        body: body.into(),
        submitted_at: fixtures::now(),
        comments,
    }
}

fn detail(timeline: Vec<TimelineItem>) -> PrDetail {
    PrDetail {
        pr: fixtures::pr(),
        body: "Descrição".into(),
        head_oid: "abc123".into(),
        base_oid: Some("base999".into()),
        labels: Vec::new(),
        reviewers: Vec::new(),
        timeline,
        truncated_timeline: false,
        threads: Vec::new(),
        truncated_threads: false,
        commits: Vec::new(),
    }
}

fn job(id: u64, name: &str, status: RunStatus) -> Job {
    Job {
        id,
        name: name.into(),
        status,
        html_url: String::new(),
        started_at: None,
        completed_at: None,
        steps: Vec::new(),
    }
}

fn run(name: &str, jobs: Vec<Job>) -> WorkflowRun {
    WorkflowRun {
        id: 1,
        name: name.into(),
        status: RunStatus::Failure,
        html_url: String::new(),
        run_attempt: 1,
        jobs,
        jobs_error: None,
    }
}

#[test]
fn diff_concatenates_patches_with_git_headers() {
    let files = [
        file("src/a.rs", None, Some("@@ -1 +1 @@\n-a\n+b")),
        file("bin.png", None, None),
        file(
            "src/new.rs",
            Some("src/old.rs"),
            Some("@@ -1 +1 @@\n-x\n+y"),
        ),
    ];
    assert_eq!(
        unified_diff(&files),
        "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@\n-a\n+b\n\
         diff --git a/src/old.rs b/src/new.rs\n--- a/src/old.rs\n+++ b/src/new.rs\n@@ -1 +1 @@\n-x\n+y\n"
    );
}

#[test]
fn previous_reviews_include_state_body_and_code_comments() {
    let comment = ReviewComment {
        path: "src/a.rs".into(),
        line: Some(12),
        body: "trocar\npor isso".into(),
        diff_hunk: String::new(),
    };
    let detail = detail(vec![
        review(
            "lia",
            ReviewState::ChangesRequested,
            "Faltam testes",
            vec![comment],
        ),
        review("bot", ReviewState::Commented, "  ", Vec::new()),
        review("me", ReviewState::Pending, "rascunho", Vec::new()),
        TimelineItem::Comment {
            id: "c".into(),
            author: "ana".into(),
            body: "comentário solto".into(),
            created_at: fixtures::now(),
            url: None,
        },
    ]);
    let text = previous_reviews(&detail);
    let date = fixtures::now().format("%Y-%m-%d");
    assert_eq!(
        text,
        format!(
            "### lia — Changes requested ({date})\nFaltam testes\n- `src/a.rs:12`: trocar\n  por isso\n"
        )
    );
}

#[test]
fn failed_jobs_lists_only_failures_with_run_name() {
    let runs = [
        run(
            "CI",
            vec![
                job(1, "build", RunStatus::Success),
                job(2, "test", RunStatus::Failure),
            ],
        ),
        run("Lint", vec![job(3, "eslint", RunStatus::Failure)]),
    ];
    assert_eq!(
        failed_jobs(&runs),
        vec![
            FailedJob {
                id: 2,
                name: "CI / test".into()
            },
            FailedJob {
                id: 3,
                name: "Lint / eslint".into()
            },
        ]
    );
}

#[test]
fn tail_keeps_last_lines_without_timestamps() {
    let log: String = (1..=100)
        .map(|n| format!("2026-11-02T10:00:00.0000000Z line {n}\n"))
        .collect();
    let tail = tail_lines(&log, CI_LOG_LINES);
    let lines: Vec<&str> = tail.lines().collect();
    assert_eq!(lines.len(), CI_LOG_LINES);
    assert_eq!(lines[0], "line 21");
    assert_eq!(lines[CI_LOG_LINES - 1], "line 100");
    assert_eq!(tail_lines("a\nb", CI_LOG_LINES), "a\nb");
}

#[test]
fn ci_failures_lists_jobs_and_first_log_tail() {
    let jobs = vec![
        FailedJob {
            id: 2,
            name: "CI / test".into(),
        },
        FailedJob {
            id: 3,
            name: "Lint / eslint".into(),
        },
    ];
    let text = ci_failures(&jobs, Some(Ok("boom\nfailed".into())));
    assert_eq!(
        text,
        format!(
            "{}\n- CI / test\n- Lint / eslint\n\n{}\nboom\nfailed\n",
            labels::CI_HEADING,
            labels::ci_log_heading("CI / test", CI_LOG_LINES)
        )
    );
    let unavailable = ci_failures(&jobs, Some(Err("404".into())));
    assert!(unavailable.ends_with(&format!(
        "{}\n",
        labels::ci_log_unavailable("CI / test", "404")
    )));
    assert_eq!(ci_failures(&[], None), "");
}

#[test]
fn prompt_context_respects_options() {
    let gathered = Gathered {
        detail: detail(vec![review("lia", ReviewState::Approved, "ok", Vec::new())]),
        files: vec![file("src/a.rs", None, Some("@@ -1 +1 @@\n-a\n+b"))],
    };
    let on = ContextOptions {
        include_reviews: true,
        include_ci: true,
    };
    let ctx = prompt_context(&gathered, Some("ci"), on, 1_000);
    assert!(ctx.diff.starts_with("diff --git a/src/a.rs"));
    assert_eq!(ctx.files, ["src/a.rs"]);
    assert_eq!(ctx.file_stats, [("src/a.rs".to_owned(), 1, 0)]);
    assert_eq!(ctx.pr_body, "Descrição");
    assert_eq!(ctx.base_sha.as_deref(), Some("base999"));
    assert!(ctx.previous_reviews.contains("lia"));
    assert_eq!(ctx.ci_failures, "ci");
    let off = ContextOptions {
        include_reviews: false,
        include_ci: false,
    };
    let ctx = prompt_context(&gathered, Some("ci"), off, 0);
    assert!(ctx.diff.is_empty());
    assert!(ctx.previous_reviews.is_empty());
    assert!(ctx.ci_failures.is_empty());
}

#[test]
fn diff_is_inline_only_within_a_positive_limit() {
    assert!(!diff_inline("abc", 0));
    assert!(!diff_inline("", 0));
    assert!(diff_inline("abc", 3));
    assert!(!diff_inline("abcd", 3));
}

#[test]
fn context_redacts_secrets_from_reviews_and_ci_logs() {
    let detail = detail(vec![review(
        "lia",
        ReviewState::Commented,
        "use o token ghp_abcdefghijklmnopqrstuvwx e password=hunter22",
        Vec::new(),
    )]);
    let reviews = previous_reviews(&detail);
    assert!(!reviews.contains("ghp_abcdef"), "{reviews}");
    assert!(!reviews.contains("hunter22"), "{reviews}");
    let jobs = [FailedJob {
        id: 1,
        name: "CI / test".into(),
    }];
    let log = "curl -H Authorization: Bearer abc123secret\n-----BEGIN PRIVATE KEY-----\nMIIE\n\
               -----END PRIVATE KEY-----\nAKIAABCDEFGHIJKLMNOP";
    let text = ci_failures(&jobs, Some(Ok(log.into())));
    for secret in ["abc123secret", "MIIE", "AKIAABCD"] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
}
