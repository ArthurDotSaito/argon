use std::sync::{Arc, PoisonError, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use argon_core::{Config, Paths};
use argon_github::{Account, GithubClient, GithubError, Viewer, discover_accounts, token_for};
use chrono::{DateTime, Local, Utc};
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::{error, info, warn};

use super::store_handle::StoreHandle;

const THREAD_NAME: &str = "argon-services";
const RUNTIME_THREAD_NAME: &str = "argon-tokio";
const RUNTIME_WORKERS: usize = 2;
const DEFAULT_HOST: &str = "github.com";
const MIN_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncEvent {
    Started,
    Finished { at: DateTime<Utc>, count: usize },
    Failed { host: String, error: String },
    AuthMissing { host: String },
}

pub(super) struct SyncRequest;

pub(super) struct Shared {
    config: RwLock<Config>,
    accounts: RwLock<Vec<Account>>,
}

impl Shared {
    pub(super) fn new(config: Config) -> Self {
        Self {
            config: RwLock::new(config),
            accounts: RwLock::default(),
        }
    }

    pub(super) fn config(&self) -> Config {
        self.config
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) fn accounts(&self) -> Vec<Account> {
        self.accounts
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_accounts(&self, accounts: Vec<Account>) {
        *self
            .accounts
            .write()
            .unwrap_or_else(PoisonError::into_inner) = accounts;
    }

    fn initialize_orgs(&self, tracked: Vec<String>, paths: &Paths) {
        let mut config = self.config.write().unwrap_or_else(PoisonError::into_inner);
        config.orgs.tracked = tracked;
        config.orgs.initialized = true;
        match config.save(&paths.config_file()) {
            Ok(()) => info!(
                tracked = config.orgs.tracked.len(),
                "initialized tracked organizations"
            ),
            Err(error) => warn!(%error, "failed to save initialized config"),
        }
    }
}

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
    blocked_until: Option<Instant>,
}

impl SyncLoop {
    fn new(worker: SyncWorker) -> Self {
        Self {
            worker,
            sessions: Vec::new(),
            blocked_until: None,
        }
    }

    async fn run(mut self) {
        let mut pending = true;
        let mut next_cycle = Instant::now();
        loop {
            let due = if pending { Instant::now() } else { next_cycle };
            let due = self.blocked_until.map_or(due, |until| due.max(until));
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

    async fn cycle(&mut self) {
        let started = Instant::now();
        self.emit(SyncEvent::Started);
        info!("sync cycle started");
        if self.sessions.is_empty() {
            self.connect().await;
        }
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
            let orgs = orgs_for_host(&config.orgs.tracked, &session.account.host);
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
                Err(error) => self.report(&session.account.host, error),
            });
        }

        for outcome in &outcomes {
            if let AccountOutcome::RateLimited(reset_at) = outcome {
                self.block_until(*reset_at);
            }
        }
        let mut index = 0;
        self.sessions.retain(|_| {
            let keep = !matches!(outcomes[index], AccountOutcome::Expired);
            index += 1;
            keep
        });
        self.publish_accounts();

        let failures = outcomes
            .iter()
            .filter(|outcome| !matches!(outcome, AccountOutcome::Synced))
            .count();
        if failures > 0 {
            warn!(
                failures,
                elapsed_ms = elapsed_ms(started),
                "sync cycle finished with errors"
            );
            return;
        }
        self.finish(started).await;
    }

    async fn save_prs(
        &self,
        account: &Account,
        prs: Vec<argon_core::PullRequest>,
        fetched: Instant,
    ) -> AccountOutcome {
        let fetched_count = prs.len();
        let host = account.host.clone();
        let saved = self
            .worker
            .store
            .call(move |store| store.replace_prs(&host, &prs))
            .await;
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
                self.emit(SyncEvent::Failed {
                    host: account.host.clone(),
                    error: format!("falha ao gravar no banco local: {error}"),
                });
                AccountOutcome::Failed
            }
        }
    }

    async fn finish(&self, started: Instant) {
        let counted = self
            .worker
            .store
            .call(|store| store.list_prs().map(|prs| prs.len()))
            .await;
        match counted {
            Ok(count) => {
                info!(
                    count,
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
                self.emit(SyncEvent::Failed {
                    host: DEFAULT_HOST.into(),
                    error: format!("falha ao ler o banco local: {error}"),
                });
            }
        }
    }

    async fn connect(&mut self) {
        let accounts = match discover_accounts() {
            Ok(accounts) => accounts,
            Err(error) => {
                warn!(%error, "failed to read gh hosts");
                self.emit(SyncEvent::Failed {
                    host: DEFAULT_HOST.into(),
                    error: format!("não foi possível ler a configuração do gh: {error}"),
                });
                return;
            }
        };
        if accounts.is_empty() {
            info!("no gh accounts found");
            self.emit(SyncEvent::AuthMissing {
                host: DEFAULT_HOST.into(),
            });
            return;
        }

        let mut viewers = Vec::new();
        for account in accounts {
            if let Some((session, viewer)) = self.open_session(&account.host).await {
                self.sessions.push(session);
                viewers.push((account.host, viewer));
            }
        }
        self.publish_accounts();

        if !viewers.is_empty() && !self.worker.shared.config().orgs.initialized {
            self.worker
                .shared
                .initialize_orgs(admin_orgs(&viewers), &self.worker.paths);
        }
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
            self.block_until(reset_at);
        }
    }

    fn block_until(&mut self, reset_at: DateTime<Utc>) {
        let wait = (reset_at - Utc::now()).to_std().unwrap_or_default();
        let until = Instant::now() + wait;
        self.blocked_until = Some(
            self.blocked_until
                .map_or(until, |current| current.max(until)),
        );
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
                self.emit(SyncEvent::Failed {
                    host: host.into(),
                    error: rate_limit_message(reset_at),
                });
                AccountOutcome::RateLimited(reset_at)
            }
            error => {
                error!(host, %error, "sync request failed");
                self.emit(SyncEvent::Failed {
                    host: host.into(),
                    error: error.to_string(),
                });
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

fn orgs_for_host(tracked: &[String], host: &str) -> Vec<String> {
    tracked
        .iter()
        .filter_map(|entry| entry.split_once('/'))
        .filter(|(entry_host, _)| *entry_host == host)
        .map(|(_, org)| org.to_string())
        .collect()
}

fn admin_orgs(viewers: &[(String, Viewer)]) -> Vec<String> {
    viewers
        .iter()
        .flat_map(|(host, viewer)| {
            viewer
                .orgs
                .iter()
                .filter(|org| org.can_admin)
                .map(move |org| format!("{host}/{}", org.login))
        })
        .collect()
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
