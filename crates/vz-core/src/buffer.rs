use crate::encoding::TextEncoding;
use crate::huge::{EditError, HugeDoc, HUGE_FILE_THRESHOLD};
use std::borrow::Cow;
use std::collections::VecDeque;
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
/// 巨大ファイルで表示桁を数える上限（これより右は文字数を桁とみなす）
const HUGE_VISUAL_COL_LIMIT: usize = 200_000;
/// 巨大ファイルの単語移動で、カーソルの前後に見る文字数
const HUGE_WORD_WINDOW: usize = 65_536;

/// ディスク上のファイルの状態（更新日時, サイズ）。外部での変更の検知に使う。
pub type FileStamp = (SystemTime, u64);

/// ファイルの現在の状態。読めなければ None。
pub fn file_stamp(path: &Path) -> Option<FileStamp> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// ファイルを読み、改行を `\n` にそろえて行に分ける（元の改行コードも返す）。
/// 文字コードは自動で判定する（UTF-8・UTF-8 BOM・Shift_JIS）。
pub fn read_lines(path: &Path) -> io::Result<(Vec<String>, &'static str)> {
    read_document(path).map(|(lines, line_ending, _)| (lines, line_ending))
}

/// `read_lines` に加えて、判定した文字コードも返す。
pub fn read_document(path: &Path) -> io::Result<(Vec<String>, &'static str, TextEncoding)> {
    let bytes = fs::read(path)?;
    let encoding = TextEncoding::detect(&bytes, false);
    let raw = encoding.decode(&bytes);
    let line_ending = if raw.contains("\r\n") { "\r\n" } else { "\n" };
    // split は常に 1 つ以上を返すので、空のファイルでも 1 行になる
    let lines = normalize_newlines(&raw)
        .split('\n')
        .map(String::from)
        .collect();
    Ok((lines, line_ending, encoding))
}

/// 行を `encoding` のバイト列にする（BOM と改行コードを含む）。
/// 表せない文字があれば、その位置（1 始まりの行・桁）を書いたエラーにする。
pub fn encode_lines(
    lines: &[String],
    line_ending: &str,
    encoding: TextEncoding,
) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(lines.iter().map(|l| l.len() + 2).sum());
    out.extend_from_slice(encoding.bom());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(line_ending.as_bytes());
        }
        let bytes = encoding
            .encode(line)
            .map_err(|e| unencodable_error(encoding, i, e))?;
        out.extend_from_slice(&bytes);
    }
    Ok(out)
}

/// 表せない文字のエラー（`line` は 0 始まり）。
pub fn unencodable_error(
    encoding: TextEncoding,
    line: usize,
    e: crate::encoding::EncodeError,
) -> io::Error {
    io::Error::other(format!(
        "{}行{}桁の「{}」は {encoding} で保存できません（ファイル→文字コードを指定して保存で UTF-8 などに変えてください）",
        line + 1,
        e.index + 1,
        e.ch,
    ))
}

/// 改行（\r\n・\r）を \n にそろえる。\r が無ければコピーしない。
fn normalize_newlines(s: &str) -> Cow<'_, str> {
    if s.contains('\r') {
        Cow::Owned(s.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(s)
    }
}

#[derive(Debug, Clone)]
struct UndoSnapshot {
    lines: Vec<String>,
    cursor_line: usize,
    cursor_col: usize,
    dirty: bool,
}

/// コピー・切り取りでクリップボードへ渡す上限（巨大ファイル）
pub const CLIPBOARD_LIMIT: u64 = 16 << 20;

