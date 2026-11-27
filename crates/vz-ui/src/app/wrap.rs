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

/// 巨大ファイルの折り返しで、長い行の表示行の開始位置の目印を何表示行ごとに取るか
const HUGE_WRAP_STRIDE: usize = 1024;
/// 巨大ファイルの折り返しで目印を覚えておく行の数
const HUGE_WRAP_LINES: usize = 16;

/// 巨大ファイルの折り返しの表示行（論理行・行内の表示行番号・文字範囲 [start, end)・行の最後の表示行か）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HugeRow {
    pub(super) line: usize,
    pub(super) seg: usize,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) last: bool,
}

/// 巨大ファイルの折り返し: 全行は折り返さず、表示中の行だけを計算する。
/// 長い行は HUGE_WRAP_STRIDE 表示行ごとの開始位置を覚え、行の途中へは近い目印から数える。
#[derive(Default)]
pub(super) struct HugeWrapCache {
    rev: u64,
    cols: usize,
    lines: Vec<HugeWrapLine>,
}

struct HugeWrapLine {
    line: usize,
    /// marks[k] = 表示行 k*HUGE_WRAP_STRIDE の開始文字位置
    marks: Vec<usize>,
    /// 行の表示行数（行末まで数えたら）
    total: Option<usize>,
}

impl HugeWrapCache {
    /// 行の目印（無ければ作る）。文書か幅が変わっていたら全部捨てる。
    fn entry(&mut self, buf: &TextBuffer, cols: usize, line: usize) -> usize {
        if self.rev != buf.revision || self.cols != cols {
            self.rev = buf.revision;
            self.cols = cols;
            self.lines.clear();
        }
        if let Some(i) = self.lines.iter().position(|l| l.line == line) {
            return i;
        }
        if self.lines.len() >= HUGE_WRAP_LINES {
            self.lines.remove(0);
        }
        self.lines.push(HugeWrapLine {
            line,
            marks: vec![0],
            total: None,
        });
        self.lines.len() - 1
    }

    /// 目印を k 個目まで（または行末まで）伸ばす。
    fn extend_to(&mut self, buf: &TextBuffer, cols: usize, i: usize, k: usize) {
        let line = self.lines[i].line;
        while self.lines[i].marks.len() <= k && self.lines[i].total.is_none() {
            let m = self.lines[i].marks.len() - 1;
            let c = self.lines[i].marks[m];
            // 1 表示行は cols 文字以下なので、これだけ読めば STRIDE 個目の折り返し位置まで届く
            let text = buf.line_segment(line, c, HUGE_WRAP_STRIDE * cols + 1);
            let br = wrap_breaks(&text, cols);
            if br.len() >= HUGE_WRAP_STRIDE {
                self.lines[i].marks.push(c + br[HUGE_WRAP_STRIDE - 1]);
            } else {
                self.lines[i].total = Some(m * HUGE_WRAP_STRIDE + br.len() + 1);
            }
        }
    }

    /// 表示行 seg の開始文字位置。行の表示行数を超えていれば None。
    fn seg_start(
        &mut self,
        buf: &TextBuffer,
        cols: usize,
        line: usize,
        seg: usize,
    ) -> Option<usize> {
        let i = self.entry(buf, cols, line);
        let k = seg / HUGE_WRAP_STRIDE;
        self.extend_to(buf, cols, i, k);
        let e = &self.lines[i];
        if e.total.is_some_and(|t| seg >= t) {
            return None;
        }
        let c = *e.marks.get(k)?;
        let j = seg % HUGE_WRAP_STRIDE;
        if j == 0 {
            return Some(c);
        }
        let text = buf.line_segment(line, c, j * cols + 1);
        wrap_breaks(&text, cols).get(j - 1).map(|b| c + b)
    }

    /// 行の表示行数。
    fn seg_count(&mut self, buf: &TextBuffer, cols: usize, line: usize) -> usize {
        let i = self.entry(buf, cols, line);
        self.extend_to(buf, cols, i, usize::MAX);
        self.lines[i].total.unwrap_or(1)
    }

    /// 文字位置 col を含む表示行（行内の番号, 開始文字位置）。折り返し位置ちょうどは次の表示行。
    pub(super) fn seg_of_col(
        &mut self,
        buf: &TextBuffer,
        cols: usize,
        line: usize,
        col: usize,
    ) -> (usize, usize) {
        let i = self.entry(buf, cols, line);
        while self.lines[i].total.is_none() && *self.lines[i].marks.last().unwrap_or(&0) <= col {
            let next = self.lines[i].marks.len();
            self.extend_to(buf, cols, i, next);
        }
        let marks = &self.lines[i].marks;
        let k = marks.partition_point(|&m| m <= col).saturating_sub(1);
        let c = marks[k];
        let text = buf.line_segment(line, c, col - c + 1);
        let br = wrap_breaks(&text, cols);
        let n = br.partition_point(|&b| b <= col - c);
        let start = if n == 0 { c } else { c + br[n - 1] };
        (k * HUGE_WRAP_STRIDE + n, start)
    }

