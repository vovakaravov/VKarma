mod config;
mod fountain;
mod live_preview;
mod markdown;
mod spreadsheet;
mod theme;
mod typst_engine;
mod ui;
mod vault;
mod watcher;

use gpui_kit::assets::AllAssets;
use gpui_kit::{
    AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use std::path::PathBuf;

fn main() {
    let initial_target = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .filter(|p| p.exists());

    gpui_kit::application()
        .with_assets(AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::register_custom_languages();
            theme::apply_kanagawa_dragon_theme(cx);
            ui::register_keybindings(cx);

            let bounds = Bounds::centered(None, size(px(1520.), px(940.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("VKarma".into()),
                    ..Default::default()
                }),
                app_id: Some("vkarma".to_string()),
                ..Default::default()
            };

            let arg = initial_target.clone();
            gpui_kit::open_window(options, cx, move |window, cx| {
                window.set_window_title("VKarma");
                cx.new(|cx| ui::VKarmaApp::new(arg.clone(), window, cx))
            })
            .expect("failed to open VKarma window");
        });
}
