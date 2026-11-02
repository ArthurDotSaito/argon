use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::dock::{
    BasePanelView, DockArea, DockEvent, DockLayout, DockPlacement, DockSkin, PaneRef, PanelId,
    PanelStyle, panel_handle, register_panel,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EntityId, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, Subscription,
    Window, div, px,
};

use crate::actions::{CloseActiveItem, ToggleBottomDock, ToggleLeftDock, ToggleRightDock};
use crate::center::{
    CENTER_PANEL, CenterContentFactory, CenterItem, CenterPanel, CenterPanelEvent,
};
use crate::inbox::{InboxEvent, InboxModel, InboxView, ViewsPanel};
use crate::keymap::WORKSPACE_CONTEXT;
use crate::layout_store::{self, LAYOUT_VERSION, LayoutSaver};
use crate::placeholders::{DockPlaceholder, DockSlot};
use crate::status_bar::WorkspaceStatusBar;
use crate::sync_status::SyncStatus;
use crate::title_bar::AppTitleBar;

const DOCK_AREA_ID: &str = "argon-workspace";

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
            Self::Right => DockSlot::Agents,
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

pub struct Workspace {
    dock_area: Entity<DockArea>,
    content_factory: CenterContentFactory,
    focus_handle: FocusHandle,
    layout_saver: LayoutSaver,
    center_subscriptions: HashMap<EntityId, Subscription>,
    sync_status: Entity<SyncStatus>,
    inbox: Entity<InboxModel>,
    _subscriptions: [Subscription; 4],
}

impl Workspace {
    pub fn new(
        content_factory: CenterContentFactory,
        sync_status: Entity<SyncStatus>,
        inbox: Entity<InboxModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        register_panels(content_factory.clone(), inbox.clone(), cx);
        let (dock_area, skin) = DockSkin::dock_area(DOCK_AREA_ID, Some(LAYOUT_VERSION), window, cx);
        skin.set_panel_style(PanelStyle::TabBar, cx);
        skin.set_toggle_button_visible(false, cx);
        skin.set_close_button_visible(true, cx);
        let dock_subscription = cx.subscribe(&dock_area, Self::on_dock_event);
        let sync_subscription = cx.observe(&sync_status, |_, _, cx| cx.notify());
        let inbox_observation = cx.observe(&inbox, |_, _, cx| cx.notify());
        let inbox_subscription = cx.subscribe_in(&inbox, window, Self::on_inbox_event);

        let workspace = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            _ = workspace.update(cx, |workspace, cx| workspace.save_now(cx));
            true
        });

        let mut this = Self {
            dock_area,
            content_factory,
            focus_handle: cx.focus_handle(),
            layout_saver: LayoutSaver::default(),
            center_subscriptions: HashMap::new(),
            sync_status,
            inbox,
            _subscriptions: [
                dock_subscription,
                sync_subscription,
                inbox_observation,
                inbox_subscription,
            ],
        };
        if !this.restore_layout(window, cx) {
            this.reset_layout(window, cx);
        } else if this.center_panels(cx).is_empty() {
            this.open_item(CenterItem::Inbox, false, window, cx);
        }
        this.watch_center_panels(cx);
        match this.active_center_panel(window, cx) {
            Some(panel) => panel.read(cx).focus_handle(cx).focus(window, cx),
            None => this.focus_handle.focus(window, cx),
        }
        this
    }

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
                .find(|panel| panel.read(cx).is_preview())
                .cloned()
                .inspect(|panel| {
                    panel.update(cx, |panel, cx| panel.replace(item.clone(), window, cx));
                    self.schedule_save(cx);
                }),
            (None, false) => None,
        };

        let panel = match target {
            Some(panel) => {
                self.select_center_panel(&panel, window, cx);
                panel
            }
            None => self.add_center_panel(item, preview, window, cx),
        };
        panel.read(cx).focus_handle(cx).focus(window, cx);
        window.refresh();
    }

    pub fn toggle_dock(&mut self, dock: DockPosition, window: &mut Window, cx: &mut Context<Self>) {
        self.dock_area.update(cx, |area, cx| {
            area.toggle_dock(dock.placement(), window, cx)
        });
        cx.notify();
    }

    pub fn show_view(&mut self, view: InboxView, cx: &mut Context<Self>) {
        self.inbox.update(cx, |inbox, cx| inbox.show_view(view, cx));
    }

    fn on_inbox_event(
        &mut self,
        _: &Entity<InboxModel>,
        event: &InboxEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InboxEvent::OpenPr { id, preview } => {
                self.open_item(CenterItem::Pr(id.clone()), *preview, window, cx);
            }
            InboxEvent::ShowList => self.open_item(CenterItem::Inbox, false, window, cx),
        }
    }

    pub fn close_active_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.active_center_panel(window, cx) else {
            return;
        };
        self.dock_area
            .update(cx, |area, cx| area.remove_panel(panel, window, cx));
        self.focus_handle.focus(window, cx);
    }

    fn add_center_panel(
        &mut self,
        item: CenterItem,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<CenterPanel> {
        let factory = self.content_factory.clone();
        let panel = cx.new(|cx| CenterPanel::new(item, preview, factory, window, cx));
        let handle = panel_handle(panel.clone());
        self.dock_area.update(cx, |area, cx| {
            area.add_panel_view(handle, DockPlacement::Center, None, window, cx)
        });
        self.watch_center_panels(cx);
        panel
    }

    fn select_center_panel(
        &mut self,
        panel: &Entity<CenterPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = PanelId::from(panel.entity_id());
        self.dock_area
            .update(cx, |area, cx| area.select_panel(id, window, cx));
    }

    fn center_panels(&self, cx: &App) -> Vec<Entity<CenterPanel>> {
        let area = self.dock_area.read(cx);
        let Some(tree) = area.layout(DockPlacement::Center) else {
            return Vec::new();
        };
        tree.panels()
            .filter_map(|id| area.panel(id))
            .filter_map(|panel| panel.view().downcast::<CenterPanel>().ok())
            .collect()
    }

    fn active_center_panel(&self, window: &Window, cx: &App) -> Option<Entity<CenterPanel>> {
        let panels = self.center_panels(cx);
        let focused = panels
            .iter()
            .find(|panel| panel.read(cx).focus_handle(cx).contains_focused(window, cx));
        if let Some(panel) = focused {
            return Some(panel.clone());
        }

        let tree = self.dock_area.read(cx).layout(DockPlacement::Center)?;
        let mut active = None;
        tree.root().walk(&mut |node| {
            if let (None, PaneRef::Tabs { panels, active_ix }) = (active, node.kind()) {
                active = panels.get(active_ix).copied();
            }
        });
        let active = active?;
        panels
            .into_iter()
            .find(|panel| PanelId::from(panel.entity_id()) == active)
    }

    fn watch_center_panels(&mut self, cx: &mut Context<Self>) {
        let panels = self.center_panels(cx);
        self.center_subscriptions
            .retain(|id, _| panels.iter().any(|panel| panel.entity_id() == *id));
        for panel in panels {
            self.center_subscriptions
                .entry(panel.entity_id())
                .or_insert_with(|| {
                    cx.subscribe(&panel, |this, _, _: &CenterPanelEvent, cx| {
                        this.schedule_save(cx);
                        cx.notify();
                    })
                });
        }
    }

    fn dock_open_states(&self, cx: &App) -> Vec<(DockPosition, bool)> {
        let area = self.dock_area.read(cx);
        DockPosition::ALL
            .iter()
            .map(|dock| (*dock, area.is_dock_open(dock.placement())))
            .collect()
    }

    fn restore_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let docks = DockPosition::ALL.map(DockPosition::placement);
        layout_store::restore(&self.dock_area, &docks, window, cx)
    }

    fn reset_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let inbox = self.inbox.clone();
        self.dock_area.update(cx, |area, cx| {
            area.set_center(DockLayout::tabs(), window, cx);
            for dock in DockPosition::ALL {
                let panel = dock_panel(dock.slot(), &inbox, cx);
                area.set_dock(
                    dock.placement(),
                    DockLayout::tabs().panel_view(panel, cx),
                    window,
                    cx,
                );
                area.set_dock_size(dock.placement(), px(dock.default_size()), window, cx);
            }
        });
        self.open_item(CenterItem::Inbox, false, window, cx);
    }

    fn on_dock_event(&mut self, _: Entity<DockArea>, event: &DockEvent, cx: &mut Context<Self>) {
        if let DockEvent::LayoutChanged = event {
            self.watch_center_panels(cx);
            self.schedule_save(cx);
            cx.notify();
        }
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.layout_saver.schedule(self.dock_area.downgrade(), cx);
    }

    fn save_now(&mut self, cx: &mut Context<Self>) {
        self.layout_saver.save_now(&self.dock_area, cx);
    }

    fn on_toggle_left_dock(
        &mut self,
        _: &ToggleLeftDock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_dock(DockPosition::Left, window, cx);
    }

    fn on_toggle_right_dock(
        &mut self,
        _: &ToggleRightDock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_dock(DockPosition::Right, window, cx);
    }

    fn on_toggle_bottom_dock(
        &mut self,
        _: &ToggleBottomDock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_dock(DockPosition::Bottom, window, cx);
    }

    fn on_close_active_item(
        &mut self,
        _: &CloseActiveItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_active_item(window, cx);
    }
}

