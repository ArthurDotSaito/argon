mod claude;
mod codex;
pub mod defaults;
mod http;
mod jsonrpc;
mod opencode;
pub mod process;
pub mod redact;
mod registry;
pub mod session;
mod usage;
mod util;
mod worktree;

pub use defaults::{ModelSelection, Transport};
pub use registry::{
    AgentSpec, Detection, ModelInfo, ModelsCache, Support, agent_spec, agent_spec_for, detect_all,
    effective_transport, list_models, listing_spec, lists_models, override_applies_to,
    spec_fingerprint, support_for, warm_up_agents,
};
pub use session::{
    AgentEvent, AgentSessionHandle, PendingPermission, PermissionOption, SavedFinding,
    SessionContext, SessionError, SessionHandle, SessionOptions, SessionWriter, Snapshot,
    ToolStatus, new_session_id, resume_session, start_session, start_session_with,
};
pub use usage::{AccountUsage, UsageSeverity, UsageWindow, account_usage};
pub use worktree::{FoundWorktree, WorktreeError, WorktreeManager, orphan_worktrees};
