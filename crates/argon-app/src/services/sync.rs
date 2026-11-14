use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use argon_core::{DEFAULT_HOST, Involvement, Issue, SubjectKind, is_mention_candidate};
use argon_github::{
    Account, Fetched, GithubClient, GithubError, OrgFetch, Viewer, discover_accounts, token_for,
};
use argon_store::Store;
use chrono::{DateTime, Duration as ChronoDuration, Local, Utc};
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::runtime::{Handle, Runtime};
use tracing::{error, info, warn};

use super::orgs::{admin_orgs, cache_orgs, orgs_for_host};
use super::shared::Shared;
use super::store_handle::{StoreHandle, StoreStartup, flatten};

const THREAD_NAME: &str = "argon-services";
const RUNTIME_THREAD_NAME: &str = "argon-tokio";
const RUNTIME_WORKERS: usize = 2;
const LOCAL_STORE_HOST: &str = "argon.db";
const MIN_INTERVAL: Duration = Duration::from_secs(10);
const HYDRATION_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncEvent {
    Started,
    Partial,
    Finished { at: DateTime<Utc>, count: usize },
    Failed { host: String, error: String },
    AuthMissing { host: String },
}

pub(super) struct SyncRequest;

struct Session {
    account: Account,
    client: Arc<GithubClient>,
}

enum Step {
    Important(HashSet<String>),
    Done(AccountOutcome),
}

enum AccountOutcome {
    Synced,
    SyncedThenRateLimited(DateTime<Utc>),
    Failed,
    RateLimited(DateTime<Utc>),
    Expired,
}

enum IssueError {
    Github(GithubError),
    Store(String),
}

impl From<GithubError> for IssueError {
    fn from(error: GithubError) -> Self {
        Self::Github(error)
    }
}

struct IssueSync {
    orgs_with_more: Vec<String>,
    mention_error: Option<IssueError>,
}

#[derive(Default)]
struct MentionCheck {
    candidates: usize,
    found: usize,
    retry: Vec<String>,
    error: Option<IssueError>,
}

pub(super) struct SyncWorker {
    pub(super) shared: Arc<Shared>,
    pub(super) store: StoreHandle,
    pub(super) store_startup: StoreStartup,
    pub(super) events: UnboundedSender<SyncEvent>,
    pub(super) requests: UnboundedReceiver<SyncRequest>,
}

