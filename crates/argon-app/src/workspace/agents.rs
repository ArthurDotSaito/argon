use argon_core::{PrKey, SessionKind, Side};
use gpui_kit::component::Placement;
use gpui_kit::component::dock::{InsertTarget, PanelId};
use gpui_kit::{App, Context, Entity, Focusable as _, Global, WeakEntity, Window, px};

use super::Workspace;
use crate::actions::{PlanWithAgent, ReviewWithAi};
use crate::agents::{AgentSessions, AgentsEvent, SessionCatalog, SessionFilter};
use crate::center::CenterItem;
use crate::dock_items;
use crate::issue::IssueView;
use crate::pr::PrView;
use crate::services::AppServices;

const SESSION_SPLIT_WIDTH: f32 = 420.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPlacement {
    Center,
    BesideSubject,
}

pub fn subject_item(key: PrKey, kind: SessionKind) -> CenterItem {
    match kind {
        SessionKind::Review => CenterItem::Pr(key),
        SessionKind::Plan => CenterItem::Issue(key),
    }
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

    pub fn issue_view(&self, key: &PrKey, cx: &App) -> Option<Entity<IssueView>> {
        let item = CenterItem::Issue(key.clone());
        self.center_panels(cx)
            .into_iter()
            .find(|panel| panel.read(cx).item() == &item)
            .and_then(|panel| panel.read(cx).view().clone().downcast::<IssueView>().ok())
    }

    fn active_issue_view(&self, window: &Window, cx: &App) -> Option<Entity<IssueView>> {
        let panel = self.active_center_panel(window, cx)?;
        panel.read(cx).view().clone().downcast::<IssueView>().ok()
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

    pub fn plan_issue_with_agent(
        &mut self,
        issue: PrKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_item(CenterItem::Issue(issue.clone()), false, window, cx);
        if let Some(view) = self.issue_view(&issue, cx) {
            view.update(cx, |view, cx| view.open_agent_plan(window, cx));
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
        let beside = match placement {
            SessionPlacement::Center => None,
            SessionPlacement::BesideSubject => SessionCatalog::global(cx)
                .and_then(|catalog| catalog.read(cx).subject_of(&session_id, cx))
                .map(|(key, kind)| subject_item(key, kind)),
        };
        self.open_session_for(session_id, beside, window, cx);
    }

    fn open_session_for(
        &mut self,
        session_id: String,
        beside: Option<CenterItem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = CenterItem::AgentSession(session_id);
        let already_open = self
            .center_panels(cx)
            .iter()
            .any(|panel| panel.read(cx).item() == &item);
        let Some(subject) = beside.filter(|_| !already_open) else {
            self.open_item(item, false, window, cx);
            return;
        };
        self.open_item(subject.clone(), false, window, cx);
        let area = self.dock_area.read(cx);
        let subject_node = self
            .center_panels(cx)
            .iter()
            .find(|panel| panel.read(cx).item() == &subject)
            .and_then(|panel| dock_items::locate(area, PanelId::from(panel.entity_id())))
            .map(|(_, node)| node);
        let session_ids: Vec<PanelId> = self
            .center_panels(cx)
            .iter()
            .filter(|panel| matches!(panel.read(cx).item(), CenterItem::AgentSession(_)))
            .map(|panel| PanelId::from(panel.entity_id()))
            .collect();
        let sibling_group = session_ids
            .iter()
            .filter_map(|id| dock_items::locate(area, *id))
            .map(|(_, node)| node)
            .find(|node| {
                Some(*node) != subject_node
                    && dock_items::group_panels(area, *node)
                        .iter()
                        .all(|id| session_ids.contains(id))
            });
        let subject_panel = self
            .center_panels(cx)
            .iter()
            .find(|panel| panel.read(cx).item() == &subject)
            .map(|panel| PanelId::from(panel.entity_id()));
        let panel = self.add_center_panel(item, false, window, cx);
        let id = PanelId::from(panel.entity_id());
        let target = match (sibling_group, subject_node) {
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
            self.dock_area.update(cx, |area, cx| {
                area.move_panel(id, target, window, cx);
                if let Some(subject_panel) = subject_panel {
                    area.select_panel(subject_panel, window, cx);
                }
            });
        }
        self.reveal_center_panel(&panel, window, cx);
        panel.read(cx).focus_handle(cx).focus(window, cx);
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
        } else if let Some(pr) = self.active_session_pr(window, cx) {
            self.review_pr_with_ai(pr, window, cx);
        }
    }

    pub(super) fn on_plan_with_agent(
        &mut self,
        _: &PlanWithAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(view) = self.active_issue_view(window, cx) {
            view.update(cx, |view, cx| view.open_agent_plan(window, cx));
        } else if let Some((issue, SessionKind::Plan)) = self.active_session_subject(window, cx) {
            self.plan_issue_with_agent(issue, window, cx);
        }
    }

    fn active_session_subject(&self, window: &Window, cx: &App) -> Option<(PrKey, SessionKind)> {
        let panel = self.active_center_panel(window, cx)?;
        let CenterItem::AgentSession(id) = panel.read(cx).item() else {
            return None;
        };
        SessionCatalog::global(cx)?.read(cx).subject_of(id, cx)
    }

    pub(super) fn active_session_pr(&self, window: &Window, cx: &App) -> Option<PrKey> {
        match self.active_session_subject(window, cx)? {
            (pr, SessionKind::Review) => Some(pr),
            (_, SessionKind::Plan) => None,
        }
    }

    pub(super) fn start_review_of(
        &mut self,
        pr: PrKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_item(CenterItem::Pr(pr.clone()), false, window, cx);
        if let Some(view) = self.pr_view(&pr, cx) {
            view.update(cx, |view, cx| view.start_review(window, cx));
        }
    }

    pub(super) fn on_agents_event(
        &mut self,
        _: &Entity<AgentSessions>,
        event: &AgentsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let AgentsEvent::SessionStarted {
            session_id,
            pr,
            kind,
        } = event
        {
            let beside = subject_item(pr.clone(), *kind);
            self.open_session_for(session_id.clone(), Some(beside), window, cx);
        }
    }
}
