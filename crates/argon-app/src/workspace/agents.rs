use argon_core::{PrKey, Side};
use gpui_kit::component::Placement;
use gpui_kit::component::dock::{InsertTarget, PanelId};
use gpui_kit::{App, Context, Entity, Focusable as _, Global, WeakEntity, Window, px};

use super::Workspace;
use crate::actions::ReviewWithAi;
use crate::agents::{AgentSessions, AgentsEvent, SessionCatalog, SessionFilter};
use crate::center::CenterItem;
use crate::dock_items;
use crate::pr::PrView;
use crate::services::AppServices;

const SESSION_SPLIT_WIDTH: f32 = 420.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPlacement {
    Center,
    BesidePr,
}

struct WorkspaceHandle(WeakEntity<Workspace>);

impl Global for WorkspaceHandle {}

impl Workspace {
    pub fn global(cx: &App) -> Option<WeakEntity<Workspace>> {
        cx.try_global::<WorkspaceHandle>()
            .map(|handle| handle.0.clone())
    }

    pub(super) fn register_global(cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        cx.set_global(WorkspaceHandle(weak));
    }

    fn pr_view(&self, pr: &PrKey, cx: &App) -> Option<Entity<PrView>> {
        let item = CenterItem::Pr(pr.clone());
        self.center_panels(cx)
            .into_iter()
            .find(|panel| panel.read(cx).item() == &item)
            .and_then(|panel| panel.read(cx).view().clone().downcast::<PrView>().ok())
    }

    fn active_pr_view(&self, window: &Window, cx: &App) -> Option<Entity<PrView>> {
        let panel = self.active_center_panel(window, cx)?;
        panel.read(cx).view().clone().downcast::<PrView>().ok()
    }

    pub fn reveal_in_diff(
        &mut self,
        pr: PrKey,
        path: &str,
        line: u32,
        side: Side,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_item(CenterItem::Pr(pr.clone()), false, window, cx);
        let Some(view) = self.pr_view(&pr, cx) else {
            return;
        };
        view.update(cx, |view, cx| view.reveal(path, line, side, cx));
    }

    pub fn selected_diff_line(&self, pr: &PrKey, cx: &App) -> Option<(String, u32)> {
        self.pr_view(pr, cx)?.read(cx).selected_line(cx)
    }

    pub fn review_pr_with_ai(&mut self, pr: PrKey, window: &mut Window, cx: &mut Context<Self>) {
        self.open_item(CenterItem::Pr(pr.clone()), false, window, cx);
        if let Some(view) = self.pr_view(&pr, cx) {
            view.update(cx, |view, cx| view.open_ai_review(window, cx));
        }
    }

    pub(super) fn close_requested(&mut self, cx: &mut Context<Self>) -> bool {
        let services = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone());
        match (AgentSessions::global(cx), services) {
            (Some(sessions), Some(services)) => {
                sessions.update(cx, |sessions, cx| sessions.begin_close(&services, cx))
            }
            _ => true,
        }
    }

    pub fn shutdown_then(
        &mut self,
        after: impl FnOnce(&mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.save_now(cx);
        let services = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone());
        match (AgentSessions::global(cx), services) {
            (Some(sessions), Some(services)) => sessions.update(cx, |sessions, cx| {
                sessions.shutdown_then(&services, after, cx)
            }),
            _ => after(cx),
        }
    }

    pub fn show_agents(
        &mut self,
        filter: SessionFilter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(catalog) = SessionCatalog::global(cx) {
            catalog.update(cx, |catalog, cx| {
                catalog.set_filter(filter, cx);
                catalog.set_agents_active(true, cx);
            });
        }
        self.open_item(CenterItem::Agents, false, window, cx);
    }

    pub fn open_session(
        &mut self,
        session_id: String,
        placement: SessionPlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pr = match placement {
            SessionPlacement::Center => None,
            SessionPlacement::BesidePr => SessionCatalog::global(cx)
                .and_then(|catalog| catalog.read(cx).pr_of(&session_id, cx)),
        };
        self.open_session_for(session_id, pr, window, cx);
    }

    fn open_session_for(
        &mut self,
        session_id: String,
        pr: Option<PrKey>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = CenterItem::AgentSession(session_id);
        let already_open = self
            .center_panels(cx)
            .iter()
            .any(|panel| panel.read(cx).item() == &item);
        let Some(pr) = pr.filter(|_| !already_open) else {
            self.open_item(item, false, window, cx);
            return;
        };
        self.open_item(CenterItem::Pr(pr.clone()), false, window, cx);
        let area = self.dock_area.read(cx);
        let pr_node = self
            .center_panels(cx)
            .iter()
            .find(|panel| panel.read(cx).item() == &CenterItem::Pr(pr.clone()))
            .and_then(|panel| dock_items::locate(area, PanelId::from(panel.entity_id())))
            .map(|(_, node)| node);
        let sibling_group = self
            .center_panels(cx)
            .iter()
            .filter(|panel| matches!(panel.read(cx).item(), CenterItem::AgentSession(_)))
            .filter_map(|panel| dock_items::locate(area, PanelId::from(panel.entity_id())))
            .map(|(_, node)| node)
            .find(|node| Some(*node) != pr_node);
        let panel = self.add_center_panel(item, false, window, cx);
        let id = PanelId::from(panel.entity_id());
        let target = match (sibling_group, pr_node) {
            (Some(node), _) => Some(InsertTarget::Tabs {
                node,
                ix: None,
                activate: true,
            }),
            (None, Some(node)) => Some(InsertTarget::Split {
                node,
                placement: Placement::Right,
                size: Some(px(SESSION_SPLIT_WIDTH)),
            }),
            (None, None) => None,
        };
        if let Some(target) = target {
            self.dock_area
                .update(cx, |area, cx| area.move_panel(id, target, window, cx));
        }
        self.reveal_center_panel(&panel, window, cx);
        panel.read(cx).focus_handle(cx).focus(window, cx);
        self.schedule_save(cx);
        window.refresh();
    }

    pub(super) fn on_review_with_ai(
        &mut self,
        _: &ReviewWithAi,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(view) = self.active_pr_view(window, cx) {
            view.update(cx, |view, cx| view.open_ai_review(window, cx));
        }
    }

    pub(super) fn on_agents_event(
        &mut self,
        _: &Entity<AgentSessions>,
        event: &AgentsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let AgentsEvent::SessionStarted { session_id, pr } = event {
            self.open_session_for(session_id.clone(), Some(pr.clone()), window, cx);
        }
    }
}
