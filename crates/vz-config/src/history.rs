//! ファイル履歴・フォルダ履歴（`<設定フォルダ>/vzeditor/history.ini`）。
//!
//! 全プロセスで 1 つのファイルを共有する。記録のたびにファイルを読み直して追加し、すぐ書き戻すので、
//! 他のプロセスが書いた履歴は消えない。新しいものが先頭で、各 [`MAX_ENTRIES`] 件を超えたら古いものから消す。
//!
//! ```text
//! [files]
//! UTF-8<TAB>C:\work\a.txt
//! [folders]
//! C:\work
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// ファイル履歴・フォルダ履歴それぞれの上限。
pub const MAX_ENTRIES: usize = 100;

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

/// 記録する出来事。
#[derive(Debug, Clone, Copy)]
pub enum Visit<'a> {
    /// ファイルを開いた・保存した（パス、文字コードの表示名）
    File(&'a Path, &'a str),
    /// フォルダを開いた
    Folder(&'a Path),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    /// 新しい順
    pub files: Vec<FileRecord>,
    /// 新しい順
    pub folders: Vec<PathBuf>,
}

impl History {
    pub fn parse(text: &str) -> Self {
        let mut history = Self::default();
        let mut in_files = false;
        let mut in_folders = false;
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') {
                in_files = line == "[files]";
                in_folders = line == "[folders]";
                continue;
            }
            if in_files && history.files.len() < MAX_ENTRIES {
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
            } else if in_folders && history.folders.len() < MAX_ENTRIES {
                let path = Path::new(line);
                if !history.folders.iter().any(|d| same_path(d, path)) {
                    history.folders.push(path.to_path_buf());
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
        text
    }

    /// 先頭に加える。同じパスがあれば先頭へ移し、上限を超えた古いものを消す。
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
            Visit::Folder(path) => {
                let Some(path) = normalize(path) else {
                    return;
                };
                self.folders.retain(|d| !same_path(d, &path));
                self.folders.insert(0, path);
                self.folders.truncate(MAX_ENTRIES);
            }
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
        let mut history = Self::load_from(file);
        for &visit in visits {
            history.apply(visit);
        }
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
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
        }
        assert_eq!(h.files.len(), MAX_ENTRIES);
        assert_eq!(h.folders.len(), MAX_ENTRIES);
        assert_eq!(h.files[0].path, abs(&format!("f{}.txt", MAX_ENTRIES + 4)));
        assert_eq!(h.files[MAX_ENTRIES - 1].path, abs("f5.txt"));
        assert_eq!(h.folders[MAX_ENTRIES - 1], abs("d5"));
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
        assert_eq!(History::parse(&h.serialize()), h);
        assert_eq!(History::parse(""), History::default());
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
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
