mod actions;
mod assets;
mod bottom;
mod center;
mod config_load;
mod content;
mod dock_items;
mod inbox;
mod keymap;
mod layout_store;
mod palette;
mod placeholders;
mod pr;
mod review;
mod services;
mod status_bar;
mod sync_status;
mod theme;
mod title_bar;
mod ui;
mod workspace;

use argon_core::Paths;
use gpui_kit::component::TitleBar;
use gpui_kit::{
    App, AppContext as _, Bounds, Entity, Global, TitlebarOptions, WindowBounds, WindowOptions, px,
    size,
};
use tracing_appender::non_blocking::WorkerGuard;

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
            theme::apply_one_dark(cx);
            keymap::bind(cx);
            let (services, events) = Services::start(paths, config);
            let sync_status = cx.new(|cx| SyncStatus::new(services.clone(), events, cx));
            let inbox = cx.new(|cx| InboxModel::new(services.clone(), &sync_status, cx));
            cx.set_global(AppServices(services));
            cx.set_global(InboxHandle(inbox.clone()));
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
