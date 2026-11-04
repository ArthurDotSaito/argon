mod detect;
mod login;
mod models;
mod models_cache;
mod probe;
mod spec;
mod version;
mod warmup;

use argon_core::Config;
use futures::future::join_all;

use crate::defaults::AGENT_DEFAULTS;

pub use models::{list_models, listing_spec, lists_models};
pub use models_cache::{ModelsCache, spec_fingerprint};
pub(crate) use spec::login_hint;
pub use spec::{AgentSpec, ModelInfo, Support, agent_spec, agent_spec_for, support_for};
pub use warmup::warm_up_agents;

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
