//! 折り返し表示（`WrapLayout`）と、表示行単位のカーソル移動。

use super::*;

/// 折り返し位置（2つ目以降の表示行の先頭の文字位置）。
/// 幅は半角 1・全角 2・タブ 4 のセル数で数え、`cols` セルを超える文字の前で折り返す。
pub(super) fn wrap_breaks(line: &str, cols: usize) -> Vec<usize> {
    let cols = cols.max(1);
    let mut breaks = Vec::new();
    let mut acc = 0usize;
    for (i, c) in line.chars().enumerate() {
        let w = vz_core::buffer::char_display_width(c);
        if acc > 0 && acc + w > cols {
            breaks.push(i);
            acc = 0;
        }
        acc += w;
    }
    breaks
}

/// 折り返し表示の行割り。論理行（改行で区切った行）を、幅に収まる表示行に分ける。
pub(super) struct WrapLayout {
    pub(super) rev: u64,
    pub(super) cols: usize,
    /// 各論理行の折り返し位置
    pub(super) breaks: Vec<Vec<usize>>,
    /// 各論理行の最初の表示行番号（末尾に総表示行数を足して 行数+1 個）
    pub(super) row_start: Vec<usize>,
}

impl WrapLayout {
    pub(super) fn build(lines: &[String], cols: usize, rev: u64) -> Self {
        let breaks: Vec<Vec<usize>> = lines.iter().map(|l| wrap_breaks(l, cols)).collect();
        let mut row_start = Vec::with_capacity(lines.len() + 1);
        let mut acc = 0usize;
        for b in &breaks {
            row_start.push(acc);
            acc += b.len() + 1;
        }
        row_start.push(acc);
        Self {
            rev,
            cols,
            breaks,
            row_start,
        }
    }

    pub(super) fn is_valid_for(&self, buf: &TextBuffer, cols: usize) -> bool {
        self.rev == buf.revision && self.cols == cols && self.breaks.len() == buf.text_lines().len()
    }

    pub(super) fn total_rows(&self) -> usize {
        self.row_start.last().copied().unwrap_or(0)
    }

    /// 文字位置が何番目の表示行（行内）にあるか。折り返し位置ちょうどは次の表示行の先頭。
    pub(super) fn seg_of(&self, line: usize, col: usize) -> usize {
        self.breaks
            .get(line)
            .map(|b| b.partition_point(|&x| x <= col))
            .unwrap_or(0)
    }

    pub(super) fn row_of(&self, line: usize, col: usize) -> usize {
        self.row_start.get(line).copied().unwrap_or(0) + self.seg_of(line, col)
    }

    /// 表示行 → (論理行, 行内の表示行番号)
    pub(super) fn line_of_row(&self, row: usize) -> (usize, usize) {
        let lines = self.breaks.len();
        if lines == 0 {
            return (0, 0);
        }
        let line = self
            .row_start
            .partition_point(|&s| s <= row)
            .saturating_sub(1)
            .min(lines - 1);
        let seg = (row - self.row_start[line].min(row)).min(self.breaks[line].len());
        (line, seg)
    }

    /// 表示行の文字範囲 [開始, 終了)。`len` は論理行の文字数。
    pub(super) fn seg_range(&self, line: usize, seg: usize, len: usize) -> (usize, usize) {
        let Some(b) = self.breaks.get(line) else {
            return (0, len);
        };
        let start = if seg == 0 {
            0
        } else {
            b.get(seg - 1).copied().unwrap_or(len)
        };
        let end = b.get(seg).copied().unwrap_or(len);
        (start.min(len), end.min(len).max(start.min(len)))
    }

    pub(super) fn is_last_seg(&self, line: usize, seg: usize) -> bool {
        self.breaks.get(line).is_none_or(|b| seg >= b.len())
    }
}

impl VzEditorApp {
    pub(super) fn toggle_word_wrap(&mut self) {
        self.word_wrap = !self.word_wrap;
        for slot in &mut self.buffers {
            slot.scroll_x = 0.0;
            slot.wrap = None;
        }
        self.need_scroll_to_cursor = true;
        self.status_message = if self.word_wrap {
            "折り返し表示: オン（ウィンドウ幅で折り返す。Alt+W で解除）".to_string()
        } else {
            "折り返し表示: オフ".to_string()
        };
        self.save_settings();
    }

    /// 折り返し表示が効いているか（巨大ファイルは折り返さない）。
    pub(super) fn wrap_active(&self) -> bool {
        self.word_wrap && !self.cur_buf().is_huge()
    }

    /// 現在のバッファの折り返し行割り（幅 `cols`）を、必要なら作り直して返す。
    pub(super) fn ensure_wrap_layout(&mut self, cols: usize) {
        let slot = &mut self.buffers[self.active_buf];
        if !slot
            .wrap
            .as_ref()
            .is_some_and(|w| w.is_valid_for(&slot.buffer, cols))
        {
            slot.wrap = Some(WrapLayout::build(
                slot.buffer.text_lines(),
                cols,
                slot.buffer.revision,
            ));
        }
    }

    /// 折り返し表示のとき、カーソルを表示行単位で `delta` 行動かす（左端からの見た目の位置を保つ）。
    /// 折り返し表示でなければ何もせず false。
    pub(super) fn move_cursor_row(&mut self, delta: isize) -> bool {
        if !self.wrap_active() || self.is_markdown_view() {
            return false;
        }
        let cols = self.wrap_cols;
        let (line, col) = self.cur_buf().cursor_pos();
        self.ensure_wrap_layout(cols);
        let (target_line, target_col) = {
            let slot = &self.buffers[self.active_buf];
            let Some(layout) = slot.wrap.as_ref() else {
                return false;
            };
            let buf = &slot.buffer;
            let line_str = buf.line(line);
            let seg = layout.seg_of(line, col);
            let (s, _) = layout.seg_range(line, seg, buf.line_char_len(line));
            let x: usize = line_str
                .chars()
                .skip(s)
                .take(col.saturating_sub(s))
                .map(vz_core::buffer::char_display_width)
                .sum();
            let row = layout.row_of(line, col) as isize;
            let max_row = layout.total_rows().saturating_sub(1) as isize;
            let target_row = (row + delta).clamp(0, max_row) as usize;
            let (tl, tseg) = layout.line_of_row(target_row);
            let tlen = buf.line_char_len(tl);
            let (ts, te) = layout.seg_range(tl, tseg, tlen);
            // 最後以外の表示行の終端は次の表示行の先頭になるため、1文字手前までにする
            let limit = if layout.is_last_seg(tl, tseg) || te == ts {
                te
            } else {
                te - 1
            };
            let mut acc = 0usize;
            let mut tcol = ts;
            for c in buf.line(tl).chars().skip(ts).take(limit - ts) {
                let w = vz_core::buffer::char_display_width(c);
                if acc + w > x {
                    break;
                }
                acc += w;
                tcol += 1;
            }
            (tl, tcol)
        };
        self.cur_buf_mut().set_cursor(target_line, target_col);
        true
    }

    pub(super) fn cursor_up(&mut self) {
        if !self.move_cursor_row(-1) {
            self.cur_buf_mut().move_up();
        }
    }

    pub(super) fn cursor_down(&mut self) {
        if !self.move_cursor_row(1) {
            self.cur_buf_mut().move_down();
        }
    }

    // --- 更新時の保存して終了 / 状態の復元（vz --save-and-quit / --restore） ---
}
