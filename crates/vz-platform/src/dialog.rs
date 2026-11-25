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
    /// 開くファイル（複数選択可）。空ならキャンセル。
    OpenPaths(Vec<PathBuf>),
    /// ネイティブダイアログが使えないため、組み込みのパス入力欄を使う。
    #[cfg_attr(windows, allow(dead_code))]
    Unavailable { is_save: bool },
}

const SAVE_TITLE: &str = "VZ Editor - 名前を付けて保存";
const OPEN_TITLE: &str = "VZ Editor - ファイルを開く";

/// 名前を付けて保存ダイアログを非同期で開く。
pub fn spawn_save_dialog(initial_path: Option<&Path>) -> Receiver<DialogResult> {
    spawn_dialog(initial_path, run_save_dialog)
}

/// ファイルを開くダイアログ（複数選択可）を非同期で開く。
pub fn spawn_open_dialog(initial_dir: Option<&Path>) -> Receiver<DialogResult> {
    spawn_dialog(initial_dir, run_open_dialog)
}

/// ダイアログを別スレッドで開き、結果を受け取る Receiver を返す。
fn spawn_dialog(
    initial: Option<&Path>,
    run: fn(Option<&Path>) -> DialogResult,
) -> Receiver<DialogResult> {
    let initial = initial.map(|p| p.to_path_buf());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(run(initial.as_deref()));
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

/// 改行区切りの出力（zenity / kdialog / osascript）をパスの一覧にする。
#[cfg_attr(windows, allow(dead_code))]
fn lines_to_paths(s: &[u8]) -> Vec<PathBuf> {
    String::from_utf8_lossy(s)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
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
    DialogResult::OpenPaths(dlg.pick_files().unwrap_or_default())
}

// ---------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
fn applescript_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// osascript を実行して標準出力を返す。起動できなければ None、キャンセル時は空。
#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> Option<Vec<u8>> {
    let output = std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;
    // キャンセル時は osascript が非 0 で終了する
    Some(if output.status.success() {
        output.stdout
    } else {
        Vec::new()
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
        Some(out) => DialogResult::SavePath(non_empty_path(&out)),
        None => DialogResult::Unavailable { is_save: true },
    }
}

#[cfg(target_os = "macos")]
fn run_open_dialog(initial_dir: Option<&Path>) -> DialogResult {
    let mut script = format!("set fs to (choose file with prompt {}", applescript_quote(OPEN_TITLE));
    if let Some(d) = initial_dir {
        script.push_str(&format!(
            " default location (POSIX file {})",
            applescript_quote(&d.to_string_lossy())
        ));
    }
    // 複数選択を許可し、選ばれたファイルを1行1パスで返す
    script.push_str(concat!(
        " with multiple selections allowed)\n",
        "set out to \"\"\n",
        "repeat with f in fs\n",
        "set out to out & (POSIX path of f) & linefeed\n",
        "end repeat\n",
        "return out",
    ));
    match run_osascript(&script) {
        Some(out) => DialogResult::OpenPaths(lines_to_paths(&out)),
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
    cmd.args([
        "--file-selection",
        "--multiple",
        "--separator=\n",
        &format!("--title={OPEN_TITLE}"),
    ]);
    if let Some(p) = initial_dir {
        cmd.arg(format!("--filename={}/", p.display()));
    }
    if let Ok(output) = cmd.output() {
        return DialogResult::OpenPaths(if output.status.success() {
            lines_to_paths(&output.stdout)
        } else {
            Vec::new()
        });
    }

    let mut kcmd = Command::new("kdialog");
    kcmd.args(["--getopenfilename", "--multiple", "--separate-output"]);
    match initial_dir {
        Some(p) => kcmd.arg(p),
        None => kcmd.arg("."),
    };
    if let Ok(output) = kcmd.output() {
        return DialogResult::OpenPaths(if output.status.success() {
            lines_to_paths(&output.stdout)
        } else {
            Vec::new()
        });
    }

    DialogResult::Unavailable { is_save: false }
}
