use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use argon_core::{Paths, PullRequest};
use argon_github::{Account, GithubClient, GithubError, Viewer, discover_accounts, token_for};
use chrono::{DateTime, Local, Utc};
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::{error, info, warn};

use super::orgs::{admin_orgs, orgs_for_host};
use super::shared::Shared;
use super::store_handle::{StoreHandle, StoreStartup};

const THREAD_NAME: &str = "argon-services";
const RUNTIME_THREAD_NAME: &str = "argon-tokio";
const RUNTIME_WORKERS: usize = 2;
const DEFAULT_HOST: &str = "github.com";
const LOCAL_STORE_HOST: &str = "argon.db";
const MIN_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncEvent {
    Started,
    Finished { at: DateTime<Utc>, count: usize },
    Failed { host: String, error: String },
    AuthMissing { host: String },
}

pub(super) struct SyncRequest;

struct Session {
    account: Account,
    client: GithubClient,
}

enum AccountOutcome {
    Synced,
    Failed,
    RateLimited(DateTime<Utc>),
    Expired,
}

pub(super) struct SyncWorker {
    pub(super) paths: Paths,
    pub(super) shared: Arc<Shared>,
    pub(super) store: StoreHandle,
    pub(super) store_startup: StoreStartup,
    pub(super) events: UnboundedSender<SyncEvent>,
    pub(super) requests: UnboundedReceiver<SyncRequest>,
}

impl SyncWorker {
    pub(super) fn spawn(self) {
        let spawned = thread::Builder::new()
            .name(THREAD_NAME.into())
            .spawn(move || self.run_blocking());
        if let Err(error) = spawned {
            error!(%error, "failed to start services thread");
        }
    }

    fn run_blocking(self) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(RUNTIME_WORKERS)
            .thread_name(RUNTIME_THREAD_NAME)
            .enable_all()
            .build();
        match runtime {
            Ok(runtime) => runtime.block_on(SyncLoop::new(self).run()),
            Err(error) => {
                error!(%error, "failed to build tokio runtime");
                _ = self.events.unbounded_send(SyncEvent::Failed {
                    host: DEFAULT_HOST.into(),
                    error: format!("falha ao iniciar o serviço de sincronização: {error}"),
                });
            }
        }
    }
}

struct SyncLoop {
    worker: SyncWorker,
    sessions: Vec<Session>,
    blocked: HashMap<String, DateTime<Utc>>,
    store_error: Option<String>,
    seeding_orgs: bool,
    seeded_hosts: HashSet<String>,
}

impl SyncLoop {
    fn new(worker: SyncWorker) -> Self {
        let seeding_orgs = !worker.shared.config().orgs.initialized;
        Self {
            worker,
            sessions: Vec::new(),
            blocked: HashMap::new(),
            store_error: None,
            seeding_orgs,
            seeded_hosts: HashSet::new(),
        }
    }

    async fn run(mut self) {
        self.store_error = match (&mut self.worker.store_startup).await {
            Ok(error) => error,
            Err(_) => Some("o banco local não iniciou".into()),
        };
        let mut pending = true;
        let mut next_cycle = Instant::now();
        loop {
            let due = if pending { Instant::now() } else { next_cycle };
            tokio::select! {
                () = tokio::time::sleep_until(due.into()) => {}
                request = self.worker.requests.next() => {
                    if request.is_none() {
                        info!("sync loop stopped");
                        return;
                    }
                    pending = true;
                    continue;
                }
            }
            self.cycle().await;
            pending = self.drain_requests();
            next_cycle = Instant::now() + self.interval();
        }
    }

    fn interval(&self) -> Duration {
        let seconds = self.worker.shared.config().sync.interval_secs;
        Duration::from_secs(seconds).max(MIN_INTERVAL)
    }

    fn drain_requests(&mut self) -> bool {
        let mut requested = false;
        while self.worker.requests.try_recv().is_ok() {
            requested = true;
        }
        requested
    }

    fn emit(&self, event: SyncEvent) {
        _ = self.worker.events.unbounded_send(event);
    }

