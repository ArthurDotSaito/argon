use std::sync::Arc;

use gpui_kit::component::dock::{
    BasePanelView, DockArea, DockEvent, DockLayout, DockPlacement, panel_handle, register_panel,
};
use gpui_kit::{App, AppContext as _, Context, Entity, Window, px};

use super::Workspace;
use crate::actions::{ToggleBottomDock, ToggleLeftDock, ToggleRightDock};
use crate::bottom::{self, JobLogEvent, JobLogModel, LogPanel};
use crate::center::{CENTER_PANEL, CenterContentFactory, CenterItem, CenterPanel};
use crate::inbox::{InboxModel, ViewsPanel};
use crate::layout_store;
use crate::placeholders::{DockPlaceholder, DockSlot};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockPosition {
    Left,
    Bottom,
    Right,
}

impl DockPosition {
    pub const ALL: [Self; 3] = [Self::Left, Self::Bottom, Self::Right];

    fn placement(self) -> DockPlacement {
        match self {
            Self::Left => DockPlacement::Left,
            Self::Bottom => DockPlacement::Bottom,
            Self::Right => DockPlacement::Right,
        }
    }

    fn slot(self) -> DockSlot {
        match self {
            Self::Left => DockSlot::Views,
            Self::Bottom => DockSlot::Log,
            Self::Right => DockSlot::Aside,
        }
    }

    fn default_size(self) -> f32 {
        match self {
            Self::Left => 240.,
            Self::Bottom => 180.,
            Self::Right => 340.,
        }
    }
}

impl Workspace {
    pub fn toggle_dock(&mut self, dock: DockPosition, window: &mut Window, cx: &mut Context<Self>) {
        self.dock_area.update(cx, |area, cx| {
            area.toggle_dock(dock.placement(), window, cx)
        });
        cx.notify();
    }

    pub(super) fn on_job_log_event(
        &mut self,
        model: &Entity<JobLogModel>,
        event: &JobLogEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let JobLogEvent::Reveal = event;
        self.dock_area.update(cx, |area, cx| {
            if bottom::locate_log_panel(area).is_none() {
                let panel = cx.new(|cx| LogPanel::new(model.clone(), window, cx));
                area.add_panel_view(panel_handle(panel), DockPlacement::Bottom, None, window, cx);
            }
            let Some((placement, id)) = bottom::locate_log_panel(area) else {
                return;
            };
            if placement != DockPlacement::Center && !area.is_dock_open(placement) {
                area.toggle_dock(placement, window, cx);
            }
            area.select_panel(id, window, cx);
        });
        cx.notify();
    }

    pub(super) fn dock_open_states(&self, cx: &App) -> Vec<(DockPosition, bool)> {
        let area = self.dock_area.read(cx);
        DockPosition::ALL
            .iter()
            .map(|dock| (*dock, area.is_dock_open(dock.placement())))
            .collect()
    }

    pub(super) fn restore_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let docks = DockPosition::ALL.map(DockPosition::placement);
        layout_store::restore(&self.dock_area, &docks, window, cx)
    }

    pub(super) fn reset_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let inbox = self.inbox.clone();
        self.dock_area.update(cx, |area, cx| {
            area.set_center(DockLayout::tabs(), window, cx);
            for dock in DockPosition::ALL {
                let panel = dock_panel(dock.slot(), &inbox, window, cx);
                area.set_dock(
                    dock.placement(),
                    DockLayout::tabs().panel_view(panel, cx),
                    window,
                    cx,
                );
                area.set_dock_size(dock.placement(), px(dock.default_size()), window, cx);
            }
            let right = DockPosition::Right.placement();
            if area.is_dock_open(right) {
                area.toggle_dock(right, window, cx);
            }
        });
        self.open_item(CenterItem::Inbox, false, window, cx);
    }

    pub(super) fn on_dock_event(
        &mut self,
        _: Entity<DockArea>,
        event: &DockEvent,
        cx: &mut Context<Self>,
    ) {
        if let DockEvent::LayoutChanged = event {
            self.watch_center_panels(cx);
            self.schedule_save(cx);
            cx.notify();
        }
    }

    pub(super) fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.layout_saver.schedule(self.dock_area.downgrade(), cx);
    }

    pub(super) fn save_now(&mut self, cx: &mut Context<Self>) {
        self.layout_saver.save_now(&self.dock_area, cx);
    }

    pub(super) fn on_toggle_left_dock(
        &mut self,
        _: &ToggleLeftDock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_dock(DockPosition::Left, window, cx);
    }

    pub(super) fn on_toggle_right_dock(
        &mut self,
        _: &ToggleRightDock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_dock(DockPosition::Right, window, cx);
    }

    pub(super) fn on_toggle_bottom_dock(
        &mut self,
        _: &ToggleBottomDock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_dock(DockPosition::Bottom, window, cx);
    }
}

pub(super) fn register_panels(
    content_factory: CenterContentFactory,
    inbox: Entity<InboxModel>,
    cx: &mut App,
) {
    register_panel(cx, CENTER_PANEL, move |context, window, cx| {
        let info = context.info().clone();
        let factory = content_factory.clone();
        panel_handle(cx.new(|cx| CenterPanel::restore(&info, factory, window, cx)))
    });
    for slot in DockSlot::ALL {
        let inbox = inbox.clone();
        register_panel(cx, slot.panel_name(), move |_, window, cx| {
            dock_panel(slot, &inbox, window, cx)
        });
    }
}

pub(super) fn dock_panel(
    slot: DockSlot,
    inbox: &Entity<InboxModel>,
    window: &mut Window,
    cx: &mut App,
) -> Arc<dyn BasePanelView> {
    match (slot, JobLogModel::global(cx)) {
        (DockSlot::Views, _) => panel_handle(cx.new(|cx| ViewsPanel::new(inbox.clone(), cx))),
        (DockSlot::Log, Some(model)) => panel_handle(cx.new(|cx| LogPanel::new(model, window, cx))),
        (slot, _) => panel_handle(cx.new(|cx| DockPlaceholder::new(slot, cx))),
    }
}
