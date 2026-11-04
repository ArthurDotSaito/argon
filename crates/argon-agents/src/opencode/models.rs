use std::sync::Arc;
use std::time::Duration;

use super::protocol::{MODELS_PATH, parse_models};
use super::server::{config_dir, start};
use crate::registry::{AgentSpec, ModelInfo};
use crate::session::StderrTail;

const LISTING_TIMEOUT: Duration = Duration::from_secs(100);

pub(crate) async fn list_models(spec: &AgentSpec) -> Result<Vec<ModelInfo>, String> {
    let mut spec = spec.clone();
    spec.args.clear();
    tokio::time::timeout(LISTING_TIMEOUT, fetch(&spec))
        .await
        .map_err(|_| {
            format!(
                "listing OpenCode models timed out after {}s",
                LISTING_TIMEOUT.as_secs()
            )
        })?
}

async fn fetch(spec: &AgentSpec) -> Result<Vec<ModelInfo>, String> {
    let server = start(spec, &config_dir(), Arc::new(StderrTail::default())).await?;
    let body = server.api.get(MODELS_PATH).await;
    server.stop().await;
    body.map(|body| parse_models(&body))
}
