use chrono::{DateTime, Local};
use std::cmp::Reverse;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    Left,
    Right,
}

impl ActivePane {
    pub fn toggled(self) -> Self {
        match self {
            ActivePane::Left => ActivePane::Right,
            ActivePane::Right => ActivePane::Left,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    NameAsc,
    ModifiedDesc,
    SizeDesc,
    ExtAsc,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            SortMode::NameAsc => SortMode::ModifiedDesc,
            SortMode::ModifiedDesc => SortMode::SizeDesc,
            SortMode::SizeDesc => SortMode::ExtAsc,
            SortMode::ExtAsc => SortMode::NameAsc,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortMode::NameAsc => "名前順",
            SortMode::ModifiedDesc => "更新順",
            SortMode::SizeDesc => "サイズ順",
            SortMode::ExtAsc => "拡張子順",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_parent: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub modified_str: String,
}

pub struct FilerPane {
    pub current_dir: PathBuf,
    pub entries: Vec<FileEntry>,
    pub cursor: usize,
    pub scroll_y: f32,
    pub need_scroll_to_cursor: bool,
    pub marked: HashSet<PathBuf>,
    pub sort_mode: SortMode,
}

impl FilerPane {
    pub fn new(initial_dir: PathBuf) -> Self {
        let dir = fs::canonicalize(&initial_dir).unwrap_or(initial_dir);
        let mut pane = Self {
            current_dir: dir,
            entries: Vec::new(),
            cursor: 0,
            scroll_y: 0.0,
            need_scroll_to_cursor: false,
            marked: HashSet::new(),
            sort_mode: SortMode::NameAsc,
        };
        let _ = pane.refresh();
        pane
    }

    pub fn refresh(&mut self) -> io::Result<()> {
        let prev_name = self.entries.get(self.cursor).map(|e| e.name.clone());
        let mut entries: Vec<FileEntry> = Vec::new();

        // Add parent directory entry `..` unless at filesystem root
        if let Some(parent) = self.current_dir.parent() {
            entries.push(FileEntry {
                name: "..".to_string(),
                path: parent.to_path_buf(),
                is_dir: true,
                is_parent: true,
                size: 0,
                modified: None,
                modified_str: String::new(),
            });
        }

        let mut children: Vec<FileEntry> = Vec::new();
        for dir_entry in fs::read_dir(&self.current_dir)? {
            let Ok(entry) = dir_entry else {
                continue;
            };
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let meta = fs::metadata(&path).or_else(|_| entry.metadata());
            let (is_dir, size, modified) = match meta {
                Ok(m) => (m.is_dir(), m.len(), m.modified().ok()),
                Err(_) => (false, 0, None),
            };
            let modified_str = modified
                .map(format_system_time)
                .unwrap_or_else(|| "----/--/-- --:--".to_string());
            children.push(FileEntry {
                name,
                path,
                is_dir,
                is_parent: false,
                size,
                modified,
                modified_str,
            });
        }

        sort_entries(&mut children, self.sort_mode);
        entries.extend(children);

        // Remove marks for paths that no longer exist in this directory
        if !self.marked.is_empty() {
            let valid_paths: HashSet<&Path> = entries
                .iter()
                .filter(|e| !e.is_parent)
                .map(|e| e.path.as_path())
                .collect();
            self.marked.retain(|p| valid_paths.contains(p.as_path()));
        }

        self.entries = entries;

        // Restore cursor on previously focused item if present
        if let Some(name) = prev_name {
            if let Some(idx) = self.entries.iter().position(|e| e.name == name) {
                self.cursor = idx;
            }
        }
        self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
        Ok(())
    }

    pub fn enter_dir(&mut self, dir: PathBuf) -> io::Result<()> {
        let canonical = fs::canonicalize(&dir).unwrap_or(dir);
        let prev_dir = self.current_dir.clone();
        self.current_dir = canonical;
        self.marked.clear();
        self.cursor = 0;
        self.scroll_y = 0.0;
        if let Err(e) = self.refresh() {
            self.current_dir = prev_dir;
            let _ = self.refresh();
            return Err(e);
        }
        self.need_scroll_to_cursor = true;
        Ok(())
    }

    pub fn go_parent(&mut self) -> io::Result<bool> {
        let Some(parent) = self.current_dir.parent().map(|p| p.to_path_buf()) else {
            return Ok(false);
        };
        let old_name = self
            .current_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        self.enter_dir(parent)?;
        if let Some(name) = old_name {
            if let Some(idx) = self.entries.iter().position(|e| e.name == name) {
                self.cursor = idx;
                self.need_scroll_to_cursor = true;
            }
        }
        Ok(true)
    }

    pub fn selected_entry(&self) -> Option<&FileEntry> {
        self.entries.get(self.cursor)
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.need_scroll_to_cursor = true;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < self.entries.len() {
            self.cursor += 1;
            self.need_scroll_to_cursor = true;
        }
    }

    pub fn move_page_up(&mut self, page_rows: usize) {
        self.cursor = self.cursor.saturating_sub(page_rows.max(1));
        self.need_scroll_to_cursor = true;
    }

    pub fn move_page_down(&mut self, page_rows: usize) {
        if !self.entries.is_empty() {
            self.cursor = (self.cursor + page_rows.max(1)).min(self.entries.len() - 1);
            self.need_scroll_to_cursor = true;
        }
    }

    pub fn move_top(&mut self) {
        self.cursor = 0;
        self.need_scroll_to_cursor = true;
    }

    pub fn move_bottom(&mut self) {
        if !self.entries.is_empty() {
            self.cursor = self.entries.len() - 1;
            self.need_scroll_to_cursor = true;
        }
    }

    /// Toggle mark on the current entry (`Space` key) and advance cursor down.
    pub fn toggle_mark_and_advance(&mut self) {
        if let Some(entry) = self.entries.get(self.cursor) {
            if !entry.is_parent && !self.marked.remove(&entry.path) {
                self.marked.insert(entry.path.clone());
            }
        }
        self.move_down();
    }

    pub fn cycle_sort(&mut self) -> SortMode {
        self.sort_mode = self.sort_mode.next();
        let _ = self.refresh();
        self.sort_mode
    }

    /// Returns all marked entries, or if none are marked, the entry under the cursor (excluding `..`).
    pub fn target_entries(&self) -> Vec<FileEntry> {
        if !self.marked.is_empty() {
            self.entries
                .iter()
                .filter(|e| !e.is_parent && self.marked.contains(&e.path))
                .cloned()
                .collect()
        } else {
            self.selected_entry()
                .filter(|e| !e.is_parent)
                .cloned()
                .into_iter()
                .collect()
        }
    }
}

pub struct FilerState {
    pub left: FilerPane,
    pub right: FilerPane,
    pub active: ActivePane,
}

impl FilerState {
    pub fn new(initial_dir: PathBuf) -> Self {
        Self {
            left: FilerPane::new(initial_dir.clone()),
            right: FilerPane::new(initial_dir),
            active: ActivePane::Left,
        }
    }

    pub fn active_pane(&self) -> &FilerPane {
        match self.active {
            ActivePane::Left => &self.left,
            ActivePane::Right => &self.right,
        }
    }

    pub fn active_pane_mut(&mut self) -> &mut FilerPane {
        match self.active {
            ActivePane::Left => &mut self.left,
            ActivePane::Right => &mut self.right,
        }
    }

    pub fn inactive_pane(&self) -> &FilerPane {
        match self.active {
            ActivePane::Left => &self.right,
            ActivePane::Right => &self.left,
        }
    }

    pub fn refresh_both(&mut self) {
        let _ = self.left.refresh();
        let _ = self.right.refresh();
    }
}

fn sort_entries(entries: &mut [FileEntry], mode: SortMode) {
    // 比較ごとに小文字化しないよう、キーを 1 回だけ作って並べる（安定ソート）。
    // フォルダを常にファイルより前に置く（classic VZ / FDclone style）。
    entries.sort_by_cached_key(|e| {
        let (modified, size, ext) = match mode {
            SortMode::NameAsc => (None, 0, String::new()),
            SortMode::ModifiedDesc => (e.modified, 0, String::new()),
            SortMode::SizeDesc => (None, e.size, String::new()),
            SortMode::ExtAsc => (
                None,
                0,
                Path::new(&e.name)
                    .extension()
                    .and_then(|x| x.to_str())
                    .unwrap_or("")
                    .to_lowercase(),
            ),
        };
        (
            Reverse(e.is_dir),
            Reverse(modified),
            Reverse(size),
            ext,
            e.name.to_lowercase(),
        )
    });
}

fn format_system_time(st: SystemTime) -> String {
    let dt: DateTime<Local> = st.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

pub fn format_size(size: u64) -> String {
    if size < 10_000 {
        format!("{size} B")
    } else if size < 1024 * 1024 {
        format!("{:.1} K", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.1} M", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} G", size as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn validate_single_name(name: &str) -> io::Result<&str> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "無効なファイル名です",
        ));
    }
    Ok(trimmed)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_child = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_child)?;
        } else {
            fs::copy(entry.path(), dst_child)?;
        }
    }
    Ok(())
}

