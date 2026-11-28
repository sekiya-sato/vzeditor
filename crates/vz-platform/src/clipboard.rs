//! システムのクリップボードからテキストを読む（メニューからの貼り付け用）。
//! 書き込みは egui 側（フレームの出力）で行う。

/// Windows / macOS: arboard で読む。
#[cfg(any(windows, target_os = "macos"))]
pub fn read_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// Linux: システムのクリップボードから UTF-8 テキストを読む (Wayland `wl-paste`, X11 `xclip`/`xsel`).
#[cfg(not(any(windows, target_os = "macos")))]
pub fn read_text() -> Option<String> {
    use std::process::Command;

    let read = |bin: &str, args: &[&str]| {
        let out = Command::new(bin).args(args).output().ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8(out.stdout).ok()
    };

    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if let Some(s) = read("wl-paste", &["--no-newline"]) {
            return Some(s);
        }
    }
    read("xclip", &["-o", "-selection", "clipboard"])
        .or_else(|| read("xsel", &["--clipboard", "--output"]))
}
