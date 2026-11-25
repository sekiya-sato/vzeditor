// リリースビルドの Windows ではコンソールウィンドウを出さない
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod icon;

use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;
use vz_config::session;
use vz_config::settings::Settings;
use vz_ui::VzEditorApp;

fn main() -> eframe::Result<()> {
    // 複数ファイル・フォルダを引数で受け取る（エクスプローラーで exe へ複数ドロップした場合も同じ）
    let mut restore = false;
    let mut initial_paths: Vec<PathBuf> = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.to_str() {
            // 起動中の vz に「保存して終了」を要求する（更新用。ウィンドウは開かない）
            Some("--save-and-quit") => {
                let handled = session::session_dir()
                    .map(|d| session::request_save_and_quit(&d).unwrap_or(0))
                    .unwrap_or(0);
                // 終了コード: 受け取った vz の数（0 なら起動中の vz なし）
                std::process::exit(handled.min(100) as i32);
            }
            // --save-and-quit で保存した状態を復元して起動する
            Some("--restore") => restore = true,
            _ => initial_paths.push(PathBuf::from(arg)),
        }
    }
    let settings = Settings::load();

    let mut viewport = egui::ViewportBuilder::default()
        .with_app_id("vzeditor")
        .with_title("VZ Editor")
        .with_icon(Arc::new(icon::build_app_icon()))
        .with_inner_size([1024.0, 680.0])
        .with_min_inner_size([640.0, 400.0])
        // 背面透過を実行中に切り替えられるよう、常に透過可能なウィンドウとして作成する
        .with_transparent(true)
        // ファイルのドラッグ＆ドロップを受け付ける（Windows / macOS / Linux X11）
        .with_drag_and_drop(true);
    // 前回の位置とサイズ。作成時の位置は主モニターの拡大率で換算されるため、
    // 拡大率の違うモニターでは起動直後に app 側（restore_window_position）で合わせ直す
    if let Some(w) = settings.window {
        viewport = viewport
            .with_inner_size([w.width.max(640.0), w.height.max(400.0)])
            .with_position([w.x as f32 / w.scale, w.y as f32 / w.scale]);
    }
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
        Box::new(move |cc| Ok(Box::new(VzEditorApp::new(cc, initial_paths, settings, restore)))),
    )
}
