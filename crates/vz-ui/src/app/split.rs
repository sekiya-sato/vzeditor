//! エディターの上下分割（同じバッファを 2 つのペインで表示する）。
//!
//! 操作中のペインの状態はバッファ（カーソル・選択）とスロット（スクロール）がそのまま持ち、
//! もう一方のペインの状態だけを `SplitView::other` に置く。ペインを切り替えるときは入れ替える。

use super::*;

/// 分割境界（ルーラー）の高さ。目盛りの数字が読めるようフォントサイズに合わせる
pub(super) fn ruler_height(font_size: f32) -> f32 {
    (font_size * 0.7 + 6.0).max(16.0)
}

/// ペインの本文の桁の位置（ルーラーを合わせるため）。
pub(super) struct PaneLayout {
    /// ボタンがこのペインで押されている
    pub(super) pressed: bool,
    /// 0 桁目の左端（横スクロール済み）
    pub(super) text_x: f32,
    /// 半角 1 文字の幅
    pub(super) cell_w: f32,
    /// カーソルの左端と幅（全角・タブなら広い。画面外なら左端は大きな負の値）
    pub(super) caret_x: f32,
    pub(super) caret_w: f32,
    /// 本文の表示域の左右（行番号欄とスクロールバーを除く）
    pub(super) min_x: f32,
    pub(super) max_x: f32,
}

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

/// 分割境界の位置から (上のペイン, 境界, 下のペイン) を求める。境界の高さは `sep_h`、
/// 各ペインは最低 `min_h` を残す。
pub(super) fn split_rects(area: Rect, ratio: f32, sep_h: f32, min_h: f32) -> (Rect, Rect, Rect) {
    let usable = (area.height() - sep_h).max(0.0);
    let min_h = min_h.min(usable / 2.0);
    let top_h = (usable * ratio).clamp(min_h, usable - min_h);
    let sep_y = area.min.y + top_h;
    let top = Rect::from_min_max(area.min, Pos2::new(area.max.x, sep_y));
    let sep = Rect::from_min_max(
        Pos2::new(area.min.x, sep_y),
        Pos2::new(area.max.x, sep_y + sep_h),
    );
    let bottom = Rect::from_min_max(Pos2::new(area.min.x, sep_y + sep_h), area.max);
    (top, sep, bottom)
}

/// ルーラーの目盛りの高さの段階（0: 1 桁、1: 5 桁、2: 10 桁で数字付き）。
pub(super) fn ruler_tick_level(col: usize) -> u8 {
    if col.is_multiple_of(10) {
        2
    } else if col.is_multiple_of(5) {
        1
    } else {
        0
    }
}

/// 表示域 `min_x..=max_x` に入る桁の範囲（0 桁目の左端 `text_x`、1 桁の幅 `cell_w`）。
pub(super) fn ruler_cols(
    text_x: f32,
    cell_w: f32,
    min_x: f32,
    max_x: f32,
) -> std::ops::Range<usize> {
    let cell_w = cell_w.max(1.0);
    let first = ((min_x - text_x) / cell_w).ceil().max(0.0) as usize;
    let last = ((max_x - text_x) / cell_w).floor().max(-1.0) + 1.0;
    first..(last as usize).max(first)
}

/// 分割境界をルーラーとして描く。1 桁ごとの目盛り、10 桁ごとに 10 の位の数字、カーソル桁の印。
pub(super) fn paint_ruler(ui: &egui::Ui, rect: Rect, layout: &PaneLayout) {
    let painter = ui.painter_at(rect);
    window_style::bg_painter(&painter).rect_filled(rect, CornerRadius::ZERO, bg(BG_GUTTER));
    let base_y = rect.max.y - 0.5;
    painter.line_segment(
        [Pos2::new(rect.min.x, base_y), Pos2::new(rect.max.x, base_y)],
        Stroke::new(1.0_f32, GUTTER_SEP),
    );

    let body = Rect::from_min_max(
        Pos2::new(layout.min_x, rect.min.y),
        Pos2::new(layout.max_x, rect.max.y),
    );
    let painter = ui.painter_at(body);
    // カーソル桁の印（目盛りの下に敷く）
    let mark = Rect::from_min_size(
        Pos2::new(layout.caret_x, rect.min.y + 1.0),
        Vec2::new(layout.caret_w.max(2.0), rect.height() - 2.0),
    );
    if body.intersects(mark) {
        painter.rect_filled(mark, CornerRadius::ZERO, CURSOR_FILL);
    }

    let h = rect.height();
    let num_font = FontId::monospace((h * 0.6).max(9.0));
    let stroke = Stroke::new(1.0_f32, TEXT_DIM_CYAN);
    for col in ruler_cols(layout.text_x, layout.cell_w, layout.min_x, layout.max_x) {
        let x = (layout.text_x + col as f32 * layout.cell_w).round() + 0.5;
        let level = ruler_tick_level(col);
        let tick_h = match level {
            2 => h * 0.8,
            1 => h * 0.45,
            _ => h * 0.22,
        };
        painter.line_segment(
            [Pos2::new(x, base_y), Pos2::new(x, base_y - tick_h)],
            stroke,
        );
        if level == 2 {
            painter.text(
                Pos2::new(x + 2.0, rect.min.y + 1.0),
                Align2::LEFT_TOP,
                (col / 10).to_string(),
                num_font.clone(),
                TEXT_DIM_CYAN,
            );
        }
    }
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
