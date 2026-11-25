use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Whitespace,
    AsciiWord,
    AsciiPunct,
    Hiragana,
    Katakana,
    Cjk,
    Other,
}

fn classify_char(c: char) -> CharClass {
    if c.is_whitespace() {
        CharClass::Whitespace
    } else if c.is_ascii_alphanumeric() || c == '_' {
        CharClass::AsciiWord
    } else if c.is_ascii_punctuation() {
        CharClass::AsciiPunct
    } else if ('\u{3040}'..='\u{309F}').contains(&c) {
        CharClass::Hiragana
    } else if ('\u{30A0}'..='\u{30FF}').contains(&c) || ('\u{FF65}'..='\u{FF9F}').contains(&c) {
        CharClass::Katakana
    } else if ('\u{4E00}'..='\u{9FFF}').contains(&c) || ('\u{3400}'..='\u{4DBF}').contains(&c) {
        CharClass::Cjk
    } else {
        CharClass::Other
    }
}

/// Visual column width of a single character in a monospace terminal/editor grid.
pub fn char_display_width(c: char) -> usize {
    if c == '\t' {
        4
    } else {
        c.width().unwrap_or(1).max(1)
    }
}

/// Convert a character index in a line string to its byte index.
pub fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(s.len())
}

/// Expand tabs to spaces for consistent visual width display.
pub fn expand_tabs_for_display(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '\t' {
            out.push_str("    ");
        } else {
            out.push(c);
        }
    }
    out
}

const MAX_UNDO_HISTORY: usize = 200;

/// ディスク上のファイルの状態（更新日時, サイズ）。外部での変更の検知に使う。
pub type FileStamp = (SystemTime, u64);

/// ファイルの現在の状態。読めなければ None。
pub fn file_stamp(path: &Path) -> Option<FileStamp> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// ファイルを読み、改行を `\n` にそろえて行に分ける（元の改行コードも返す）。
pub fn read_lines(path: &Path) -> io::Result<(Vec<String>, &'static str)> {
    let raw = fs::read_to_string(path)?;
    let line_ending = if raw.contains("\r\n") { "\r\n" } else { "\n" };
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = normalized.split('\n').map(String::from).collect();
    if lines.is_empty() {
        lines.push(String::new());
    }
    Ok((lines, line_ending))
}

#[derive(Debug, Clone)]
struct UndoSnapshot {
    lines: Vec<String>,
    cursor_line: usize,
    cursor_col: usize,
    dirty: bool,
}

pub struct TextBuffer {
    pub lines: Vec<String>,
    /// 0-indexed current line
    pub cursor_line: usize,
    /// 0-indexed character column within `lines[cursor_line]`
    pub cursor_col: usize,
    /// Remembered visual column for vertical diamond movement (Ctrl+E / Ctrl+X)
    preferred_visual_col: Option<usize>,
    pub file_path: Option<PathBuf>,
    pub dirty: bool,
    /// Monotonically increasing revision incremented on every text mutation (for view caches)
    pub revision: u64,
    pub encoding: &'static str,
    pub line_ending: &'static str,
    undo_stack: Vec<UndoSnapshot>,
    redo_stack: Vec<UndoSnapshot>,
    /// 範囲選択の起点（行, 文字位置）。カーソルとの間が選択範囲。None なら選択なし。
    /// サクラ互換のキー割り当てでのみ使う（VZ 互換では常に None）。
    pub anchor: Option<(usize, usize)>,
    /// 読み込み・保存した時点のファイルの状態（外部での変更の検知用）。未作成なら None。
    pub disk_stamp: Option<FileStamp>,
}

