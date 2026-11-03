use argon_core::{PrKey, Side};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::{App, Context, Entity, Global, WeakEntity, Window};

use super::{DockPosition, Workspace};
use crate::actions::ReviewWithAi;
use crate::agents::{AgentSessions, AgentsEvent};
use crate::center::CenterItem;
use crate::pr::PrView;

const OPEN_A_PR: &str = "Abra a aba de um PR para revisar com IA";

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

    pub fn review_active_pr_with_ai(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.active_pr_view(window, cx) {
            Some(view) => view.update(cx, |view, cx| view.open_ai_review(window, cx)),
            None => window.push_notification(Notification::info(OPEN_A_PR), cx),
        }
    }

    pub fn reveal_agents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reveal_dock(DockPosition::Right, window, cx);
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
        let AgentsEvent::SessionStarted { .. } = event;
        self.reveal_agents(window, cx);
    }
}
