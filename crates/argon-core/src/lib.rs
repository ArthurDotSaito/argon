mod config;
mod diff;
mod fence;
mod findings;
#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures;
mod mention;
mod model;
mod paths;
mod plan_output;
mod prompt;
mod reason;
mod redact;
mod review;
mod time;
mod views;

pub use config::{
    AgentCfg, AgentKind, AppearanceCfg, Config, ConfigError, Density, MAX_RELATED_REPOS,
    NotificationsCfg, OrgsCfg, PermissionMode, ReviewCfg, SyncCfg, ThemeChoice, repo_key,
};
pub use diff::{DiffError, DiffLine, Hunk, LineKind, SplitRow, anchor, parse_patch, split_rows};
pub use findings::{
    Finding, FindingsError, FindingsParse, Severity, filter_findings, parse_findings,
};
pub use mention::{is_mention_candidate, mentions_login};
pub use model::{
    ChecksState, DEFAULT_HOST, Involvement, Issue, IssueLabel, IssueRef, IssueState,
    IssueStateReason, LatestReview, LinkedPr, Mergeable, PrId, PrKey, PrState, PullRequest,
    ReviewDecision, ReviewState, SessionKind, Side, SubIssues, SubjectKind,
};
pub use paths::Paths;
pub use plan_output::parse_plan;
pub use prompt::{
    InvalidTemplate, PROFILE_END, PROFILE_SLOT, PlanComment, PlanContext, ProfileParts,
    PromptContext, PromptError, PromptTemplate, RenderedPrompt, TemplateKind, TemplateSet,
    base_body, base_parts, detached_body, ensure_default_templates, load_templates, profile_parts,
    render_plan_prompt, render_prompt, resolve_template, scan_templates,
};
pub use reason::{Reason, issue_reason, reason};
pub use redact::{redact_line, redact_text};
pub use review::{
    DraftComment, DraftReview, DraftSource, DraftThreadInput, ReviewError, ReviewEvent,
    ReviewSubmission, build_submission, single_comment_submission,
};
pub use time::{DateBucket, date_bucket};
pub use views::{ViewSet, classify, classify_issue, is_ready};
