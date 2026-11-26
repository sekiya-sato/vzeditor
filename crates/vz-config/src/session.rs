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
//! 状態を書き終えたら削除する。複数の vz が起動中なら、要求側は同じ要求 ID で繰り返す。
//! 終了を中止した vz は、次の要求 ID を受け付ける。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SESSION_FILE: &str = "session.ini";
const REQUEST_FILE: &str = "quit-request";

/// 状態を保存するフォルダ（`<設定フォルダ>/vzeditor/session`）。
pub fn session_dir() -> Option<PathBuf> {
    crate::settings::app_dir().map(|d| d.join("session"))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum EntryKind {
    /// ファイルを開き直す（保存済み。未作成の新規ファイルを含む）
    #[default]
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionEntry {
    pub kind: EntryKind,
    pub line: usize,
    pub col: usize,
    pub markdown_view: bool,
    pub active: bool,
    pub crlf: bool,
    /// 文字コードの名前（`TextEncoding::label`。Unsaved で元のファイルの文字コードを戻す）
    pub encoding: Option<String>,
    /// 読み込み・保存した時点のファイルの状態（更新時刻の UNIX ナノ秒, 長さ）。外部変更の検知用
    pub disk_stamp: Option<(u64, u64)>,
    /// 一時保存ファイル。`session.ini` にはファイル名だけを書き、読むときに同じフォルダで解決する
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

/// `改行[;文字コード[;更新時刻ns;長さ]]`（古い形式は `0` / `1` だけ）
fn line_ending_field(e: &SessionEntry) -> String {
    let mut out = flag(e.crlf).to_string();
    if let Some(enc) = &e.encoding {
        out.push(';');
        out.push_str(enc);
        if let Some((ns, len)) = e.disk_stamp {
            out.push_str(&format!(";{ns};{len}"));
        }
    }
    out
}

fn parse_line_ending_field(s: &str) -> (bool, Option<String>, Option<(u64, u64)>) {
    let mut it = s.split(';');
    let crlf = it.next() == Some("1");
    let encoding = it.next().filter(|e| !e.is_empty()).map(String::from);
    let stamp = match (it.next(), it.next()) {
        (Some(ns), Some(len)) => ns.parse().ok().zip(len.parse().ok()),
        _ => None,
    };
    (crlf, encoding, stamp)
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
            line_ending_field(e),
            // フォルダを改名しても参照できるよう、一時保存はファイル名だけを書く
            path_field(
                &e.temp
                    .as_ref()
                    .map(|t| t.file_name().map_or_else(|| t.clone(), PathBuf::from))
            ),
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
        let (crlf, encoding, disk_stamp) = parse_line_ending_field(f[5]);
        let entry = SessionEntry {
            kind,
            line: f[1].parse().unwrap_or(0),
            col: f[2].parse().unwrap_or(0),
            markdown_view: f[3] == "1",
            active: f[4] == "1",
            crlf,
            encoding,
            disk_stamp,
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
    parse_in(dir, &text)
}

/// `dir/session.ini` の内容を読む。一時保存のファイル名は `dir` からのパスにする（絶対パスはそのまま）。
fn parse_in(dir: &Path, text: &str) -> Vec<SessionEntry> {
    let mut entries = parse(text);
    for e in &mut entries {
        if let Some(t) = &e.temp {
            e.temp = Some(dir.join(t));
        }
    }
    entries
}

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
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for ((pid, _), path) in candidates {
        let dir = saved.join(format!("{RESTORING_PREFIX}{}-{stamp}", std::process::id()));
        if rename_retry(&path, &dir).is_err() {
            continue;
        }
        let text = fs::read_to_string(dir.join(SESSION_FILE)).unwrap_or_default();
        let entries = parse_in(&dir, &text);
        return Some(SavedState { dir, pid, entries });
    }
    None
}

/// 復元を終える。`failed`（戻せなかった項目）が無ければフォルダを消し、
/// あればその項目だけを `session.ini` に書き直して再公開する（次の Shift 起動で試し直す）。
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
            let mut last_request = None;
            loop {
                std::thread::sleep(Duration::from_millis(300));
                let Ok(id) = fs::read_to_string(&request) else {
                    continue;
                };
                // 同じコマンドから繰り返し届く要求は一度だけ処理する。
                if last_request.as_ref() == Some(&id) || claim.exists() {
                    continue;
                }
                // 改名に成功した1プロセスだけが受け取る
                if fs::rename(&request, &claim).is_ok() {
                    last_request = Some(id);
                    flag2.store(true, Ordering::SeqCst);
                    repaint();
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
    let request_id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let pending = dir.join(format!(".{REQUEST_FILE}-{request_id}"));
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
        // 監視側が要求 ID の書きかけを読まないよう、書き終えてから公開する。
        if let Err(e) =
            fs::write(&pending, &request_id).and_then(|()| fs::rename(&pending, &request))
        {
            let _ = fs::remove_file(&pending);
            return Err(e);
        }
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
                encoding: None,
                disk_stamp: None,
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
                encoding: Some("Shift_JIS".into()),
                disk_stamp: Some((1_732_600_000_123_456_789, 1234)),
                temp: Some(PathBuf::from("unsaved-1-0.txt")),
                path: None,
            },
            SessionEntry {
                kind: EntryKind::Conflict,
                line: 1,
                col: 0,
                markdown_view: false,
                active: false,
                crlf: false,
                encoding: None,
                disk_stamp: None,
                temp: Some(PathBuf::from("unsaved-1-1.txt")),
                path: Some(PathBuf::from("/work/README.md")),
            },
            SessionEntry {
                kind: EntryKind::HugeEdits,
                line: 5_000_000,
                col: 3,
                markdown_view: false,
                active: false,
                crlf: true,
                encoding: None,
                disk_stamp: None,
                temp: Some(PathBuf::from("huge-1-2.vzedit")),
                path: Some(PathBuf::from("/data/big.log")),
            },
            SessionEntry {
                kind: EntryKind::Terminal,
                line: 0,
                col: 0,
                markdown_view: false,
                active: true,
                crlf: false,
                encoding: None,
                disk_stamp: None,
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
                encoding: None,
                disk_stamp: None,
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
    fn test_line_ending_field_old_and_new() {
        assert_eq!(parse_line_ending_field("1"), (true, None, None));
        assert_eq!(parse_line_ending_field("0"), (false, None, None));
        assert_eq!(
            parse_line_ending_field("0;UTF-8 BOM"),
            (false, Some("UTF-8 BOM".into()), None)
        );
        assert_eq!(
            parse_line_ending_field("1;UTF-8;5;6"),
            (true, Some("UTF-8".into()), Some((5, 6)))
        );
    }

    #[test]
    fn test_temp_is_written_as_file_name_and_resolved_in_dir() {
        let entry = SessionEntry {
            kind: EntryKind::Unsaved,
            temp: Some(PathBuf::from("/somewhere/old/unsaved-9-0.txt")),
            ..Default::default()
        };
        let text = serialize(&[entry]);
        assert!(text.contains("\tunsaved-9-0.txt\t"));
        let dir = PathBuf::from("/new/place");
        assert_eq!(
            parse_in(&dir, &text)[0].temp,
            Some(dir.join("unsaved-9-0.txt"))
        );
    }

    #[test]
    fn test_parse_saved_name() {
        assert_eq!(parse_saved_name("123"), Some((123, 0)));
        assert_eq!(parse_saved_name("123-2"), Some((123, 2)));
        assert_eq!(parse_saved_name(".writing-123"), None);
        assert_eq!(parse_saved_name("restoring-1-2"), None);
        assert_eq!(parse_saved_name("abc"), None);
    }

    fn write_saved(saved: &Path, pid: u32, path: &str) -> PathBuf {
        let writing = begin_saved_state(saved).unwrap();
        let entry = SessionEntry {
            kind: EntryKind::File,
            path: Some(PathBuf::from(path)),
            ..Default::default()
        };
        append_session(&writing, &[entry]).unwrap();
        publish_saved_state(saved, &writing, pid).unwrap()
    }

    #[test]
    fn test_saved_states_are_taken_one_by_one_in_pid_order() {
        let saved = std::env::temp_dir().join(format!("vz_saved_{}", std::process::id()));
        let _ = fs::remove_dir_all(&saved);
        assert!(take_saved_state(&saved).is_none());

        assert_eq!(write_saved(&saved, 300, "c"), saved.join("300"));
        write_saved(&saved, 20, "a");
        // 同じ PID（再利用）は連番を付けて両方残す
        assert_eq!(write_saved(&saved, 300, "d"), saved.join("300-1"));
        // 書きかけ・名前が数値でないフォルダは対象外
        fs::create_dir_all(saved.join("note")).unwrap();
        fs::write(
            saved.join("note").join(SESSION_FILE),
            "file\t0\t0\t0\t0\t0\t\tx\n",
        )
        .unwrap();

        let mut order = Vec::new();
        while let Some(state) = take_saved_state(&saved) {
            assert!(state
                .dir
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(RESTORING_PREFIX));
            order.push((state.pid, state.entries[0].path.clone().unwrap()));
            finish_saved_state(&saved, &state, &[]).unwrap();
            assert!(!state.dir.exists());
        }
        assert_eq!(
            order,
            vec![
                (20, PathBuf::from("a")),
                (300, PathBuf::from("c")),
                (300, PathBuf::from("d")),
            ]
        );
        let _ = fs::remove_dir_all(&saved);
    }

    #[test]
    fn test_failed_entries_are_published_again() {
        let saved = std::env::temp_dir().join(format!("vz_saved_retry_{}", std::process::id()));
        let _ = fs::remove_dir_all(&saved);
        write_saved(&saved, 7, "a");
        let state = take_saved_state(&saved).unwrap();
        let temp = state.dir.join("huge-7-0.vzedit");
        fs::write(&temp, "edits").unwrap();
        let failed = SessionEntry {
            kind: EntryKind::HugeEdits,
            temp: Some(temp),
            path: Some(PathBuf::from("big.log")),
            ..Default::default()
        };
        finish_saved_state(&saved, &state, std::slice::from_ref(&failed)).unwrap();
        let again = take_saved_state(&saved).unwrap();
        assert_eq!(again.pid, 7);
        assert_eq!(again.entries.len(), 1);
        let temp = again.entries[0].temp.clone().unwrap();
        assert_eq!(fs::read_to_string(temp).unwrap(), "edits");
        let _ = fs::remove_dir_all(&saved);
    }

    #[test]
    fn test_request_without_running_instance_returns_zero() {
        let dir = std::env::temp_dir().join(format!("vz_session_req_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(request_save_and_quit(&dir).unwrap(), 0);
        assert!(!dir.join(REQUEST_FILE).exists());

        // 同じウィンドウが開いたままなら、別のコマンドから再試行できる。
        let flag = start_watcher(dir.clone(), || {});
        let dir2 = dir.clone();
        let flag2 = flag.clone();
        let responder = std::thread::spawn(move || {
            for _ in 0..2 {
                assert!(wait_until(Duration::from_secs(10), || flag2
                    .swap(false, Ordering::SeqCst)));
                finish_request(&dir2);
            }
        });
        assert_eq!(request_save_and_quit(&dir).unwrap(), 1);
        assert_eq!(request_save_and_quit(&dir).unwrap(), 1);
        responder.join().unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}
