pub mod defaults;
pub mod process;
mod registry;
pub mod session;
mod worktree;

pub use defaults::{ModelSelection, Transport};
pub use registry::{AgentSpec, Detection, ModelInfo, Support, agent_spec, detect_all, support_for};
pub use session::{
    AgentEvent, AgentSessionHandle, PermissionOption, SessionContext, SessionError, SessionHandle,
    StoreSink, ToolStatus, new_session_id, resume_session, start_session,
};
pub use worktree::{WorktreeError, WorktreeManager};
