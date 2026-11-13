mod config;
mod diff;
mod findings;
#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures;
mod model;
mod paths;
mod prompt;
mod reason;
mod review;
mod time;
mod views;

pub use config::{
    AgentCfg, AgentKind, AppearanceCfg, Config, ConfigError, Density, NotificationsCfg, OrgsCfg,
    PermissionMode, ReviewCfg, SyncCfg, ThemeChoice,
};
pub use diff::{DiffError, DiffLine, Hunk, LineKind, SplitRow, anchor, parse_patch, split_rows};
pub use findings::{
    Finding, FindingsError, FindingsParse, Severity, filter_findings, parse_findings,
};
pub use model::{
    ChecksState, DEFAULT_HOST, Involvement, Issue, IssueLabel, IssueRef, IssueState,
    IssueStateReason, LatestReview, LinkedPr, Mergeable, PrId, PrKey, PrState, PullRequest,
    ReviewDecision, ReviewState, SessionKind, Side, SubIssues, SubjectKind,
};
pub use paths::Paths;
pub use prompt::{
    InvalidTemplate, PromptContext, PromptError, PromptTemplate, RenderedPrompt, TemplateSet,
    ensure_default_templates, load_templates, render_prompt, scan_templates,
};
pub use reason::{Reason, reason};
pub use review::{
    DraftComment, DraftReview, DraftSource, DraftThreadInput, ReviewError, ReviewEvent,
    ReviewSubmission, build_submission, single_comment_submission,
};
pub use time::{DateBucket, date_bucket};
pub use views::{ViewSet, classify, is_ready};
