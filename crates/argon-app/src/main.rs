mod actions;
mod center;
mod keymap;
mod layout_store;
mod placeholders;
mod services;
mod status_bar;
mod sync_status;
mod theme;
mod title_bar;
mod workspace;

use argon_core::{Config, Paths};
use gpui_kit::component::TitleBar;
use gpui_kit::{
    App, AppContext as _, Bounds, Entity, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};

use crate::services::{AppServices, Services};
use crate::sync_status::SyncStatus;
use crate::workspace::Workspace;

const WINDOW_TITLE: &str = "Argon";
const WINDOW_WIDTH: f32 = 1440.;
const WINDOW_HEIGHT: f32 = 900.;

fn main() {
    let paths = Paths::resolve();
    services::init_logging(&paths.log_dir());
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "argon starting");
    let config = load_config(&paths);

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::apply_one_dark(cx);
            keymap::bind(cx);
            let (services, events) = Services::start(paths, config);
            let sync_status = cx.new(|cx| SyncStatus::new(services.clone(), events, cx));
            cx.set_global(AppServices(services));
            open_main_window(sync_status, cx);
            cx.activate(true);
        });
}

fn load_config(paths: &Paths) -> Config {
    let path = paths.config_file();
    Config::load(&path).unwrap_or_else(|error| {
        tracing::warn!(%error, "failed to load config, using defaults without overwriting it");
        let mut config = Config::default();
        config.orgs.initialized = true;
        config
    })
}

fn open_main_window(sync_status: Entity<SyncStatus>, cx: &mut App) {
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
                placeholders::placeholder_content_factory(),
                sync_status,
                window,
                cx,
            )
        })
    })
    .expect("failed to open main window");
}
