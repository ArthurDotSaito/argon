use std::collections::{HashMap, HashSet};

use argon_agents::defaults::agent_defaults;
use argon_core::PrKey;
use argon_store::{SessionRow, SessionStatus, Store};
use chrono::{DateTime, Utc};
use gpui_kit::{App, AppContext as _, Context, Entity, Global, SharedString, Subscription, Task};

use super::card_status::{CardStatus, SessionFilter, card_status};
use super::finding_set::SeverityCounts;
use super::sessions::{AgentSessions, AgentsEvent};
use super::sessions_list::SessionCard;
use super::thread_model::{SessionInfo, ThreadModel};
use crate::inbox::InboxModel;
use crate::services::{AppServices, flatten};

struct HistoryRow {
    row: SessionRow,
    counts: SeverityCounts,
    worktree_present: bool,
}

fn load_rows(store: &mut Store) -> argon_store::Result<Vec<HistoryRow>> {
    let mut rows = Vec::new();
    for row in store.sessions()? {
        let findings = store.findings(&row.id)?;
        let counts = SeverityCounts::tally(
            findings
                .iter()
                .map(|stored| (&stored.finding, stored.state)),
        );
        let worktree_present = row
            .worktree_path
            .as_deref()
            .is_some_and(std::path::Path::exists);
        rows.push(HistoryRow {
            row,
            counts,
            worktree_present,
        });
    }
    Ok(rows)
}

pub(super) fn session_info(row: &SessionRow, worktree_present: bool, cx: &App) -> SessionInfo {
    let number =
        InboxModel::global(cx).and_then(|inbox| inbox.read(cx).pr(&row.pr).map(|pr| pr.number));
    SessionInfo {
        session_id: row.id.clone(),
        pr: row.pr.clone(),
        number,
        agent_label: agent_defaults(row.agent).label,
        model: row.model.clone(),
        model_label: row.model_label.clone().unwrap_or_else(|| row.model.clone()),
        worktree: row.worktree_path.clone().filter(|_| worktree_present),
        created_at: row.created_at,
        ended_at: Some(row.updated_at),
        resumable: agent_defaults(row.agent).load_session
            && row.acp_session_id.is_some()
            && row.status != SessionStatus::Archived,
        failure_reason: row.failure_reason.clone(),
        head_oid: None,
    }
}

struct SessionCatalogHandle(Entity<SessionCatalog>);

impl Global for SessionCatalogHandle {}

pub struct SessionCatalog {
    sessions: Entity<AgentSessions>,
    history: Vec<HistoryRow>,
    loaded: bool,
    historical: HashMap<String, Entity<ThreadModel>>,
    filter: SessionFilter,
    agents_active: bool,
    history_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl SessionCatalog {
    pub fn init(sessions: Entity<AgentSessions>, cx: &mut App) {
        if Self::global(cx).is_some() {
            return;
        }
        let catalog = cx.new(|cx| Self::new(sessions, cx));
        cx.set_global(SessionCatalogHandle(catalog));
    }

    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<SessionCatalogHandle>()
            .map(|handle| handle.0.clone())
    }

