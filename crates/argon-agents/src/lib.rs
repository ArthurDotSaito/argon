pub mod defaults;
pub mod process;
mod registry;
mod worktree;

pub use defaults::{ModelSelection, Transport};
pub use registry::{AgentSpec, Detection, ModelInfo, Support, agent_spec, detect_all, support_for};
pub use worktree::{WorktreeError, WorktreeManager};