    fn fail(&self, host: &str, error: String) {
        self.emit(SyncEvent::Failed {
            host: host.into(),
            error,
        });
    }

    async fn cycle(&mut self) {
        let started = Instant::now();
        self.emit(SyncEvent::Started);
        info!("sync cycle started");
        if let Some(error) = &self.store_error {
            self.fail(LOCAL_STORE_HOST, error.clone());
        }
        self.connect_missing().await;
        if self.sessions.is_empty() {
            info!(
                elapsed_ms = elapsed_ms(started),
                "sync cycle skipped: no authenticated account"
            );
            return;
        }

        let config = self.worker.shared.config();
        let mut outcomes = Vec::with_capacity(self.sessions.len());
        for session in &self.sessions {
            let host = &session.account.host;
            if self.report_blocked(host) {
                outcomes.push(AccountOutcome::Failed);
                continue;
            }
            let orgs = orgs_for_host(&config.orgs.tracked, host);
            let fetched = Instant::now();
            let result = session
                .client
                .fetch_inbox(
                    &session.account.login,
                    &orgs,
                    config.sync.closed_window_days,
                )
                .await;
            outcomes.push(match result {
                Ok(prs) => self.save_prs(&session.account, prs, fetched).await,
                Err(error) => self.report(host, error),
            });
        }
        self.apply_outcomes(&outcomes);

        let synced = outcomes
            .iter()
            .filter(|outcome| matches!(outcome, AccountOutcome::Synced))
            .count();
        let failures = outcomes.len() - synced;
        if synced > 0 {
            self.finish(started, failures).await;
        } else {
            warn!(
                failures,
                elapsed_ms = elapsed_ms(started),
                "sync cycle finished without any synced account"
            );
        }
    }

    fn apply_outcomes(&mut self, outcomes: &[AccountOutcome]) {
        for (session, outcome) in self.sessions.iter().zip(outcomes) {
            if let AccountOutcome::RateLimited(reset_at) = outcome {
                self.blocked.insert(session.account.host.clone(), *reset_at);
            }
        }
        let mut expired = outcomes
            .iter()
            .map(|outcome| matches!(outcome, AccountOutcome::Expired));
        self.sessions
            .retain(|_| !expired.next().unwrap_or_default());
        self.publish_accounts();
    }

    async fn save_prs(
        &self,
        account: &Account,
        prs: Vec<PullRequest>,
        fetched: Instant,
    ) -> AccountOutcome {
        let fetched_count = prs.len();
        let host = account.host.clone();
        let saved = self
            .worker
            .store
            .call(move |store| store.replace_prs(&host, &prs))
            .await
            .map_err(|error| error.to_string())
            .and_then(|saved| saved.map_err(|error| error.to_string()));
        match saved {
            Ok(()) => {
                info!(
                    host = %account.host,
                    login = %account.login,
                    prs = fetched_count,
                    elapsed_ms = elapsed_ms(fetched),
                    "inbox synced"
                );
                AccountOutcome::Synced
            }
            Err(error) => {
                error!(host = %account.host, %error, "failed to store pull requests");
                self.fail(
                    &account.host,
                    format!("falha ao gravar no banco local: {error}"),
                );
                AccountOutcome::Failed
            }
        }
    }

    async fn finish(&self, started: Instant, failures: usize) {
        let counted = self
            .worker
            .store
            .call(|store| store.list_prs().map(|prs| prs.len()))
            .await
            .map_err(|error| error.to_string())
            .and_then(|counted| counted.map_err(|error| error.to_string()));
        match counted {
            Ok(count) => {
                info!(
                    count,
                    failures,
                    elapsed_ms = elapsed_ms(started),
                    "sync cycle finished"
                );
                self.emit(SyncEvent::Finished {
                    at: Utc::now(),
                    count,
                });
            }
            Err(error) => {
                error!(%error, "failed to count stored pull requests");
                self.fail(
                    LOCAL_STORE_HOST,
                    format!("falha ao ler o banco local: {error}"),
                );
            }
        }
    }

