use argon_core::RepoKey;
use gpui_kit::component::dock::{DockPlacement, InsertTarget, NodeId, PanelId, panel_handle};
use gpui_kit::{App, AppContext as _, Context, Entity, Focusable as _, Window};

use super::Workspace;
use crate::actions::CloseActiveItem;
use crate::agents::SessionCatalog;
use crate::center::{CenterItem, CenterPanel, CenterPanelEvent};
use crate::dock_items;
use crate::project::ProjectView;

impl Workspace {
    pub fn open_item(
        &mut self,
        item: CenterItem,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panels = self.center_panels(cx);
        let existing = panels.iter().find(|panel| panel.read(cx).item() == &item);
        let target = match (existing, preview) {
            (Some(panel), false) => {
                panel.update(cx, |panel, cx| panel.pin(cx));
                Some(panel.clone())
            }
            (Some(panel), true) => Some(panel.clone()),
            (None, true) => panels
                .iter()
                .find(|panel| {
                    panel.read(cx).is_preview()
                        && dock_items::is_visible(
                            self.dock_area.read(cx),
                            PanelId::from(panel.entity_id()),
                        )
                })
                .cloned()
                .inspect(|panel| {
                    panel.update(cx, |panel, cx| panel.replace(item.clone(), window, cx));
                    self.schedule_save(cx);
                }),
            (None, false) => None,
        };

        let panel = match target {
            Some(panel) => {
                self.reveal_center_panel(&panel, window, cx);
                panel
            }
            None => self.add_center_panel(item, preview, window, cx),
        };
        panel.read(cx).focus_handle(cx).focus(window, cx);
        window.refresh();
    }

    pub fn open_project_architecture(
        &mut self,
        key: RepoKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = CenterItem::Project(key);
        self.open_item(item.clone(), false, window, cx);
        let view = self
            .center_panels(cx)
            .into_iter()
            .find(|panel| panel.read(cx).item() == &item)
            .and_then(|panel| panel.read(cx).view().clone().downcast::<ProjectView>().ok());
        if let Some(view) = view {
            view.update(cx, |view, cx| view.show_architecture(cx));
        }
    }

    pub fn close_item(&mut self, item: &CenterItem, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self
            .center_panels(cx)
            .into_iter()
            .find(|panel| panel.read(cx).item() == item)
        else {
            return;
        };
        if self.last_focused == Some(PanelId::from(panel.entity_id())) {
            self.last_focused = None;
        }
        self.dock_area
            .update(cx, |area, cx| area.remove_panel(panel, window, cx));
    }