/// コピー・移動（`op` は "コピー" / "移動"）の前の確認。宛先のパスを返す。
fn check_transfer(src: &Path, dst_dir: &Path, op: &str) -> io::Result<PathBuf> {
    let name = src.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{op}元ファイル名が取得できません"),
        )
    })?;
    let dst = dst_dir.join(name);

    let canon_src = fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());
    let canon_dst_dir = fs::canonicalize(dst_dir).unwrap_or_else(|_| dst_dir.to_path_buf());
    if canon_src == canon_dst_dir.join(name) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{op}元と{op}先が同じです"),
        ));
    }
    if src.is_dir() && canon_dst_dir.starts_with(&canon_src) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("ディレクトリを自身の配下へ{op}できません"),
        ));
    }
    Ok(dst)
}

/// Copy a file or directory into `dst_dir`.
pub fn copy_entry(src: &Path, dst_dir: &Path) -> io::Result<PathBuf> {
    let dst = check_transfer(src, dst_dir, "コピー")?;

    if src.is_dir() {
        copy_dir_recursive(src, &dst)?;
    } else {
        fs::copy(src, &dst)?;
    }
    Ok(dst)
}

/// Move a file or directory into `dst_dir` (falls back to copy+delete across filesystems).
pub fn move_entry(src: &Path, dst_dir: &Path) -> io::Result<PathBuf> {
    let dst = check_transfer(src, dst_dir, "移動")?;

    match fs::rename(src, &dst) {
        Ok(()) => Ok(dst),
        Err(_) => {
            copy_entry(src, dst_dir)?;
            delete_entry(src)?;
            Ok(dst)
        }
    }
}

