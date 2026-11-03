mod detect;
mod login;
mod probe;
mod spec;
mod version;

use argon_core::Config;
use futures::future::join_all;

use crate::defaults::AGENT_DEFAULTS;

pub(crate) use spec::login_hint;
pub use spec::{AgentSpec, ModelInfo, Support, agent_spec, support_for};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    Found { version: String },
    NotFound,
    NeedsLogin { hint: String },
    Error(String),
}

pub async fn detect_all(cfg: &Config) -> Vec<(AgentSpec, Support, Detection)> {
    join_all(
        AGENT_DEFAULTS
            .iter()
            .map(|defaults| detect::detect_agent(defaults.kind, cfg)),
    )
    .await
}
