//! 更新時の「保存して終了 → 再起動で元の状態に戻す」。
//!
//! - `vz --save-and-quit`: 起動中の vz に終了を要求する。要求を受けた vz は、パスのある未保存バッファを
//!   保存し、無題（または保存できなかった）バッファの内容は設定フォルダの `session/` に一時保存して、
//!   開いていたバッファの一覧を `session/session.ini` に書いてから終了する。
//!   vz で読み込んだ後に外部でファイルが変更されていた未保存バッファは上書きせず、vz の編集内容を
//!   一時保存して（`conflict`）、復元後にどちらを採用するか確認する。
//! - `vz --restore`: `session.ini` のバッファを開き直し（カーソル位置・Markdown ビュー・表示中のバッファも戻す）、
//!   読み終えた `session.ini` と一時保存ファイルを削除する。
//!   ターミナル（起動したフォルダ）とブラウザ（タブごとの表示中の URL と選択中のタブ）も、開いていれば記録して開き直す。
//!   シェルは新しく起動する（実行中のコマンドや画面の内容は戻らない）。
//!   2画面ファイラーは左右のフォルダを記録し、表示中だった場合はその画面も戻す。
//!
//! 要求は設定フォルダの `session/quit-request` ファイルで伝える。起動中の vz は監視スレッドで
//! このファイルを見つけると `quit-request.<pid>` へ改名して受け取り（同時に1つの vz だけが受け取る）、
//! 状態を書き終えたら削除する。複数の vz が起動中なら、要求側は受け取られなくなるまで繰り返す。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SESSION_FILE: &str = "session.ini";
const REQUEST_FILE: &str = "quit-request";

/// 状態を保存するフォルダ（`<設定フォルダ>/vzeditor/session`）。
pub fn session_dir() -> Option<PathBuf> {
    crate::settings::app_dir().map(|d| d.join("session"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    /// ファイルを開き直す（保存済み。未作成の新規ファイルを含む）
    File,
    /// 内容を一時保存ファイルから戻し、未保存のバッファにする（無題、または保存できなかったファイル）
    Unsaved,
    /// 外部での変更と衝突したため上書きしなかった vz の編集内容。一時保存ファイルから無題のバッファに戻し、
    /// 復元後に `path` のファイルとの差分を見せて、どちらを採用するか確認する
    Conflict,
    /// ターミナル。`path` はシェルを起動したフォルダ。`active` はターミナルを表示していたか
    Terminal,
    /// 簡易ブラウザのタブ（タブごとに 1 行、並び順のまま）。`path` は表示中の URL（無ければ案内ページ）。
    /// `active` はこのタブを表示していたか、`line` は 1 なら選択中のタブ（ブラウザを表示していなくても）
    Browser,
    /// 保存できなかった巨大ファイル。`path` のファイルを開き直し、`temp` に一時保存した
    /// 未保存の編集内容（ピース列と追加した文字）を当て直す
    HugeEdits,
    /// ファイラーの左側。path はフォルダ、active はファイラーを表示していたか
    FilerLeft,
    /// ファイラーの右側。path はフォルダ
    FilerRight,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEntry {
    pub kind: EntryKind,
    pub line: usize,
    pub col: usize,
    pub markdown_view: bool,
    pub active: bool,
    pub crlf: bool,
    /// 一時保存ファイル（Unsaved のとき）
    pub temp: Option<PathBuf>,
    /// 元のファイルのパス（無題なら None）、またはツールのフォルダ・URL
    pub path: Option<PathBuf>,
}

fn flag(b: bool) -> &'static str {
    if b {
        "1"
    } else {
        "0"
    }
}

fn path_field(p: &Option<PathBuf>) -> String {
    p.as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 1項目1行。タブ区切りで、パスは改行以外を含められるよう最後の欄に置く。
pub fn serialize(entries: &[SessionEntry]) -> String {
    let mut out = String::new();
    for e in entries {
        let kind = match e.kind {
            EntryKind::File => "file",
            EntryKind::Unsaved => "unsaved",
            EntryKind::Conflict => "conflict",
            EntryKind::HugeEdits => "huge-edits",
            EntryKind::Terminal => "terminal",
            EntryKind::Browser => "browser",
            EntryKind::FilerLeft => "filer-left",
            EntryKind::FilerRight => "filer-right",
        };
        out.push_str(&format!(
            "{kind}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            e.line,
            e.col,
            flag(e.markdown_view),
            flag(e.active),
            flag(e.crlf),
            path_field(&e.temp),
            path_field(&e.path),
        ));
    }
    out
}

pub fn parse(text: &str) -> Vec<SessionEntry> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.splitn(8, '\t').collect();
        if f.len() != 8 {
            continue;
        }
        let kind = match f[0] {
            "file" => EntryKind::File,
            "unsaved" => EntryKind::Unsaved,
            "conflict" => EntryKind::Conflict,
            "huge-edits" => EntryKind::HugeEdits,
            "terminal" => EntryKind::Terminal,
            "browser" => EntryKind::Browser,
            "filer-left" => EntryKind::FilerLeft,
            "filer-right" => EntryKind::FilerRight,
            _ => continue,
        };
        let opt = |s: &str| (!s.is_empty()).then(|| PathBuf::from(s));
        let entry = SessionEntry {
            kind,
            line: f[1].parse().unwrap_or(0),
            col: f[2].parse().unwrap_or(0),
            markdown_view: f[3] == "1",
            active: f[4] == "1",
            crlf: f[5] == "1",
            temp: opt(f[6]),
            path: opt(f[7]),
        };
        let valid = match entry.kind {
            EntryKind::File => entry.path.is_some(),
            EntryKind::Unsaved => entry.temp.is_some(),
            EntryKind::Conflict | EntryKind::HugeEdits => {
                entry.temp.is_some() && entry.path.is_some()
            }
            EntryKind::Terminal | EntryKind::FilerLeft | EntryKind::FilerRight => {
                entry.path.is_some()
            }
            EntryKind::Browser => true,
        };
        if valid {
            entries.push(entry);
        }
    }
    entries
}

/// 一時保存ファイルのパス（プロセス ID と連番で重複させない）。
pub fn temp_path(dir: &Path, index: usize) -> PathBuf {
    dir.join(format!("unsaved-{}-{index}.txt", std::process::id()))
}

/// 巨大ファイルの未保存の編集内容を一時保存するパス。
pub fn huge_edits_path(dir: &Path, index: usize) -> PathBuf {
    dir.join(format!("huge-{}-{index}.vzedit", std::process::id()))
}

/// 状態を追記する（複数の vz が順に終了した場合も、すべてのバッファを残す）。
pub fn append_session(dir: &Path, entries: &[SessionEntry]) -> io::Result<()> {
    use std::io::Write;
    fs::create_dir_all(dir)?;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(SESSION_FILE))?;
    f.write_all(serialize(entries).as_bytes())
}

