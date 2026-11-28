//! エディターの上下分割（同じバッファを 2 つのペインで表示する）。
//!
//! 操作中のペインの状態はバッファ（カーソル・選択）とスロット（スクロール）がそのまま持ち、
//! もう一方のペインの状態だけを `SplitView::other` に置く。ペインを切り替えるときは入れ替える。

use super::*;

/// 分割境界の太さ（ピクセル）
pub(super) const SPLIT_SEP_H: f32 = 6.0;

/// 操作していないペインの表示状態。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct PaneView {
    pub(super) cursor: TextPos,
    pub(super) anchor: Option<TextPos>,
    pub(super) scroll_x: f32,
    pub(super) scroll_y: f64,
}

pub(super) struct SplitView {
    /// 上のペインの高さの割合（0〜1）
    pub(super) ratio: f32,
    /// 操作中のペインが上か
    pub(super) active_top: bool,
    pub(super) other: PaneView,
}

impl BufferSlot {
    /// 操作中のペインと、もう一方のペインの状態を入れ替える。
    /// 相手側の位置は、その後の編集で範囲外になっていれば文書内に収める。
    pub(super) fn swap_split_pane(&mut self) {
        let Some(split) = self.split.as_mut() else {
            return;
        };
        let buf = &mut self.buffer;
        let mine = PaneView {
            cursor: (buf.cursor_line, buf.cursor_col),
            anchor: buf.anchor,
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
        };
        let other = std::mem::replace(&mut split.other, mine);
        buf.set_cursor(other.cursor.0, other.cursor.1);
        buf.anchor = other.anchor.map(|(line, col)| {
            let line = line.min(buf.line_count().saturating_sub(1));
            (line, col.min(buf.line_char_len(line)))
        });
        self.scroll_x = other.scroll_x;
        self.scroll_y = other.scroll_y;
    }
}

/// 分割境界の位置から (上のペイン, 境界, 下のペイン) を求める。各ペインは最低 `min_h` を残す。
pub(super) fn split_rects(area: Rect, ratio: f32, min_h: f32) -> (Rect, Rect, Rect) {
    let usable = (area.height() - SPLIT_SEP_H).max(0.0);
    let min_h = min_h.min(usable / 2.0);
    let top_h = (usable * ratio).clamp(min_h, usable - min_h);
    let sep_y = area.min.y + top_h;
    let top = Rect::from_min_max(area.min, Pos2::new(area.max.x, sep_y));
    let sep = Rect::from_min_max(
        Pos2::new(area.min.x, sep_y),
        Pos2::new(area.max.x, sep_y + SPLIT_SEP_H),
    );
    let bottom = Rect::from_min_max(Pos2::new(area.min.x, sep_y + SPLIT_SEP_H), area.max);
    (top, sep, bottom)
}

impl VzEditorApp {
    /// 上下分割の切替（Alt+S）。分割すると、両方のペインが今の位置を表示する。
    pub(super) fn toggle_split(&mut self) {
        if self.mode != AppMode::Editor {
            return;
        }
        if self.cur_buf().is_huge() {
            self.status_message = "巨大ファイルは分割表示できません".to_string();
            return;
        }
        let slot = self.cur_slot_mut();
        if slot.split.take().is_some() {
            self.status_message = "分割表示: オフ".to_string();
        } else {
            let buf = &slot.buffer;
            slot.split = Some(SplitView {
                ratio: 0.5,
                active_top: false,
                other: PaneView {
                    cursor: (buf.cursor_line, buf.cursor_col),
                    anchor: buf.anchor,
                    scroll_x: slot.scroll_x,
                    scroll_y: slot.scroll_y,
                },
            });
            self.status_message =
                "分割表示: オン（Alt+O でペイン切替、境界のドラッグで高さ変更）".to_string();
        }
        self.need_scroll_to_cursor = true;
    }

    /// もう一方のペインへ切り替える（Alt+O・非操作側のペインのクリック）。
    pub(super) fn switch_split_pane(&mut self) {
        let slot = self.cur_slot_mut();
        let Some(split) = slot.split.as_mut() else {
            return;
        };
        split.active_top = !split.active_top;
        slot.swap_split_pane();
        self.prefix = PrefixKey::None;
        self.select_mode = false;
        self.need_scroll_to_cursor = true;
    }
}