/// テキストのバッファ。中身は行の配列（通常ファイルモード）か、
/// 巨大ファイルモードの `HugeDoc`（Piece Tree）のどちらか。画面側はどちらも同じ API で扱う。
pub struct TextBuffer {
    /// 通常ファイルモードの行（巨大ファイルモードでは使わない）
    pub(crate) lines: Vec<String>,
    /// 巨大ファイルモードの文書
    huge: Option<HugeDoc>,
    /// 巨大ファイルで編集できなかった理由（画面側が取り出してステータスに出す）
    error: Option<String>,
    /// 巨大ファイルで、続けて入力している位置（Undo を 1 回にまとめる）
    typing_at: Option<TextPos>,
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
    pub encoding: TextEncoding,
    pub line_ending: &'static str,
    undo_stack: VecDeque<UndoSnapshot>,
    redo_stack: Vec<UndoSnapshot>,
    /// 範囲選択の起点（行, 文字位置）。カーソルとの間が選択範囲。None なら選択なし。
    /// サクラ互換の編集操作と、両方式の検索結果の選択に使う。
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
            huge: None,
            error: None,
            typing_at: None,
            cursor_line: 0,
            cursor_col: 0,
            preferred_visual_col: None,
            file_path: None,
            dirty: false,
            revision: 0,
            encoding: TextEncoding::Utf8,
            line_ending: "\n",
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            anchor: None,
            disk_stamp: None,
        }
    }

    /// テキストの内容から作る（無題・テスト用）。
    pub fn from_lines(lines: Vec<String>) -> Self {
        let mut buf = Self::new();
        if !lines.is_empty() {
            buf.lines = lines;
        }
        buf
    }

    /// ファイルを開く。`HUGE_FILE_THRESHOLD` 以上なら巨大ファイルモードにする。
    pub fn from_file(path: &Path) -> io::Result<Self> {
        let stamp = file_stamp(path);
        if stamp.is_some_and(|(_, len)| len >= HUGE_FILE_THRESHOLD) {
            return Self::open_huge(path);
        }
        let (lines, line_ending, encoding) = read_document(path)?;
        Ok(Self {
            huge: None,
            error: None,
            typing_at: None,
            lines,
            cursor_line: 0,
            cursor_col: 0,
            preferred_visual_col: None,
            file_path: Some(path.to_path_buf()),
            dirty: false,
            revision: 1,
            encoding,
            line_ending,
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            anchor: None,
            disk_stamp: stamp,
        })
    }

    /// 巨大ファイルモードで開く（大きさに関わらず。テストでも使う）。
    pub fn open_huge(path: &Path) -> io::Result<Self> {
        let doc = HugeDoc::open(path)?;
        let mut buf = Self::new();
        buf.encoding = doc.encoding;
        buf.line_ending = doc.line_ending;
        buf.huge = Some(doc);
        buf.file_path = Some(path.to_path_buf());
        buf.disk_stamp = file_stamp(path);
        buf.revision = 1;
        Ok(buf)
    }

    /// 巨大ファイルモードか。
    pub fn is_huge(&self) -> bool {
        self.huge.is_some()
    }

    pub fn huge(&self) -> Option<&HugeDoc> {
        self.huge.as_ref()
    }

    pub fn huge_mut(&mut self) -> Option<&mut HugeDoc> {
        self.huge.as_mut()
    }

    /// 巨大ファイルの索引を取り込む（毎フレーム呼ぶ）。行が増えたら true。
    pub fn poll(&mut self) -> bool {
        let grew = self.huge.as_mut().is_some_and(HugeDoc::poll_index);
        if grew {
            self.revision = self.revision.wrapping_add(1);
        }
        grew
    }

    /// 編集できなかった理由を取り出す（巨大ファイル）。
    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    /// 行の数（巨大ファイルの索引の作成中は、位置が確定した行まで）。
    pub fn line_count(&self) -> usize {
        match &self.huge {
            Some(h) => h.line_count(),
            None => self.lines.len(),
        }
    }

    /// 行の文字列。範囲外は空。巨大ファイルの長い行は丸ごと読むので、表示には `line_segment` を使う。
    pub fn line(&self, i: usize) -> Cow<'_, str> {
        match &self.huge {
            Some(h) => h.line_text(i),
            None => self
                .lines
                .get(i)
                .map_or(Cow::Borrowed(""), |l| Cow::Borrowed(l.as_str())),
        }
    }

    /// 行の start_col 文字目から最大 max_cols 文字（表示用）。
    pub fn line_segment(&self, i: usize, start_col: usize, max_cols: usize) -> Cow<'_, str> {
        match &self.huge {
            Some(h) => Cow::Owned(h.line_segment(i, start_col, max_cols)),
            None => {
                let line = self.lines.get(i).map_or("", String::as_str);
                let a = char_to_byte_idx(line, start_col);
                let rest = &line[a..];
                let b = char_to_byte_idx(rest, max_cols);
                Cow::Borrowed(&rest[..b])
            }
        }
    }

    /// 通常ファイルモードの行（巨大ファイルモードでは空）。Markdown・すべて置換など、
    /// 巨大ファイルでは使わない機能とテスト用。
    pub fn text_lines(&self) -> &[String] {
        if self.huge.is_some() {
            &[]
        } else {
            &self.lines
        }
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
        // 巨大ファイルは読み比べない（変わったものとして扱う）
        self.huge.is_none() && read_lines(path).is_ok_and(|(lines, _)| lines == self.lines)
    }

    /// ファイルを読み直す（外部での変更の取り込み）。読む前の内容は Ctrl+Z で戻せる。
    pub fn reload_from_disk(&mut self) -> io::Result<()> {
        let path = self
            .file_path
            .clone()
            .ok_or_else(|| io::Error::other("ファイル名がありません"))?;
        if self.huge.is_some() {
            // 巨大ファイルは開き直す（編集内容と Undo の履歴は捨てる）
            let (line, col) = self.cursor_pos();
            *self = Self::open_huge(&path)?;
            self.set_cursor(line, col);
            return Ok(());
        }
        let stamp = file_stamp(&path);
        let (lines, line_ending, encoding) = read_document(&path)?;
        let mut snap = self.current_snapshot();
        snap.dirty = true;
        self.push_undo(snap);
        self.lines = lines;
        self.line_ending = line_ending;
        self.encoding = encoding;
        self.disk_stamp = stamp;
        self.dirty = false;
        self.after_replace_all();
        Ok(())
    }

    /// 内容を `lines` に置き換えて未保存にする（Ctrl+Z で戻せる）。
    pub fn replace_all(&mut self, lines: Vec<String>) {
        if self.huge.is_some() {
            self.error = Some("巨大ファイルでは内容を丸ごと置き換えられません".into());
            return;
        }
        self.push_undo_snapshot();
        self.lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };
        self.dirty = true;
        self.after_replace_all();
    }

    /// 内容を丸ごと入れ替えた後の共通処理（選択解除、カーソルを範囲内へ）。
    fn after_replace_all(&mut self) {
        self.anchor = None;
        self.revision = self.revision.wrapping_add(1);
        let (line, col) = (self.cursor_line, self.cursor_col);
        self.set_cursor(line, col);
    }

    pub fn save_to_path(&mut self, path: &Path) -> io::Result<()> {
        self.save_to_path_as(path, self.encoding)
    }

    /// 文字コードを `encoding` にして保存する。保存できたら、以後の保存もその文字コードにする。
    pub fn save_to_path_as(&mut self, path: &Path, encoding: TextEncoding) -> io::Result<()> {
        if let Some(h) = self.huge.as_mut() {
            // 巨大ファイル: 一時ファイルへ順に書き出して置き換える（この関数はその場で待つ。
            // 画面からは裏のスレッドで保存し、終わったら `finish_huge_save` を呼ぶ）
            let rev = h.current_rev();
            let snap = h.doc_snapshot();
            let (progress, cancel) = (
                std::sync::atomic::AtomicU64::new(0),
                std::sync::atomic::AtomicBool::new(false),
            );
            snap.save(path, encoding, &progress, &cancel)?;
            self.finish_huge_save(path, encoding, rev);
            return Ok(());
        }
        let content = encode_lines(&self.lines, self.line_ending, encoding)?;
        fs::write(path, content)?;
        self.encoding = encoding;
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

    /// 巨大ファイルのすべて置換の結果（作業ファイル）を中身にする。Undo 1 回で戻る。
    pub fn adopt_huge_work_file(&mut self, path: &Path) -> io::Result<()> {
        let Some(h) = self.huge.as_mut() else {
            return Ok(());
        };
        h.adopt_work_file(path)?;
        self.dirty = h.is_modified();
        self.anchor = None;
        self.typing_at = None;
        self.revision = self.revision.wrapping_add(1);
        let (line, col) = self.cursor_pos();
        self.set_cursor(line, col);
        Ok(())
    }

    /// 巨大ファイルの保存が終わったときの処理（`rev` は保存を始めたときの `HugeDoc::current_rev`）。
    pub fn finish_huge_save(&mut self, path: &Path, encoding: TextEncoding, rev: u64) {
        if let Some(h) = self.huge.as_mut() {
            h.saved_as(encoding, rev);
            self.dirty = h.is_modified();
        }
        self.encoding = encoding;
        self.disk_stamp = file_stamp(path);
        self.file_path = Some(path.to_path_buf());
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

    pub fn current_line_str(&self) -> Cow<'_, str> {
        self.line(self.cursor_line)
    }

    pub fn line_char_len(&self, line_idx: usize) -> usize {
        match &self.huge {
            Some(h) => h.line_char_len(line_idx),
            None => self.lines.get(line_idx).map_or(0, |l| l.chars().count()),
        }
    }

    /// 0-indexed visual column (accounting for full-width Japanese characters and tabs).
    /// 巨大ファイルの長い行で桁がとても大きいときは、読む量を抑えるため文字数をそのまま返す。
    pub fn visual_col_at(&self, line_idx: usize, char_col: usize) -> usize {
        if self.huge.is_some() {
            if char_col > HUGE_VISUAL_COL_LIMIT {
                return char_col;
            }
            return self
                .line_segment(line_idx, 0, char_col)
                .chars()
                .map(char_display_width)
                .sum();
        }
        self.line(line_idx)
            .chars()
            .take(char_col)
            .map(char_display_width)
            .sum()
    }

    /// 単語移動などで見る範囲の文字（窓の先頭の文字位置, 文字）。巨大ファイルはカーソルの前後だけ。
    fn chars_around(&self, line: usize, col: usize) -> (usize, Vec<char>) {
        if self.huge.is_some() {
            let base = col.saturating_sub(HUGE_WORD_WINDOW);
            let chars = self
                .line_segment(line, base, HUGE_WORD_WINDOW * 2)
                .chars()
                .collect();
            return (base, chars);
        }
        (0, self.line(line).chars().collect())
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
        // 表示桁 target_visual までに入る文字は target_visual + 1 文字以下
        let text = if self.huge.is_some() {
            self.line_segment(line_idx, 0, target_visual + 1)
        } else {
            self.line(line_idx)
        };
        for (i, c) in text.chars().enumerate() {
            let cw = char_display_width(c);
            if width + cw > target_visual {
                // Pick the closer boundary
                if target_visual.saturating_sub(width) >= (width + cw).saturating_sub(target_visual)
                {
                    return i + 1;
                }
                return i;
            }
            width += cw;
        }
        self.line_char_len(line_idx)
    }

    pub fn set_cursor(&mut self, line: usize, col: usize) {
        let max_line = self.line_count().saturating_sub(1);
        self.cursor_line = line.min(max_line);
        self.cursor_col = col.min(self.line_char_len(self.cursor_line));
        self.preferred_visual_col = None;
    }

    // --- VZ Diamond Cursor Movement (Ctrl+E / S / D / X) ---

    /// 上下移動で保つ表示桁（最初の上下移動の時点の桁を覚える）。
    fn sticky_visual_col(&mut self) -> usize {
        let vcol = self
            .preferred_visual_col
            .unwrap_or_else(|| self.visual_col_at(self.cursor_line, self.cursor_col));
        self.preferred_visual_col = Some(vcol);
        vcol
    }

    pub fn move_up(&mut self) {
        if self.cursor_line == 0 {
            return;
        }
        let target_vcol = self.sticky_visual_col();
        self.cursor_line -= 1;
        self.cursor_col = self.char_col_for_visual_col(self.cursor_line, target_vcol);
    }

    pub fn move_down(&mut self) {
        if self.cursor_line + 1 >= self.line_count() {
            return;
        }
        let target_vcol = self.sticky_visual_col();
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
        } else if self.cursor_line + 1 < self.line_count() {
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

        let (base, chars) = self.chars_around(self.cursor_line, self.cursor_col);
        let mut idx = self.cursor_col - base;

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
        self.cursor_col = base + idx;
    }

    pub fn move_word_right(&mut self) {
        self.preferred_visual_col = None;
        let (base, chars) = self.chars_around(self.cursor_line, self.cursor_col);
        let len = chars.len();
        if self.cursor_col - base >= len {
            if self.cursor_line + 1 < self.line_count() {
                self.cursor_line += 1;
                self.cursor_col = 0;
            }
            return;
        }

        let mut idx = self.cursor_col - base;
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
        self.cursor_col = base + idx;
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
        self.cursor_line = self.line_count().saturating_sub(1);
        self.cursor_col = self.line_char_len(self.cursor_line);
    }

    pub fn move_page_up(&mut self, page_lines: usize) {
        let step = page_lines.max(1);
        let target_vcol = self.sticky_visual_col();
        self.cursor_line = self.cursor_line.saturating_sub(step);
        self.cursor_col = self.char_col_for_visual_col(self.cursor_line, target_vcol);
    }

    pub fn move_page_down(&mut self, page_lines: usize) {
        let step = page_lines.max(1);
        let target_vcol = self.sticky_visual_col();
        let max_line = self.line_count().saturating_sub(1);
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
        self.push_undo(self.current_snapshot());
    }

    /// アンドゥ履歴に積む（上限を超えたら古いものから捨てる）。リドゥ履歴は消す。
    fn push_undo(&mut self, snap: UndoSnapshot) {
        if self.undo_stack.len() >= MAX_UNDO_HISTORY {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(snap);
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        match &self.huge {
            Some(h) => h.can_undo(),
            None => !self.undo_stack.is_empty(),
        }
    }

    pub fn can_redo(&self) -> bool {
        match &self.huge {
            Some(h) => h.can_redo(),
            None => !self.redo_stack.is_empty(),
        }
    }

    /// 巨大ファイルの Undo / Redo の後: 変更の位置へカーソルを置く。
    fn after_huge_history(&mut self, pos: Option<u64>) -> bool {
        let Some(h) = self.huge.as_ref() else {
            return false;
        };
        let Some(pos) = pos else {
            return false;
        };
        let (line, col) = h.offset_to_pos(pos).unwrap_or((0, 0));
        self.dirty = h.is_modified();
        self.anchor = None;
        self.typing_at = None;
        self.set_cursor(line, col);
        self.revision = self.revision.wrapping_add(1);
        true
    }

    /// 巨大ファイルの編集: [start, end) を text に置き換え、カーソルを挿入した文字列の末尾へ置く。
    /// coalesce なら直前の入力と同じ Undo 単位にする。できなければ理由を `error` に入れて false。
    fn huge_replace(&mut self, start: TextPos, end: TextPos, text: &str, coalesce: bool) -> bool {
        let Some(h) = self.huge.as_mut() else {
            return false;
        };
        self.typing_at = None;
        let result = (|| {
            let a = h
                .pos_to_offset(start.0, start.1)
                .ok_or(EditError::Indexing)?;
            let b = h.pos_to_offset(end.0, end.1).ok_or(EditError::Indexing)?;
            h.check_encodable(text)?;
            if !coalesce {
                h.snapshot();
            }
            h.delete(a, b)?;
            let n = h.insert(a, text)?;
            let pos = h.offset_to_pos(a + n).ok_or(EditError::Indexing)?;
            Ok::<_, EditError>((pos, h.is_modified()))
        })();
        match result {
            Ok(((line, col), modified)) => {
                self.cursor_line = line;
                self.cursor_col = col;
                self.anchor = None;
                self.preferred_visual_col = None;
                self.dirty = modified;
                self.revision = self.revision.wrapping_add(1);
                true
            }
            Err(e) => {
                self.error = Some(e.to_string());
                false
            }
        }
    }

    /// 選択範囲（無ければカーソル位置）。
    fn selection_or_cursor(&self) -> (TextPos, TextPos) {
        let c = self.cursor_pos();
        self.selection_range().unwrap_or((c, c))
    }

    pub fn undo(&mut self) -> bool {
        if let Some(h) = self.huge.as_mut() {
            let pos = h.undo();
            return self.after_huge_history(pos);
        }
        let Some(prev) = self.undo_stack.pop_back() else {
            return false;
        };
        self.redo_stack.push(self.current_snapshot());
        self.restore_snapshot(prev);
        true
    }

    pub fn redo(&mut self) -> bool {
        if let Some(h) = self.huge.as_mut() {
            let pos = h.redo();
            return self.after_huge_history(pos);
        }
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack.push_back(self.current_snapshot());
        self.restore_snapshot(next);
        true
    }

    fn restore_snapshot(&mut self, snap: UndoSnapshot) {
        self.anchor = None;
        self.lines = snap.lines;
        self.cursor_line = snap.cursor_line;
        self.cursor_col = snap.cursor_col;
        self.dirty = snap.dirty;
        self.preferred_visual_col = None;
        self.revision = self.revision.wrapping_add(1);
    }

    // --- Editing Operations ---

    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.huge.is_some() {
            let (start, end) = self.selection_or_cursor();
            let single = !text.contains('\n');
            let coalesce = single && start == end && self.typing_at == Some(start);
            if self.huge_replace(start, end, text, coalesce) && single {
                self.typing_at = Some(self.cursor_pos());
            }
            return;
        }
        self.push_undo_snapshot();
        // 選択中なら選択範囲を置き換える（アンドゥは1回）
        self.delete_selection_internal();
        self.insert_text_internal(text);
    }

    /// 上書きモードの文字入力。カーソル位置から入力文字数ぶんを置き換える（改行は上書きしない）。
    /// 選択中は挿入と同じく選択範囲を置き換える。
    pub fn overwrite_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.huge.is_some() {
            let (start, mut end) = self.selection_or_cursor();
            if start == end {
                let first_len = normalize_newlines(text)
                    .split('\n')
                    .next()
                    .map_or(0, |s| s.chars().count());
                end.1 = (end.1 + first_len).min(self.line_char_len(end.0));
            }
            self.huge_replace(start, end, text, false);
            return;
        }
        self.push_undo_snapshot();
        if !self.delete_selection_internal() {
            let first_len = normalize_newlines(text)
                .split('\n')
                .next()
                .map_or(0, |s| s.chars().count());
            let line = &mut self.lines[self.cursor_line];
            let start = char_to_byte_idx(line, self.cursor_col);
            let end = char_to_byte_idx(line, self.cursor_col + first_len);
            line.replace_range(start..end, "");
        }
        self.insert_text_internal(text);
    }

    fn insert_text_internal(&mut self, text: &str) {
        self.preferred_visual_col = None;
        let normalized = normalize_newlines(text);
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
        if self.huge.is_some() {
            let (start, end) = self.selection_or_cursor();
            self.huge_replace(start, end, "\n", false);
            return;
        }
        self.push_undo_snapshot();
        self.delete_selection_internal();
        self.preferred_visual_col = None;
        self.insert_newline_internal();
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }

    /// 改行して、現在行の行頭の空白（タブ・半角スペース）を新しい行の先頭に写す（自動インデント）。
    /// カーソルが行頭の空白の途中にあるときは、カーソルより前の分だけを写す。アンドゥは1回で戻る。
    pub fn insert_newline_with_indent(&mut self) {
        if self.huge.is_some() {
            let (start, end) = self.selection_or_cursor();
            let indent: String = self
                .line_segment(start.0, 0, start.1)
                .chars()
                .take_while(|c| matches!(c, '\t' | ' '))
                .collect();
            self.huge_replace(start, end, &format!("\n{indent}"), false);
            return;
        }
        self.push_undo_snapshot();
        self.delete_selection_internal();
        self.preferred_visual_col = None;
        let indent: String = self.lines[self.cursor_line]
            .chars()
            .take(self.cursor_col)
            .take_while(|c| matches!(c, '\t' | ' '))
            .collect();
        self.insert_newline_internal();
        if !indent.is_empty() {
            self.lines[self.cursor_line].insert_str(0, &indent);
            self.cursor_col = indent.chars().count();
        }
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
        if self.huge.is_some() {
            let (line, col) = self.cursor_pos();
            let prev = if col > 0 {
                (line, col - 1)
            } else if line > 0 {
                (line - 1, self.line_char_len(line - 1))
            } else {
                return;
            };
            self.huge_replace(prev, (line, col), "", false);
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
        if self.huge.is_some() {
            let (line, col) = self.cursor_pos();
            let next = if col < self.line_char_len(line) {
                (line, col + 1)
            } else if line + 1 < self.line_count() {
                (line + 1, 0)
            } else {
                return;
            };
            self.huge_replace((line, col), next, "", false);
            self.set_cursor(line, col);
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
        if self.huge.is_some() {
            let (line, col) = self.cursor_pos();
            let range = if line + 1 < self.line_count() {
                ((line, 0), (line + 1, 0))
            } else if line > 0 {
                (
                    (line - 1, self.line_char_len(line - 1)),
                    (line, self.line_char_len(line)),
                )
            } else {
                ((0, 0), (0, self.line_char_len(0)))
            };
            if range.0 != range.1 && self.huge_replace(range.0, range.1, "", false) {
                let line = line.min(self.line_count().saturating_sub(1));
                self.set_cursor(line, col);
            }
            return;
        }
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
        let line = pos.0.min(self.line_count().saturating_sub(1));
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
        let last = self.line_count().saturating_sub(1);
        let end = (last, self.line_char_len(last));
        self.select_range((0, 0), end);
    }

    /// 1行選択（改行を含む。最終行は行末まで）。
    pub fn select_line(&mut self, line: usize) {
        let line = line.min(self.line_count().saturating_sub(1));
        let (start, end) = self.line_range(line);
        self.select_range(start, end);
    }

    /// 行全体の範囲（次の行があれば改行を含む）。
    pub fn line_range(&self, line: usize) -> (TextPos, TextPos) {
        if line + 1 < self.line_count() {
            ((line, 0), (line + 1, 0))
        } else {
            ((line, 0), (line, self.line_char_len(line)))
        }
    }

    /// 指定位置の単語の範囲（同じ文字種が続く区間）。
    pub fn word_range_at(&self, pos: TextPos) -> (usize, usize) {
        let (line, col) = self.clamp_pos(pos);
        let (base, chars) = self.chars_around(line, col);
        let col = col - base;
        if chars.is_empty() {
            return (base, base);
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
        (base + start, base + end)
    }

    pub fn select_word_at_cursor(&mut self) {
        let line = self.cursor_line;
        let (start, end) = self.word_range_at(self.cursor_pos());
        self.select_range((line, start), (line, end));
    }

    /// 指定範囲の文字列（行の区切りは \n）。
    pub fn text_in_range(&self, start: TextPos, end: TextPos) -> String {
        let (start, end) = (self.clamp_pos(start), self.clamp_pos(end));
        if let Some(h) = &self.huge {
            let (Some(a), Some(b)) = (
                h.pos_to_offset(start.0, start.1),
                h.pos_to_offset(end.0, end.1),
            ) else {
                return String::new();
            };
            return h.text_between(a, b, CLIPBOARD_LIMIT).unwrap_or_default();
        }
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
        if self.huge.is_some() {
            let (start, end) = self.selection_or_cursor();
            return self.huge_replace(start, end, "", false);
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
        if self.huge.is_some() {
            return self.huge_replace(start, end, "", false).then_some(text);
        }
        self.push_undo_snapshot();
        self.anchor = None;
        self.delete_range_internal(start, end);
        Some(text)
    }

    /// カーソル行を下に複製する（サクラの「行の二重化」）。
    pub fn duplicate_line(&mut self) {
        if self.huge.is_some() {
            let (line, col) = self.cursor_pos();
            let text = format!("\n{}", self.line(line));
            let end = (line, self.line_char_len(line));
            if self.huge_replace(end, end, &text, false) {
                self.set_cursor(line + 1, col);
            }
            return;
        }
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
    fn test_undo_history_cap_and_redo() {
        let mut buf = TextBuffer::new();
        for _ in 0..(MAX_UNDO_HISTORY + 5) {
            buf.insert_text("x");
        }
        let mut undone = 0;
        while buf.undo() {
            undone += 1;
        }
        assert_eq!(undone, MAX_UNDO_HISTORY);
        assert_eq!(buf.lines[0].len(), 5);
        assert!(buf.redo());
        assert_eq!(buf.lines[0].len(), 6);
        // 改行コードをそろえる（CR が無ければコピーしない）
        assert_eq!(normalize_newlines("a\r\nb\rc"), "a\nb\nc");
        assert!(matches!(normalize_newlines("a\nb"), Cow::Borrowed(_)));
    }

    #[test]
    fn test_shift_jis_open_edit_save() {
        let dir = std::env::temp_dir().join(format!("vz_sjis_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sjis.txt");
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("一般書籍 ホビー\r\n二行目");
        fs::write(&path, &sjis).unwrap();

        let mut buf = TextBuffer::from_file(&path).unwrap();
        assert_eq!(buf.encoding, TextEncoding::ShiftJis);
        assert_eq!(buf.lines, vec!["一般書籍 ホビー", "二行目"]);
        assert!(buf.same_as_disk());
        buf.set_cursor(1, 3);
        buf.insert_text("です");
        buf.save_to_path(&path).unwrap();
        let (expected, _, _) = encoding_rs::SHIFT_JIS.encode("一般書籍 ホビー\r\n二行目です");
        assert_eq!(fs::read(&path).unwrap(), expected.as_ref());

        // Shift_JIS で表せない文字は保存せず、位置を知らせる
        buf.insert_text("😀");
        let err = buf.save_to_path(&path).unwrap_err().to_string();
        assert!(err.contains("2行6桁"), "{err}");
        assert_eq!(fs::read(&path).unwrap(), expected.as_ref());
        // UTF-8 に変えて保存すると、以後も UTF-8
        buf.save_to_path_as(&path, TextEncoding::Utf8Bom).unwrap();
        assert_eq!(buf.encoding, TextEncoding::Utf8Bom);
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"\xEF\xBB\xBF"));
        assert_eq!(
            TextBuffer::from_file(&path).unwrap().encoding,
            TextEncoding::Utf8Bom
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_huge_mode_uses_same_editing_api() {
        let dir = std::env::temp_dir().join(format!("vz_huge_buf_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.txt");
        fs::write(&path, "abc\r\n  def\r\nghi").unwrap();
        let mut buf = TextBuffer::open_huge(&path).unwrap();
        buf.huge_mut().unwrap().wait_index();
        assert!(buf.is_huge());
        assert_eq!(buf.line_count(), 3);
        assert_eq!(buf.line(1), "  def");
        assert_eq!(buf.line_ending, "\r\n");

        // 続けて入力すると Undo は 1 回で戻る
        buf.set_cursor(0, 3);
        for c in ["x", "y", "z"] {
            buf.insert_text(c);
        }
        assert_eq!(buf.line(0), "abcxyz");
        assert!(buf.dirty);
        buf.set_cursor(1, 5);
        buf.insert_newline_with_indent();
        assert_eq!((buf.line(1), buf.line(2)), ("  def".into(), "  ".into()));
        assert_eq!(buf.cursor_pos(), (2, 2));
        buf.backspace();
        buf.backspace();
        buf.backspace();
        assert_eq!(buf.line(1), "  def");
        assert_eq!(buf.line_count(), 3);
        buf.set_cursor(1, 0);
        buf.select_range((1, 0), (2, 1));
        assert_eq!(buf.selected_text().as_deref(), Some("  def\ng"));
        buf.delete_selection();
        assert_eq!(buf.line(1), "hi");
        buf.duplicate_line();
        assert_eq!(buf.line_count(), 3);
        assert_eq!(buf.line(2), "hi");
        while buf.undo() {}
        assert_eq!(buf.line(0), "abc");
        assert_eq!(buf.line(2), "ghi");
        assert!(!buf.dirty);
        assert!(buf.redo());
        assert!(buf.dirty);

        buf.save_to_path(&path).unwrap();
        assert!(!buf.dirty);
        assert_eq!(fs::read_to_string(&path).unwrap(), "abcxyz\r\n  def\r\nghi");
        // Shift_JIS ではない文字コードなので、絵文字も入る
        buf.insert_text("😀");
        assert_eq!(buf.take_error(), None);
        drop(buf);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_overwrite_text() {
        let mut buf = TextBuffer::new();
        buf.insert_text("あいう\nxyz");
        buf.set_cursor(0, 1);
        buf.overwrite_text("AB");
        assert_eq!(buf.lines, vec!["あAB", "xyz"]);
        // 行末を越えた分は追加し、改行は消さない
        buf.overwrite_text("CD");
        assert_eq!(buf.lines, vec!["あABCD", "xyz"]);
        // アンドゥは1回で戻る
        assert!(buf.undo());
        assert_eq!(buf.lines, vec!["あAB", "xyz"]);
        // 選択中は選択範囲だけを置き換える
        buf.select_range((1, 0), (1, 1));
        buf.overwrite_text("Q");
        assert_eq!(buf.lines, vec!["あAB", "Qyz"]);
    }

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
    fn test_insert_newline_with_indent() {
        let mut buf = TextBuffer::new();
        buf.insert_text("\t\tこれはテスト");
        buf.insert_newline_with_indent();
        assert_eq!(buf.lines, vec!["\t\tこれはテスト", "\t\t"]);
        assert_eq!(buf.cursor_pos(), (1, 2));

        // アンドゥ1回で改行とインデントがまとめて戻る
        assert!(buf.undo());
        assert_eq!(buf.lines, vec!["\t\tこれはテスト"]);
        assert_eq!(buf.cursor_pos(), (0, 8));

        // スペース混じりはそのまま写す。行の途中なら後ろの文字は新しい行のインデントの後へ
        let mut buf = TextBuffer::new();
        buf.insert_text(" \t abc");
        buf.set_cursor(0, 4);
        buf.insert_newline_with_indent();
        assert_eq!(buf.lines, vec![" \t a", " \t bc"]);
        assert_eq!(buf.cursor_pos(), (1, 3));

        // 行頭の空白の途中ではカーソルより前の分だけ（行頭なら何も入れない）
        let mut buf = TextBuffer::new();
        buf.insert_text("\t\tx");
        buf.set_cursor(0, 1);
        buf.insert_newline_with_indent();
        assert_eq!(buf.lines, vec!["\t", "\t\tx"]);
        buf.set_cursor(1, 0);
        buf.insert_newline_with_indent();
        assert_eq!(buf.lines, vec!["\t", "", "\t\tx"]);

        // 範囲選択中は選択を消してから、残った行の行頭の空白を写す
        let mut buf = TextBuffer::new();
        buf.insert_text("\tabc\ndef");
        buf.select_range((0, 2), (1, 1));
        buf.insert_newline_with_indent();
        assert_eq!(buf.lines, vec!["\ta", "\tef"]);
        assert!(buf.undo());
        assert_eq!(buf.lines, vec!["\tabc", "def"]);
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
        assert_eq!(
            buf.selected_text().as_deref(),
            Some("hello 日本語テスト\nsecond")
        );

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
