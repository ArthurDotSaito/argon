#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod agents;
mod app_id;
mod assets;
mod bottom;
mod center;
mod config_load;
mod config_watch;
mod content;
mod dock_items;
mod inbox;
mod keymap;
mod layout_store;
mod notify;
mod palette;
mod placeholders;
mod pr;
mod review;
mod services;
mod settings;
mod status_bar;
mod sync_status;
mod theme;
mod title_bar;
mod ui;
mod update;
mod workspace;

use argon_core::Paths;
use gpui_kit::component::TitleBar;
use gpui_kit::{
    App, AppContext as _, Bounds, Entity, Global, TitlebarOptions, WindowBounds, WindowOptions, px,
    size,
};
use tracing_appender::non_blocking::WorkerGuard;

use crate::agents::AgentSessions;
use crate::assets::AppAssets;
use crate::inbox::{InboxHandle, InboxModel};
use crate::services::{AppServices, Services};
use crate::sync_status::SyncStatus;
use crate::workspace::Workspace;

const WINDOW_TITLE: &str = "Argon";
const UI_LOCALE: &str = "pt-BR";
const WINDOW_WIDTH: f32 = 1440.;
const WINDOW_HEIGHT: f32 = 900.;

struct LogGuard(Option<WorkerGuard>);

impl Global for LogGuard {}

fn main() {
    app_id::register_process();
    let paths = Paths::resolve();
    let log_guard = services::init_logging(&paths.log_dir());
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "argon starting");
    let config = config_load::load(&paths);

    gpui_kit::application()
        .with_assets(AppAssets)
        .run(move |cx| {
            cx.set_global(LogGuard(log_guard));
            cx.on_app_quit(|cx| {
                tracing::info!("argon quitting");
                cx.global_mut::<LogGuard>().0.take();
                async {}
            })
            .detach();
            gpui_kit::init(cx);
            gpui_kit::component::set_locale(UI_LOCALE);
            keymap::bind(cx);
            let sessions = AgentSessions::init(paths.clone(), cx);
            let (services, events) = Services::start(paths, config);
            AgentSessions::recover(&sessions, &services, cx);
            let sync_status = cx.new(|cx| SyncStatus::new(services.clone(), events, cx));
            let inbox = cx.new(|cx| InboxModel::new(services.clone(), &sync_status, cx));
            cx.set_global(AppServices(services));
            config_watch::init(cx);
            agents::warmup::on_app_start(cx);
            update::init(cx);
            theme::init(cx);
            cx.set_global(InboxHandle(inbox.clone()));
            notify::init(&sync_status, &inbox, cx);
            open_main_window(sync_status, inbox, cx);
            cx.activate(true);
        });
}

fn open_main_window(sync_status: Entity<SyncStatus>, inbox: Entity<InboxModel>, cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(WINDOW_TITLE.into()),
            ..TitleBar::title_bar_options()
        }),
        ..TitleBar::window_options()
    };
    gpui_kit::open_window(options, cx, |window, cx| {
        theme::follow_system_appearance(window);
        cx.new(|cx| {
            Workspace::new(
                content::content_factory(inbox.clone()),
                sync_status,
                inbox,
                window,
                cx,
            )
        })
    })
    .expect("failed to open main window");
}
