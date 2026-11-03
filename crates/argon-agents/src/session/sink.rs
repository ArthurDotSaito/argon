use std::sync::Mutex;

use argon_core::Finding;
use argon_store::{NewAgentSession, SessionStatus, Store};

use super::events::AgentEvent;

pub trait StoreSink: Send + Sync {
    fn create_session(&self, session: &NewAgentSession);
    fn set_status(&self, id: &str, status: SessionStatus);
    fn set_model(&self, id: &str, model: &str);
    fn append_event(&self, id: &str, event: &AgentEvent);
    fn save_findings(&self, id: &str, findings: &[Finding]);
}

fn with_store(
    store: &Mutex<Store>,
    action: &str,
    f: impl FnOnce(&mut Store) -> argon_store::Result<()>,
) {
    let mut guard = match store.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Err(error) = f(&mut guard) {
        tracing::warn!(%error, action, "failed to persist agent session data");
    }
}

impl StoreSink for Mutex<Store> {
    fn create_session(&self, session: &NewAgentSession) {
        with_store(self, "create_session", |store| {
            store.create_session(session)
        });
    }

    fn set_status(&self, id: &str, status: SessionStatus) {
        with_store(self, "set_status", |store| {
            store.set_session_status(id, status)
        });
    }

    fn set_model(&self, id: &str, model: &str) {
        with_store(self, "set_model", |store| {
            store.set_session_model(id, model)
        });
    }

    fn append_event(&self, id: &str, event: &AgentEvent) {
        with_store(self, "append_event", |store| {
            store.append_event(id, event).map(|_| ())
        });
    }

    fn save_findings(&self, id: &str, findings: &[Finding]) {
        with_store(self, "save_findings", |store| {
            store.save_findings(id, findings).map(|_| ())
        });
    }
}
