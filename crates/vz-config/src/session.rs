//! 利用者が記録する状態と、リリース時に引き継ぐ状態の記録形式。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::settings::WindowGeometry;

const SESSION_FILE: &str = "session.ini";

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

/// ウィンドウの位置（物理モニター）。`session.ini` の `monitor` 行。
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorState {
    /// `settings.ini` の `window_*` と同じ意味
    pub geometry: WindowGeometry,
    /// モニターの名前（Windows の `\\.\DISPLAY1` など。記録に残すだけ）
    pub name: Option<String>,
}

/// 仮想デスクトップ。`session.ini` の `desktop` 行（取得できたときだけ書く）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopState {
    /// 取得した仕組み（`windows` / `x11`）
    pub backend: String,
    /// Windows: GUID、X11: デスクトップ番号
    pub id: String,
    pub name: Option<String>,
}

/// 1 つの vz のウィンドウの状態。物理モニターと仮想デスクトップを別々に持つ。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionWindow {
    pub monitor: Option<MonitorState>,
    /// `monitor` が無いときは書かない・使わない
    pub desktop: Option<DesktopState>,
}

/// 名前の欄: 行が壊れないようタブ・改行を空白にする。
fn name_field(name: Option<&str>) -> String {
    name.unwrap_or_default().replace(['\t', '\r', '\n'], " ")
}

fn opt_name(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// `monitor` 行と、その直後の `desktop` 行（位置が無ければ何も書かない）。
pub fn serialize_window(w: &SessionWindow) -> String {
    let Some(m) = &w.monitor else {
        return String::new();
    };
    let g = &m.geometry;
    let mut out = format!(
        "monitor\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        g.x,
        g.y,
        g.width,
        g.height,
        g.scale,
        flag(g.maximized),
        name_field(m.name.as_deref())
    );
    if let Some(d) = &w.desktop {
        out.push_str(&format!(
            "desktop\t{}\t{}\t{}\n",
            name_field(Some(&d.backend)),
            name_field(Some(&d.id)),
            name_field(d.name.as_deref())
        ));
    }
    out
}

fn parse_monitor(line: &str) -> Option<MonitorState> {
    let f: Vec<&str> = line.splitn(8, '\t').collect();
    if f.len() != 8 || f[0] != "monitor" {
        return None;
    }
    let positive = |s: &str| s.parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0);
    Some(MonitorState {
        geometry: WindowGeometry {
            x: f[1].parse().ok()?,
            y: f[2].parse().ok()?,
            width: positive(f[3])?,
            height: positive(f[4])?,
            scale: positive(f[5])?,
            maximized: f[6] == "1",
        },
        name: opt_name(f[7]),
    })
}

fn parse_desktop(line: &str) -> Option<DesktopState> {
    let f: Vec<&str> = line.splitn(4, '\t').collect();
    if f.len() != 4 || f[0] != "desktop" {
        return None;
    }
    Some(DesktopState {
        backend: opt_name(f[1])?,
        id: opt_name(f[2])?,
        name: opt_name(f[3]),
    })
}

/// 最初の `monitor` 行と、その直後にある `desktop` 行を読む（複数の vz の追記で、
/// 別の vz の位置とデスクトップを組み合わせないため）。
pub fn parse_window(text: &str) -> SessionWindow {
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if let Some(monitor) = parse_monitor(line) {
            let desktop = lines.next().and_then(parse_desktop);
            return SessionWindow {
                monitor: Some(monitor),
                desktop,
            };
        }
    }
    SessionWindow::default()
}

/// 一時保存ファイルのパス（プロセス ID と連番で重複させない）。
pub fn temp_path(dir: &Path, index: usize) -> PathBuf {
    dir.join(format!("unsaved-{}-{index}.txt", std::process::id()))
}

/// 巨大ファイルの未保存の編集内容を一時保存するパス。
pub fn huge_edits_path(dir: &Path, index: usize) -> PathBuf {
    dir.join(format!("huge-{}-{index}.vzedit", std::process::id()))
}

