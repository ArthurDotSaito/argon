use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui_kit::{Context, EventEmitter, SharedString, Task};

use crate::services::{Services, SyncEvent};

const CLOCK_TICK: Duration = Duration::from_secs(30);
const DEFAULT_HOST: &str = "github.com";
const MAX_ERROR_CHARS: usize = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncState {
    Idle,
    Syncing,
    Ok { at: DateTime<Utc>, count: usize },
    Error(String),
    AuthMissing(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncTone {
    Neutral,
    Busy,
    Success,
    Danger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastSync {
    pub at: DateTime<Utc>,
    pub count: usize,
}

pub struct SyncIndicator {
    pub label: SharedString,
    pub tone: SyncTone,
}

pub struct SyncStatus {
    pub state: SyncState,
    pub last_ok: Option<LastSync>,
    pub login: Option<String>,
    _tasks: [Task<()>; 2],
}

impl SyncStatus {
    pub fn new(
        services: Services,
        mut events: UnboundedReceiver<SyncEvent>,
        cx: &mut Context<Self>,
    ) -> Self {
        let listener = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                let login = services
                    .accounts()
                    .into_iter()
                    .next()
                    .map(|account| account.login);
                let applied = this.update(cx, |status, cx| {
                    status.apply(&event, login);
                    cx.emit(event);
                    cx.notify();
                });
                if applied.is_err() {
                    break;
                }
            }
        });
        let clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CLOCK_TICK).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        Self {
            state: SyncState::Idle,
            last_ok: None,
            login: None,
            _tasks: [listener, clock],
        }
    }

    pub fn indicator(&self, now: DateTime<Utc>) -> SyncIndicator {
        let (label, tone) = match &self.state {
            SyncState::Idle => ("aguardando sincronização".to_string(), SyncTone::Neutral),
            SyncState::Syncing => ("sincronizando…".to_string(), SyncTone::Busy),
            SyncState::Ok { at, .. } => (synced_label(now, *at), SyncTone::Success),
            SyncState::Error(error) => (format!("erro: {}", truncate(error)), SyncTone::Danger),
            SyncState::AuthMissing(host) => (auth_missing_label(host), SyncTone::Danger),
        };
        SyncIndicator {
            label: label.into(),
            tone,
        }
    }

    fn apply(&mut self, event: &SyncEvent, login: Option<String>) {
        self.login = login;
        match event {
            SyncEvent::Started => self.state = SyncState::Syncing,
            SyncEvent::Finished { at, count } => {
                self.last_ok = Some(LastSync {
                    at: *at,
                    count: *count,
                });
                if self.state == SyncState::Syncing {
                    self.state = SyncState::Ok {
                        at: *at,
                        count: *count,
                    };
                }
            }
            SyncEvent::Failed { error, .. } => self.state = SyncState::Error(error.clone()),
            SyncEvent::AuthMissing { host } => self.state = SyncState::AuthMissing(host.clone()),
        }
    }
}

impl EventEmitter<SyncEvent> for SyncStatus {}

fn synced_label(now: DateTime<Utc>, at: DateTime<Utc>) -> String {
    let minutes = (now - at).num_minutes().max(0);
    match minutes {
        0 => "sincronizado agora".to_string(),
        1..60 => format!("sincronizado há {minutes} min"),
        60..1440 => format!("sincronizado há {} h", minutes / 60),
        _ => format!("sincronizado há {} d", minutes / 1440),
    }
}

fn auth_missing_label(host: &str) -> String {
    if host == DEFAULT_HOST {
        "gh não autenticado — rode gh auth login".to_string()
    } else {
        format!("gh não autenticado — rode gh auth login --hostname {host}")
    }
}

fn truncate(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default();
    if line.chars().count() <= MAX_ERROR_CHARS {
        return line.to_string();
    }
    let mut short: String = line.chars().take(MAX_ERROR_CHARS).collect();
    short.push('…');
    short
}
