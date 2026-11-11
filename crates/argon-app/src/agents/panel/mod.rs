use std::time::{Duration, Instant};

use chrono::Utc;

use gpui_kit::component::dock::{BasePanel, Panel, PanelControl, PanelEvent};
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, Subscription, Task, Window,
};

use super::card_status::SessionFilter;
use super::catalog::SessionCatalog;
use super::panel_labels as labels;
use super::session_tab::open_session_tab;
use super::sessions::{AgentSessions, AgentsEvent};
use super::sessions_list::render_list;
use crate::workspace::Workspace;

const PANEL_NAME: &str = "argon.agents";
const TICK: Duration = Duration::from_secs(1);

pub struct AgentPanel {
    focus_handle: FocusHandle,
    sessions: Option<Entity<AgentSessions>>,
    filter: SessionFilter,
    catalog: Option<Entity<SessionCatalog>>,
    last_render: Option<Instant>,
    _ticker: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl AgentPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sessions = AgentSessions::global(cx);
        let catalog = SessionCatalog::global(cx);
        let mut subscriptions = Vec::new();
        if let Some(catalog) = &catalog {
            subscriptions.push(cx.observe(catalog, |_, _, cx| cx.notify()));
        }
        if let Some(sessions) = &sessions {
            subscriptions.push(cx.observe(sessions, |_, _, cx| cx.notify()));
            subscriptions.push(cx.subscribe_in(
                sessions,
                window,
                |_, _, event: &AgentsEvent, window, cx| {
                    if let AgentsEvent::SessionStarted { session_id, .. } = event {
                        open_session_tab(session_id, window, cx);
                    }
                },
            ));
        }
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let alive = this.update(cx, |this, cx| {
                    if this.is_visible() && this.has_active(cx) {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        });
        Self {
            focus_handle: cx.focus_handle(),
            sessions,
            filter: SessionFilter::All,
            catalog,
            last_render: None,
            _ticker: ticker,
            _subscriptions: subscriptions,
        }
    }

    fn is_visible(&self) -> bool {
        self.last_render
            .is_some_and(|at| at.elapsed() < TICK + TICK)
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

    fn render_list(&mut self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let (cards, loaded) = match &self.catalog {
            Some(catalog) => {
                let catalog = catalog.read(cx);
                (catalog.cards(Utc::now(), cx), catalog.is_loaded())
            }
            None => (Vec::new(), false),
        };
        render_list(cards, self.filter, loaded, cx)
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
        self.last_render = Some(Instant::now());
        let body = self.render_list(cx);
        v_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .child(body)
    }
}
