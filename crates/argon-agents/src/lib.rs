pub mod defaults;
pub mod process;
mod registry;
pub mod session;
mod util;
mod worktree;

pub use defaults::{ModelSelection, Transport};
pub use registry::{AgentSpec, Detection, ModelInfo, Support, agent_spec, detect_all, support_for};
pub use session::{
    AgentEvent, AgentSessionHandle, PendingPermission, PermissionOption, SavedFinding,
    SessionContext, SessionError, SessionHandle, SessionOptions, SessionWriter, Snapshot,
    ToolStatus, new_session_id, resume_session, start_session, start_session_with,
};
pub use worktree::{WorktreeError, WorktreeManager};
