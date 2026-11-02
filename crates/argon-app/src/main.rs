mod shell;
mod theme;
mod window;

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            theme::apply_one_dark(cx);
            window::open_main_window(cx);
            cx.activate(true);
        });
}