pub fn delete_entry(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

pub fn rename_entry(src: &Path, new_name: &str) -> io::Result<PathBuf> {
    let valid = validate_single_name(new_name)?;
    let parent = src.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "親ディレクトリが見つかりません",
        )
    })?;
    let dst = parent.join(valid);
    if dst == src {
        return Ok(dst);
    }
    if dst.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "同名のファイルまたはフォルダが既に存在します",
        ));
    }
    fs::rename(src, &dst)?;
    Ok(dst)
}

pub fn create_file(dir: &Path, name: &str) -> io::Result<PathBuf> {
    let valid = validate_single_name(name)?;
    let dst = dir.join(valid);
    if dst.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "同名のファイルが既に存在します",
        ));
    }
    fs::write(&dst, "")?;
    Ok(dst)
}

pub fn create_dir(dir: &Path, name: &str) -> io::Result<PathBuf> {
    let valid = validate_single_name(name)?;
    let dst = dir.join(valid);
    fs::create_dir(&dst)?;
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_sort_entries_order() {
        let t = |secs: u64| Some(UNIX_EPOCH + std::time::Duration::from_secs(secs));
        let e = |name: &str, is_dir: bool, size: u64, modified| FileEntry {
            name: name.to_string(),
            path: PathBuf::from(name),
            is_dir,
            is_parent: false,
            size,
            modified,
            modified_str: String::new(),
        };
        let base = vec![
            e("b.txt", false, 10, t(2)),
            e("A.md", false, 30, t(1)),
            e("zdir", true, 0, t(5)),
            e("a.TXT", false, 10, None),
            e("Adir", true, 0, t(9)),
            e("c.md", false, 30, t(2)),
        ];
        let names = |mode| {
            let mut v = base.clone();
            sort_entries(&mut v, mode);
            v.into_iter().map(|e| e.name).collect::<Vec<_>>()
        };
        assert_eq!(
            names(SortMode::NameAsc),
            ["Adir", "zdir", "A.md", "a.TXT", "b.txt", "c.md"]
        );
        assert_eq!(
            names(SortMode::ModifiedDesc),
            ["Adir", "zdir", "b.txt", "c.md", "A.md", "a.TXT"]
        );
        assert_eq!(
            names(SortMode::SizeDesc),
            ["Adir", "zdir", "A.md", "c.md", "a.TXT", "b.txt"]
        );
        assert_eq!(
            names(SortMode::ExtAsc),
            ["Adir", "zdir", "A.md", "c.md", "a.TXT", "b.txt"]
        );
    }

    fn make_temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("lz_editor_test_{tag}_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_filer_pane_navigation_mark_and_file_ops() {
        let root = make_temp_dir("ops");
        let left_dir = root.join("left");
        let right_dir = root.join("right");
        fs::create_dir_all(&left_dir).unwrap();
        fs::create_dir_all(&right_dir).unwrap();

        let f1 = create_file(&left_dir, "alpha.txt").unwrap();
        fs::write(&f1, "hello").unwrap();
        let sub = create_dir(&left_dir, "sub").unwrap();
        let _f2 = create_file(&sub, "nested.txt").unwrap();

        let mut pane = FilerPane::new(left_dir.clone());
        // Entries: `..`, `sub` (dir first), `alpha.txt`
        assert_eq!(pane.entries.len(), 3);
        assert!(pane.entries[0].is_parent);
        assert_eq!(pane.entries[1].name, "sub");
        assert!(pane.entries[1].is_dir);
        assert_eq!(pane.entries[2].name, "alpha.txt");

        // Mark `alpha.txt`
        pane.cursor = 2;
        pane.toggle_mark_and_advance();
        assert_eq!(pane.marked.len(), 1);
        assert_eq!(pane.target_entries().len(), 1);

        // Copy to right_dir
        let copied = copy_entry(&f1, &right_dir).unwrap();
        assert!(copied.exists());
        assert_eq!(fs::read_to_string(&copied).unwrap(), "hello");

        // Rename copied file
        let renamed = rename_entry(&copied, "beta.txt").unwrap();
        assert!(renamed.exists());
        assert!(!copied.exists());

        // Move `sub` directory to right_dir
        let moved_sub = move_entry(&sub, &right_dir).unwrap();
        assert!(moved_sub.join("nested.txt").exists());
        assert!(!sub.exists());

        // Delete moved_sub
        delete_entry(&moved_sub).unwrap();
        assert!(!moved_sub.exists());

        let _ = fs::remove_dir_all(&root);
    }
}
