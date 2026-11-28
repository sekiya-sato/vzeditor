use super::{
    parse_in, parse_window, serialize, unix_nanos, SessionEntry, SessionWindow, SESSION_FILE,
};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

// --- Alt+Shift+F4 で記録した状態（vz ごと。Shift を押して起動すると 1 つずつ戻す） ---

const SAVED_DIR: &str = "saved";
const WRITING_PREFIX: &str = ".writing-";
const RESTORING_PREFIX: &str = "restoring-";

/// vz ごとの記録を置くフォルダ（`session/saved`）。
pub fn saved_dir(session_dir: &Path) -> PathBuf {
    session_dir.join(SAVED_DIR)
}

/// 記録したフォルダ（復元中）。
#[derive(Debug)]
pub struct SavedState {
    /// 復元中のフォルダ（`restoring-<自分の pid>-<時刻>`）
    pub dir: PathBuf,
    /// 記録した vz のプロセス ID（戻せなかった分の再公開に使う）
    pub pid: u32,
    pub entries: Vec<SessionEntry>,
    /// 記録したウィンドウの位置と仮想デスクトップ
    pub window: SessionWindow,
}

/// 公開済みの記録の名前（`<pid>` / `<pid>-<k>`）を (pid, k) にする。
fn parse_saved_name(name: &str) -> Option<(u32, u32)> {
    let (pid, k) = match name.split_once('-') {
        Some((pid, k)) => (pid, k.parse().ok()?),
        None => (name, 0),
    };
    Some((pid.parse().ok()?, k))
}

/// Windows ではウイルス対策ソフトなどが開いていると改名が一時的に失敗するので、少し待って試し直す。
fn rename_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut result = fs::rename(from, to);
    for _ in 0..3 {
        if result.is_ok() || to.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
        result = fs::rename(from, to);
    }
    result
}

/// 書き込み用のフォルダ（`.writing-<pid>`）を空にして作る。
pub fn begin_saved_state(saved: &Path) -> io::Result<PathBuf> {
    let dir = saved.join(format!("{WRITING_PREFIX}{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 書き終えたフォルダを `<pid>`（ふさがっていれば `<pid>-<k>`）へ改名して公開する。
pub fn publish_saved_state(saved: &Path, from: &Path, pid: u32) -> io::Result<PathBuf> {
    for k in 0..1000u32 {
        let name = if k == 0 {
            pid.to_string()
        } else {
            format!("{pid}-{k}")
        };
        let to = saved.join(name);
        if to.exists() {
            continue;
        }
        match rename_retry(from, &to) {
            Ok(()) => return Ok(to),
            // 同時に同じ名前が作られた場合は次の番号へ
            Err(_) if to.exists() => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("記録の名前が足りません"))
}

/// PID の小さい順に記録を 1 つ受け取る（改名に成功した 1 プロセスだけが受け取る）。無ければ None。
pub fn take_saved_state(saved: &Path) -> Option<SavedState> {
    let mut candidates: Vec<((u32, u32), PathBuf)> = fs::read_dir(saved)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let key = parse_saved_name(&e.file_name().to_string_lossy())?;
            let path = e.path();
            path.join(SESSION_FILE).is_file().then_some((key, path))
        })
        .collect();
    candidates.sort();
    let stamp = unix_nanos();
    for ((pid, _), path) in candidates {
        let dir = saved.join(format!("{RESTORING_PREFIX}{}-{stamp}", std::process::id()));
        if rename_retry(&path, &dir).is_err() {
            continue;
        }
        let text = fs::read_to_string(dir.join(SESSION_FILE)).unwrap_or_default();
        let entries = parse_in(&dir, &text);
        let window = parse_window(&text);
        return Some(SavedState {
            dir,
            pid,
            entries,
            window,
        });
    }
    None
}

/// 復元を終える。`failed`（戻せなかった項目）が無ければフォルダを消し、
/// あればその項目だけを `session.ini` に書き直して再公開する（次の Shift 起動で試し直す）。
/// ウィンドウの行は書かない（試し直す vz の位置を動かさないため）。
pub fn finish_saved_state(
    saved: &Path,
    state: &SavedState,
    failed: &[SessionEntry],
) -> io::Result<()> {
    if failed.is_empty() {
        return fs::remove_dir_all(&state.dir);
    }
    fs::write(state.dir.join(SESSION_FILE), serialize(failed))?;
    publish_saved_state(saved, &state.dir, state.pid).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_saved_name() {
        assert_eq!(parse_saved_name("123"), Some((123, 0)));
        assert_eq!(parse_saved_name("123-2"), Some((123, 2)));
        assert_eq!(parse_saved_name(".writing-123"), None);
        assert_eq!(parse_saved_name("restoring-1-2"), None);
        assert_eq!(parse_saved_name("abc"), None);
    }
}