    /// (line, seg) から最大 n 表示行（行をまたいで先へ）。
    pub(super) fn rows_from(
        &mut self,
        buf: &TextBuffer,
        cols: usize,
        pos: (usize, usize),
        n: usize,
    ) -> Vec<HugeRow> {
        let mut out = Vec::with_capacity(n);
        let (mut line, mut seg) = pos;
        let lines = buf.line_count();
        while out.len() < n && line < lines {
            let Some(start) = self.seg_start(buf, cols, line, seg) else {
                line += 1;
                seg = 0;
                continue;
            };
            let want = (n - out.len()) * cols + 1;
            let text = buf.line_segment(line, start, want);
            let nchars = text.chars().count();
            let at_end = nchars < want;
            let br = wrap_breaks(&text, cols);
            let count = br.len() + 1;
            for r in 0..count {
                let is_last = r + 1 == count;
                // 行末まで読めていない最後の表示行は途中で切れているので使わない
                if is_last && !at_end {
                    break;
                }
                let s = if r == 0 { 0 } else { br[r - 1] };
                let e = if is_last { nchars } else { br[r] };
                out.push(HugeRow {
                    line,
                    seg: seg + r,
                    start: start + s,
                    end: start + e,
                    last: is_last,
                });
                if out.len() == n {
                    return out;
                }
            }
            if !at_end {
                break;
            }
            line += 1;
            seg = 0;
        }
        out
    }

    /// pos から k 表示行前の位置（行頭を越えたら前の行の最後の表示行へ）。
    pub(super) fn back(
        &mut self,
        buf: &TextBuffer,
        cols: usize,
        pos: (usize, usize),
        k: usize,
    ) -> (usize, usize) {
        let (mut line, mut seg, mut k) = (pos.0, pos.1, k);
        while k > 0 {
            if seg >= k {
                seg -= k;
                break;
            }
            k -= seg;
            seg = 0;
            if line == 0 || k == 0 {
                break;
            }
            line -= 1;
            seg = self.seg_count(buf, cols, line) - 1;
            k -= 1;
        }
        (line, seg)
    }

    /// pos から k 表示行先の位置（文書の最後の表示行で止まる）。
    pub(super) fn forward(
        &mut self,
        buf: &TextBuffer,
        cols: usize,
        pos: (usize, usize),
        k: usize,
    ) -> (usize, usize) {
        let (mut line, mut seg, mut k) = (pos.0, pos.1, k);
        let lines = buf.line_count();
        while k > 0 {
            let count = self.seg_count(buf, cols, line);
            if seg + k < count {
                seg += k;
                break;
            }
            if line + 1 >= lines {
                seg = count - 1;
                break;
            }
            k -= count - seg;
            line += 1;
            seg = 0;
        }
        (line, seg)
    }
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
        if self.word_wrap && self.cur_buf().is_huge() {
            return self.huge_move_cursor_row(delta);
        }
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

    /// 巨大ファイルの折り返し表示で、カーソルを表示行単位で `delta` 行動かす（左端からの見た目の位置を保つ）。
    fn huge_move_cursor_row(&mut self, delta: isize) -> bool {
        let cols = self.wrap_cols.max(1);
        let idx = self.active_buf;
        let BufferSlot {
            buffer,
            huge:
                huge::HugeBufferState {
                    wrap_cache: huge_wrap,
                    ..
                },
            ..
        } = &mut self.buffers[idx];
        let (line, col) = buffer.cursor_pos();
        let (seg, start) = huge_wrap.seg_of_col(buffer, cols, line, col);
        let x: usize = buffer
            .line_segment(line, start, col - start)
            .chars()
            .map(vz_core::buffer::char_display_width)
            .sum();
        let target = if delta >= 0 {
            huge_wrap.forward(buffer, cols, (line, seg), delta as usize)
        } else {
            huge_wrap.back(buffer, cols, (line, seg), delta.unsigned_abs())
        };
        let Some(row) = huge_wrap
            .rows_from(buffer, cols, target, 1)
            .first()
            .copied()
        else {
            return true;
        };
        // 最後以外の表示行の終端は次の表示行の先頭になるため、1文字手前までにする
        let limit = if row.last || row.end == row.start {
            row.end
        } else {
            row.end - 1
        };
        let mut acc = 0usize;
        let mut tcol = row.start;
        for c in buffer
            .line_segment(row.line, row.start, limit - row.start)
            .chars()
        {
            let w = vz_core::buffer::char_display_width(c);
            if acc + w > x {
                break;
            }
            acc += w;
            tcol += 1;
        }
        buffer.set_cursor(row.line, tcol);
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
}
