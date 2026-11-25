//! ファイルを OS の関連付けで開く（実行する）。
//!
//! - Windows: `ShellExecuteW`（エクスプローラーでダブルクリックしたのと同じ。.bat・.exe は実行される）
//! - macOS: `open`
//! - Linux: `xdg-open`
//!
//! どの OS もファイルのあるフォルダを作業フォルダにする。

use std::path::Path;

/// `path` を OS の関連付けで開く。フォルダならエクスプローラー（Finder など）で開く。
#[cfg(windows)]
pub fn open_with_os(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    // 正規化したパス（\\?\C:\...）は cmd（.bat）などが扱えないので通常の形に戻す
    let path = plain_path(path);
    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain(Some(0)).collect() };
    let file = wide(&path);
    let dir = path.parent().map(wide);
    // SAFETY: 文字列はすべて NUL 終端で、呼び出しの間は生きている。動詞は null（既定の操作）。
    let code = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            null(),
            file.as_ptr(),
            null(),
            dir.as_ref().map_or(null(), |d| d.as_ptr()),
            SW_SHOWNORMAL,
        )
    } as isize;
    // 32 より大きければ成功（ShellExecute の仕様）
    match code {
        c if c > 32 => Ok(()),
        2 | 3 => Err("ファイルが見つかりません".into()),
        5 => Err("アクセスが拒否されました".into()),
        31 => Err("関連付けられたアプリがありません".into()),
        c => Err(format!("エラーコード {c}")),
    }
}

/// `path` を OS の関連付けで開く。フォルダならエクスプローラー（Finder など）で開く。
#[cfg(not(windows))]
pub fn open_with_os(path: &Path) -> Result<(), String> {
    use std::process::{Command, Stdio};

    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut cmd = Command::new(opener);
    cmd.arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = path.parent().filter(|d| d.is_dir()) {
        cmd.current_dir(dir);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{opener} を起動できません: {e}"))?;
    // open / xdg-open はすぐ終わるので、別スレッドで待って後始末する（ゾンビを残さない）
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// `\\?\C:\a` → `C:\a`、`\\?\UNC\server\share` → `\\server\share`。それ以外はそのまま。
#[cfg(windows)]
fn plain_path(path: &Path) -> std::path::PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}").into()
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.into()
    } else {
        path.to_path_buf()
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn plain_path_strips_verbatim_prefix() {
        assert_eq!(
            plain_path(Path::new(r"\\?\C:\a b\x.bat")),
            Path::new(r"C:\a b\x.bat")
        );
        assert_eq!(
            plain_path(Path::new(r"\\?\UNC\srv\share\x.txt")),
            Path::new(r"\\srv\share\x.txt")
        );
        assert_eq!(plain_path(Path::new(r"C:\x.txt")), Path::new(r"C:\x.txt"));
    }
}
