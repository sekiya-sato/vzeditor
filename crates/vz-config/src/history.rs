//! ファイル・フォルダ・起動したファイル・検索入力の履歴（`<設定フォルダ>/vzeditor/history.ini`）。
//!
//! 全プロセスで 1 つのファイルを共有する。記録のたびにファイルを読み直して追加し、すぐ書き戻すので、
//! 他のプロセスが書いた履歴は消えない。新しいものが先頭で、上限を超えたら古いものから消す。
//!
//! ```text
//! [files]
//! UTF-8<TAB>C:\work\a.txt
//! [folders]
//! C:\work
//! [launched_files]
//! C:\work\tool.exe
//! [search]
//! =検索語
//! ```

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// ファイル・フォルダ・起動したファイルの履歴それぞれの上限。
pub const MAX_ENTRIES: usize = 100;

const MAX_INPUT_ENTRIES: usize = 50;

const HISTORY_FILE: &str = "history.ini";

/// 履歴を保存するファイル。
pub fn history_path() -> Option<PathBuf> {
    crate::settings::app_dir().map(|d| d.join(HISTORY_FILE))
}

/// ファイル履歴の 1 件。`encoding` は開いた・保存したときの文字コードの表示名（例: `UTF-8`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileRecord {
    pub path: PathBuf,
    pub encoding: String,
}

