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

    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if let Ok(out) = Command::new("wl-paste").arg("--no-newline").output() {
            if out.status.success() {
                if let Ok(s) = String::from_utf8(out.stdout) {
                    return Some(s);
                }
            }
        }
    }

    for (bin, args) in [
        ("xclip", &["-o", "-selection", "clipboard"][..]),
        ("xsel", &["--clipboard", "--output"][..]),
    ] {
        if let Ok(out) = Command::new(bin).args(args).output() {
            if out.status.success() {
                if let Ok(s) = String::from_utf8(out.stdout) {
                    return Some(s);
                }
            }
        }
    }

    None
}
