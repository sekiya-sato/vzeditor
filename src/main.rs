// リリースビルドの Windows ではコンソールウィンドウを出さない
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod icon;
mod release;

use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;
use vz_config::settings::Settings;
use vz_ui::{StartupRestore, VzEditorApp};

fn main() -> eframe::Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let release_state = match release::run(&args) {
        Ok(release::Startup::Normal) => None,
        Ok(release::Startup::Handled) => return Ok(()),
        Ok(release::Startup::Restore(state)) => Some(state),
        Err(e) => {
            eprintln!("リリース処理: {e}");
            std::process::exit(1);
        }
    };
    // 複数ファイル・フォルダを引数で受け取る（エクスプローラーで exe へ複数ドロップした場合も同じ）
    let tab_transfer = match args.as_slice() {
        [flag, id] if flag == "--tab-transfer" => Some(id.to_string_lossy().into_owned()),
        _ => None,
    };
    let (restore, initial_paths) = if let Some(state) = release_state {
        (StartupRestore::Release(state), Vec::new())
    } else if let Some(id) = tab_transfer {
        (StartupRestore::TabTransfer(id), Vec::new())
    } else {
        regular_startup(args)
    };
    let mut settings = Settings::load();
    if matches!(&restore, StartupRestore::TabTransfer(_)) {
        // 切り離した窓を元の窓と同じ保存位置に重ねず、OS に配置を任せる。
        settings.window = None;
    }

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
        Box::new(move |cc| {
            Ok(Box::new(VzEditorApp::new(
                cc,
                initial_paths,
                settings,
                restore,
            )))
        }),
    )
}

fn regular_startup(args: Vec<std::ffi::OsString>) -> (StartupRestore, Vec<PathBuf>) {
    // Shift を押して起動したら、Alt+Shift+F4 で記録した状態を戻す。
    let restore = match vz_platform::keyboard::shift_held() {
        Some(true) => StartupRestore::Saved,
        Some(false) => StartupRestore::None,
        None => StartupRestore::SavedIfShift,
    };
    (restore, args.into_iter().map(PathBuf::from).collect())
}