/// 文書内の位置（行, 文字位置）。
pub type TextPos = (usize, usize);

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextBuffer {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_line: 0,
            cursor_col: 0,
            preferred_visual_col: None,
            file_path: None,
            dirty: false,
            revision: 0,
            encoding: "UTF-8",
            line_ending: "\n",
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            anchor: None,
            disk_stamp: None,
        }
    }

    pub fn from_file(path: &Path) -> io::Result<Self> {
        let stamp = file_stamp(path);
        let (lines, line_ending) = read_lines(path)?;
        Ok(Self {
            lines,
            cursor_line: 0,
            cursor_col: 0,
            preferred_visual_col: None,
            file_path: Some(path.to_path_buf()),
            dirty: false,
            revision: 1,
            encoding: "UTF-8",
            line_ending,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            anchor: None,
            disk_stamp: stamp,
        })
    }

    /// 読み込み・保存の後に外部でファイルが変更されていれば、現在の状態を返す。
    /// 未作成のファイル・読めないファイル（削除された場合を含む）は None。
    pub fn changed_on_disk(&self) -> Option<FileStamp> {
        let known = self.disk_stamp?;
        let now = file_stamp(self.file_path.as_deref()?)?;
        (now != known).then_some(now)
    }

    /// ディスク上の内容がバッファの内容と同じか（改行コードの違いは無視）。読めなければ false。
    pub fn same_as_disk(&self) -> bool {
        let Some(path) = self.file_path.as_deref() else {
            return false;
        };
        read_lines(path).is_ok_and(|(lines, _)| lines == self.lines)
    }

    /// ファイルを読み直す（外部での変更の取り込み）。読む前の内容は Ctrl+Z で戻せる。
    pub fn reload_from_disk(&mut self) -> io::Result<()> {
        let path = self
            .file_path
            .clone()
            .ok_or_else(|| io::Error::other("ファイル名がありません"))?;
        let stamp = file_stamp(&path);
        let (lines, line_ending) = read_lines(&path)?;
        let mut snap = self.current_snapshot();
        snap.dirty = true;
        if self.undo_stack.len() >= MAX_UNDO_HISTORY {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(snap);
        self.redo_stack.clear();
        self.lines = lines;
        self.line_ending = line_ending;
        self.disk_stamp = stamp;
        self.dirty = false;
        self.anchor = None;
        self.revision = self.revision.wrapping_add(1);
        let (line, col) = (self.cursor_line, self.cursor_col);
        self.set_cursor(line, col);
        Ok(())
    }

    /// 内容を `lines` に置き換えて未保存にする（Ctrl+Z で戻せる）。
    pub fn replace_all(&mut self, lines: Vec<String>) {
        self.push_undo_snapshot();
        self.lines = if lines.is_empty() { vec![String::new()] } else { lines };
        self.dirty = true;
        self.anchor = None;
        self.revision = self.revision.wrapping_add(1);
        let (line, col) = (self.cursor_line, self.cursor_col);
        self.set_cursor(line, col);
    }

    pub fn save_to_path(&mut self, path: &Path) -> io::Result<()> {
        let content = self.lines.join(self.line_ending);
        fs::write(path, content)?;
        self.disk_stamp = file_stamp(path);
        self.file_path = Some(path.to_path_buf());
        self.dirty = false;
        // Mark previous undo snapshots as dirty since they differ from the newly saved file
        for snap in &mut self.undo_stack {
            snap.dirty = true;
        }
        for snap in &mut self.redo_stack {
            snap.dirty = true;
        }
        Ok(())
    }

    pub fn display_name(&self) -> String {
        self.file_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "[無題]".to_string())
    }

    /// Returns true if the buffer's file path has a Markdown extension (`.md` or `.markdown`).
    pub fn is_markdown(&self) -> bool {
        self.file_path
            .as_ref()
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .is_some_and(|ext| {
                ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
            })
    }

    pub fn current_line_str(&self) -> &str {
        &self.lines[self.cursor_line]
    }

    pub fn line_char_len(&self, line_idx: usize) -> usize {
        self.lines[line_idx].chars().count()
    }

    /// 0-indexed visual column (accounting for full-width Japanese characters and tabs).
    pub fn visual_col_at(&self, line_idx: usize, char_col: usize) -> usize {
        self.lines[line_idx]
            .chars()
            .take(char_col)
            .map(char_display_width)
            .sum()
    }

    /// 1-indexed line number for status bar display.
    pub fn display_line_num(&self) -> usize {
        self.cursor_line + 1
    }

    /// 1-indexed visual column number for status bar display (VZ style).
    pub fn display_col_num(&self) -> usize {
        self.visual_col_at(self.cursor_line, self.cursor_col) + 1
    }

    /// Find the closest char index on `line_idx` to a target visual column.
    fn char_col_for_visual_col(&self, line_idx: usize, target_visual: usize) -> usize {
        let mut width = 0usize;
        for (i, c) in self.lines[line_idx].chars().enumerate() {
            let cw = char_display_width(c);
            if width + cw > target_visual {
                // Pick the closer boundary
                if target_visual.saturating_sub(width) >= (width + cw).saturating_sub(target_visual) {
                    return i + 1;
                }
                return i;
            }
            width += cw;
        }
        self.line_char_len(line_idx)
    }

    pub fn set_cursor(&mut self, line: usize, col: usize) {
        let max_line = self.lines.len().saturating_sub(1);
        self.cursor_line = line.min(max_line);
        self.cursor_col = col.min(self.line_char_len(self.cursor_line));
        self.preferred_visual_col = None;
    }

    // --- VZ Diamond Cursor Movement (Ctrl+E / S / D / X) ---

    pub fn move_up(&mut self) {
        if self.cursor_line == 0 {
            return;
        }
        let target_vcol = self
            .preferred_visual_col
            .unwrap_or_else(|| self.visual_col_at(self.cursor_line, self.cursor_col));
        self.preferred_visual_col = Some(target_vcol);
        self.cursor_line -= 1;
        self.cursor_col = self.char_col_for_visual_col(self.cursor_line, target_vcol);
    }

    pub fn move_down(&mut self) {
        if self.cursor_line + 1 >= self.lines.len() {
            return;
        }
        let target_vcol = self
            .preferred_visual_col
            .unwrap_or_else(|| self.visual_col_at(self.cursor_line, self.cursor_col));
        self.preferred_visual_col = Some(target_vcol);
        self.cursor_line += 1;
        self.cursor_col = self.char_col_for_visual_col(self.cursor_line, target_vcol);
    }

    pub fn move_left(&mut self) {
        self.preferred_visual_col = None;
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.line_char_len(self.cursor_line);
        }
    }

    pub fn move_right(&mut self) {
        self.preferred_visual_col = None;
        let len = self.line_char_len(self.cursor_line);
        if self.cursor_col < len {
            self.cursor_col += 1;
        } else if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = 0;
        }
    }

    // --- Word Movement (Ctrl+A / Ctrl+F) ---

    pub fn move_word_left(&mut self) {
        self.preferred_visual_col = None;
        if self.cursor_col == 0 {
            if self.cursor_line > 0 {
                self.cursor_line -= 1;
                self.cursor_col = self.line_char_len(self.cursor_line);
            }
            return;
        }

        let chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
        let mut idx = self.cursor_col;

        // Skip trailing whitespace to the left
        while idx > 0 && classify_char(chars[idx - 1]) == CharClass::Whitespace {
            idx -= 1;
        }
        if idx > 0 {
            let class = classify_char(chars[idx - 1]);
            while idx > 0 && classify_char(chars[idx - 1]) == class {
                idx -= 1;
            }
        }
        self.cursor_col = idx;
    }

    pub fn move_word_right(&mut self) {
        self.preferred_visual_col = None;
        let chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
        let len = chars.len();
        if self.cursor_col >= len {
            if self.cursor_line + 1 < self.lines.len() {
                self.cursor_line += 1;
                self.cursor_col = 0;
            }
            return;
        }

        let mut idx = self.cursor_col;
        let start_class = classify_char(chars[idx]);
        if start_class != CharClass::Whitespace {
            while idx < len && classify_char(chars[idx]) == start_class {
                idx += 1;
            }
        }
        // Skip whitespace after the word so cursor lands on the start of the next word (VZ style)
        while idx < len && classify_char(chars[idx]) == CharClass::Whitespace {
            idx += 1;
        }
        self.cursor_col = idx;
    }

    // --- Line & Document Navigation (Ctrl+Q+S / Ctrl+Q+D / Home / End / PageUp / PageDown) ---

    pub fn move_line_start(&mut self) {
        self.preferred_visual_col = None;
        self.cursor_col = 0;
    }

    pub fn move_line_end(&mut self) {
        self.preferred_visual_col = None;
        self.cursor_col = self.line_char_len(self.cursor_line);
    }

    pub fn move_doc_start(&mut self) {
        self.preferred_visual_col = None;
        self.cursor_line = 0;
        self.cursor_col = 0;
    }

    pub fn move_doc_end(&mut self) {
        self.preferred_visual_col = None;
        self.cursor_line = self.lines.len().saturating_sub(1);
        self.cursor_col = self.line_char_len(self.cursor_line);
    }

    pub fn move_page_up(&mut self, page_lines: usize) {
        let step = page_lines.max(1);
        let target_vcol = self
            .preferred_visual_col
            .unwrap_or_else(|| self.visual_col_at(self.cursor_line, self.cursor_col));
        self.preferred_visual_col = Some(target_vcol);
        self.cursor_line = self.cursor_line.saturating_sub(step);
        self.cursor_col = self.char_col_for_visual_col(self.cursor_line, target_vcol);
    }

    pub fn move_page_down(&mut self, page_lines: usize) {
        let step = page_lines.max(1);
        let target_vcol = self
            .preferred_visual_col
            .unwrap_or_else(|| self.visual_col_at(self.cursor_line, self.cursor_col));
        self.preferred_visual_col = Some(target_vcol);
        let max_line = self.lines.len().saturating_sub(1);
        self.cursor_line = (self.cursor_line + step).min(max_line);
        self.cursor_col = self.char_col_for_visual_col(self.cursor_line, target_vcol);
    }

    // --- Undo / Redo ---

    fn current_snapshot(&self) -> UndoSnapshot {
        UndoSnapshot {
            lines: self.lines.clone(),
            cursor_line: self.cursor_line,
            cursor_col: self.cursor_col,
            dirty: self.dirty,
        }
    }

    fn push_undo_snapshot(&mut self) {
        if self.undo_stack.len() >= MAX_UNDO_HISTORY {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(self.current_snapshot());
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack.push(self.current_snapshot());
        self.anchor = None;
        self.lines = prev.lines;
        self.cursor_line = prev.cursor_line;
        self.cursor_col = prev.cursor_col;
        self.dirty = prev.dirty;
        self.preferred_visual_col = None;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack.push(self.current_snapshot());
        self.anchor = None;
        self.lines = next.lines;
        self.cursor_line = next.cursor_line;
        self.cursor_col = next.cursor_col;
        self.dirty = next.dirty;
        self.preferred_visual_col = None;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    // --- Editing Operations ---

    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.push_undo_snapshot();
        // 選択中なら選択範囲を置き換える（アンドゥは1回）
        self.delete_selection_internal();
        self.insert_text_internal(text);
    }

    fn insert_text_internal(&mut self, text: &str) {
        self.preferred_visual_col = None;
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut parts = normalized.split('\n');

        if let Some(first) = parts.next() {
            let byte_idx = char_to_byte_idx(&self.lines[self.cursor_line], self.cursor_col);
            self.lines[self.cursor_line].insert_str(byte_idx, first);
            self.cursor_col += first.chars().count();
        }

        for part in parts {
            self.insert_newline_internal();
            if !part.is_empty() {
                let byte_idx = char_to_byte_idx(&self.lines[self.cursor_line], self.cursor_col);
                self.lines[self.cursor_line].insert_str(byte_idx, part);
                self.cursor_col += part.chars().count();
            }
        }
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn insert_newline(&mut self) {
        self.push_undo_snapshot();
        self.delete_selection_internal();
        self.preferred_visual_col = None;
        self.insert_newline_internal();
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }

    fn insert_newline_internal(&mut self) {
        let byte_idx = char_to_byte_idx(&self.lines[self.cursor_line], self.cursor_col);
        let tail = self.lines[self.cursor_line].split_off(byte_idx);
        self.cursor_line += 1;
        self.lines.insert(self.cursor_line, tail);
        self.cursor_col = 0;
    }

    /// Delete the character before the cursor (BackSpace / Ctrl+H).
    pub fn backspace(&mut self) {
        self.preferred_visual_col = None;
        if self.delete_selection() {
            return;
        }
        if self.cursor_col > 0 {
            self.push_undo_snapshot();
            let line = &mut self.lines[self.cursor_line];
            let start = char_to_byte_idx(line, self.cursor_col - 1);
            let end = char_to_byte_idx(line, self.cursor_col);
            line.replace_range(start..end, "");
            self.cursor_col -= 1;
            self.dirty = true;
            self.revision = self.revision.wrapping_add(1);
        } else if self.cursor_line > 0 {
            self.push_undo_snapshot();
            let current = self.lines.remove(self.cursor_line);
            self.cursor_line -= 1;
            let prev_len = self.line_char_len(self.cursor_line);
            self.lines[self.cursor_line].push_str(&current);
            self.cursor_col = prev_len;
            self.dirty = true;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Delete the character at the cursor (Ctrl+G / Delete).
    pub fn delete_char_at_cursor(&mut self) {
        self.preferred_visual_col = None;
        if self.delete_selection() {
            return;
        }
        let len = self.line_char_len(self.cursor_line);
        if self.cursor_col < len {
            self.push_undo_snapshot();
            let line = &mut self.lines[self.cursor_line];
            let start = char_to_byte_idx(line, self.cursor_col);
            let end = char_to_byte_idx(line, self.cursor_col + 1);
            line.replace_range(start..end, "");
            self.dirty = true;
            self.revision = self.revision.wrapping_add(1);
        } else if self.cursor_line + 1 < self.lines.len() {
            self.push_undo_snapshot();
            let next = self.lines.remove(self.cursor_line + 1);
            self.lines[self.cursor_line].push_str(&next);
            self.dirty = true;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Delete the entire current line (Ctrl+Y).
    pub fn delete_current_line(&mut self) {
        self.preferred_visual_col = None;
        self.anchor = None;
        if self.lines.len() == 1 {
            if !self.lines[0].is_empty() {
                self.push_undo_snapshot();
                self.lines[0].clear();
                self.cursor_col = 0;
                self.dirty = true;
                self.revision = self.revision.wrapping_add(1);
            }
            return;
        }

        self.push_undo_snapshot();
        self.lines.remove(self.cursor_line);
        if self.cursor_line >= self.lines.len() {
            self.cursor_line = self.lines.len() - 1;
        }
        let max_col = self.line_char_len(self.cursor_line);
        self.cursor_col = self.cursor_col.min(max_col);
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }

    // --- 範囲選択（サクラ互換） ---

    pub fn cursor_pos(&self) -> TextPos {
        (self.cursor_line, self.cursor_col)
    }

    /// 位置を文書内に収める。
    fn clamp_pos(&self, pos: TextPos) -> TextPos {
        let line = pos.0.min(self.lines.len().saturating_sub(1));
        (line, pos.1.min(self.line_char_len(line)))
    }

    /// 選択範囲（開始, 終了）。開始 < 終了 のときだけ返す。
    pub fn selection_range(&self) -> Option<(TextPos, TextPos)> {
        let a = self.clamp_pos(self.anchor?);
        let c = self.cursor_pos();
        match a.cmp(&c) {
            std::cmp::Ordering::Less => Some((a, c)),
            std::cmp::Ordering::Greater => Some((c, a)),
            std::cmp::Ordering::Equal => None,
        }
    }

    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Shift+移動の前に呼ぶ。選択が無ければ現在位置を起点にする。
    pub fn begin_selection_if_none(&mut self) {
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor_pos());
        }
    }

    /// 起点と終点を指定して選択する（カーソルは終点）。
    pub fn select_range(&mut self, start: TextPos, end: TextPos) {
        let start = self.clamp_pos(start);
        let end = self.clamp_pos(end);
        self.anchor = Some(start);
        self.cursor_line = end.0;
        self.cursor_col = end.1;
        self.preferred_visual_col = None;
    }

    pub fn select_all(&mut self) {
        let last = self.lines.len().saturating_sub(1);
        let end = (last, self.line_char_len(last));
        self.select_range((0, 0), end);
    }

    /// 1行選択（改行を含む。最終行は行末まで）。
    pub fn select_line(&mut self, line: usize) {
        let line = line.min(self.lines.len().saturating_sub(1));
        let end = if line + 1 < self.lines.len() {
            (line + 1, 0)
        } else {
            (line, self.line_char_len(line))
        };
        self.select_range((line, 0), end);
    }

    /// 指定位置の単語の範囲（同じ文字種が続く区間）。
    pub fn word_range_at(&self, pos: TextPos) -> (usize, usize) {
        let (line, col) = self.clamp_pos(pos);
        let chars: Vec<char> = self.lines[line].chars().collect();
        if chars.is_empty() {
            return (0, 0);
        }
        let probe = if col < chars.len() { col } else { col - 1 };
        let class = classify_char(chars[probe]);
        let mut start = probe;
        while start > 0 && classify_char(chars[start - 1]) == class {
            start -= 1;
        }
        let mut end = probe + 1;
        while end < chars.len() && classify_char(chars[end]) == class {
            end += 1;
        }
        (start, end)
    }

    pub fn select_word_at_cursor(&mut self) {
        let line = self.cursor_line;
        let (start, end) = self.word_range_at(self.cursor_pos());
        self.select_range((line, start), (line, end));
    }

    /// 指定範囲の文字列（行の区切りは \n）。
    pub fn text_in_range(&self, start: TextPos, end: TextPos) -> String {
        let (start, end) = (self.clamp_pos(start), self.clamp_pos(end));
        if start.0 == end.0 {
            let line = &self.lines[start.0];
            let a = char_to_byte_idx(line, start.1);
            let b = char_to_byte_idx(line, end.1);
            return line[a..b].to_string();
        }
        let mut out = String::new();
        let first = &self.lines[start.0];
        out.push_str(&first[char_to_byte_idx(first, start.1)..]);
        for line in &self.lines[start.0 + 1..end.0] {
            out.push('\n');
            out.push_str(line);
        }
        out.push('\n');
        let last = &self.lines[end.0];
        out.push_str(&last[..char_to_byte_idx(last, end.1)]);
        out
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection_range()
            .map(|(start, end)| self.text_in_range(start, end))
    }

    /// 範囲を削除してカーソルを開始位置へ置く（アンドゥ記録なし）。
    fn delete_range_internal(&mut self, start: TextPos, end: TextPos) {
        let (start, end) = (self.clamp_pos(start), self.clamp_pos(end));
        if start >= end {
            return;
        }
        let tail = {
            let last = &self.lines[end.0];
            last[char_to_byte_idx(last, end.1)..].to_string()
        };
        let head_end = char_to_byte_idx(&self.lines[start.0], start.1);
        self.lines[start.0].truncate(head_end);
        self.lines[start.0].push_str(&tail);
        if end.0 > start.0 {
            self.lines.drain(start.0 + 1..=end.0);
        }
        self.cursor_line = start.0;
        self.cursor_col = start.1;
        self.preferred_visual_col = None;
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }

    /// 選択範囲を削除する（アンドゥ記録なし）。選択が無ければ起点だけ消す。
    fn delete_selection_internal(&mut self) -> bool {
        let range = self.selection_range();
        self.anchor = None;
        match range {
            Some((start, end)) => {
                self.delete_range_internal(start, end);
                true
            }
            None => false,
        }
    }

    /// 選択範囲を削除する。削除したら true。
    pub fn delete_selection(&mut self) -> bool {
        if !self.has_selection() {
            self.anchor = None;
            return false;
        }
        self.push_undo_snapshot();
        self.delete_selection_internal()
    }

    /// 範囲を削除する（アンドゥ記録あり）。削除した文字列を返す。
    pub fn delete_range(&mut self, start: TextPos, end: TextPos) -> Option<String> {
        let (start, end) = (self.clamp_pos(start), self.clamp_pos(end));
        if start >= end {
            return None;
        }
        let text = self.text_in_range(start, end);
        self.push_undo_snapshot();
        self.anchor = None;
        self.delete_range_internal(start, end);
        Some(text)
    }

    /// カーソル行を下に複製する（サクラの「行の二重化」）。
    pub fn duplicate_line(&mut self) {
        self.push_undo_snapshot();
        self.anchor = None;
        let line = self.lines[self.cursor_line].clone();
        self.lines.insert(self.cursor_line + 1, line);
        self.cursor_line += 1;
        self.cursor_col = self.cursor_col.min(self.line_char_len(self.cursor_line));
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }

    /// 指定行の選択部分（開始文字位置, 終了文字位置, 行末の改行も選択されているか）。
    pub fn line_selection(&self, line: usize) -> Option<(usize, usize, bool)> {
        let (start, end) = self.selection_range()?;
        if line < start.0 || line > end.0 {
            return None;
        }
        let from = if line == start.0 { start.1 } else { 0 };
        let to = if line == end.0 {
            end.1
        } else {
            self.line_char_len(line)
        };
        Some((from, to, line < end.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diamond_movement_and_editing() {
        let mut buf = TextBuffer::new();
        buf.insert_text("こんにちは世界\nabc def");
        assert_eq!(buf.lines, vec!["こんにちは世界", "abc def"]);
        assert_eq!((buf.cursor_line, buf.cursor_col), (1, 7));

        buf.move_line_start();
        assert_eq!(buf.cursor_col, 0);

        buf.move_word_right();
        assert_eq!(buf.cursor_col, 4); // lands on 'd'

        buf.move_word_left();
        assert_eq!(buf.cursor_col, 0);

        buf.move_up();
        assert_eq!(buf.cursor_line, 0);
        assert_eq!(buf.cursor_col, 0);

        buf.move_line_end();
        assert_eq!(buf.cursor_col, 7);

        buf.backspace();
        assert_eq!(buf.lines[0], "こんにちは世");

        buf.move_line_start();
        buf.delete_char_at_cursor();
        assert_eq!(buf.lines[0], "んにちは世");

        buf.delete_current_line();
        assert_eq!(buf.lines, vec!["abc def"]);
        assert_eq!(buf.cursor_line, 0);
    }

    #[test]
    fn test_japanese_word_movement() {
        let mut buf = TextBuffer::new();
        buf.insert_text("日本語テストです hello");
        buf.move_line_start();
        buf.move_word_right();
        assert_eq!(buf.cursor_col, 3); // after 日本語
        buf.move_word_right();
        assert_eq!(buf.cursor_col, 6); // after テスト
        buf.move_word_right();
        assert_eq!(buf.cursor_col, 9); // after です + space -> hello
        buf.move_word_left();
        assert_eq!(buf.cursor_col, 6); // back to です
    }

    #[test]
    fn test_is_markdown_and_revision() {
        let mut buf = TextBuffer::new();
        assert!(!buf.is_markdown());
        assert_eq!(buf.revision, 0);

        buf.file_path = Some(PathBuf::from("README.md"));
        assert!(buf.is_markdown());

        buf.file_path = Some(PathBuf::from("/tmp/notes.MARKDOWN"));
        assert!(buf.is_markdown());

        buf.file_path = Some(PathBuf::from("main.rs"));
        assert!(!buf.is_markdown());

        buf.insert_text("# Title");
        let rev1 = buf.revision;
        assert!(rev1 > 0);
        buf.insert_newline();
        assert!(buf.revision > rev1);
    }

    #[test]
    fn test_undo_redo() {
        let mut buf = TextBuffer::new();
        assert!(!buf.can_undo());
        assert!(!buf.can_redo());

        buf.insert_text("hello");
        assert!(buf.dirty);
        assert!(buf.can_undo());
        buf.insert_text(" world");
        assert_eq!(buf.lines, vec!["hello world"]);

        assert!(buf.undo());
        assert_eq!(buf.lines, vec!["hello"]);
        assert!(buf.dirty);
        assert!(buf.can_redo());

        assert!(buf.undo());
        assert_eq!(buf.lines, vec![""]);
        assert!(!buf.dirty);
        assert!(!buf.can_undo());

        assert!(buf.redo());
        assert_eq!(buf.lines, vec!["hello"]);
        assert!(buf.dirty);

        // New edit clears redo history
        buf.insert_text("!");
        assert!(!buf.can_redo());
        assert_eq!(buf.lines, vec!["hello!"]);
    }

    #[test]
    fn test_selection_replace_and_delete() {
        let mut buf = TextBuffer::new();
        buf.insert_text("abc def\n日本語\nxyz");
        // 2行目の「本語」から3行目の「x」まで選択
        buf.select_range((1, 1), (2, 1));
        assert_eq!(buf.selected_text().as_deref(), Some("本語\nx"));
        assert_eq!(buf.line_selection(1), Some((1, 3, true)));
        assert_eq!(buf.line_selection(2), Some((0, 1, false)));
        assert_eq!(buf.line_selection(0), None);

        // 入力で置き換え、アンドゥ1回で元に戻る
        buf.insert_text("ん");
        assert_eq!(buf.lines, vec!["abc def", "日んyz"]);
        assert!(!buf.has_selection());
        buf.undo();
        assert_eq!(buf.lines, vec!["abc def", "日本語", "xyz"]);

        // 逆向き（カーソルが起点より前）でも同じ範囲
        buf.select_range((2, 1), (1, 1));
        assert_eq!(buf.selection_range(), Some(((1, 1), (2, 1))));
        buf.backspace();
        assert_eq!(buf.lines, vec!["abc def", "日yz"]);
        assert_eq!(buf.cursor_pos(), (1, 1));
    }

    #[test]
    fn test_select_word_line_all_and_duplicate() {
        let mut buf = TextBuffer::new();
        buf.insert_text("hello 日本語テスト\nsecond");
        buf.set_cursor(0, 7);
        buf.select_word_at_cursor();
        assert_eq!(buf.selected_text().as_deref(), Some("日本語"));
        buf.select_line(0);
        assert_eq!(buf.selected_text().as_deref(), Some("hello 日本語テスト\n"));
        buf.select_all();
        assert_eq!(buf.selected_text().as_deref(), Some("hello 日本語テスト\nsecond"));

        buf.clear_selection();
        buf.set_cursor(1, 2);
        buf.duplicate_line();
        assert_eq!(buf.lines, vec!["hello 日本語テスト", "second", "second"]);
        assert_eq!(buf.cursor_pos(), (2, 2));

        let cut = buf.delete_range((1, 0), (2, 0));
        assert_eq!(cut.as_deref(), Some("second\n"));
        assert_eq!(buf.lines, vec!["hello 日本語テスト", "second"]);
    }
}