/// 保存された状態を読み、`session.ini` を削除する（一時保存ファイルは呼び出し側が読んでから消す）。
pub fn take_session(dir: &Path) -> Vec<SessionEntry> {
    let path = dir.join(SESSION_FILE);
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let _ = fs::remove_file(&path);
    parse(&text)
}

fn claim_path(dir: &Path) -> PathBuf {
    dir.join(format!("{REQUEST_FILE}.{}", std::process::id()))
}

/// 終了要求の監視スレッドを開始する。要求を受け取ったら true になり、`repaint` を呼ぶ。
pub fn start_watcher(dir: PathBuf, repaint: impl Fn() + Send + 'static) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let flag2 = flag.clone();
    std::thread::Builder::new()
        .name("vz-quit-request".into())
        .spawn(move || {
            let request = dir.join(REQUEST_FILE);
            let claim = claim_path(&dir);
            loop {
                std::thread::sleep(Duration::from_millis(300));
                // 改名に成功した1プロセスだけが受け取る
                if request.exists() && fs::rename(&request, &claim).is_ok() {
                    flag2.store(true, Ordering::SeqCst);
                    repaint();
                    break;
                }
            }
        })
        .ok();
    flag
}

/// 状態を書き終えたことを要求側へ知らせる。
pub fn finish_request(dir: &Path) {
    let _ = fs::remove_file(claim_path(dir));
}

fn wait_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    done()
}

/// 受け取り済みの要求ファイル（`{REQUEST_FILE}.<pid>`）の名前か。
fn is_claim_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy()
        .strip_prefix(REQUEST_FILE)
        .is_some_and(|rest| rest.starts_with('.'))
}

fn has_claim(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|rd| rd.flatten().any(|e| is_claim_name(&e.file_name())))
}

