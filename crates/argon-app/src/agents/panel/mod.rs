mod history;

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::Utc;

use gpui_kit::component::dock::{BasePanel, Panel, PanelControl, PanelEvent};
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, Subscription,
    Task, Window,
};

use self::history::HistoryRow;
pub(super) use self::history::session_info;
use super::card_status::SessionFilter;
use super::panel_labels as labels;
use super::sessions::{AgentSessions, AgentsEvent};
use super::sessions_list::{SessionCard, render_list};
use super::thread_model::ThreadModel;
use super::thread_view::{ThreadView, ThreadViewEvent};
use crate::workspace::Workspace;

const PANEL_NAME: &str = "argon.agents";
const TICK: Duration = Duration::from_secs(1);

pub struct AgentPanel {
    focus_handle: FocusHandle,
    sessions: Option<Entity<AgentSessions>>,
    filter: SessionFilter,
    history: Vec<HistoryRow>,
    history_loaded: bool,
    history_task: Option<Task<()>>,
    historical: HashMap<String, Entity<ThreadModel>>,
    thread: Option<(Entity<ThreadView>, Subscription)>,
    _ticker: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl AgentPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sessions = AgentSessions::global(cx);
        let mut subscriptions = Vec::new();
        if let Some(sessions) = &sessions {
            subscriptions.push(cx.observe(sessions, |_, _, cx| cx.notify()));
            subscriptions.push(cx.subscribe_in(
                sessions,
                window,
                |this, _, event: &AgentsEvent, window, cx| {
                    let AgentsEvent::SessionStarted { session_id, .. } = event;
                    this.open_thread(session_id, window, cx);
                },
            ));
        }
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let alive = this.update(cx, |this, cx| {
                    if this.has_active(cx) {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        });
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            sessions,
            filter: SessionFilter::All,
            history: Vec::new(),
            history_loaded: false,
            history_task: None,
            historical: HashMap::new(),
            thread: None,
            _ticker: ticker,
            _subscriptions: subscriptions,
        };
        this.reload_history(cx);
        this
    }

    fn has_active(&self, cx: &App) -> bool {
        self.sessions.as_ref().is_some_and(|sessions| {
            sessions
                .read(cx)
                .threads()
                .any(|thread| thread.read(cx).card_status().is_active())
        })
    }

    pub(super) fn set_filter(&mut self, filter: SessionFilter, cx: &mut Context<Self>) {
        if self.filter != filter {
            self.filter = filter;
            cx.notify();
        }
    }

    pub(super) fn open_thread(
        &mut self,
        session_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let live = self
            .sessions
            .as_ref()
            .and_then(|sessions| sessions.read(cx).thread(session_id));
        let Some(model) = live.or_else(|| self.historical_thread(session_id, cx)) else {
            return;
        };
        let view = cx.new(|cx| ThreadView::new(model, window, cx));
        let subscription = cx.subscribe(&view, |this, _, event: &ThreadViewEvent, cx| {
            let ThreadViewEvent::Back = event;
            this.thread = None;
            this.reload_history(cx);
            cx.notify();
        });
        self.thread = Some((view, subscription));
        cx.notify();
    }

    fn render_list(&mut self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let now = Utc::now();
        let mut cards = Vec::new();
        let mut active = HashSet::new();
        if let Some(sessions) = &self.sessions {
            for thread in sessions.read(cx).threads() {
                let thread = thread.read(cx);
                active.insert(thread.info().session_id.clone());
                cards.push(SessionCard::from_thread(thread, now, cx));
            }
        }
        for history in &self.history {
            if !active.contains(&history.row.id) {
                cards.push(SessionCard::from_row(&history.row, history.counts, cx));
            }
        }
        render_list(cards, self.filter, self.history_loaded, cx)
    }

    pub(super) fn new_review(window: &mut Window, cx: &mut App) {
        if let Some(workspace) = Workspace::global(cx) {
            _ = workspace.update(cx, |workspace, cx| {
                workspace.review_active_pr_with_ai(window, cx)
            });
        }
    }
}

impl EventEmitter<PanelEvent> for AgentPanel {}

impl Focusable for AgentPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for AgentPanel {
    fn panel_name(&self) -> &'static str {
        PANEL_NAME
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn zoomable(&self, _: &App) -> bool {
        false
    }
}

impl Panel for AgentPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        labels::PANEL_TITLE
    }

    fn zoom_control(&self, _: &App) -> Option<PanelControl> {
        None
    }
}

impl Render for AgentPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.thread {
            Some((view, _)) => view.clone().into_any_element(),
            None => self.render_list(cx).into_any_element(),
        };
        v_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .child(body)
    }
}
