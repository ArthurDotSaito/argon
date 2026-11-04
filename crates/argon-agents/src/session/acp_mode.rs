use std::future::Future;
use std::time::Duration;

use argon_core::PermissionMode;
use tokio::sync::{mpsc, oneshot};

use super::{Result, SessionError};
use crate::registry::ModelInfo;

pub(super) type ModeRequest = (PermissionMode, oneshot::Sender<Result<()>>);

fn candidates(mode: PermissionMode) -> &'static [&'static str] {
    match mode {
        PermissionMode::Supervised | PermissionMode::AcceptEdits => &["default"],
        PermissionMode::FullAccess => &["bypassPermissions", "bypass"],
    }
}

pub(super) fn acp_mode_value(mode: PermissionMode, offered: &[ModelInfo]) -> Option<String> {
    candidates(mode)
        .iter()
        .find(|candidate| offered.iter().any(|value| value.id == **candidate))
        .map(|candidate| (*candidate).to_owned())
}

pub(super) fn mode_failure(value: &str, reason: String) -> SessionError {
    SessionError::SetPermissionMode {
        mode: value.to_owned(),
        reason,
    }
}

async fn apply<S, F>(
    mode: PermissionMode,
    offered: &[ModelInfo],
    set: &S,
    timeout: Duration,
) -> Result<()>
where
    S: Fn(String) -> F,
    F: Future<Output = std::result::Result<(), String>>,
{
    let Some(value) = acp_mode_value(mode, offered) else {
        return Ok(());
    };
    match tokio::time::timeout(timeout, set(value.clone())).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(reason)) => Err(mode_failure(&value, reason)),
        Err(_) => Err(mode_failure(
            &value,
            format!("sem resposta em {}s", timeout.as_secs()),
        )),
    }
}

pub(super) fn spawn_mode_applier<S, F, C>(
    offered: Vec<ModelInfo>,
    set: S,
    commit: C,
    timeout: Duration,
) -> mpsc::UnboundedSender<ModeRequest>
where
    S: Fn(String) -> F + Send + Sync + 'static,
    F: Future<Output = std::result::Result<(), String>> + Send,
    C: Fn(PermissionMode) + Send + 'static,
{
    let (requests, mut received) = mpsc::unbounded_channel::<ModeRequest>();
    tokio::spawn(async move {
        while let Some((mode, reply)) = received.recv().await {
            let result = apply(mode, &offered, &set, timeout).await;
            if result.is_ok() {
                commit(mode);
            }
            let _ = reply.send(result);
        }
    });
    requests
}

#[cfg(test)]
#[path = "acp_mode_tests.rs"]
mod tests;