    fn new(sessions: Entity<AgentSessions>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.observe(&sessions, |_, _, cx| cx.notify()),
            cx.subscribe(&sessions, |this, _, event: &AgentsEvent, cx| {
                let (AgentsEvent::SessionStarted { session_id, .. }
                | AgentsEvent::SessionArchived { session_id }) = event;
                this.historical.remove(session_id);
                this.reload(cx);
            }),
        ];
        let mut this = Self {
            sessions,
            history: Vec::new(),
            loaded: false,
            historical: HashMap::new(),
            filter: SessionFilter::All,
            agents_active: false,
            history_task: None,
            _subscriptions: subscriptions,
        };
        this.reload(cx);
        this
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(services) = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone())
        else {
            return;
        };
        self.history_task = Some(cx.spawn(async move |this, cx| {
            let loaded = flatten(services.store().call(load_rows).await);
            _ = this.update(cx, |this, cx| {
                this.loaded = true;
                match loaded {
                    Ok(rows) => this.history = rows,
                    Err(error) => tracing::warn!(%error, "failed to load agent sessions"),
                }
                cx.notify();
            });
        }));
    }

    pub fn cards(&self, now: DateTime<Utc>, cx: &App) -> Vec<SessionCard> {
        let mut cards = Vec::new();
        let mut live = HashSet::new();
        for thread in self.sessions.read(cx).threads() {
            let thread = thread.read(cx);
            live.insert(thread.info().session_id.clone());
            cards.push(SessionCard::from_thread(thread, now));
        }
        for history in &self.history {
            if !live.contains(&history.row.id) {
                cards.push(SessionCard::from_row(
                    &history.row,
                    history.counts,
                    history.worktree_present,
                    cx,
                ));
            }
        }
        cards
    }

    pub fn counts(&self, cx: &App) -> [usize; SessionFilter::ALL.len()] {
        let mut counts = [0; SessionFilter::ALL.len()];
        let mut tally = |status: CardStatus| {
            for (slot, filter) in counts.iter_mut().zip(SessionFilter::ALL) {
                *slot += usize::from(status.matches(filter));
            }
        };
        let mut live = HashSet::new();
        for thread in self.sessions.read(cx).threads() {
            let thread = thread.read(cx);
            live.insert(thread.info().session_id.clone());
            tally(thread.card_status());
        }
        for history in &self.history {
            if !live.contains(&history.row.id) {
                tally(card_status(history.row.status, false, 0));
            }
        }
        counts
    }

    pub fn tab_title(&self, session_id: &str, cx: &App) -> Option<SharedString> {
        let (number, agent_label) = match self.sessions.read(cx).thread(session_id) {
            Some(live) => {
                let info = live.read(cx).info();
                (info.number, info.agent_label)
            }
            None => {
                let history = self
                    .history
                    .iter()
                    .find(|history| history.row.id == session_id)?;
                let info = session_info(&history.row, history.worktree_present, cx);
                (info.number, info.agent_label)
            }
        };
        Some(match number {
            Some(number) => format!("#{number} · {agent_label}").into(),
            None => agent_label.into(),
        })
    }

    pub fn thread(
        &mut self,
        session_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ThreadModel>> {
        if let Some(live) = self.sessions.read(cx).thread(session_id) {
            return Some(live);
        }
        if let Some(thread) = self.historical.get(session_id) {
            return Some(thread.clone());
        }
        let history = self
            .history
            .iter()
            .find(|history| history.row.id == session_id)?;
        let info = session_info(&history.row, history.worktree_present, cx);
        let status = history.row.status;
        let thread = cx.new(|cx| ThreadModel::historical(info, status, cx));
        self.historical
            .insert(session_id.to_owned(), thread.clone());
        Some(thread)
    }

    pub fn pr_of(&self, session_id: &str, cx: &App) -> Option<PrKey> {
        if let Some(live) = self.sessions.read(cx).thread(session_id) {
            return Some(live.read(cx).info().pr.clone());
        }
        self.history
            .iter()
            .find(|history| history.row.id == session_id)
            .map(|history| history.row.pr.clone())
    }

    pub fn latest_for(&self, pr: &PrKey, cx: &App) -> Option<String> {
        let live = self
            .sessions
            .read(cx)
            .threads()
            .map(|thread| thread.read(cx))
            .filter(|thread| thread.info().pr == *pr)
            .map(|thread| {
                let info = thread.info();
                (
                    thread.card_status() != CardStatus::Archived,
                    info.created_at,
                    info.session_id.clone(),
                )
            });
        let stored = self
            .history
            .iter()
            .filter(|history| history.row.pr == *pr)
            .map(|history| {
                (
                    history.row.status != SessionStatus::Archived,
                    history.row.created_at,
                    history.row.id.clone(),
                )
            });
        live.chain(stored)
            .max_by_key(|(active, created_at, _)| (*active, *created_at))
            .map(|(_, _, id)| id)
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn filter(&self) -> SessionFilter {
        self.filter
    }

    pub fn set_filter(&mut self, filter: SessionFilter, cx: &mut Context<Self>) {
        if self.filter != filter {
            self.filter = filter;
            cx.notify();
        }
    }

    pub fn agents_active(&self) -> bool {
        self.agents_active
    }

    pub fn set_agents_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.agents_active != active {
            self.agents_active = active;
            cx.notify();
        }
    }
}
