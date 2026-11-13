mod agent_events;
mod native;
mod needs_you;
mod open;
mod policy;
mod toast;

use std::time::Duration;

use argon_core::ViewSet;
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui_kit::{App, AppContext as _, Context, Entity, Global, Subscription, Task};

use self::agent_events::{AgentTracker, SessionSnapshot, Signal, phase_of};
use self::needs_you::{NeedsYouEntry, NeedsYouTracker};
use self::toast::{Channel, ToastSpec, ToastTarget};
use crate::agents::AgentSessions;
use crate::config_watch;
use crate::inbox::InboxModel;
use crate::services::SyncEvent;
use crate::sync_status::SyncStatus;
use crate::update::{self, UpdateEvent, UpdateModel};

const RELOAD_SETTLE: Duration = Duration::from_secs(2);
const FINDINGS_SETTLE: Duration = Duration::from_secs(1);

struct Notifier {
    inbox: Entity<InboxModel>,
    needs_you: NeedsYouTracker,
    agents: AgentTracker,
    clicks: UnboundedSender<ToastTarget>,
    sync_check: Option<Task<()>>,
    _click_listener: Task<()>,
    _subscriptions: Vec<Subscription>,
}

struct NotifierHandle {
    _notifier: Entity<Notifier>,
}

impl Global for NotifierHandle {}

pub fn init(sync_status: &Entity<SyncStatus>, inbox: &Entity<InboxModel>, cx: &mut App) {
    let sessions = AgentSessions::global(cx);
    let updates = update::entity(cx);
    let notifier = cx.new(|cx| Notifier::new(sync_status, inbox.clone(), sessions, &updates, cx));
    cx.set_global(NotifierHandle {
        _notifier: notifier,
    });
}

impl Notifier {
    fn new(
        sync_status: &Entity<SyncStatus>,
        inbox: Entity<InboxModel>,
        sessions: Option<Entity<AgentSessions>>,
        updates: &Entity<UpdateModel>,
        cx: &mut Context<Self>,
    ) -> Self {
        let (clicks, mut clicked) = unbounded();
        let click_listener = cx.spawn(async move |this, cx| {
            while let Some(target) = clicked.next().await {
                tracing::info!(?target, "toast activated");
                if this
                    .update(cx, |_, cx| open::open_target(target, cx))
                    .is_err()
                {
                    return;
                }
            }
        });
        let mut subscriptions = vec![
            cx.subscribe(sync_status, |this, _, event: &SyncEvent, cx| {
                if matches!(event, SyncEvent::Finished { .. }) {
                    this.schedule_needs_you_check(cx);
                }
            }),
            cx.subscribe(updates, |this, _, event: &UpdateEvent, cx| {
                let UpdateEvent::Available(release) = event;
                let spec = ToastSpec {
                    channel: Channel::AppUpdate,
                    body: update::toast_body(release),
                    target: ToastTarget::AppUpdate,
                };
                this.dispatch(vec![spec], cx);
            }),
        ];
        if let Some(sessions) = sessions {
            subscriptions.push(cx.observe(&sessions, |this, sessions, cx| {
                this.on_sessions_changed(&sessions, cx);
            }));
        }
        Self {
            inbox,
            needs_you: NeedsYouTracker::default(),
            agents: AgentTracker::default(),
            clicks,
            sync_check: None,
            _click_listener: click_listener,
            _subscriptions: subscriptions,
        }
    }

    fn schedule_needs_you_check(&mut self, cx: &mut Context<Self>) {
        self.sync_check = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RELOAD_SETTLE).await;
            this.update(cx, |this, cx| this.check_needs_you(cx)).ok();
        }));
    }

    fn check_needs_you(&mut self, cx: &mut Context<Self>) {
        let inbox = self.inbox.read(cx);
        if !inbox.is_loaded() {
            return;
        }
        let current = inbox
            .items()
            .iter()
            .filter(|item| item.views.contains(ViewSet::NEEDS_YOU))
            .filter_map(|item| {
                let pr = item.subject.as_pr()?;
                Some(NeedsYouEntry::new(
                    pr.key(),
                    pr.number,
                    &pr.title,
                    &item.reason,
                ))
            })
            .collect();
        let entered = self.needs_you.observe(current);
        self.dispatch(needs_you::toasts(entered), cx);
    }

    fn on_sessions_changed(&mut self, sessions: &Entity<AgentSessions>, cx: &mut Context<Self>) {
        let snapshots = read_snapshots(sessions, None, cx);
        for (signal, snapshot) in self.agents.observe(&snapshots) {
            match signal {
                Signal::Permission | Signal::Failed => {
                    self.dispatch(vec![agent_events::toast(signal, &snapshot)], cx);
                }
                Signal::Completed => self.announce_completion(sessions.clone(), snapshot, cx),
            }
        }
    }

    fn announce_completion(
        &mut self,
        sessions: Entity<AgentSessions>,
        snapshot: SessionSnapshot,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FINDINGS_SETTLE).await;
            this.update(cx, |this, cx| {
                let latest = read_snapshots(&sessions, Some(&snapshot.session_id), cx);
                if let Some(latest) = latest
                    .into_iter()
                    .find(|latest| latest.phase == agent_events::Phase::Completed)
                {
                    this.dispatch(vec![agent_events::toast(Signal::Completed, &latest)], cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn dispatch(&self, specs: Vec<ToastSpec>, cx: &mut App) {
        if specs.is_empty() {
            return;
        }
        let config = config_watch::current(cx).notifications;
        let focused = open::is_window_focused(cx);
        for spec in specs {
            if policy::allowed(config, spec.channel, focused) {
                native::show(&spec, &self.clicks);
            }
        }
    }
}

fn read_snapshots(
    sessions: &Entity<AgentSessions>,
    only: Option<&str>,
    cx: &App,
) -> Vec<SessionSnapshot> {
    sessions
        .read(cx)
        .threads()
        .map(|thread| thread.read(cx))
        .filter(|thread| only.is_none_or(|id| thread.info().session_id == id))
        .map(|thread| {
            let info = thread.info();
            SessionSnapshot {
                session_id: info.session_id.clone(),
                pr: info.pr.clone(),
                number: info.number,
                agent_label: info.agent_label.to_string(),
                phase: phase_of(
                    thread.is_live(),
                    thread.status(),
                    thread.transcript().pending_count(),
                ),
                findings: thread.counts().total(),
            }
        })
        .collect()
}