/// 起動中のすべての vz に「保存して終了」を要求する。受け取った vz の数を返す。
pub fn request_save_and_quit(dir: &Path) -> io::Result<usize> {
    fs::create_dir_all(dir)?;
    let request = dir.join(REQUEST_FILE);
    let mut handled = 0usize;
    // 前回の失敗で残った受け取り済みファイルは消しておく（受け取った vz が居なければ完了しない）
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            if is_claim_name(&e.file_name()) {
                let _ = fs::remove_file(e.path());
            }
        }
    }
    loop {
        fs::write(&request, b"")?;
        // 監視は 0.3 秒ごと。最小化中などで受け取りが遅れても待てるよう余裕を持たせる
        let claimed = wait_until(Duration::from_secs(3), || !request.exists());
        if !claimed {
            let _ = fs::remove_file(&request);
            return Ok(handled);
        }
        // 状態の書き込み（保存）が終わるまで待つ
        wait_until(Duration::from_secs(30), || !has_claim(dir));
        handled += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_claim_name() {
        assert!(is_claim_name(std::ffi::OsStr::new("quit-request.123")));
        assert!(!is_claim_name(std::ffi::OsStr::new("quit-request")));
        assert!(!is_claim_name(std::ffi::OsStr::new("quit-requestx.1")));
    }

    #[test]
    fn test_session_roundtrip_and_garbage() {
        let entries = vec![
            SessionEntry {
                kind: EntryKind::File,
                line: 3,
                col: 5,
                markdown_view: true,
                active: false,
                crlf: false,
                temp: None,
                path: Some(PathBuf::from("C:\\work\\a b.md")),
            },
            SessionEntry {
                kind: EntryKind::Unsaved,
                line: 0,
                col: 0,
                markdown_view: false,
                active: true,
                crlf: true,
                temp: Some(PathBuf::from("/tmp/unsaved-1-0.txt")),
                path: None,
            },
            SessionEntry {
                kind: EntryKind::Conflict,
                line: 1,
                col: 0,
                markdown_view: false,
                active: false,
                crlf: false,
                temp: Some(PathBuf::from("/tmp/unsaved-1-1.txt")),
                path: Some(PathBuf::from("/work/README.md")),
            },
            SessionEntry {
                kind: EntryKind::HugeEdits,
                line: 5_000_000,
                col: 3,
                markdown_view: false,
                active: false,
                crlf: true,
                temp: Some(PathBuf::from("/tmp/huge-1-2.vzedit")),
                path: Some(PathBuf::from("/data/big.log")),
            },
            SessionEntry {
                kind: EntryKind::Terminal,
                line: 0,
                col: 0,
                markdown_view: false,
                active: true,
                crlf: false,
                temp: None,
                path: Some(PathBuf::from(r"C:\work")),
            },
            SessionEntry {
                kind: EntryKind::Browser,
                line: 0,
                col: 0,
                markdown_view: false,
                active: false,
                crlf: false,
                temp: None,
                path: Some(PathBuf::from("http://localhost:3000/a?b=1#c")),
            },
        ];
        // conflict は元のファイルのパス、terminal はフォルダが無ければ捨てる。未知の種類（新しい版の項目）は読み飛ばす
        let text = format!(
            "garbage\nfile\t1\t2\nconflict\t0\t0\t0\t0\t0\t/tmp/x.txt\t\nterminal\t0\t0\t0\t1\t0\t\t\nfuture\t0\t0\t0\t0\t0\t\t/x\n{}",
            serialize(&entries)
        );
        assert_eq!(parse(&text), entries);
    }

    #[test]
    fn test_request_without_running_instance_returns_zero() {
        let dir = std::env::temp_dir().join(format!("vz_session_req_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(request_save_and_quit(&dir).unwrap(), 0);
        assert!(!dir.join(REQUEST_FILE).exists());

        // 監視スレッドが要求を受け取り、完了を知らせると 1 件として数える
        let flag = start_watcher(dir.clone(), || {});
        let dir2 = dir.clone();
        let flag2 = flag.clone();
        let responder = std::thread::spawn(move || {
            wait_until(Duration::from_secs(10), || flag2.load(Ordering::SeqCst));
            finish_request(&dir2);
        });
        assert_eq!(request_save_and_quit(&dir).unwrap(), 1);
        responder.join().unwrap();
        assert!(flag.load(Ordering::SeqCst));
        let _ = fs::remove_dir_all(&dir);
    }
}
