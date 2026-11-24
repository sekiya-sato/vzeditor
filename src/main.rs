// リリースビルドの Windows ではコンソールウィンドウを出さない
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod buffer;
mod dialog;
mod filer;
mod fonts;
mod icon;
mod markdown;
mod settings;
mod window_style;

use app::VzEditorApp;
use eframe::egui;
use settings::Settings;
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> eframe::Result<()> {
    let initial_file = std::env::args_os().nth(1).map(PathBuf::from);
    let settings = Settings::load();

    let mut viewport = egui::ViewportBuilder::default()
        .with_app_id("vzeditor")
        .with_title("VZ Editor")
        .with_icon(Arc::new(icon::build_app_icon()))
        .with_inner_size([1024.0, 680.0])
        .with_min_inner_size([640.0, 400.0])
        // 背面透過を実行中に切り替えられるよう、常に透過可能なウィンドウとして作成する
        .with_transparent(true);
    if settings.always_on_top {
        viewport = viewport.with_always_on_top();
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "VZ Editor",
        native_options,
        Box::new(move |cc| Ok(Box::new(VzEditorApp::new(cc, initial_file, settings)))),
    )
}
