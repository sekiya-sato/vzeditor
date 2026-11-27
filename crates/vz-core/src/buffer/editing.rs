//! テキスト編集と範囲選択。

use super::{
    char_to_byte_idx, classify_char, normalize_newlines, TextBuffer, TextPos, CLIPBOARD_LIMIT,
};
use crate::huge::EditError;

impl TextBuffer {
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
