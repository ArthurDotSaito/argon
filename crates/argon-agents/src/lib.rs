mod claude;
pub mod defaults;
#[allow(dead_code)]
mod http;
pub mod process;
pub mod redact;
mod registry;
pub mod session;
mod util;
mod worktree;

pub use defaults::{ModelSelection, Transport};
pub use registry::{
    AgentSpec, Detection, ModelInfo, Support, agent_spec, agent_spec_for, detect_all, support_for,
    warm_up_agents,
};
pub use session::{
    AgentEvent, AgentSessionHandle, PendingPermission, PermissionOption, SavedFinding,
    SessionContext, SessionError, SessionHandle, SessionOptions, SessionWriter, Snapshot,
    ToolStatus, new_session_id, resume_session, start_session, start_session_with,
};
pub use worktree::{FoundWorktree, WorktreeError, WorktreeManager, orphan_worktrees};
