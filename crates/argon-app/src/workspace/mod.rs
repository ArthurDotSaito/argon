mod agents;
mod docks;
mod palette_slot;
mod tabs;

use std::collections::HashMap;

use chrono::Utc;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::dock::{DockArea, DockSkin, PanelId, PanelStyle};
use gpui_kit::{
    Context, Entity, EntityId, FocusHandle, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, Subscription, Window, div,
};

pub use self::agents::SessionPlacement;
pub use self::docks::DockPosition;
use self::palette_slot::PaletteSlot;
use crate::actions::{CheckForUpdates, GoToAgents, GoToView, OpenSettings, StartReview, SyncNow};
use crate::agents::{AgentSessions, SessionFilter};
use crate::bottom;
use crate::center::{CenterContentFactory, CenterItem};
use crate::inbox::{InboxEvent, InboxModel, InboxView};
use crate::keymap::WORKSPACE_CONTEXT;
use crate::layout_store::{LAYOUT_VERSION, LayoutSaver};
use crate::pr::PrView;
use crate::settings::SettingsView;
use crate::status_bar::WorkspaceStatusBar;
use crate::sync_status::SyncStatus;
use crate::title_bar::{self, AppTitleBar};
use crate::update;

const DOCK_AREA_ID: &str = "argon-workspace";

pub struct Workspace {
    dock_area: Entity<DockArea>,
    content_factory: CenterContentFactory,
    focus_handle: FocusHandle,
    layout_saver: LayoutSaver,
    center_subscriptions: HashMap<EntityId, Subscription>,
    last_focused: Option<PanelId>,
    sync_status: Entity<SyncStatus>,
    inbox: Entity<InboxModel>,
    palette: Option<PaletteSlot>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(
        content_factory: CenterContentFactory,
        sync_status: Entity<SyncStatus>,
        inbox: Entity<InboxModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let job_log = bottom::init(cx);
        docks::register_panels(content_factory.clone(), inbox.clone(), cx);
        let (dock_area, skin) = DockSkin::dock_area(DOCK_AREA_ID, Some(LAYOUT_VERSION), window, cx);
        skin.set_panel_style(PanelStyle::TabBar, cx);
        skin.set_toggle_button_visible(false, cx);
        skin.set_close_button_visible(true, cx);
        let dock_subscription = cx.subscribe(&dock_area, Self::on_dock_event);
        let sync_subscription = cx.observe(&sync_status, |_, _, cx| cx.notify());
        let inbox_observation = cx.observe(&inbox, |_, _, cx| cx.notify());
        let update_observation = cx.observe(&update::entity(cx), |_, _, cx| cx.notify());
        let inbox_subscription = cx.subscribe_in(&inbox, window, Self::on_inbox_event);
        let job_log_subscription = cx.subscribe_in(&job_log, window, Self::on_job_log_event);

        let workspace = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            workspace
                .update(cx, |workspace, cx| {
                    workspace.save_now(cx);
                    workspace.close_requested(cx)
                })
                .unwrap_or(true)
        });

        let mut this = Self {
            dock_area,
            content_factory,
            focus_handle: cx.focus_handle(),
            layout_saver: LayoutSaver::default(),
            center_subscriptions: HashMap::new(),
            last_focused: None,
            sync_status,
            inbox,
            palette: None,
            _subscriptions: vec![
                dock_subscription,
                sync_subscription,
                inbox_observation,
                update_observation,
                inbox_subscription,
                job_log_subscription,
            ],
        };
        Self::register_global(cx);
        if let Some(sessions) = AgentSessions::global(cx) {
            let subscription = cx.subscribe_in(&sessions, window, Self::on_agents_event);
            this._subscriptions.push(subscription);
            this._subscriptions
                .push(cx.observe(&sessions, |_, _, cx| cx.notify()));
        }
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

    pub fn show_view(&mut self, view: InboxView, cx: &mut Context<Self>) {
        self.inbox.update(cx, |inbox, cx| inbox.show_view(view, cx));
    }

    fn on_go_to_view(&mut self, action: &GoToView, _: &mut Window, cx: &mut Context<Self>) {
        self.show_view(action.0, cx);
    }

    fn on_go_to_agents(&mut self, _: &GoToAgents, window: &mut Window, cx: &mut Context<Self>) {
        self.show_agents(SessionFilter::All, window, cx);
    }

    fn on_sync_now(&mut self, _: &SyncNow, _: &mut Window, cx: &mut Context<Self>) {
        title_bar::request_sync(cx);
    }

    fn on_start_review(&mut self, _: &StartReview, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pr) = self.active_session_pr(window, cx) {
            self.start_review_of(pr, window, cx);
            return;
        }
        let Some(panel) = self.active_center_panel(window, cx) else {
            return;
        };
        let Ok(view) = panel.read(cx).view().clone().downcast::<PrView>() else {
            return;
        };
        view.update(cx, |view, cx| view.start_review(window, cx));
    }

    fn on_open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.open_item(CenterItem::Settings, false, window, cx);
    }

    pub fn show_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_item(CenterItem::Settings, false, window, cx);
        let settings = self
            .center_panels(cx)
            .into_iter()
            .find(|panel| panel.read(cx).item() == &CenterItem::Settings)
            .and_then(|panel| {
                panel
                    .read(cx)
                    .view()
                    .clone()
                    .downcast::<SettingsView>()
                    .ok()
            });
        if let Some(settings) = settings {
            settings.update(cx, |settings, cx| settings.show_about(window, cx));
        }
    }

    fn on_check_for_updates(
        &mut self,
        _: &CheckForUpdates,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        update::entity(cx).update(cx, |model, cx| model.check(cx));
        self.show_about(window, cx);
    }

    fn on_inbox_event(
        &mut self,
        _: &Entity<InboxModel>,
        event: &InboxEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InboxEvent::OpenPr { key, preview } => {
                self.open_item(CenterItem::Pr(key.clone()), *preview, window, cx);
            }
            InboxEvent::OpenIssue { key, preview } => {
                self.open_item(CenterItem::Issue(key.clone()), *preview, window, cx);
            }
            InboxEvent::ShowList => self.open_item(CenterItem::Inbox, false, window, cx),
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let docks = self.dock_open_states(cx);
        let needs_you = self.inbox.read(cx).count(InboxView::NeedsYou);
        let agents = AgentSessions::global(cx)
            .map(|sessions| sessions.read(cx).activity(cx))
            .unwrap_or_default();
        let update_label = update::entity(cx).read(cx).status_bar_label();
        let status_bar =
            WorkspaceStatusBar::new(cx.weak_entity(), docks, needs_you, agents, update_label);
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
            .on_action(cx.listener(Self::on_open_palette))
            .on_action(cx.listener(Self::on_go_to_view))
            .on_action(cx.listener(Self::on_go_to_agents))
            .on_action(cx.listener(Self::on_sync_now))
            .on_action(cx.listener(Self::on_open_settings))
            .on_action(cx.listener(Self::on_check_for_updates))
            .on_action(cx.listener(Self::on_start_review))
            .on_action(cx.listener(Self::on_review_with_ai))
            .child(title_bar)
            .child(div().flex_1().min_h_0().child(self.dock_area.clone()))
            .child(status_bar)
            .children(self.palette.as_ref().map(|slot| slot.view.clone()))
    }
}
