use argon_core::{ChecksState, Mergeable, PullRequest, ReviewDecision, ReviewState};
use argon_github::{PrDetail, RunStatus, WorkflowRun};

use super::labels::settled_review;
use crate::bottom::{LineKind, classify, clean_line};

const EXIT_CODE_PREFIX: &str = "Process completed with exit code";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Ok,
    Waiting,
    Blocked,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FailedCheck {
    pub run_id: u64,
    pub job_id: Option<u64>,
    pub run: String,
    pub job: Option<String>,
    pub step: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Item {
    Draft,
    Ci {
        level: Level,
        failed: Vec<FailedCheck>,
        total: Option<usize>,
        running: usize,
    },
    Reviews {
        level: Level,
        decision: Option<ReviewDecision>,
        pending: Vec<String>,
        yours: bool,
        blocking: Vec<String>,
    },
    Threads {
        level: Level,
        open: usize,
        first: Option<(String, Option<u32>)>,
    },
    Branch {
        level: Level,
        mergeable: Mergeable,
    },
}

impl Item {
    pub fn blocks_merge(&self) -> bool {
        !matches!(self, Item::Threads { .. })
    }

    pub fn level(&self) -> Level {
        match self {
            Item::Draft => Level::Waiting,
            Item::Ci { level, .. }
            | Item::Reviews { level, .. }
            | Item::Threads { level, .. }
            | Item::Branch { level, .. } => *level,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Readiness {
    pub items: Vec<Item>,
    pub pending: usize,
}

pub fn readiness(
    pr: &PullRequest,
    detail: &PrDetail,
    runs: Option<&[WorkflowRun]>,
    viewer: Option<&str>,
) -> Readiness {
    let items: Vec<Item> = pr
        .is_draft
        .then_some(Item::Draft)
        .into_iter()
        .chain([
            ci(pr, runs),
            reviews(pr, detail, viewer),
            threads(detail),
            branch(pr),
        ])
        .collect();
    let pending = items
        .iter()
        .filter(|item| item.blocks_merge() && item.level() != Level::Ok)
        .count();
    Readiness { items, pending }
}

fn ci(pr: &PullRequest, runs: Option<&[WorkflowRun]>) -> Item {
    let reported = match pr.checks {
        Some(ChecksState::Failure | ChecksState::Error) => Level::Blocked,
        Some(ChecksState::Pending) => Level::Waiting,
        Some(ChecksState::Success) | None => Level::Ok,
    };
    let Some(runs) = runs else {
        return Item::Ci {
            level: reported,
            failed: Vec::new(),
            total: None,
            running: 0,
        };
    };
    let failed: Vec<FailedCheck> = runs
        .iter()
        .filter(|run| run.status == RunStatus::Failure)
        .flat_map(failed_checks)
        .collect();
    let running = runs
        .iter()
        .filter(|run| matches!(run.status, RunStatus::Queued | RunStatus::InProgress))
        .count();
    let total = runs
        .iter()
        .filter(|run| run.status != RunStatus::Skipped)
        .count();
    let observed = if !failed.is_empty() {
        Level::Blocked
    } else if running > 0 {
        Level::Waiting
    } else {
        Level::Ok
    };
    Item::Ci {
        level: reported.max(observed),
        failed,
        total: Some(total),
        running,
    }
}

fn failed_checks(run: &WorkflowRun) -> Vec<FailedCheck> {
    let jobs: Vec<FailedCheck> = run
        .jobs
        .iter()
        .filter(|job| job.status == RunStatus::Failure)
        .map(|job| FailedCheck {
            run_id: run.id,
            job_id: Some(job.id),
            run: run.name.clone(),
            job: Some(job.name.clone()),
            step: job
                .steps
                .iter()
                .find(|step| step.status == RunStatus::Failure)
                .map(|step| step.name.clone()),
        })
        .collect();
    if !jobs.is_empty() {
        return jobs;
    }
    vec![FailedCheck {
        run_id: run.id,
        job_id: None,
        run: run.name.clone(),
        job: None,
        step: None,
    }]
}

fn reviews(pr: &PullRequest, detail: &PrDetail, viewer: Option<&str>) -> Item {
    let mut pending = Vec::new();
    let mut blocking = Vec::new();
    for reviewer in &detail.reviewers {
        match settled_review(reviewer.state, reviewer.requested) {
            None => pending.push(reviewer.login.clone()),
            Some(ReviewState::ChangesRequested) => blocking.push(reviewer.login.clone()),
            Some(_) => {}
        }
    }
    let yours = viewer.is_some_and(|viewer| {
        pending
            .iter()
            .any(|login| login.eq_ignore_ascii_case(viewer))
    });
    let decision = pr.review_decision;
    let approved = match decision {
        Some(decision) => decision == ReviewDecision::Approved,
        None => pr
            .latest_reviews
            .iter()
            .any(|review| review.state == ReviewState::Approved),
    };
    let level = if decision == Some(ReviewDecision::ChangesRequested) || !blocking.is_empty() {
        Level::Blocked
    } else if approved {
        Level::Ok
    } else {
        Level::Waiting
    };
    Item::Reviews {
        level,
        decision,
        pending,
        yours,
        blocking,
    }
}

fn threads(detail: &PrDetail) -> Item {
    let mut open = detail
        .threads
        .iter()
        .filter(|thread| !thread.resolved && !thread.outdated)
        .peekable();
    let first = open
        .peek()
        .map(|thread| (thread.path.clone(), thread.line.or(thread.original_line)));
    let open = open.count();
    Item::Threads {
        level: if open > 0 { Level::Waiting } else { Level::Ok },
        open,
        first,
    }
}

fn branch(pr: &PullRequest) -> Item {
    let level = match pr.mergeable {
        Mergeable::Mergeable => Level::Ok,
        Mergeable::Unknown => Level::Waiting,
        Mergeable::Conflicting => Level::Blocked,
    };
    Item::Branch {
        level,
        mergeable: pr.mergeable,
    }
}

pub fn error_line(log: &str) -> Option<String> {
    let lines: Vec<String> = log.lines().map(clean_line).collect();
    let message = |ix: usize| classify(lines[ix].trim_start()).1.trim().to_string();
    let errors: Vec<usize> = (0..lines.len())
        .filter(|ix| classify(lines[*ix].trim_start()).0 == LineKind::Error)
        .collect();
    let last = *errors.last()?;
    let specific = errors
        .iter()
        .rev()
        .map(|ix| message(*ix))
        .find(|text| !text.is_empty() && !text.starts_with(EXIT_CODE_PREFIX));
    if specific.is_some() {
        return specific;
    }
    lines[..last]
        .iter()
        .rev()
        .map(|line| line.trim())
        .find(|line| !line.is_empty() && classify(line).0 == LineKind::Plain)
        .map(str::to_string)
        .or_else(|| Some(message(last)))
        .filter(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use argon_core::{Side, fixtures};
    use argon_github::{Job, ReviewThread, ReviewerStatus, Step};

    use super::*;

    fn detail(reviewers: Vec<ReviewerStatus>, threads: Vec<ReviewThread>) -> PrDetail {
        PrDetail {
            pr: fixtures::pr(),
            body: String::new(),
            head_oid: "abc".into(),
            base_oid: None,
            labels: Vec::new(),
            reviewers,
            timeline: Vec::new(),
            truncated_timeline: false,
            threads,
            truncated_threads: false,
            commits: Vec::new(),
        }
    }

    fn reviewer(login: &str, state: Option<ReviewState>, requested: bool) -> ReviewerStatus {
        ReviewerStatus {
            login: login.into(),
            state,
            requested,
        }
    }

    fn thread(resolved: bool, outdated: bool) -> ReviewThread {
        ReviewThread {
            id: "t".into(),
            resolved,
            outdated,
            path: "src/cache.rs".into(),
            line: Some(42),
            start_line: None,
            original_line: None,
            original_start_line: None,
            side: Side::Right,
            start_side: None,
            comments: Vec::new(),
            truncated_comments: false,
        }
    }

    fn run(id: u64, name: &str, status: RunStatus, jobs: Vec<Job>) -> WorkflowRun {
        WorkflowRun {
            id,
            name: name.into(),
            status,
            html_url: String::new(),
            run_attempt: 1,
            jobs,
            jobs_error: None,
        }
    }

    fn job(id: u64, name: &str, status: RunStatus, steps: &[(&str, RunStatus)]) -> Job {
        Job {
            id,
            name: name.into(),
            status,
            html_url: String::new(),
            started_at: None,
            completed_at: None,
            steps: steps
                .iter()
                .enumerate()
                .map(|(ix, (name, status))| Step {
                    name: (*name).into(),
                    status: *status,
                    number: ix as u32 + 1,
                })
                .collect(),
        }
    }

    fn levels(readiness: &Readiness) -> Vec<Level> {
        readiness.items.iter().map(Item::level).collect()
    }

    #[test]
    fn a_clean_pr_has_nothing_pending() {
        let mut pr = fixtures::pr();
        pr.checks = Some(ChecksState::Success);
        pr.review_decision = Some(ReviewDecision::Approved);
        let readiness = readiness(&pr, &detail(Vec::new(), Vec::new()), None, Some("me"));
        assert_eq!(readiness.pending, 0);
        assert_eq!(levels(&readiness), [Level::Ok; 4]);
    }

    #[test]
    fn a_failed_job_names_its_workflow_job_and_step() {
        let mut pr = fixtures::pr();
        pr.checks = Some(ChecksState::Failure);
        let runs = [
            run(1, "Tests", RunStatus::Success, Vec::new()),
            run(2, "Notify", RunStatus::Skipped, Vec::new()),
            run(
                3,
                "Version Gate",
                RunStatus::Failure,
                vec![job(
                    30,
                    "check-nuget-version",
                    RunStatus::Failure,
                    &[
                        ("Checkout", RunStatus::Success),
                        ("Check versions", RunStatus::Failure),
                    ],
                )],
            ),
        ];
        pr.review_decision = Some(ReviewDecision::Approved);
        let readiness = readiness(&pr, &detail(Vec::new(), Vec::new()), Some(&runs), None);
        assert_eq!(
            readiness.items[0],
            Item::Ci {
                level: Level::Blocked,
                failed: vec![FailedCheck {
                    run_id: 3,
                    job_id: Some(30),
                    run: "Version Gate".into(),
                    job: Some("check-nuget-version".into()),
                    step: Some("Check versions".into()),
                }],
                total: Some(2),
                running: 0,
            }
        );
        assert_eq!(readiness.pending, 1);
    }

    #[test]
    fn a_failed_workflow_without_job_details_is_still_listed() {
        let runs = [run(3, "Deploy", RunStatus::Failure, Vec::new())];
        let readiness = readiness(
            &fixtures::pr(),
            &detail(Vec::new(), Vec::new()),
            Some(&runs),
            None,
        );
        let Item::Ci { failed, .. } = &readiness.items[0] else {
            panic!("expected ci first");
        };
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].job, None);
    }

    #[test]
    fn running_workflows_make_ci_wait() {
        let runs = [run(1, "Tests", RunStatus::InProgress, Vec::new())];
        let readiness = readiness(
            &fixtures::pr(),
            &detail(Vec::new(), Vec::new()),
            Some(&runs),
            None,
        );
        assert_eq!(readiness.items[0].level(), Level::Waiting);
    }

    #[test]
    fn a_failure_reported_outside_actions_still_blocks() {
        let mut pr = fixtures::pr();
        pr.checks = Some(ChecksState::Failure);
        let runs = [run(1, "Tests", RunStatus::Success, Vec::new())];
        let readiness = readiness(&pr, &detail(Vec::new(), Vec::new()), Some(&runs), None);
        assert_eq!(readiness.items[0].level(), Level::Blocked);
    }

    #[test]
    fn without_loaded_workflows_ci_follows_the_pr_state() {
        let mut pr = fixtures::pr();
        pr.checks = Some(ChecksState::Pending);
        let readiness = readiness(&pr, &detail(Vec::new(), Vec::new()), None, None);
        assert_eq!(
            readiness.items[0],
            Item::Ci {
                level: Level::Waiting,
                failed: Vec::new(),
                total: None,
                running: 0,
            }
        );
    }

    #[test]
    fn requested_changes_block_and_name_who_asked() {
        let reviewers = vec![
            reviewer("ana", Some(ReviewState::ChangesRequested), false),
            reviewer("vitor", Some(ReviewState::Approved), false),
        ];
        let readiness = readiness(
            &fixtures::pr(),
            &detail(reviewers, Vec::new()),
            None,
            Some("me"),
        );
        assert_eq!(
            readiness.items[1],
            Item::Reviews {
                level: Level::Blocked,
                decision: None,
                pending: Vec::new(),
                yours: false,
                blocking: vec!["ana".into()],
            }
        );
    }

    #[test]
    fn your_pending_review_makes_reviews_wait() {
        let reviewers = vec![
            reviewer("Me", None, true),
            reviewer("ana", Some(ReviewState::Approved), true),
        ];
        let readiness = readiness(
            &fixtures::pr(),
            &detail(reviewers, Vec::new()),
            None,
            Some("me"),
        );
        assert_eq!(
            readiness.items[1],
            Item::Reviews {
                level: Level::Waiting,
                decision: None,
                pending: vec!["Me".into(), "ana".into()],
                yours: true,
                blocking: Vec::new(),
            }
        );
    }

    #[test]
    fn without_any_approval_reviews_wait_like_the_merge_button_does() {
        let reviewers = vec![reviewer("ana", None, true)];
        let waiting = readiness(
            &fixtures::pr(),
            &detail(reviewers, Vec::new()),
            None,
            Some("me"),
        );
        assert_eq!(waiting.items[1].level(), Level::Waiting);
        let mut pr = fixtures::pr();
        pr.latest_reviews = vec![fixtures::review(
            "ana",
            ReviewState::Approved,
            fixtures::now(),
        )];
        let approved = readiness(&pr, &detail(Vec::new(), Vec::new()), None, Some("me"));
        assert_eq!(approved.items[1].level(), Level::Ok);
    }

    #[test]
    fn an_open_thread_is_shown_but_does_not_hold_the_merge() {
        let mut pr = fixtures::pr();
        pr.review_decision = Some(ReviewDecision::Approved);
        let readiness = readiness(
            &pr,
            &detail(Vec::new(), vec![thread(false, false)]),
            None,
            None,
        );
        assert_eq!(readiness.items[2].level(), Level::Waiting);
        assert_eq!(readiness.pending, 0);
    }

    #[test]
    fn an_empty_error_marker_yields_no_error_line() {
        assert_eq!(error_line("##[group]Run\n##[error]"), None);
    }

    #[test]
    fn a_required_review_waits() {
        let mut pr = fixtures::pr();
        pr.review_decision = Some(ReviewDecision::ReviewRequired);
        let readiness = readiness(&pr, &detail(Vec::new(), Vec::new()), None, None);
        assert_eq!(readiness.items[1].level(), Level::Waiting);
    }

    #[test]
    fn only_unresolved_current_threads_count_as_open() {
        let threads = vec![
            thread(true, false),
            thread(false, true),
            thread(false, false),
        ];
        let readiness = readiness(&fixtures::pr(), &detail(Vec::new(), threads), None, None);
        assert_eq!(
            readiness.items[2],
            Item::Threads {
                level: Level::Waiting,
                open: 1,
                first: Some(("src/cache.rs".into(), Some(42))),
            }
        );
    }

    #[test]
    fn conflicts_block_and_an_unknown_state_waits() {
        let mut pr = fixtures::pr();
        pr.mergeable = Mergeable::Conflicting;
        let blocked = readiness(&pr, &detail(Vec::new(), Vec::new()), None, None);
        assert_eq!(blocked.items[3].level(), Level::Blocked);
        pr.mergeable = Mergeable::Unknown;
        let waiting = readiness(&pr, &detail(Vec::new(), Vec::new()), None, None);
        assert_eq!(waiting.items[3].level(), Level::Waiting);
    }

    #[test]
    fn a_draft_comes_first_and_counts_as_pending() {
        let mut pr = fixtures::pr();
        pr.is_draft = true;
        pr.review_decision = Some(ReviewDecision::Approved);
        let readiness = readiness(&pr, &detail(Vec::new(), Vec::new()), None, None);
        assert_eq!(readiness.items[0], Item::Draft);
        assert_eq!(readiness.pending, 1);
    }

    #[test]
    fn error_line_prefers_the_message_over_the_exit_code() {
        let log = "2026-11-18T10:00:00.1Z Status: 10 versions behind.\n\
                   2026-11-18T10:00:00.2Z ERROR: Package CMUtil is too far behind.\n\
                   2026-11-18T10:00:00.3Z ##[error]Process completed with exit code 1.\n\
                   2026-11-18T10:00:00.4Z Post job cleanup.";
        assert_eq!(
            error_line(log).as_deref(),
            Some("ERROR: Package CMUtil is too far behind.")
        );
    }

    #[test]
    fn error_line_uses_the_last_specific_error_marker() {
        let log = "##[error]first failure\nnoise\n##[error]second failure\n\
                   ##[error]Process completed with exit code 2.";
        assert_eq!(error_line(log).as_deref(), Some("second failure"));
    }

    #[test]
    fn error_line_falls_back_to_the_exit_code_when_nothing_precedes_it() {
        let log = "##[group]Run tests\n##[error]Process completed with exit code 1.";
        assert_eq!(
            error_line(log).as_deref(),
            Some("Process completed with exit code 1.")
        );
    }

    #[test]
    fn a_log_without_errors_has_no_error_line() {
        assert_eq!(error_line("all good\n##[notice]done"), None);
    }
}
