use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::dock::{
    DockArea, DockEvent, DockLayout, DockPlacement, DockSkin, PaneRef, PanelId, PanelStyle,
    panel_handle, register_panel,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EntityId, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, Subscription,
    Task, Window, div, px,
};

use crate::actions::{CloseActiveItem, ToggleBottomDock, ToggleLeftDock, ToggleRightDock};
use crate::center::{CENTER_PANEL, CenterItem, CenterPanel, CenterPanelEvent};
use crate::keymap::WORKSPACE_CONTEXT;
use crate::layout_store;
use crate::placeholders::{DockPlaceholder, DockSlot};
use crate::status_bar::WorkspaceStatusBar;
use crate::title_bar::AppTitleBar;

const DOCK_AREA_ID: &str = "argon-workspace";
const LAYOUT_VERSION: usize = 1;
const SAVE_DEBOUNCE: Duration = Duration::from_millis(300);
const SAMPLE_PR: &str = "orbita-health/scheduling-api#482";
const ACCOUNT_PLACEHOLDER: &str = "@arthursaito";
const SYNC_PLACEHOLDER: &str = "sincronizado há 2 min";
const NEEDS_YOU_PLACEHOLDER: usize = 4;
const RUNNING_AGENTS_PLACEHOLDER: usize = 1;

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
    focus_handle: FocusHandle,
    pending_save: Option<Task<()>>,
    center_subscriptions: HashMap<EntityId, Subscription>,
    _dock_subscription: Subscription,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        register_panels(cx);
        let (dock_area, skin) = DockSkin::dock_area(DOCK_AREA_ID, Some(LAYOUT_VERSION), window, cx);
        skin.set_panel_style(PanelStyle::TabBar, cx);
        skin.set_toggle_button_visible(false, cx);
        skin.set_close_button_visible(true, cx);
        let dock_subscription = cx.subscribe(&dock_area, Self::on_dock_event);

        let workspace = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            _ = workspace.update(cx, |workspace, cx| workspace.save_now(cx));
            true
        });

        let mut this = Self {
            dock_area,
            focus_handle: cx.focus_handle(),
            pending_save: None,
            center_subscriptions: HashMap::new(),
            _dock_subscription: dock_subscription,
        };
        if !this.restore_layout(window, cx) {
            this.reset_layout(window, cx);
        }
        this.watch_center_panels(cx);
        this.focus_handle.focus(window, cx);
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
                    panel.update(cx, |panel, cx| panel.replace(item.clone(), cx));
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
        let panel = cx.new(|cx| CenterPanel::new(item, preview, cx));
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
        let Some(state) = layout_store::load() else {
            return false;
        };
        if state.version != Some(LAYOUT_VERSION) {
            return false;
        }
        let loaded = self
            .dock_area
            .update(cx, |area, cx| area.load(state, window, cx));
        if let Err(error) = loaded {
            eprintln!("failed to restore layout: {error:#}");
            return false;
        }
        self.layout_is_complete(cx)
    }

    fn layout_is_complete(&self, cx: &App) -> bool {
        let area = self.dock_area.read(cx);
        let docks_present = DockPosition::ALL
            .iter()
            .all(|dock| area.has_dock(dock.placement()));
        docks_present && !self.center_panels(cx).is_empty()
    }

    fn reset_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dock_area.update(cx, |area, cx| {
            area.set_center(DockLayout::tabs(), window, cx);
            for dock in DockPosition::ALL {
                let slot = dock.slot();
                let panel = panel_handle(cx.new(|cx| DockPlaceholder::new(slot, cx)));
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
        self.open_item(CenterItem::Pr(SAMPLE_PR.into()), true, window, cx);
    }

    fn on_dock_event(&mut self, _: Entity<DockArea>, event: &DockEvent, cx: &mut Context<Self>) {
        if let DockEvent::LayoutChanged = event {
            self.watch_center_panels(cx);
            self.schedule_save(cx);
            cx.notify();
        }
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.pending_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            let Ok(state) = this.update(cx, |this, cx| this.dock_area.read(cx).dump(cx)) else {
                return;
            };
            let saved = cx
                .background_spawn(async move { layout_store::save(&state) })
                .await;
            if let Err(error) = saved {
                eprintln!("failed to save layout: {error}");
            }
        }));
    }

    fn save_now(&mut self, cx: &mut Context<Self>) {
        self.pending_save = None;
        let state = self.dock_area.read(cx).dump(cx);
        if let Err(error) = layout_store::save(&state) {
            eprintln!("failed to save layout: {error}");
        }
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

fn register_panels(cx: &mut App) {
    register_panel(cx, CENTER_PANEL, |context, _, cx| {
        let info = context.info().clone();
        panel_handle(cx.new(|cx| CenterPanel::restore(&info, cx)))
    });
    for slot in DockSlot::ALL {
        register_panel(cx, slot.panel_name(), move |_, _, cx| {
            panel_handle(cx.new(|cx| DockPlaceholder::new(slot, cx)))
        });
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let docks = self.dock_open_states(cx);
        let status_bar = WorkspaceStatusBar::new(
            cx.weak_entity(),
            docks,
            NEEDS_YOU_PLACEHOLDER,
            RUNNING_AGENTS_PLACEHOLDER,
        );
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
            .child(AppTitleBar::new(ACCOUNT_PLACEHOLDER, SYNC_PLACEHOLDER))
            .child(div().flex_1().min_h_0().child(self.dock_area.clone()))
            .child(status_bar)
    }
}