/// 1 ウィンドウ分の状態を書き込む。
pub fn write_session(
    dir: &Path,
    entries: &[SessionEntry],
    window: &SessionWindow,
) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let mut text = serialize_window(window);
    text.push_str(&serialize(entries));
    fs::write(dir.join(SESSION_FILE), text)
}

/// 状態を読み込む。呼び出し側が復元後に記録を処理する。
pub fn read_session(dir: &Path) -> io::Result<(Vec<SessionEntry>, SessionWindow)> {
    let text = fs::read_to_string(dir.join(SESSION_FILE))?;
    Ok((parse_in(dir, &text), parse_window(&text)))
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

pub mod release;
mod saved_state;

pub use saved_state::{
    begin_saved_state, finish_saved_state, publish_saved_state, saved_dir, take_saved_state,
    SavedState,
};
#[cfg(test)]
mod tests {
    use super::*;

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

    fn sample_window() -> SessionWindow {
        SessionWindow {
            monitor: Some(MonitorState {
                geometry: WindowGeometry {
                    x: -1920,
                    y: 40,
                    width: 1024.0,
                    height: 680.5,
                    scale: 1.5,
                    maximized: true,
                },
                name: Some(r"\\.\DISPLAY2".into()),
            }),
            desktop: Some(DesktopState {
                backend: "windows".into(),
                id: "{AA509086-5CA9-4C25-8F95-589D3C07B48A}".into(),
                name: Some("仕事\tA".into()),
            }),
        }
    }

    /// 読み直した `sample_window`（名前のタブは空白になる）
    fn sample_window_read() -> SessionWindow {
        let mut w = sample_window();
        if let Some(d) = w.desktop.as_mut() {
            d.name = Some("仕事 A".into());
        }
        w
    }

    #[test]
    fn test_window_roundtrip_and_compat() {
        let w = sample_window();
        let text = serialize_window(&w);
        assert_eq!(parse_window(&text), sample_window_read());
        // 項目と混ぜても parse は項目だけを返す（古い版も monitor / desktop 行を読み飛ばす）
        let entry = SessionEntry {
            kind: EntryKind::File,
            path: Some(PathBuf::from("a.txt")),
            ..Default::default()
        };
        let all = format!("{text}{}", serialize(std::slice::from_ref(&entry)));
        assert_eq!(parse(&all), vec![entry]);
        assert_eq!(parse_window(&all), sample_window_read());
        // 古い記録（行が無い）
        assert_eq!(parse_window(&serialize(&[])), SessionWindow::default());
        // 仮想デスクトップが取れなかった vz
        let no_desktop = SessionWindow { desktop: None, ..w };
        assert_eq!(parse_window(&serialize_window(&no_desktop)), no_desktop);
        // 位置が無ければ何も書かない
        assert_eq!(serialize_window(&SessionWindow::default()), "");
    }

    #[test]
    fn test_desktop_must_follow_first_monitor() {
        let a = SessionWindow {
            desktop: None,
            ..sample_window()
        };
        // A（デスクトップなし）の後に B が追記されても、A の位置と B のデスクトップを組み合わせない
        let text = format!(
            "{}file\t0\t0\t0\t0\t0\t\tx\n{}",
            serialize_window(&a),
            serialize_window(&sample_window())
        );
        assert_eq!(parse_window(&text), a);
    }

    fn write_saved(saved: &Path, pid: u32, path: &str) -> PathBuf {
        let writing = begin_saved_state(saved).unwrap();
        let entry = SessionEntry {
            kind: EntryKind::File,
            path: Some(PathBuf::from(path)),
            ..Default::default()
        };
        write_session(&writing, &[entry], &sample_window()).unwrap();
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
                .starts_with("restoring-"));
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
        assert_eq!(state.window, sample_window_read());
        let again = take_saved_state(&saved).unwrap();
        assert_eq!(again.pid, 7);
        // 再公開した記録にはウィンドウの行が無い
        assert_eq!(again.window, SessionWindow::default());
        assert_eq!(again.entries.len(), 1);
        let temp = again.entries[0].temp.clone().unwrap();
        assert_eq!(fs::read_to_string(temp).unwrap(), "edits");
        let _ = fs::remove_dir_all(&saved);
    }
}
