//! ネイティブのファイルダイアログ（別スレッドで実行し、結果をチャネルで返す）。
//!
//! - Windows: `rfd`（IFileDialog）
//! - macOS:   `osascript`（AppleScript の choose file）
//! - Linux:   `zenity`、無ければ `kdialog`
//!
//! どれも使えない場合は `Unavailable` を返し、組み込みのパス入力欄に切り替える。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;

#[derive(Debug, Clone)]
pub enum DialogResult {
    SavePath(Option<PathBuf>),
    OpenPath(Option<PathBuf>),
    /// ネイティブダイアログが使えないため、組み込みのパス入力欄を使う。
    #[cfg_attr(windows, allow(dead_code))]
    Unavailable { is_save: bool },
}

const SAVE_TITLE: &str = "VZ Editor - 名前を付けて保存";
const OPEN_TITLE: &str = "VZ Editor - ファイルを開く";

/// 名前を付けて保存ダイアログを非同期で開く。
pub fn spawn_save_dialog(initial_path: Option<&Path>) -> Receiver<DialogResult> {
    let initial = initial_path.map(|p| p.to_path_buf());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(run_save_dialog(initial.as_deref()));
    });
    rx
}

/// ファイルを開くダイアログを非同期で開く。
pub fn spawn_open_dialog(initial_dir: Option<&Path>) -> Receiver<DialogResult> {
    let initial = initial_dir.map(|p| p.to_path_buf());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(run_open_dialog(initial.as_deref()));
    });
    rx
}

#[cfg_attr(windows, allow(dead_code))]
fn non_empty_path(s: &[u8]) -> Option<PathBuf> {
    let s = String::from_utf8_lossy(s).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
fn run_save_dialog(initial_path: Option<&Path>) -> DialogResult {
    let mut dlg = rfd::FileDialog::new().set_title(SAVE_TITLE);
    match initial_path {
        Some(p) => {
            if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                dlg = dlg.set_directory(dir);
            }
            if let Some(name) = p.file_name() {
                dlg = dlg.set_file_name(name.to_string_lossy());
            }
        }
        None => {
            if let Ok(cwd) = std::env::current_dir() {
                dlg = dlg.set_directory(cwd);
            }
            dlg = dlg.set_file_name("untitled.txt");
        }
    }
    DialogResult::SavePath(dlg.save_file())
}

#[cfg(windows)]
fn run_open_dialog(initial_dir: Option<&Path>) -> DialogResult {
    let mut dlg = rfd::FileDialog::new().set_title(OPEN_TITLE);
    if let Some(dir) = initial_dir {
        dlg = dlg.set_directory(dir);
    }
    DialogResult::OpenPath(dlg.pick_file())
}

// ---------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
fn applescript_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> Option<Option<PathBuf>> {
    let output = std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;
    // キャンセル時は osascript が非 0 で終了する
    Some(if output.status.success() {
        non_empty_path(&output.stdout)
    } else {
        None
    })
}

#[cfg(target_os = "macos")]
fn run_save_dialog(initial_path: Option<&Path>) -> DialogResult {
    let (dir, name) = match initial_path {
        Some(p) => (
            p.parent()
                .filter(|d| !d.as_os_str().is_empty())
                .map(|d| d.to_path_buf()),
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "untitled.txt".into()),
        ),
        None => (std::env::current_dir().ok(), "untitled.txt".into()),
    };
    let mut script = format!(
        "POSIX path of (choose file name with prompt {} default name {}",
        applescript_quote(SAVE_TITLE),
        applescript_quote(&name)
    );
    if let Some(d) = dir {
        script.push_str(&format!(
            " default location (POSIX file {})",
            applescript_quote(&d.to_string_lossy())
        ));
    }
    script.push(')');
    match run_osascript(&script) {
        Some(p) => DialogResult::SavePath(p),
        None => DialogResult::Unavailable { is_save: true },
    }
}

#[cfg(target_os = "macos")]
fn run_open_dialog(initial_dir: Option<&Path>) -> DialogResult {
    let mut script = format!(
        "POSIX path of (choose file with prompt {}",
        applescript_quote(OPEN_TITLE)
    );
    if let Some(d) = initial_dir {
        script.push_str(&format!(
            " default location (POSIX file {})",
            applescript_quote(&d.to_string_lossy())
        ));
    }
    script.push(')');
    match run_osascript(&script) {
        Some(p) => DialogResult::OpenPath(p),
        None => DialogResult::Unavailable { is_save: false },
    }
}

// ---------------------------------------------------------------- Linux / other Unix

#[cfg(not(any(windows, target_os = "macos")))]
fn run_save_dialog(initial_path: Option<&Path>) -> DialogResult {
    use std::process::Command;

    let mut cmd = Command::new("zenity");
    cmd.args([
        "--file-selection",
        "--save",
        "--confirm-overwrite",
        &format!("--title={SAVE_TITLE}"),
    ]);
    if let Some(p) = initial_path {
        cmd.arg(format!("--filename={}", p.display()));
    } else if let Ok(cwd) = std::env::current_dir() {
        cmd.arg(format!("--filename={}/untitled.txt", cwd.display()));
    }
    if let Ok(output) = cmd.output() {
        return DialogResult::SavePath(if output.status.success() {
            non_empty_path(&output.stdout)
        } else {
            None
        });
    }

    let mut kcmd = Command::new("kdialog");
    kcmd.arg("--getsavefilename");
    match initial_path {
        Some(p) => kcmd.arg(p),
        None => kcmd.arg("."),
    };
    if let Ok(output) = kcmd.output() {
        return DialogResult::SavePath(if output.status.success() {
            non_empty_path(&output.stdout)
        } else {
            None
        });
    }

    DialogResult::Unavailable { is_save: true }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn run_open_dialog(initial_dir: Option<&Path>) -> DialogResult {
    use std::process::Command;

    let mut cmd = Command::new("zenity");
    cmd.args(["--file-selection", &format!("--title={OPEN_TITLE}")]);
    if let Some(p) = initial_dir {
        cmd.arg(format!("--filename={}/", p.display()));
    }
    if let Ok(output) = cmd.output() {
        return DialogResult::OpenPath(if output.status.success() {
            non_empty_path(&output.stdout)
        } else {
            None
        });
    }

    let mut kcmd = Command::new("kdialog");
    kcmd.arg("--getopenfilename");
    match initial_dir {
        Some(p) => kcmd.arg(p),
        None => kcmd.arg("."),
    };
    if let Ok(output) = kcmd.output() {
        return DialogResult::OpenPath(if output.status.success() {
            non_empty_path(&output.stdout)
        } else {
            None
        });
    }

    DialogResult::Unavailable { is_save: false }
}