fn register_panels(content_factory: CenterContentFactory, inbox: Entity<InboxModel>, cx: &mut App) {
    register_panel(cx, CENTER_PANEL, move |context, window, cx| {
        let info = context.info().clone();
        let factory = content_factory.clone();
        panel_handle(cx.new(|cx| CenterPanel::restore(&info, factory, window, cx)))
    });
    for slot in DockSlot::ALL {
        let inbox = inbox.clone();
        register_panel(cx, slot.panel_name(), move |_, _, cx| {
            dock_panel(slot, &inbox, cx)
        });
    }
}

fn dock_panel(slot: DockSlot, inbox: &Entity<InboxModel>, cx: &mut App) -> Arc<dyn BasePanelView> {
    match slot {
        DockSlot::Views => panel_handle(cx.new(|cx| ViewsPanel::new(inbox.clone(), cx))),
        slot => panel_handle(cx.new(|cx| DockPlaceholder::new(slot, cx))),
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let docks = self.dock_open_states(cx);
        let needs_you = self.inbox.read(cx).count(InboxView::NeedsYou);
        let status_bar = WorkspaceStatusBar::new(cx.weak_entity(), docks, needs_you);
        let sync_status = self.sync_status.read(cx);
        let account = sync_status
            .login
            .as_ref()
            .map(|login| format!("@{login}").into());
        let title_bar = AppTitleBar::new(account, sync_status.indicator(Utc::now()));
        let theme = cx.theme();

        div()
            .id("workspace")
            .key_context(WORKSPACE_CONTEXT)
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(theme.font_family.clone())
            .text_size(theme.font_size)
            .on_action(cx.listener(Self::on_toggle_left_dock))
            .on_action(cx.listener(Self::on_toggle_right_dock))
            .on_action(cx.listener(Self::on_toggle_bottom_dock))
            .on_action(cx.listener(Self::on_close_active_item))
            .child(title_bar)
            .child(div().flex_1().min_h_0().child(self.dock_area.clone()))
            .child(status_bar)
    }
}
