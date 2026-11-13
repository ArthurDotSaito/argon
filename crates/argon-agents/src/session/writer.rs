use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use argon_core::{Finding, PermissionMode, PrKey};
use argon_store::{NewAgentSession, PlanDraft, SessionStatus, SessionWrite, Store, StoredFinding};
use tokio::sync::{mpsc as tokio_mpsc, oneshot};

use super::events::AgentEvent;

const FLUSH_INTERVAL: Duration = Duration::from_millis(250);
const FLUSH_EVENTS: usize = 64;
const IDLE_WAIT: Duration = Duration::from_secs(3600);
const THREAD_NAME: &str = "argon-agent-writer";

type Reply<T> = oneshot::Sender<Result<T, String>>;

pub(crate) enum Op {
    Create(Box<NewAgentSession>, Reply<()>),
    Event {
        session: String,
        seq: u64,
        data: String,
    },
    Status {
        session: String,
        status: SessionStatus,
    },
    Model {
        session: String,
        model: String,
    },
    PermissionMode {
        session: String,
        mode: PermissionMode,
    },
    AgentSessionId {
        session: String,
        id: String,
    },
    SaveFindings {
        session: String,
        findings: Vec<Finding>,
        reply: Reply<Vec<StoredFinding>>,
    },
    RecordPlan {
        session: String,
        subject: PrKey,
        markdown: String,
        base_sha: String,
        reply: Reply<PlanDraft>,
    },
    EventsSince {
        session: String,
        after: u64,
        reply: Reply<Vec<(u64, AgentEvent)>>,
    },
    LastSeq {
        session: String,
        reply: Reply<u64>,
    },
    Register {
        session: String,
        errors: tokio_mpsc::UnboundedSender<String>,
    },
    Unregister {
        session: String,
    },
    Flush(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct SessionWriter {
    ops: mpsc::Sender<Op>,
    transactions: Arc<AtomicU64>,
}

#[derive(Default)]
struct Pending {
    writes: Vec<SessionWrite>,
    index: HashMap<String, usize>,
    events: usize,
    since: Option<Instant>,
}

impl Pending {
    fn entry(&mut self, session: String) -> &mut SessionWrite {
        self.since.get_or_insert_with(Instant::now);
        let next = self.writes.len();
        let slot = *self.index.entry(session.clone()).or_insert(next);
        if slot == next {
            self.writes.push(SessionWrite {
                session_id: session,
                ..SessionWrite::default()
            });
        }
        &mut self.writes[slot]
    }
}

struct Worker {
    store: Store,
    pending: Pending,
    errors: HashMap<String, tokio_mpsc::UnboundedSender<String>>,
    transactions: Arc<AtomicU64>,
}

fn text<T>(result: argon_store::Result<T>) -> Result<T, String> {
    result.map_err(|error| error.to_string())
}

impl Worker {
    fn count(&self) {
        self.transactions.fetch_add(1, Ordering::Relaxed);
    }

    fn flush(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        if pending.writes.is_empty() {
            return;
        }
        self.count();
        if let Err(error) = self.store.append_batch(&pending.writes) {
            tracing::error!(%error, "failed to persist agent events");
            let message = format!("falha ao gravar o histórico da sessão: {error}");
            for write in &pending.writes {
                if let Some(errors) = self.errors.get(&write.session_id) {
                    let _ = errors.send(message.clone());
                }
            }
        }
    }

    fn handle(&mut self, op: Op) {
        match op {
            Op::Event { session, seq, data } => {
                self.pending.entry(session).events.push((seq as i64, data));
                self.pending.events += 1;
                if self.pending.events >= FLUSH_EVENTS {
                    self.flush();
                }
            }
            Op::Status { session, status } => self.pending.entry(session).status = Some(status),
            Op::Model { session, model } => self.pending.entry(session).model = Some(model),
            Op::Register { session, errors } => {
                self.errors.insert(session, errors);
            }
            Op::Unregister { session } => {
                self.flush();
                self.errors.remove(&session);
            }
            sync => {
                self.flush();
                self.handle_sync(sync);
            }
        }
    }

    fn handle_sync(&mut self, op: Op) {
        match op {
            Op::Create(session, reply) => {
                self.count();
                let _ = reply.send(text(self.store.create_session(&session)));
            }
            Op::SaveFindings {
                session,
                findings,
                reply,
            } => {
                self.count();
                let _ = reply.send(text(self.store.save_findings(&session, &findings)));
            }
            Op::RecordPlan {
                session,
                subject,
                markdown,
                base_sha,
                reply,
            } => {
                self.count();
                let _ = reply.send(text(self.store.record_plan(
                    &subject.host,
                    &subject.id.0,
                    &session,
                    &markdown,
                    &base_sha,
                )));
            }
            Op::EventsSince {
                session,
                after,
                reply,
            } => {
                let events = self
                    .store
                    .events_since::<AgentEvent>(&session, after as i64)
                    .map(|events| {
                        events
                            .into_iter()
                            .map(|(seq, event)| (seq as u64, event))
                            .collect()
                    });
                let _ = reply.send(text(events));
            }
            Op::PermissionMode { session, mode } => {
                self.count();
                if let Err(error) = self
                    .store
                    .set_session_permission_mode(&session, mode.as_str())
                {
                    tracing::error!(%error, "failed to persist permission mode");
                    if let Some(errors) = self.errors.get(&session) {
                        let _ = errors.send(format!(
                            "falha ao gravar o modo de permissão da sessão: {error}"
                        ));
                    }
                }
            }
            Op::AgentSessionId { session, id } => {
                self.count();
                if let Err(error) = self.store.set_session_acp_id(&session, &id) {
                    tracing::error!(%error, "failed to persist agent session id");
                    if let Some(errors) = self.errors.get(&session) {
                        let _ = errors
                            .send(format!("falha ao gravar o id da sessão do agente: {error}"));
                    }
                }
            }
            Op::LastSeq { session, reply } => {
                let _ = reply.send(text(self.store.last_event_seq(&session)).map(|seq| seq as u64));
            }
            Op::Flush(reply) => {
                let _ = reply.send(());
            }
            _ => {}
        }
    }

    fn run(mut self, ops: mpsc::Receiver<Op>) {
        loop {
            let wait = self.pending.since.map_or(IDLE_WAIT, |since| {
                FLUSH_INTERVAL.saturating_sub(since.elapsed())
            });
            match ops.recv_timeout(wait) {
                Ok(op) => self.handle(op),
                Err(RecvTimeoutError::Timeout) => self.flush(),
                Err(RecvTimeoutError::Disconnected) => {
                    self.flush();
                    return;
                }
            }
            if self
                .pending
                .since
                .is_some_and(|since| since.elapsed() >= FLUSH_INTERVAL)
            {
                self.flush();
            }
        }
    }
}

impl SessionWriter {
    pub fn open(db_file: &Path) -> Result<SessionWriter, String> {
        let store = text(Store::open(db_file))?;
        Self::from_store(store)
    }

    pub fn from_store(store: Store) -> Result<SessionWriter, String> {
        text(store.tune_for_background_writes())?;
        let (ops, receiver) = mpsc::channel();
        let transactions = Arc::new(AtomicU64::new(0));
        let worker = Worker {
            store,
            pending: Pending::default(),
            errors: HashMap::new(),
            transactions: Arc::clone(&transactions),
        };
        std::thread::Builder::new()
            .name(THREAD_NAME.into())
            .spawn(move || worker.run(receiver))
            .map_err(|error| error.to_string())?;
        Ok(SessionWriter { ops, transactions })
    }

    pub fn transactions(&self) -> u64 {
        self.transactions.load(Ordering::Relaxed)
    }

    pub(crate) fn push(&self, op: Op) {
        if self.ops.send(op).is_err() {
            tracing::error!("agent session writer is not running");
        }
    }

    async fn ask<T>(&self, build: impl FnOnce(Reply<T>) -> Op) -> Result<T, String> {
        let (reply, response) = oneshot::channel();
        self.ops
            .send(build(reply))
            .map_err(|_| "o gravador de sessões parou".to_owned())?;
        response
            .await
            .map_err(|_| "o gravador de sessões parou".to_owned())?
    }

    pub async fn flush(&self) {
        let (reply, response) = oneshot::channel();
        if self.ops.send(Op::Flush(reply)).is_ok() {
            let _ = response.await;
        }
    }

    pub(crate) async fn create_session(&self, session: NewAgentSession) -> Result<(), String> {
        self.ask(|reply| Op::Create(Box::new(session), reply)).await
    }

    pub(crate) async fn save_findings(
        &self,
        session: &str,
        findings: Vec<Finding>,
    ) -> Result<Vec<StoredFinding>, String> {
        let session = session.to_owned();
        self.ask(|reply| Op::SaveFindings {
            session,
            findings,
            reply,
        })
        .await
    }

    pub(crate) async fn record_plan(
        &self,
        session: &str,
        subject: &PrKey,
        markdown: &str,
        base_sha: Option<&str>,
    ) -> Result<PlanDraft, String> {
        let session = session.to_owned();
        let subject = subject.clone();
        let markdown = markdown.to_owned();
        let base_sha = base_sha.unwrap_or_default().to_owned();
        self.ask(|reply| Op::RecordPlan {
            session,
            subject,
            markdown,
            base_sha,
            reply,
        })
        .await
    }

    pub async fn events_since(
        &self,
        session: &str,
        after: u64,
    ) -> Result<Vec<(u64, AgentEvent)>, String> {
        let session = session.to_owned();
        self.ask(|reply| Op::EventsSince {
            session,
            after,
            reply,
        })
        .await
    }

    pub(crate) async fn last_seq(&self, session: &str) -> Result<u64, String> {
        let session = session.to_owned();
        self.ask(|reply| Op::LastSeq { session, reply }).await
    }
}

#[cfg(test)]
#[path = "writer_tests.rs"]
mod tests;