    pub fn close_active_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.active_center_panel(window, cx) else {
            return;
        };
        if self.last_focused == Some(PanelId::from(panel.entity_id())) {
            self.last_focused = None;
        }
        self.dock_area
            .update(cx, |area, cx| area.remove_panel(panel, window, cx));
        self.focus_handle.focus(window, cx);
    }

    pub(super) fn add_center_panel(
        &mut self,
        item: CenterItem,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<CenterPanel> {
        let is_session = matches!(item, CenterItem::AgentSession(_));
        let factory = self.content_factory.clone();
        let panel = cx.new(|cx| CenterPanel::new(item, preview, factory, window, cx));
        let id = PanelId::from(panel.entity_id());
        let handle = panel_handle(panel.clone());
        let group = self
            .last_focused_panel(cx)
            .and_then(|focused| dock_items::locate(self.dock_area.read(cx), focused))
            .map(|(_, node)| node)
            .and_then(|node| {
                if is_session || !self.is_session_group(node, cx) {
                    Some(node)
                } else {
                    self.first_non_session_group(cx)
                }
            });
        self.dock_area.update(cx, |area, cx| {
            area.add_panel_view(handle, DockPlacement::Center, None, window, cx);
            let added_to = dock_items::locate(area, id).map(|(_, node)| node);
            if let Some(node) = group.filter(|node| Some(*node) != added_to) {
                let target = InsertTarget::Tabs {
                    node,
                    ix: None,
                    activate: true,
                };
                area.move_panel(id, target, window, cx);
            }
        });
        self.watch_center_panels(cx);
        panel
    }

    fn is_session_group(&self, node: NodeId, cx: &App) -> bool {
        let area = self.dock_area.read(cx);
        let panels = self.center_panels(cx);
        let members = dock_items::group_panels(area, node);
        !members.is_empty()
            && members.iter().all(|member| {
                panels.iter().any(|panel| {
                    PanelId::from(panel.entity_id()) == *member
                        && matches!(panel.read(cx).item(), CenterItem::AgentSession(_))
                })
            })
    }

    fn first_non_session_group(&self, cx: &App) -> Option<NodeId> {
        let area = self.dock_area.read(cx);
        self.center_panels(cx)
            .iter()
            .filter_map(|panel| dock_items::locate(area, PanelId::from(panel.entity_id())))
            .filter(|(placement, _)| *placement == DockPlacement::Center)
            .map(|(_, node)| node)
            .find(|node| !self.is_session_group(*node, cx))
    }

    pub(super) fn reveal_center_panel(
        &mut self,
        panel: &Entity<CenterPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = PanelId::from(panel.entity_id());
        self.dock_area.update(cx, |area, cx| {
            if let Some((placement, _)) = dock_items::locate(area, id)
                && placement != DockPlacement::Center
                && !area.is_dock_open(placement)
            {
                area.toggle_dock(placement, window, cx);
            }
            area.select_panel(id, window, cx);
        });
    }

    pub(super) fn center_panels(&self, cx: &App) -> Vec<Entity<CenterPanel>> {
        dock_items::center_panels(self.dock_area.read(cx))
    }

    pub(super) fn last_focused_panel(&self, cx: &App) -> Option<PanelId> {
        let id = self.last_focused?;
        dock_items::is_visible(self.dock_area.read(cx), id).then_some(id)
    }

    pub(super) fn active_center_panel(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<Entity<CenterPanel>> {
        let panels = self.center_panels(cx);
        let area = self.dock_area.read(cx);
        let focused = panels.iter().find(|panel| {
            panel.read(cx).focus_handle(cx).contains_focused(window, cx)
                && dock_items::is_visible(area, PanelId::from(panel.entity_id()))
        });
        if let Some(panel) = focused {
            return Some(panel.clone());
        }

        let active = self
            .last_focused_panel(cx)
            .and_then(|panel| dock_items::active_in_group_of(area, panel))
            .or_else(|| dock_items::first_center_active(area))?;
        panels
            .into_iter()
            .find(|panel| PanelId::from(panel.entity_id()) == active)
    }

    pub(super) fn watch_center_panels(&mut self, cx: &mut Context<Self>) {
        let panels = self.center_panels(cx);
        if !panels
            .iter()
            .any(|panel| *panel.read(cx).item() == CenterItem::Agents)
            && let Some(catalog) = SessionCatalog::global(cx)
        {
            catalog.update(cx, |catalog, cx| catalog.set_agents_active(false, cx));
        }
        self.center_subscriptions
            .retain(|id, _| panels.iter().any(|panel| panel.entity_id() == *id));
        for panel in panels {
            self.center_subscriptions
                .entry(panel.entity_id())
                .or_insert_with(|| cx.subscribe(&panel, Self::on_center_panel_event));
        }
    }

    pub(super) fn on_center_panel_event(
        &mut self,
        panel: Entity<CenterPanel>,
        event: &CenterPanelEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            CenterPanelEvent::Pinned => {
                self.schedule_save(cx);
                cx.notify();
            }
            CenterPanelEvent::Focused => {
                self.last_focused = Some(PanelId::from(panel.entity_id()));
                let agents_active = match panel.read(cx).item() {
                    CenterItem::Agents => Some(true),
                    CenterItem::Inbox => Some(false),
                    _ => None,
                };
                if let (Some(active), Some(catalog)) = (agents_active, SessionCatalog::global(cx)) {
                    catalog.update(cx, |catalog, cx| catalog.set_agents_active(active, cx));
                }
            }
        }
    }

    pub(super) fn on_close_active_item(
        &mut self,
        _: &CloseActiveItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_active_item(window, cx);
    }
}