/// 履歴に反映する操作。
#[derive(Debug, Clone, Copy)]
pub enum Visit<'a> {
    /// ファイルを開いた・保存した（パス、文字コードの表示名）
    File(&'a Path, &'a str),
    /// フォルダを開いた
    Folder(&'a Path),
    LaunchFile(&'a Path),
    RemoveFile(&'a Path),
    RemoveFolder(&'a Path),
    RemoveLaunchedFile(&'a Path),
    Search(&'a str),
    Replace(&'a str),
    GrepRoot(&'a str),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    /// 新しい順
    pub files: Vec<FileRecord>,
    /// 新しい順
    pub folders: Vec<PathBuf>,
    /// OS の関連付けで起動したファイル。新しい順。
    pub launched_files: Vec<PathBuf>,
    pub queries: Vec<String>,
    pub replacements: Vec<String>,
    pub grep_roots: Vec<String>,
}

impl History {
    pub fn parse(text: &str) -> Self {
        let mut history = Self::default();
        let mut section = "";
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') {
                section = line;
                continue;
            }
            if section == "[files]" && history.files.len() < MAX_ENTRIES {
                if let Some((encoding, path)) = line.split_once('\t') {
                    if !path.is_empty()
                        && !history
                            .files
                            .iter()
                            .any(|f| same_path(&f.path, Path::new(path)))
                    {
                        history.files.push(FileRecord {
                            path: PathBuf::from(path),
                            encoding: encoding.to_string(),
                        });
                    }
                }
            } else if matches!(section, "[folders]" | "[launched_files]") {
                let entries = if section == "[folders]" {
                    &mut history.folders
                } else {
                    &mut history.launched_files
                };
                let path = Path::new(line);
                if entries.len() < MAX_ENTRIES && !entries.iter().any(|p| same_path(p, path)) {
                    entries.push(path.to_path_buf());
                }
            } else {
                let entries = match section {
                    "[search]" => &mut history.queries,
                    "[replace]" => &mut history.replacements,
                    "[grep_roots]" => &mut history.grep_roots,
                    _ => continue,
                };
                if entries.len() < MAX_INPUT_ENTRIES {
                    if let Some(value) = line.strip_prefix('=').and_then(decode_input) {
                        if !value.is_empty() && !entries.contains(&value) {
                            entries.push(value);
                        }
                    }
                }
            }
        }
        history
    }

    pub fn serialize(&self) -> String {
        let mut text = String::from("[files]\n");
        for f in &self.files {
            text.push_str(&format!("{}\t{}\n", f.encoding, f.path.display()));
        }
        text.push_str("[folders]\n");
        for d in &self.folders {
            text.push_str(&format!("{}\n", d.display()));
        }
        if !self.launched_files.is_empty() {
            text.push_str("[launched_files]\n");
            for path in &self.launched_files {
                text.push_str(&format!("{}\n", path.display()));
            }
        }
        for (section, entries) in [
            ("[search]", &self.queries),
            ("[replace]", &self.replacements),
            ("[grep_roots]", &self.grep_roots),
        ] {
            if entries.is_empty() {
                continue;
            }
            text.push_str(section);
            text.push('\n');
            for value in entries {
                text.push('=');
                text.push_str(
                    &value
                        .replace('\\', "\\\\")
                        .replace('\r', "\\r")
                        .replace('\n', "\\n"),
                );
                text.push('\n');
            }
        }
        text
    }

    /// 先頭に加えるか、指定された履歴を削除する。
    pub fn apply(&mut self, visit: Visit) {
        match visit {
            Visit::File(path, encoding) => {
                let Some(path) = normalize(path) else {
                    return;
                };
                self.files.retain(|f| !same_path(&f.path, &path));
                self.files.insert(
                    0,
                    FileRecord {
                        path,
                        encoding: encoding.to_string(),
                    },
                );
                self.files.truncate(MAX_ENTRIES);
            }
            Visit::Folder(path) => remember_path(&mut self.folders, path),
            Visit::LaunchFile(path) => remember_path(&mut self.launched_files, path),
            Visit::RemoveFile(path) => self.files.retain(|f| !same_path(&f.path, path)),
            Visit::RemoveFolder(path) => self.folders.retain(|d| !same_path(d, path)),
            Visit::RemoveLaunchedFile(path) => self.launched_files.retain(|p| !same_path(p, path)),
            Visit::Search(value) => remember_input(&mut self.queries, value),
            Visit::Replace(value) => remember_input(&mut self.replacements, value),
            Visit::GrepRoot(value) => remember_input(&mut self.grep_roots, value),
        }
    }

    /// 読めなければ空の履歴。
    pub fn load_from(file: &Path) -> Self {
        fs::read_to_string(file)
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }

    /// `file` を読み直して `visits` を順に当て、書き戻す。書き戻した後の履歴を返す。
    pub fn record(file: &Path, visits: &[Visit]) -> io::Result<Self> {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        // 置き換える履歴ファイルとは別のファイルで、読み込みから書き戻しまでを排他する。
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(file.with_extension("lock"))?;
        lock.lock()?;
        let mut history = match fs::read_to_string(file) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e),
        };
        for &visit in visits {
            history.apply(visit);
        }
        // 書きかけを他のプロセスに読ませないよう、一時ファイルに書いてから置き換える
        let temp = file.with_extension(format!("ini.{}.tmp", std::process::id()));
        fs::write(&temp, history.serialize())?;
        if let Err(e) = fs::rename(&temp, file) {
            let _ = fs::remove_file(&temp);
            return Err(e);
        }
        Ok(history)
    }
}

fn remember_path(entries: &mut Vec<PathBuf>, path: &Path) {
    let Some(path) = normalize(path) else {
        return;
    };
    entries.retain(|entry| !same_path(entry, &path));
    entries.insert(0, path);
    entries.truncate(MAX_ENTRIES);
}

fn remember_input(entries: &mut Vec<String>, value: &str) {
    if value.is_empty() || entries.first().is_some_and(|entry| entry == value) {
        return;
    }
    entries.retain(|entry| entry != value);
    entries.insert(0, value.to_owned());
    entries.truncate(MAX_INPUT_ENTRIES);
}

fn decode_input(value: &str) -> Option<String> {
    let mut text = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        text.push(if ch == '\\' {
            match chars.next()? {
                '\\' => '\\',
                'r' => '\r',
                'n' => '\n',
                _ => return None,
            }
        } else {
            ch
        });
    }
    Some(text)
}

/// 記録するパス（`\\?\` を外し、相対パスは絶対パスにする）。改行・タブを含むものは記録しない。
pub fn normalize(path: &Path) -> Option<PathBuf> {
    let text = path.to_string_lossy();
    if text.is_empty() || text.contains(['\n', '\r', '\t']) {
        return None;
    }
    let plain = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    };
    Some(std::path::absolute(&plain).unwrap_or(plain))
}

/// 同じパスか（Windows・macOS は大文字小文字を区別しない）。
pub fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abs(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn test_apply_puts_newest_first_and_moves_duplicates() {
        let mut h = History::default();
        h.apply(Visit::File(&abs("a.txt"), "UTF-8"));
        h.apply(Visit::File(&abs("b.txt"), "Shift_JIS"));
        h.apply(Visit::File(&abs("a.txt"), "UTF-8 BOM"));
        assert_eq!(h.files.len(), 2);
        assert_eq!(h.files[0].path, abs("a.txt"));
        assert_eq!(h.files[0].encoding, "UTF-8 BOM");
        assert_eq!(h.files[1].path, abs("b.txt"));

        h.apply(Visit::Folder(&abs("d1")));
        h.apply(Visit::Folder(&abs("d2")));
        h.apply(Visit::Folder(&abs("d1")));
        assert_eq!(h.folders, vec![abs("d1"), abs("d2")]);
    }

    #[test]
    fn test_apply_drops_oldest_over_limit() {
        let mut h = History::default();
        for i in 0..MAX_ENTRIES + 5 {
            h.apply(Visit::File(&abs(&format!("f{i}.txt")), "UTF-8"));
            h.apply(Visit::Folder(&abs(&format!("d{i}"))));
            h.apply(Visit::LaunchFile(&abs(&format!("app{i}.exe"))));
        }
        assert_eq!(h.files.len(), MAX_ENTRIES);
        assert_eq!(h.folders.len(), MAX_ENTRIES);
        assert_eq!(h.files[0].path, abs(&format!("f{}.txt", MAX_ENTRIES + 4)));
        assert_eq!(h.files[MAX_ENTRIES - 1].path, abs("f5.txt"));
        assert_eq!(h.folders[MAX_ENTRIES - 1], abs("d5"));
        assert_eq!(h.launched_files.len(), MAX_ENTRIES);
        assert_eq!(h.launched_files[MAX_ENTRIES - 1], abs("app5.exe"));
    }

    #[test]
    fn test_normalize_strips_verbatim_prefix_and_rejects_control_chars() {
        let mut h = History::default();
        let plain = abs("v.txt");
        let verbatim = PathBuf::from(format!(r"\\?\{}", plain.display()));
        h.apply(Visit::File(&verbatim, "UTF-8"));
        if cfg!(windows) {
            assert_eq!(h.files[0].path, plain);
        }
        h.apply(Visit::File(Path::new("bad\nname"), "UTF-8"));
        h.apply(Visit::Folder(Path::new("")));
        assert_eq!(h.files.len(), 1);
        assert!(h.folders.is_empty());
    }

    #[test]
    fn test_serialize_round_trip() {
        let mut h = History::default();
        h.apply(Visit::File(&abs("a b.txt"), "UTF-8"));
        h.apply(Visit::File(&abs("c.txt"), "UTF-16LE"));
        h.apply(Visit::Folder(&abs("dir")));
        h.apply(Visit::LaunchFile(&abs("app a.exe")));
        assert_eq!(History::parse(&h.serialize()), h);
        assert_eq!(History::parse(""), History::default());
    }

    #[test]
    fn input_history_survives_reload_and_moves_reused_values_to_front() {
        let dir = std::env::temp_dir().join(format!("vz_input_history_{}", std::process::id()));
        let file = dir.join(HISTORY_FILE);
        let query = " [files] \t日本語 ";
        let replacement = "=$1\r\n\\n";
        let root = r"\\server\share\work";
        History::record(
            &file,
            &[
                Visit::Search(query),
                Visit::Replace(replacement),
                Visit::GrepRoot(root),
                Visit::File(&abs("keep.txt"), "UTF-8"),
            ],
        )
        .unwrap();
        History::record(
            &file,
            &[
                Visit::Search("次"),
                Visit::Replace("次の置換"),
                Visit::GrepRoot("次の場所"),
            ],
        )
        .unwrap();
        assert_eq!(History::load_from(&file).queries, ["次", query]);

        History::record(
            &file,
            &[
                Visit::Search(query),
                Visit::Replace(replacement),
                Visit::GrepRoot(root),
            ],
        )
        .unwrap();
        let restored = History::load_from(&file);
        assert_eq!(restored.queries, [query, "次"]);
        assert_eq!(restored.replacements, [replacement, "次の置換"]);
        assert_eq!(restored.grep_roots, [root, "次の場所"]);
        assert_eq!(restored.files[0].path, abs("keep.txt"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_record_merges_entries_written_by_other_process() {
        let dir = std::env::temp_dir().join(format!("vz_history_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let file = dir.join(HISTORY_FILE);

        // 自プロセスは古い履歴を持ったまま、他プロセスが a を書く
        let other = History::record(&file, &[Visit::File(&abs("a.txt"), "UTF-8")]).unwrap();
        assert_eq!(other.files.len(), 1);
        // 自プロセスが b と親フォルダを記録しても a は残る
        let mine = History::record(
            &file,
            &[
                Visit::File(&abs("b.txt"), "Shift_JIS"),
                Visit::Folder(&std::env::temp_dir()),
            ],
        )
        .unwrap();
        assert_eq!(
            mine.files
                .iter()
                .map(|f| f.path.clone())
                .collect::<Vec<_>>(),
            vec![abs("b.txt"), abs("a.txt")]
        );
        assert_eq!(mine.folders.len(), 1);
        assert_eq!(History::load_from(&file), mine);
        // 一時ファイルは残らない
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        fs::remove_dir_all(&dir).unwrap();
    }
}
