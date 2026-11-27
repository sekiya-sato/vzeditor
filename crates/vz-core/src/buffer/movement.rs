//! カーソル移動と表示桁の計算。

use super::{
    char_display_width, classify_char, CharClass, TextBuffer, HUGE_VISUAL_COL_LIMIT,
    HUGE_WORD_WINDOW,
};

impl TextBuffer {
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
    pub(super) fn chars_around(&self, line: usize, col: usize) -> (usize, Vec<char>) {
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
}
