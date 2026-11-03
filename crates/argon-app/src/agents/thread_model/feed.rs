use std::time::Instant;

use argon_agents::{AgentEvent, AgentSessionHandle, Snapshot};
use argon_store::{SessionStatus, StoredFinding};
use futures::StreamExt as _;
use gpui_kit::Context;

use super::{ThreadEvent, ThreadModel};
use crate::agents::finding_set::FindingSet;
use crate::agents::transcript::Transcript;
use crate::services::{Services, flatten};

const EVENT_BATCH: usize = 64;

type History = Result<(Vec<(u64, AgentEvent)>, Vec<StoredFinding>), String>;

async fn live_history(
    handle: AgentSessionHandle,
    services: Option<Services>,
    id: String,
) -> History {
    let events = match &services {
        Some(services) => {
            let handle = handle.clone();
            services
                .spawn_runtime(async move { handle.events_since(0).await })
                .await
                .map_err(|error| error.to_string())?
        }
        None => handle.events_since(0).await,
    }
    .map_err(|error| error.to_string())?;
    let findings = stored_findings(services.as_ref(), id).await?;
    Ok((events, findings))
}

async fn stored_findings(
    services: Option<&Services>,
    id: String,
) -> Result<Vec<StoredFinding>, String> {
    match services {
        Some(services) => flatten(
            services
                .store()
                .call(move |store| store.findings(&id))
                .await,
        ),
        None => Ok(Vec::new()),
    }
}

async fn stored_history(services: Option<Services>, id: String) -> History {
    let Some(services) = services else {
        return Err(crate::agents::panel_labels::SERVICES_UNAVAILABLE.to_owned());
    };
    let session = id.clone();
    let events = flatten(
        services
            .store()
            .call(move |store| store.events_since::<AgentEvent>(&session, 0))
            .await,
    )?;
    let events = events
        .into_iter()
        .map(|(seq, event)| (u64::try_from(seq).unwrap_or_default(), event))
        .collect();
    let findings = stored_findings(Some(&services), id).await?;
    Ok((events, findings))
}

fn replay(events: &[(u64, AgentEvent)], upto: Option<u64>) -> (Transcript, FindingSet) {
    let mut transcript = Transcript::default();
    let mut findings = FindingSet::default();
    for (_, event) in events
        .iter()
        .filter(|(seq, _)| upto.is_none_or(|last| *seq <= last))
    {
        if let AgentEvent::Findings {
            findings: saved, ..
        } = event
        {
            findings.insert_saved(saved);
        }
        transcript.apply(event, None);
    }
    (transcript, findings)
}

impl ThreadModel {
    pub(super) fn start_feed(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let services = self.services.clone();
        let id = self.info.session_id.clone();
        self.feed = Some(cx.spawn(async move |this, cx| {
            loop {
                let (snapshot, stream) = handle.subscribe();
                let history = live_history(handle.clone(), services.clone(), id.clone()).await;
                if this
                    .update(cx, |this, cx| this.reset_live(snapshot, history, cx))
                    .is_err()
                {
                    return;
                }
                let mut batches = Box::pin(stream.ready_chunks(EVENT_BATCH));
                let mut resync = false;
                while let Some(batch) = batches.next().await {
                    resync = batch
                        .iter()
                        .any(|(_, event)| matches!(event, AgentEvent::Resync { .. }));
                    if resync {
                        break;
                    }
                    if this
                        .update(cx, |this, cx| this.apply_batch(batch, cx))
                        .is_err()
                    {
                        return;
                    }
                }
                if !resync {
                    break;
                }
            }
            _ = this.update(cx, |this, cx| {
                let before = this.activity();
                this.closed = true;
                this.notify_activity(before, cx);
            });
        }));
    }

    pub(super) fn load_history(&mut self, cx: &mut Context<Self>) {
        let services = self.services.clone();
        let id = self.info.session_id.clone();
        self.feed = Some(cx.spawn(async move |this, cx| {
            let history = stored_history(services, id).await;
            _ = this.update(cx, |this, cx| {
                this.loading = false;
                match history {
                    Ok((events, stored)) => {
                        let (mut transcript, mut findings) = replay(&events, None);
                        let settled = match this.stored_status {
                            SessionStatus::Failed => SessionStatus::Failed,
                            _ => SessionStatus::Completed,
                        };
                        transcript.settle(settled, &[]);
                        findings.merge_stored(stored);
                        this.transcript = transcript;
                        this.findings = findings;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to load agent session transcript");
                        this.load_error = Some(error.into());
                    }
                }
                cx.emit(ThreadEvent::Activity);
                cx.notify();
            });
        }));
    }

    fn reset_live(&mut self, snapshot: Snapshot, history: History, cx: &mut Context<Self>) {
        let before = self.activity();
        self.loading = false;
        let (events, stored) = match history {
            Ok(history) => history,
            Err(error) => {
                tracing::warn!(%error, "failed to load live agent transcript");
                self.load_error = Some(error.into());
                (Vec::new(), Vec::new())
            }
        };
        let (mut transcript, mut findings) = replay(&events, Some(snapshot.last_seq));
        if let Some((_, event)) = &snapshot.open_record {
            transcript.apply(event, Some(Instant::now()));
        }
        let pending: Vec<String> = snapshot
            .pending_permissions
            .iter()
            .map(|permission| permission.request_id.clone())
            .collect();
        transcript.settle(snapshot.status, &pending);
        findings.merge_stored(stored);
        self.transcript = transcript;
        self.findings = findings;
        if !snapshot.models.is_empty() {
            self.models = snapshot.models;
        }
        self.set_model_id(snapshot.model);
        self.notify_activity(before, cx);
    }

    fn apply_batch(&mut self, batch: Vec<(u64, AgentEvent)>, cx: &mut Context<Self>) {
        let before = self.activity();
        let now = Instant::now();
        for (_, event) in batch {
            match &event {
                AgentEvent::Findings { findings, .. } => self.findings.insert_saved(findings),
                AgentEvent::ModelsAvailable(models) => {
                    self.models = models.clone();
                    let model = self.info.model.clone();
                    self.set_model_id(model);
                    cx.emit(ThreadEvent::ModelsAvailable(models.clone()));
                }
                _ => {}
            }
            self.transcript.apply(&event, Some(now));
            if matches!(event, AgentEvent::Error(_))
                && self
                    .handle
                    .as_ref()
                    .is_some_and(|handle| handle.status() == SessionStatus::Failed)
            {
                self.transcript.mark_failed();
            }
        }
        self.notify_activity(before, cx);
    }
}