    async fn connect_missing(&mut self) {
        let accounts = match discover_accounts() {
            Ok(accounts) => accounts,
            Err(error) => {
                warn!(%error, "failed to read gh hosts");
                self.fail(
                    DEFAULT_HOST,
                    format!("não foi possível ler a configuração do gh: {error}"),
                );
                return;
            }
        };
        if accounts.is_empty() && self.sessions.is_empty() {
            info!("no gh accounts found");
            self.emit(SyncEvent::AuthMissing {
                host: DEFAULT_HOST.into(),
            });
            return;
        }

        let mut connected = Vec::new();
        for account in accounts {
            let host = account.host;
            if self.has_session(&host) || self.report_blocked(&host) {
                continue;
            }
            if let Some((session, viewer)) = self.open_session(&host).await {
                self.sessions.push(session);
                connected.push((host, viewer));
            }
        }
        if !connected.is_empty() {
            self.publish_accounts();
            self.seed_orgs(&connected);
        }
    }

    fn seed_orgs(&mut self, connected: &[(String, Viewer)]) {
        if !self.seeding_orgs {
            return;
        }
        let fresh: Vec<(String, Viewer)> = connected
            .iter()
            .filter(|(host, _)| self.seeded_hosts.insert(host.clone()))
            .cloned()
            .collect();
        if !fresh.is_empty() {
            self.worker
                .shared
                .seed_tracked(admin_orgs(&fresh), &self.worker.paths);
        }
    }

    fn has_session(&self, host: &str) -> bool {
        self.sessions
            .iter()
            .any(|session| session.account.host == host)
    }

    async fn open_session(&mut self, host: &str) -> Option<(Session, Viewer)> {
        let token = match token_for(host).await {
            Ok(token) => token,
            Err(error) => {
                self.report_connect(host, error);
                return None;
            }
        };
        let client = GithubClient::new(host, token);
        match client.viewer().await {
            Ok(viewer) => {
                info!(
                    host,
                    login = %viewer.login,
                    orgs = viewer.orgs.len(),
                    "gh account connected"
                );
                let account = Account {
                    host: host.to_string(),
                    login: viewer.login.clone(),
                };
                Some((Session { account, client }, viewer))
            }
            Err(error) => {
                self.report_connect(host, error);
                None
            }
        }
    }

    fn report_connect(&mut self, host: &str, error: GithubError) {
        if let AccountOutcome::RateLimited(reset_at) = self.report(host, error) {
            self.blocked.insert(host.to_string(), reset_at);
        }
    }

    fn report_blocked(&self, host: &str) -> bool {
        match self.blocked.get(host) {
            Some(reset_at) if *reset_at > Utc::now() => {
                info!(host, %reset_at, "skipping rate limited host");
                self.fail(host, rate_limit_message(*reset_at));
                true
            }
            _ => false,
        }
    }

    fn report(&self, host: &str, error: GithubError) -> AccountOutcome {
        match error {
            GithubError::AuthMissing { .. } => {
                warn!(host, "gh authentication missing");
                self.emit(SyncEvent::AuthMissing { host: host.into() });
                AccountOutcome::Expired
            }
            GithubError::RateLimited { reset_at } => {
                warn!(host, %reset_at, "github rate limit reached");
                self.fail(host, rate_limit_message(reset_at));
                AccountOutcome::RateLimited(reset_at)
            }
            error => {
                error!(host, %error, "sync request failed");
                self.fail(host, error.to_string());
                AccountOutcome::Failed
            }
        }
    }

    fn publish_accounts(&self) {
        let accounts = self
            .sessions
            .iter()
            .map(|session| session.account.clone())
            .collect();
        self.worker.shared.set_accounts(accounts);
    }
}

fn rate_limit_message(reset_at: DateTime<Utc>) -> String {
    let local = reset_at.with_timezone(&Local);
    format!(
        "limite de requisições do GitHub atingido — nova tentativa às {}",
        local.format("%H:%M")
    )
}

fn elapsed_ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}