impl SyncWorker {
    pub(super) fn spawn(self) -> Option<Handle> {
        let events = self.events.clone();
        let runtime = match build_runtime() {
            Ok(runtime) => runtime,
            Err(error) => {
                error!(%error, "failed to build tokio runtime");
                _ = events.unbounded_send(startup_failure(&error));
                return None;
            }
        };
        let handle = runtime.handle().clone();
        let spawned = thread::Builder::new()
            .name(THREAD_NAME.into())
            .spawn(move || runtime.block_on(SyncLoop::new(self).run()));
        match spawned {
            Ok(_) => Some(handle),
            Err(error) => {
                error!(%error, "failed to start services thread");
                _ = events.unbounded_send(startup_failure(&error));
                None
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
        let more_generation = self.worker.shared.more_orgs_requested();
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
        let window = config.sync.closed_window_days;
        let mut steps = Vec::with_capacity(self.sessions.len());
        for session in &self.sessions {
            steps.push(self.sync_important(session, window).await);
        }
        let mut outcomes = Vec::with_capacity(steps.len());
        for (session, step) in self.sessions.iter().zip(steps) {
            outcomes.push(match step {
                Step::Important(kept) => {
                    let orgs = orgs_for_host(&config.orgs.tracked, &session.account.host);
                    self.sync_host(session, orgs, kept, window).await
                }
                Step::Done(outcome) => outcome,
            });
        }
        self.apply_outcomes(&outcomes);
        self.worker.shared.serve_more_orgs(more_generation);

        let synced = outcomes
            .iter()
            .filter(|outcome| {
                matches!(
                    outcome,
                    AccountOutcome::Synced | AccountOutcome::SyncedThenRateLimited(_)
                )
            })
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
            if let AccountOutcome::RateLimited(reset_at)
            | AccountOutcome::SyncedThenRateLimited(reset_at) = outcome
            {
                self.blocked.insert(session.account.host.clone(), *reset_at);
            }
        }
        let mut expired = outcomes
            .iter()
            .map(|outcome| matches!(outcome, AccountOutcome::Expired));
        self.sessions
            .retain(|_| !expired.next().unwrap_or_default());
    }

    async fn sync_important(&self, session: &Session, window: i64) -> Step {
        let account = &session.account;
        if self.report_blocked(&account.host) {
            return Step::Done(AccountOutcome::Failed);
        }
        let fetched = Instant::now();
        let Fetched {
            items: prs, kept, ..
        } = match session.client.fetch_important(window).await {
            Ok(fetched) => fetched,
            Err(error) => return Step::Done(self.report(&account.host, error)),
        };
        let hydrated = prs.len();
        let host = account.host.clone();
        let saved = self
            .store_prs(&account.host, move |store| store.upsert_prs(&host, &prs))
            .await;
        if !saved {
            return Step::Done(AccountOutcome::Failed);
        }
        info!(
            host = %account.host,
            login = %account.login,
            prs = kept.len(),
            hydrated,
            elapsed_ms = elapsed_ms(fetched),
            "important pull requests synced"
        );
        self.emit(SyncEvent::Partial);
        Step::Important(kept)
    }

    async fn sync_host(
        &self,
        session: &Session,
        orgs: Vec<String>,
        kept: HashSet<String>,
        window: i64,
    ) -> AccountOutcome {
        let host = &session.account.host;
        let pages = self.worker.shared.org_depths(host, orgs);
        let outdated = self.cache_outdated(host).await;
        let pr_more = match self
            .sync_orgs(session, &pages, kept, window, outdated)
            .await
        {
            Ok(orgs_with_more) => orgs_with_more,
            Err(outcome) => return outcome,
        };
        let (issue_more, issue_error, mention_error) =
            match self.sync_issues(session, &pages, window, outdated).await {
                Ok(synced) => {
                    if outdated {
                        self.record_hydration_version(host).await;
                    }
                    (synced.orgs_with_more, None, synced.mention_error)
                }
                Err(error) => (Vec::new(), Some(error), None),
            };
        self.worker
            .shared
            .set_orgs_with_more(host, &combine_orgs_with_more(&pr_more, &issue_more));
        match (issue_error, mention_error) {
            (Some(error), _) => self.report_issues(host, error),
            (None, Some(error)) => report_mentions(host, error),
            (None, None) => AccountOutcome::Synced,
        }
    }

    async fn sync_orgs(
        &self,
        session: &Session,
        pages: &[(String, usize)],
        mut kept: HashSet<String>,
        window: i64,
        hydrate_all: bool,
    ) -> Result<Vec<String>, AccountOutcome> {
        let account = &session.account;
        let fetched = Instant::now();
        let stored = self
            .stored_updated_at(&account.host, SubjectKind::Pr, hydrate_all)
            .await;
        let OrgFetch {
            fetched:
                Fetched {
                    items: prs,
                    kept: org_kept,
                    unchanged,
                },
            orgs_with_more,
        } = match session
            .client
            .fetch_orgs(pages, &kept, &stored, window)
            .await
        {
            Ok(fetch) => fetch,
            Err(error) => return Err(self.report(&account.host, error)),
        };
        kept.extend(org_kept);
        let hydrated = prs.len();
        let total = kept.len();
        let host = account.host.clone();
        let saved = self
            .store_prs(&account.host, move |store| {
                store.upsert_prs(&host, &prs)?;
                store.update_involvement(&host, SubjectKind::Pr, &unchanged)?;
                store.prune_prs(&host, &kept)
            })
            .await;
        if !saved {
            return Err(AccountOutcome::Failed);
        }
        info!(
            host = %account.host,
            login = %account.login,
            hydrated,
            prs = total,
            orgs_with_more = orgs_with_more.len(),
            elapsed_ms = elapsed_ms(fetched),
            "inbox synced"
        );
        self.emit(SyncEvent::Partial);
        Ok(orgs_with_more)
    }

    async fn sync_issues(
        &self,
        session: &Session,
        pages: &[(String, usize)],
        window: i64,
        outdated: bool,
    ) -> Result<IssueSync, IssueError> {
        let account = &session.account;
        let fetched = Instant::now();
        let hydrate_all = outdated || !self.issue_baseline_done(&account.host).await;
        let stored = self
            .stored_updated_at(&account.host, SubjectKind::Issue, hydrate_all)
            .await;
        let Fetched {
            items: mut issues,
            mut kept,
            mut unchanged,
        } = session
            .client
            .fetch_important_issues(&stored, window)
            .await?;
        let OrgFetch {
            fetched:
                Fetched {
                    items: org_issues,
                    kept: org_kept,
                    unchanged: org_unchanged,
                },
            orgs_with_more,
        } = session
            .client
            .fetch_org_issues(pages, &kept, &stored, window)
            .await?;
        kept.extend(org_kept);
        unchanged.extend(org_unchanged);
        issues.extend(org_issues);
        let MentionCheck {
            candidates,
            found: new_mentions,
            retry,
            error: mention_error,
        } = self.check_mentions(session, &mut issues, window).await;
        let hydrated = issues.len();
        let total = kept.len();
        let baseline = self
            .store_issues(&account.host, issues, unchanged, kept, retry)
            .await?;
        info!(
            host = %account.host,
            login = %account.login,
            hydrated,
            issues = total,
            mention_candidates = candidates,
            new_mentions,
            orgs_with_more = orgs_with_more.len(),
            baseline,
            elapsed_ms = elapsed_ms(fetched),
            "issues synced"
        );
        Ok(IssueSync {
            orgs_with_more,
            mention_error,
        })
    }

    async fn check_mentions(
        &self,
        session: &Session,
        issues: &mut [Issue],
        window: i64,
    ) -> MentionCheck {
        let account = &session.account;
        let host = account.host.clone();
        let loaded = flatten(
            self.worker
                .store
                .call(move |store| store.last_seen_map(&host))
                .await,
        );
        let last_seen = match loaded {
            Ok(last_seen) => last_seen,
            Err(error) => {
                let every: Vec<String> = issues.iter().map(|issue| issue.id.0.clone()).collect();
                self.keep_stored_mentions(&account.host, issues, &every)
                    .await;
                return MentionCheck {
                    retry: every,
                    error: Some(IssueError::Store(error)),
                    ..MentionCheck::default()
                };
            }
        };
        let candidates: Vec<String> = issues
            .iter()
            .filter(|issue| {
                let seen = last_seen.get(&issue.id.0).copied();
                is_mention_candidate(issue, &account.login, seen)
            })
            .map(|issue| issue.id.0.clone())
            .collect();
        if candidates.is_empty() {
            return MentionCheck::default();
        }
        let window_start = Utc::now() - ChronoDuration::days(window);
        let since = mention_floors(&candidates, &last_seen, window_start);
        match session
            .client
            .latest_mentions(&candidates, &account.login, &since)
            .await
        {
            Ok(latest) => {
                apply_mentions(issues, &latest);
                MentionCheck {
                    candidates: candidates.len(),
                    found: latest.len(),
                    ..MentionCheck::default()
                }
            }
            Err(error) => {
                self.keep_stored_mentions(&account.host, issues, &candidates)
                    .await;
                MentionCheck {
                    candidates: candidates.len(),
                    retry: candidates,
                    error: Some(error.into()),
                    ..MentionCheck::default()
                }
            }
        }
    }

    async fn keep_stored_mentions(&self, host: &str, issues: &mut [Issue], ids: &[String]) {
        let owned = host.to_string();
        let stored = self
            .worker
            .store
            .call(move |store| store.stored_last_mentions(&owned))
            .await;
        match flatten(stored) {
            Ok(mut stored) => {
                stored.retain(|id, _| ids.contains(id));
                apply_mentions(issues, &stored);
            }
            Err(error) => warn!(host, %error, "failed to read stored mention times"),
        }
    }

    async fn store_issues(
        &self,
        host: &str,
        issues: Vec<Issue>,
        unchanged: Vec<(String, Involvement)>,
        kept: HashSet<String>,
        retry: Vec<String>,
    ) -> Result<bool, IssueError> {
        let host = host.to_string();
        let stored = self
            .worker
            .store
            .call(move |store| -> argon_store::Result<bool> {
                let baseline = !store.issue_baseline_done(&host)?;
                store.upsert_issues(&host, &issues)?;
                store.update_involvement(&host, SubjectKind::Issue, &unchanged)?;
                store.mark_stale(&host, SubjectKind::Issue, &retry)?;
                store.prune_issues(&host, &kept)?;
                if baseline {
                    let seen: Vec<(String, DateTime<Utc>)> = issues
                        .iter()
                        .map(|issue| (issue.id.0.clone(), issue.updated_at))
                        .collect();
                    store.mark_seen_all(&host, &seen)?;
                    store.mark_issue_baseline_done(&host, Utc::now())?;
                }
                Ok(baseline)
            })
            .await;
        flatten(stored).map_err(IssueError::Store)
    }

    async fn cache_outdated(&self, host: &str) -> bool {
        let owned = host.to_string();
        let stored = self
            .worker
            .store
            .call(move |store| store.hydration_version(&owned))
            .await;
        hydration_outdated(flatten(stored).unwrap_or_else(|error| {
            warn!(host, %error, "failed to read hydration version");
            None
        }))
    }

    async fn record_hydration_version(&self, host: &str) {
        let owned = host.to_string();
        let saved = self
            .worker
            .store
            .call(move |store| store.set_hydration_version(&owned, HYDRATION_VERSION))
            .await;
        if let Err(error) = flatten(saved) {
            warn!(host, %error, "failed to record hydration version");
        }
    }

    async fn issue_baseline_done(&self, host: &str) -> bool {
        let owned = host.to_string();
        let done = self
            .worker
            .store
            .call(move |store| store.issue_baseline_done(&owned))
            .await;
        flatten(done).unwrap_or(false)
    }

    async fn stored_updated_at(
        &self,
        host: &str,
        kind: SubjectKind,
        hydrate_all: bool,
    ) -> HashMap<String, DateTime<Utc>> {
        if hydrate_all {
            return HashMap::new();
        }
        let owned = host.to_string();
        let loaded = self
            .worker
            .store
            .call(move |store| store.updated_at_map(&owned, kind))
            .await;
        flatten(loaded).unwrap_or_else(|error| {
            warn!(host, %error, "failed to read stored update times, hydrating everything");
            HashMap::new()
        })
    }

    async fn store_prs(
        &self,
        host: &str,
        job: impl FnOnce(&mut Store) -> argon_store::Result<()> + Send + 'static,
    ) -> bool {
        match flatten(self.worker.store.call(job).await) {
            Ok(()) => true,
            Err(error) => {
                error!(host, %error, "failed to store pull requests");
                self.fail(host, format!("falha ao gravar no banco local: {error}"));
                false
            }
        }
    }

    async fn finish(&self, started: Instant, failures: usize) {
        let counted = self
            .worker
            .store
            .call(|store| {
                let issues = store.list_issues().map(|issues| issues.len());
                store.list_prs().map(|prs| (prs.len(), issues))
            })
            .await;
        match flatten(counted) {
            Ok((prs, issues)) => {
                let issues = issues.unwrap_or_else(|error| {
                    warn!(%error, "failed to count stored issues");
                    0
                });
                let count = prs + issues;
                info!(
                    count,
                    prs,
                    issues,
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
            tokio::spawn(self.worker.shared.seed_tracked(admin_orgs(&fresh)));
        }
    }

    fn has_session(&self, host: &str) -> bool {
        self.sessions
            .iter()
            .any(|session| session.account.host == host)
    }

    async fn open_session(&mut self, host: &str) -> Option<(Session, Viewer)> {
        let client = match self.worker.shared.client(host) {
            Some(client) => client,
            None => match token_for(host).await {
                Ok(token) => Arc::new(GithubClient::new(host, token)),
                Err(error) => {
                    self.report_connect(host, error);
                    return None;
                }
            },
        };
        match client.viewer().await {
            Ok(viewer) => {
                info!(
                    host,
                    login = %viewer.login,
                    orgs = viewer.orgs.len(),
                    "gh account connected"
                );
                let client = self.worker.shared.register_client(host, client);
                self.worker.shared.connect(host, &viewer);
                self.cache_viewer(host, &viewer).await;
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

    async fn cache_viewer(&self, host: &str, viewer: &Viewer) {
        let (host, login, orgs) = (host.to_string(), viewer.login.clone(), cache_orgs(viewer));
        let saved = self
            .worker
            .store
            .call(move |store| store.save_viewer(&host, &login, &orgs))
            .await
            .map_err(|error| error.to_string())
            .and_then(|saved| saved.map_err(|error| error.to_string()));
        if let Err(error) = saved {
            warn!(%error, "failed to cache gh account");
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
                self.worker.shared.forget(host);
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

    fn report_issues(&self, host: &str, error: IssueError) -> AccountOutcome {
        let detail = match error {
            IssueError::Github(
                error @ (GithubError::AuthMissing { .. } | GithubError::RateLimited { .. }),
            ) => {
                return match self.report(host, error) {
                    AccountOutcome::RateLimited(reset_at) => {
                        AccountOutcome::SyncedThenRateLimited(reset_at)
                    }
                    outcome => outcome,
                };
            }
            IssueError::Github(error) => error.to_string(),
            IssueError::Store(error) => format!("banco local: {error}"),
        };
        warn!(host, error = %detail, "issue sync failed");
        self.fail(host, format!("Falha ao sincronizar issues: {detail}"));
        AccountOutcome::Synced
    }
}

fn report_mentions(host: &str, error: IssueError) -> AccountOutcome {
    match error {
        IssueError::Github(GithubError::RateLimited { reset_at }) => {
            warn!(host, %reset_at, "github rate limit reached while checking issue mentions");
            AccountOutcome::SyncedThenRateLimited(reset_at)
        }
        IssueError::Github(error) => {
            warn!(host, %error, "issue mention check failed");
            AccountOutcome::Synced
        }
        IssueError::Store(error) => {
            warn!(host, %error, "issue mention check failed to read the local store");
            AccountOutcome::Synced
        }
    }
}

fn hydration_outdated(stored: Option<u32>) -> bool {
    stored != Some(HYDRATION_VERSION)
}

fn mention_floors(
    candidates: &[String],
    last_seen: &HashMap<String, DateTime<Utc>>,
    window_start: DateTime<Utc>,
) -> HashMap<String, DateTime<Utc>> {
    candidates
        .iter()
        .map(|id| {
            let floor = last_seen.get(id).copied().unwrap_or(window_start);
            (id.clone(), floor)
        })
        .collect()
}

fn apply_mentions(issues: &mut [Issue], mentions: &HashMap<String, DateTime<Utc>>) {
    for issue in issues {
        if let Some(at) = mentions.get(&issue.id.0) {
            issue.last_mention_at = Some(*at);
        }
    }
}

fn combine_orgs_with_more(pr: &[String], issue: &[String]) -> Vec<String> {
    let mut combined: Vec<String> = Vec::with_capacity(pr.len() + issue.len());
    for org in pr.iter().chain(issue) {
        if !combined.iter().any(|known| known.eq_ignore_ascii_case(org)) {
            combined.push(org.clone());
        }
    }
    combined
}

fn build_runtime() -> std::io::Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(RUNTIME_WORKERS)
        .thread_name(RUNTIME_THREAD_NAME)
        .enable_all()
        .build()
}

fn startup_failure(error: &std::io::Error) -> SyncEvent {
    SyncEvent::Failed {
        host: DEFAULT_HOST.into(),
        error: format!("falha ao iniciar o serviço de sincronização: {error}"),
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

#[cfg(test)]
mod tests {
    use super::*;

    type Case = (
        &'static str,
        &'static [&'static str],
        &'static [&'static str],
        &'static [&'static str],
    );

    fn strings(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn hydration_is_outdated_unless_the_stored_version_matches() {
        assert!(hydration_outdated(None));
        assert!(hydration_outdated(Some(HYDRATION_VERSION + 1)));
        assert!(hydration_outdated(Some(0)));
        assert!(!hydration_outdated(Some(HYDRATION_VERSION)));
    }

    #[test]
    fn a_failed_mention_check_still_counts_as_synced() {
        for error in [
            IssueError::Github(GithubError::GraphQl(vec!["boom".into()])),
            IssueError::Store("locked".into()),
        ] {
            assert!(matches!(
                report_mentions("github.com", error),
                AccountOutcome::Synced
            ));
        }
    }

    #[test]
    fn a_rate_limited_mention_check_blocks_the_host_without_failing() {
        let reset_at = DateTime::from_timestamp(9_000, 0).unwrap();
        let error = IssueError::Github(GithubError::RateLimited { reset_at });
        assert!(matches!(
            report_mentions("github.com", error),
            AccountOutcome::SyncedThenRateLimited(at) if at == reset_at
        ));
    }

    #[test]
    fn mention_floors_fall_back_to_the_window_start_for_unseen_issues() {
        let window_start = DateTime::from_timestamp(1_000, 0).unwrap();
        let seen_at = DateTime::from_timestamp(5_000, 0).unwrap();
        let last_seen = HashMap::from([
            ("seen".to_string(), seen_at),
            ("not_a_candidate".to_string(), seen_at),
        ]);

        let floors = mention_floors(&strings(&["seen", "unseen"]), &last_seen, window_start);

        assert_eq!(
            floors,
            HashMap::from([
                ("seen".to_string(), seen_at),
                ("unseen".to_string(), window_start),
            ])
        );
    }

    #[test]
    fn combine_orgs_with_more_unites_pr_and_issue_orgs() {
        let cases: [Case; 7] = [
            ("both empty", &[], &[], &[]),
            ("only pull requests", &["vela"], &[], &["vela"]),
            ("only issues", &[], &["atlas"], &["atlas"]),
            ("disjoint", &["vela"], &["atlas"], &["vela", "atlas"]),
            (
                "overlap keeps first appearance order",
                &["vela", "nimbus"],
                &["atlas", "nimbus"],
                &["vela", "nimbus", "atlas"],
            ),
            ("same org in another case", &["Vela"], &["vela"], &["Vela"]),
            (
                "duplicate within one side",
                &["vela", "vela"],
                &["vela"],
                &["vela"],
            ),
        ];

        for (name, pr, issue, expected) in cases {
            assert_eq!(
                combine_orgs_with_more(&strings(pr), &strings(issue)),
                strings(expected),
                "{name}"
            );
        }
    }
}
