//! エディターと Markdown ビューの描画。

use super::*;

use crate::markdown::{parse_markdown, render_markdown_view, MdBlock};

pub(super) struct MarkdownViewState {
    pub(super) visible: bool,
    pub(super) scroll_y: f32,
    pub(super) max_scroll_y: f32,
    pub(super) viewport_h: f32,
    pub(super) scroll_dirty: bool,
    cache_revision: Option<u64>,
    pub(super) blocks: Vec<MdBlock>,
}

impl Default for MarkdownViewState {
    fn default() -> Self {
        Self {
            visible: false,
            scroll_y: 0.0,
            max_scroll_y: 0.0,
            viewport_h: 400.0,
            scroll_dirty: false,
            cache_revision: None,
            blocks: Vec::new(),
        }
    }
}

impl BufferSlot {
    pub(super) fn ensure_md_cache(&mut self) {
        if self.markdown.cache_revision != Some(self.buffer.revision) {
            self.markdown.blocks = parse_markdown(self.buffer.text_lines());
            self.markdown.cache_revision = Some(self.buffer.revision);
        }
    }
}

impl VzEditorApp {
    pub(super) fn render_markdown_canvas(&mut self, ctx: &egui::Context) {
        // Disable IME while viewing Markdown so navigation/shortcut keys work cleanly.
        ctx.output_mut(|o| {
            o.ime = None;
        });

        let active_idx = self.active_buf;
        let font_size = self.font_size;
        self.cur_slot_mut().ensure_md_cache();

        let mut clicked_url: Option<String> = None;

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                let view = &mut self.buffers[active_idx].markdown;
                let apply_offset = view.scroll_dirty;
                let out = render_markdown_view(
                    ui,
                    active_idx,
                    &view.blocks,
                    font_size,
                    view.scroll_y,
                    apply_offset,
                );
                view.scroll_y = out.scroll_y;
                view.max_scroll_y = out.max_scroll_y;
                view.viewport_h = out.viewport_h;
                view.scroll_dirty = false;
                clicked_url = out.clicked_url;
            });

        if let Some(url) = clicked_url {
            ctx.open_url(egui::OpenUrl::new_tab(&url));
            self.status_message = format!("リンクを開きました: {url}");
        }
    }

    pub(super) fn render_editor_canvas(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                let avail_rect = ui.available_rect_before_wrap();
                let split = self
                    .cur_slot()
                    .split
                    .as_ref()
                    .filter(|_| !self.cur_buf().is_huge())
                    .map(|s| (s.ratio, s.active_top));
                let Some((ratio, active_top)) = split else {
                    self.paint_editor_pane(ui, ctx, avail_rect, "pane_top", true);
                    return;
                };
                let min_h = self.line_height * 2.0 + 12.0;
                let sep_h = split::ruler_height(self.font_size);
                let (top, sep, bottom) = split::split_rects(avail_rect, ratio, sep_h, min_h);

                // 分割境界: ドラッグで上下のペインの高さを変える
                let sep_resp = ui.interact(sep, ui.id().with("split_sep"), Sense::drag());
                if sep_resp.hovered() || sep_resp.dragged() {
                    ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
                }
                if sep_resp.dragged() {
                    if let Some(pos) = sep_resp.interact_pointer_pos() {
                        let usable = (avail_rect.height() - sep_h).max(1.0);
                        let ratio =
                            ((pos.y - avail_rect.min.y - sep_h * 0.5) / usable).clamp(0.0, 1.0);
                        if let Some(s) = self.cur_slot_mut().split.as_mut() {
                            s.ratio = ratio;
                        }
                    }
                }
                // 操作していないペイン: 状態を入れ替えて描き、戻す。
                // ページ移動の行数とカーソルへのスクロールは操作中のペインのものを残す
                let (active_rect, other_rect) = if active_top {
                    (top, bottom)
                } else {
                    (bottom, top)
                };
                let (active_id, other_id) = if active_top {
                    ("pane_top", "pane_bottom")
                } else {
                    ("pane_bottom", "pane_top")
                };
                let need_scroll = std::mem::take(&mut self.need_scroll_to_cursor);
                let visible_lines = self.last_visible_lines;
                self.cur_slot_mut().swap_split_pane();
                let pressed = self
                    .paint_editor_pane(ui, ctx, other_rect, other_id, false)
                    .pressed;
                self.cur_slot_mut().swap_split_pane();
                self.last_visible_lines = visible_lines;
                self.need_scroll_to_cursor = need_scroll;

                // 境界はルーラー（操作中のペインの桁に合わせ、カーソル桁に印を付ける）
                let layout = self.paint_editor_pane(ui, ctx, active_rect, active_id, true);
                split::paint_ruler(ui, sep, &layout);
                if pressed {
                    self.switch_split_pane();
                }
            });
    }

    /// エディターの 1 ペインを `avail_rect` に描き、マウス操作を処理する。
    /// `active` でないペインはキャレットと IME 位置を出さない。桁の位置とボタンが押されたかを返す。
    fn paint_editor_pane(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        avail_rect: Rect,
        id_salt: &str,
        active: bool,
    ) -> split::PaneLayout {
        let response = ui.interact(avail_rect, ui.id().with(id_salt), Sense::click_and_drag());

        let font_id = FontId::monospace(self.font_size);
        let gutter_font_size = (self.font_size * 0.875).max(9.0);
        let marker_font_size = (self.font_size * 0.875).max(9.0);

        let scrollbar_w = 12.0;
        let total_lines = self.cur_buf().line_count();
        let huge = self.cur_buf().is_huge();
        let digits_for = self
            .cur_buf()
            .huge()
            .map_or(total_lines, |h| h.estimated_line_count());
        let line_digits = digits_for.max(999).to_string().len();

        // 巨大ファイルは行全体を読まず、画面に入る範囲（窓）だけを読む。
        // 窓は横スクロール位置 hcol 文字目から window_cols 文字（全角を考えて 2 倍）
        // 半角1文字の実際の幅（折り返し幅の計算にも使う）
        let cell_w = cell_width(ui, &font_id);
        let window_cols = ((avail_rect.width() / cell_w) as usize + 8) * 2;
        // 巨大ファイルの横スクロールは文字単位（各行を hcol 文字目から表示する）。
        // ピクセル単位にすると、全角を含む行で文字位置と表示位置がずれるため
        let mut hcol = if huge {
            self.cur_slot().huge.horizontal_col
        } else {
            0
        };
        let (cursor_line, cursor_col) = self.cur_buf().cursor_pos();
        // 巨大ファイルの折り返し表示（表示中の行だけを折り返す。横スクロールしない）
        let huge_wrap = huge && self.word_wrap;
        if huge && !huge_wrap && self.need_scroll_to_cursor {
            let view_cols = ((avail_rect.width() - 120.0).max(cell_w * 8.0) / cell_w) as usize;
            if cursor_col < hcol {
                // 行頭から半画面に入るなら行頭から表示する
                hcol = cursor_col.saturating_sub(view_cols / 2);
            } else {
                let span = cursor_col - hcol;
                let w = if span > window_cols {
                    f32::INFINITY
                } else {
                    let seg = expand_tabs_for_display(&self.cur_buf().line_segment(
                        cursor_line,
                        hcol,
                        span,
                    ));
                    ui.fonts(|f| f.layout_no_wrap(seg, font_id.clone(), TEXT_WHITE).size().x)
                };
                if w + cell_w * 4.0 > view_cols as f32 * cell_w {
                    // 全角が続いても入るよう、表示幅の 1/3 の文字数だけ手前から表示する
                    hcol = cursor_col.saturating_sub(view_cols / 3);
                }
            }
        }

        // Measure cursor X, preedit width, gutter width, and cursor cell width from actual font metrics
        // カーソル行の窓の先頭（通常は 0）。巨大ファイルでカーソルが窓の外なら画面外に置く
        let cursor_offscreen = huge && !(hcol..=hcol + window_cols).contains(&cursor_col);
        let cursor_base = if cursor_offscreen { cursor_col } else { hcol };
        let cursor_line_str = if huge {
            self.cur_buf()
                .line_segment(cursor_line, cursor_base, window_cols)
                .into_owned()
        } else {
            self.cur_buf().current_line_str().into_owned()
        };
        let cursor_byte = char_to_byte_idx(&cursor_line_str, cursor_col - cursor_base);
        let preedit_disp = expand_tabs_for_display(&self.ime_preedit);
        let char_under_cursor = cursor_line_str[cursor_byte..].chars().next();
        let cursor_sample = match char_under_cursor {
            Some('\t') => "    ".to_string(),
            Some(c) => c.to_string(),
            None => "M".to_string(),
        };

        let (preedit_px_w, gutter_text_w, cursor_w, half_cell_w) = ui.fonts(|f| {
            let pw = if preedit_disp.is_empty() {
                0.0
            } else {
                f.layout_no_wrap(preedit_disp.clone(), font_id.clone(), PREEDIT_FG)
                    .size()
                    .x
            };
            let gw = f
                .layout_no_wrap(
                    "9".repeat(line_digits),
                    FontId::monospace(gutter_font_size),
                    TEXT_DIM_CYAN,
                )
                .size()
                .x;
            let half_cell_w = f
                .layout_no_wrap("M".to_string(), font_id.clone(), TEXT_WHITE)
                .size()
                .x
                .max(self.font_size * 0.55);
            let char_w = f
                .layout_no_wrap(cursor_sample.clone(), font_id.clone(), TEXT_WHITE)
                .size()
                .x;
            (pw, gw, char_w.max(half_cell_w), half_cell_w)
        });
        // 折り返し桁数は行末キャレット（半角1文字）の分だけ空ける。
        // カーソル下の文字幅（全角・タブ）で変えると、カーソル位置で画面全体の折り返しが変わる
        let wrap_reserve_w = half_cell_w;

        let gutter_pad = (self.font_size * 1.1).clamp(14.0, 28.0);
        let gutter_w = (gutter_text_w + gutter_pad).max(48.0);
        let top_pad = 6.0;
        let left_pad = (self.font_size * 0.625).clamp(8.0, 18.0);

        let editor_rect = Rect::from_min_max(
            Pos2::new(avail_rect.min.x + gutter_w, avail_rect.min.y),
            Pos2::new(
                (avail_rect.max.x - scrollbar_w).max(avail_rect.min.x + gutter_w + 20.0),
                avail_rect.max.y,
            ),
        );
        let viewport_h = (editor_rect.height() - top_pad * 2.0).max(self.line_height);
        let viewport_w = (editor_rect.width() - left_pad * 2.0).max(40.0);
        self.last_visible_lines = (viewport_h / self.line_height).floor().max(1.0) as usize;

        // 折り返し幅（セル数）
        let cols = ((viewport_w - wrap_reserve_w) / cell_w).floor().max(8.0) as usize;
        if self.word_wrap {
            self.wrap_cols = cols;
        }
        // 折り返し表示: 表示幅（セル数）で論理行を表示行に分ける。描画中は行割りをスロットから借りる
        // 巨大ファイルは折り返さない
        let wrap: Option<WrapLayout> = if self.word_wrap && !huge {
            self.ensure_wrap_layout(cols);
            self.cur_slot_mut().wrap.take()
        } else {
            None
        };
        // 巨大ファイルの折り返し: 先頭位置 (論理行, 表示行) から画面に入る分だけ折り返す。
        // 表示行の番号はこの画面の中の番号（0 = 先頭）で、縦のピクセル位置は使わない
        let visible_rows = self.last_visible_lines + 1;
        let huge_rows: Vec<wrap::HugeRow> = if huge_wrap {
            let wheel = if response.hovered() {
                ctx.input(|i| i.smooth_scroll_delta.y)
            } else {
                0.0
            };
            let need = self.need_scroll_to_cursor;
            let line_height = self.line_height;
            let idx = self.active_buf;
            let BufferSlot {
                buffer,
                huge:
                    huge::HugeBufferState {
                        wrap_cache: cache,
                        wrap_top: huge_wrap_top,
                        wrap_wheel: huge_wrap_wheel,
                        ..
                    },
                ..
            } = &mut self.buffers[idx];
            let mut top = *huge_wrap_top;
            if top.0 >= total_lines {
                top = (total_lines.saturating_sub(1), 0);
            }
            // ホイール: 1 表示行の高さごとに先頭位置を動かす
            *huge_wrap_wheel -= wheel;
            let steps = (*huge_wrap_wheel / line_height).trunc();
            *huge_wrap_wheel -= steps * line_height;
            if steps > 0.0 {
                top = cache.forward(buffer, cols, top, steps as usize);
            } else if steps < 0.0 {
                top = cache.back(buffer, cols, top, (-steps) as usize);
            }
            if need {
                let (cseg, _) = cache.seg_of_col(buffer, cols, cursor_line, cursor_col);
                let cur = (cursor_line, cseg);
                if cur < top {
                    top = cur;
                } else {
                    let rows = cache.rows_from(buffer, cols, top, visible_rows.saturating_sub(1));
                    if !rows.iter().any(|r| (r.line, r.seg) == cur) {
                        top = cache.back(buffer, cols, cur, visible_rows.saturating_sub(2));
                    }
                }
            }
            *huge_wrap_top = top;
            cache.rows_from(buffer, cols, top, visible_rows)
        } else {
            Vec::new()
        };
        // カーソルのある表示行（巨大ファイルの折り返し。画面外なら None）
        let huge_cursor_row = huge_rows.iter().position(|r| {
            r.line == cursor_line
                && r.start <= cursor_col
                && (cursor_col < r.end || (r.last && cursor_col <= r.end))
        });
        // 巨大ファイルの折り返しでは、カーソル行の窓を、カーソルのある表示行の範囲にする
        let (cursor_offscreen, cursor_line_str, cursor_byte) = if huge_wrap {
            match huge_cursor_row.map(|i| huge_rows[i]) {
                Some(r) => {
                    let text = self
                        .cur_buf()
                        .line_segment(cursor_line, r.start, r.end - r.start + 1)
                        .into_owned();
                    let byte = char_to_byte_idx(&text, cursor_col - r.start);
                    (false, text, byte)
                }
                None => (true, String::new(), 0),
            }
        } else {
            (cursor_offscreen, cursor_line_str, cursor_byte)
        };
        let total_rows = if huge_wrap {
            huge_rows.len()
        } else {
            wrap.as_ref().map_or(total_lines, |w| w.total_rows())
        };
        // 表示行 → (論理行, 行内の表示行番号)
        let row_to_line = |row: usize| -> (usize, usize) {
            if huge_wrap {
                return huge_rows
                    .get(row)
                    .or(huge_rows.last())
                    .map_or((0, 0), |r| (r.line, r.seg));
            }
            match &wrap {
                Some(w) => w.line_of_row(row),
                None => (row.min(total_lines.saturating_sub(1)), 0),
            }
        };
        // 巨大ファイルの折り返しは、表示行の範囲だけを読むので (0, 文字数)
        let seg_range = |line_idx: usize, seg: usize, len: usize| -> (usize, usize) {
            match &wrap {
                Some(w) => w.seg_range(line_idx, seg, len),
                None => (0, len),
            }
        };
        let is_last_seg = |line_idx: usize, seg: usize| {
            if huge_wrap {
                return huge_rows
                    .iter()
                    .find(|r| r.line == line_idx && r.seg == seg)
                    .is_none_or(|r| r.last);
            }
            wrap.as_ref().is_none_or(|w| w.is_last_seg(line_idx, seg))
        };
        // この画面を描くときの横スクロール位置（ホイールで変えた分は次の画面から）
        let view_hcol = hcol;
        // 表示行の窓の先頭の文字位置（通常 0、巨大ファイルは横スクロール位置か表示行の先頭）
        let row_base = |row: usize| -> usize {
            if huge_wrap {
                huge_rows.get(row).map_or(0, |r| r.start)
            } else if huge {
                view_hcol
            } else {
                0
            }
        };
        // 表示行の文字列（巨大ファイルは窓だけ）
        let row_text = |this: &Self, row: usize, line: usize| -> Cow<'_, str> {
            if huge_wrap {
                let (s, e) = huge_rows.get(row).map_or((0, 0), |r| (r.start, r.end));
                Cow::Owned(this.cur_buf().line_segment(line, s, e - s).into_owned())
            } else if huge {
                Cow::Owned(
                    this.cur_buf()
                        .line_segment(line, view_hcol, window_cols)
                        .into_owned(),
                )
            } else {
                Cow::Owned(this.cur_buf().line(line).into_owned())
            }
        };
        let cursor_seg = if huge_wrap {
            huge_cursor_row.map_or(0, |i| huge_rows[i].seg)
        } else {
            wrap.as_ref()
                .map_or(0, |w| w.seg_of(cursor_line, cursor_col))
        };
        let cursor_row = if huge_wrap {
            huge_cursor_row.unwrap_or(usize::MAX / 2)
        } else {
            wrap.as_ref()
                .map_or(cursor_line, |w| w.row_of(cursor_line, cursor_col))
        };
        let (cursor_seg_start, _) =
            seg_range(cursor_line, cursor_seg, cursor_line_str.chars().count());
        let cursor_seg_byte = char_to_byte_idx(&cursor_line_str, cursor_seg_start);
        let before_cursor_disp =
            expand_tabs_for_display(&cursor_line_str[cursor_seg_byte..cursor_byte]);
        let cursor_px_x = if cursor_offscreen {
            -10_000.0
        } else {
            ui.fonts(|f| {
                f.layout_no_wrap(before_cursor_disp, font_id.clone(), TEXT_WHITE)
                    .size()
                    .x
            })
        };

        // 縦の位置は f64 で扱う（1 億行 × 行の高さは f32 では精度が足りない）
        let line_h = f64::from(self.line_height);
        let mut scroll_y = if huge_wrap {
            0.0
        } else {
            self.cur_slot().scroll_y
        };
        let mut scroll_x = if wrap.is_some() || huge {
            0.0
        } else {
            self.cur_slot().scroll_x
        };

        if response.hovered() {
            // Ctrl+マウスホイール: フォントサイズ（1ノッチ 1段階）
            let wheel_steps: Vec<f32> = ctx.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        Event::MouseWheel {
                            delta, modifiers, ..
                        } if modifiers.ctrl || modifiers.command => Some(delta.y),
                        _ => None,
                    })
                    .collect()
            });
            for dy in wheel_steps {
                if dy > 0.0 {
                    self.zoom_in_font();
                } else if dy < 0.0 {
                    self.zoom_out_font();
                }
            }

            // Mouse wheel scrolling
            let scroll_delta = ctx.input(|i| i.smooth_scroll_delta);
            if scroll_delta.y.abs() > 0.0 {
                scroll_y -= f64::from(scroll_delta.y);
            }
            if scroll_delta.x.abs() > 0.0 && wrap.is_none() {
                if huge {
                    hcol = (hcol as f32 - scroll_delta.x / cell_w).max(0.0) as usize;
                } else {
                    scroll_x -= scroll_delta.x;
                }
            }
        }

        // Auto-scroll to keep cursor inside viewport
        let cursor_px_y = cursor_row as f64 * line_h;
        if self.need_scroll_to_cursor {
            if cursor_px_y < scroll_y {
                scroll_y = cursor_px_y;
            } else if cursor_px_y + line_h > scroll_y + f64::from(viewport_h) {
                scroll_y = cursor_px_y + line_h - f64::from(viewport_h);
            }

            let caret_end_x = cursor_px_x + preedit_px_w + cursor_w + 8.0;
            if wrap.is_some() || huge {
                // 折り返し表示では横スクロールしない（巨大ファイルは hcol で済ませた）
            } else if cursor_px_x < scroll_x {
                scroll_x = (cursor_px_x - cursor_w * 2.0).max(0.0);
            } else if caret_end_x > scroll_x + viewport_w {
                scroll_x = (caret_end_x - viewport_w).max(0.0);
            }
            self.need_scroll_to_cursor = false;
        }

        let total_content_h = (total_rows as f64 + 2.0) * line_h;
        let max_scroll_y = (total_content_h - f64::from(viewport_h)).max(0.0);
        scroll_y = if huge_wrap {
            0.0
        } else {
            scroll_y.clamp(0.0, max_scroll_y)
        };
        scroll_x = if wrap.is_some() || huge {
            0.0
        } else {
            scroll_x.max(0.0)
        };

        // マウス: クリックでカーソル移動。サクラ互換ではドラッグ・Shift+クリックで範囲選択、
        // ダブルクリックで単語、トリプルクリックで行を選択する。
        // ヘルプ表示中のクリックはヘルプを閉じるだけにする
        let can_click = self.open_menu.is_none()
            && !self.show_about
            && !self.show_help
            && self.filer_modal.is_none()
            && !self.search.is_open()
            && self.inline_prompt.is_none();
        if can_click {
            let sakura = self.is_sakura();
            let shift_held = ctx.input(|i| i.modifiers.shift);
            // 画面上の位置 → (行, 文字位置)
            let hit = |this: &Self, pos: Pos2| -> TextPos {
                let rel_y = f64::from(pos.y - editor_rect.min.y - top_pad) + scroll_y;
                let target_row =
                    ((rel_y / line_h).floor().max(0.0) as usize).min(total_rows.saturating_sub(1));
                let (target_line, seg) = row_to_line(target_row);
                let base = row_base(target_row);
                let click_x = (pos.x - editor_rect.min.x - left_pad + scroll_x).max(0.0);
                let line_str = row_text(this, target_row, target_line);
                let line_len = line_str.chars().count();
                let (seg_start, seg_end) = seg_range(target_line, seg, line_len);
                let seg_limit =
                    wrap::seg_cursor_limit(seg_start, seg_end, is_last_seg(target_line, seg));
                let target_col = seg_start
                    + ui.fonts(|f| {
                        let mut best_col = 0usize;
                        let mut best_dist = click_x;
                        let mut acc = String::new();
                        for (idx, c) in line_str
                            .chars()
                            .skip(seg_start)
                            .take(seg_limit - seg_start)
                            .enumerate()
                        {
                            if c == '\t' {
                                acc.push_str("    ");
                            } else {
                                acc.push(c);
                            }
                            let w = f
                                .layout_no_wrap(acc.clone(), font_id.clone(), TEXT_WHITE)
                                .size()
                                .x;
                            let dist = (w - click_x).abs();
                            if dist < best_dist {
                                best_dist = dist;
                                best_col = idx + 1;
                            }
                            if w > click_x + this.font_size * 2.0 {
                                break;
                            }
                        }
                        best_col
                    });
                (target_line, base + target_col)
            };

            if sakura && response.drag_started() {
                let origin = ctx
                    .input(|i| i.pointer.press_origin())
                    .or(response.interact_pointer_pos());
                if let Some(origin) = origin.filter(|p| editor_rect.contains(*p)) {
                    let pos = hit(self, origin);
                    self.prefix = PrefixKey::None;
                    self.select_mode = false;
                    let buf = self.cur_buf_mut();
                    if shift_held {
                        buf.begin_selection_if_none();
                    } else {
                        buf.anchor = Some(pos);
                    }
                    buf.set_cursor(pos.0, pos.1);
                }
            }
            if sakura && response.dragged() && self.cur_buf().anchor.is_some() {
                if let Some(pointer) = response.interact_pointer_pos() {
                    let pos = hit(self, pointer);
                    self.cur_buf_mut().set_cursor(pos.0, pos.1);
                    self.need_scroll_to_cursor = true;
                }
            }

            if let Some(pointer) = response
                .interact_pointer_pos()
                .filter(|p| editor_rect.contains(*p))
            {
                // ダブルクリックで単語、トリプルクリックで1行を選択（両方式）。
                // カーソルは選択の先頭に置き、選択の終端（次の行など）へ動かさない
                if response.triple_clicked() {
                    let pos = hit(self, pointer);
                    self.prefix = PrefixKey::None;
                    self.select_mode = false;
                    let buf = self.cur_buf_mut();
                    let (start, end) = buf.line_range(pos.0);
                    buf.select_range(end, start);
                } else if response.double_clicked() {
                    let pos = hit(self, pointer);
                    self.prefix = PrefixKey::None;
                    self.select_mode = false;
                    let buf = self.cur_buf_mut();
                    let (start, end) = buf.word_range_at(pos);
                    buf.select_range((pos.0, end), (pos.0, start));
                } else if response.clicked() {
                    let pos = hit(self, pointer);
                    self.prefix = PrefixKey::None;
                    self.select_mode = false;
                    let buf = self.cur_buf_mut();
                    if sakura && shift_held {
                        buf.begin_selection_if_none();
                    } else {
                        buf.clear_selection();
                    }
                    buf.set_cursor(pos.0, pos.1);
                }
            }
        }

        // Paint background & gutter
        let painter = ui.painter_at(avail_rect);
        window_style::bg_painter(&painter).rect_filled(avail_rect, CornerRadius::ZERO, bg(BG_NAVY));

        let gutter_rect = Rect::from_min_max(
            avail_rect.min,
            Pos2::new(avail_rect.min.x + gutter_w, avail_rect.max.y),
        );
        window_style::bg_painter(&painter).rect_filled(
            gutter_rect,
            CornerRadius::ZERO,
            bg(BG_GUTTER),
        );
        painter.line_segment(
            [
                Pos2::new(gutter_rect.max.x, avail_rect.min.y),
                Pos2::new(gutter_rect.max.x, avail_rect.max.y),
            ],
            Stroke::new(1.0_f32, GUTTER_SEP),
        );

        // Virtualized painting（表示行単位。折り返しなしなら 表示行 = 論理行）
        let first_row = (scroll_y / line_h).floor().max(0.0) as usize;
        let last_row = ((scroll_y + f64::from(viewport_h)) / line_h).ceil() as usize;
        let end_row = (last_row + 1).min(total_rows);

        let editor_painter = ui.painter_at(editor_rect);
        let gutter_painter = ui.painter_at(gutter_rect);
        let cur_line_now = self.cur_buf().cursor_line;
        let cur_col_now = self.cur_buf().cursor_col;
        let search_marks = self
            .cur_slot()
            .search_marks
            .as_ref()
            .filter(|(rev, _)| *rev == self.cur_buf().revision)
            .map(|(_, lines)| lines);
        let highlight = self.search.highlight();

        for row_idx in first_row..end_row {
            let (line_idx, seg) = row_to_line(row_idx);
            let y = editor_rect.min.y + top_pad + (row_idx as f64 * line_h - scroll_y) as f32;
            let is_current = line_idx == cur_line_now;
            // 巨大ファイルは窓だけを読む（base はその行の窓の先頭の文字位置）
            let base = row_base(row_idx);
            let full_line = row_text(self, row_idx, line_idx);
            let line_len = full_line.chars().count();
            let (seg_start, seg_end) = seg_range(line_idx, seg, line_len);
            let last_seg = is_last_seg(line_idx, seg);
            let line = &full_line
                [char_to_byte_idx(&full_line, seg_start)..char_to_byte_idx(&full_line, seg_end)];

            if is_current {
                let line_bg_rect = Rect::from_min_size(
                    Pos2::new(editor_rect.min.x, y),
                    Vec2::new(editor_rect.width(), self.line_height),
                );
                window_style::bg_painter(&editor_painter).rect_filled(
                    line_bg_rect,
                    CornerRadius::ZERO,
                    bg(BG_ACTIVE_LINE),
                );
            }

            // 行番号は論理行の最初の表示行だけ
            if seg == 0 {
                if search_marks.is_some_and(|lines| lines.binary_search(&line_idx).is_ok()) {
                    gutter_painter.circle_filled(
                        Pos2::new(gutter_rect.max.x - 4.0, y + self.line_height * 0.5),
                        2.5,
                        TEXT_CYAN,
                    );
                }
                let num_str = format!("{:>width$}", line_idx + 1, width = line_digits);
                gutter_painter.text(
                    Pos2::new(gutter_rect.max.x - 8.0, y + self.line_height * 0.5),
                    Align2::RIGHT_CENTER,
                    num_str,
                    FontId::monospace(gutter_font_size),
                    if is_current { TEXT_CYAN } else { TEXT_DIM_CYAN },
                );
            }

            let text_origin_x = editor_rect.min.x + left_pad - scroll_x;
            let text_y = y + (self.line_height - self.font_size) * 0.42;
            let x_of = |col: usize| -> f32 {
                if col == 0 {
                    return 0.0;
                }
                let b = char_to_byte_idx(line, col);
                editor_painter
                    .layout_no_wrap(
                        expand_tabs_for_display(&line[..b]),
                        font_id.clone(),
                        TEXT_WHITE,
                    )
                    .size()
                    .x
            };

            // 検索語のハイライト（表示中の行だけ照合する。巨大ファイルは読んだ窓の中だけ）
            if let Some(pattern) = highlight {
                for (from, to) in highlight_ranges(pattern, &full_line, seg_start, seg_end) {
                    editor_painter.rect_filled(
                        Rect::from_min_max(
                            Pos2::new(text_origin_x + x_of(from), y + 1.0),
                            Pos2::new(text_origin_x + x_of(to), y + self.line_height - 1.0),
                        ),
                        CornerRadius::ZERO,
                        SEARCH_HIT_BG,
                    );
                }
            }

            // 範囲選択の背景（この表示行の範囲に切り詰める）
            if let Some((from, to, newline)) = self.cur_buf().line_selection(line_idx) {
                let from = from.saturating_sub(base).clamp(seg_start, seg_end) - seg_start;
                let to = to.saturating_sub(base).clamp(seg_start, seg_end) - seg_start;
                let newline = newline && last_seg;
                let x0 = text_origin_x + x_of(from);
                let mut x1 = text_origin_x + x_of(to);
                if newline {
                    x1 += self.font_size * 0.5;
                }
                if x1 > x0 {
                    editor_painter.rect_filled(
                        Rect::from_min_max(
                            Pos2::new(x0, y + 1.0),
                            Pos2::new(x1, y + self.line_height - 1.0),
                        ),
                        CornerRadius::ZERO,
                        SELECTION_BG,
                    );
                }
            }

            // 窓で切れた長い行には改行記号を付けない
            let cut = huge && !huge_wrap && line_len >= window_cols;
            let has_newline = last_seg && line_idx + 1 < total_lines && !cut;
            let end_x = if is_current && seg == cursor_seg && !self.ime_preedit.is_empty() {
                let byte_pos = char_to_byte_idx(line, cur_col_now.saturating_sub(base + seg_start));
                let before = expand_tabs_for_display(&line[..byte_pos]);
                let after = expand_tabs_for_display(&line[byte_pos..]);
                let mut cur_x = text_origin_x;

                if !before.is_empty() {
                    let g = editor_painter.layout_no_wrap(before, font_id.clone(), TEXT_WHITE);
                    let w = g.size().x;
                    editor_painter.galley(Pos2::new(cur_x, text_y), g, TEXT_WHITE);
                    cur_x += w;
                }

                let g_pre = editor_painter.layout_no_wrap(
                    preedit_disp.clone(),
                    font_id.clone(),
                    PREEDIT_FG,
                );
                let pw = g_pre.size().x;
                let pre_rect = Rect::from_min_size(
                    Pos2::new(cur_x, y + 1.0),
                    Vec2::new(pw.max(4.0), (self.line_height - 2.0).max(4.0)),
                );
                editor_painter.rect_filled(pre_rect, CornerRadius::same(2), PREEDIT_BG);
                editor_painter.galley(Pos2::new(cur_x, text_y), g_pre, PREEDIT_FG);
                let underline_w = (self.font_size / 10.0).clamp(1.2, 3.0);
                editor_painter.line_segment(
                    [
                        Pos2::new(cur_x, y + self.line_height - 2.0),
                        Pos2::new(cur_x + pw, y + self.line_height - 2.0),
                    ],
                    Stroke::new(underline_w, PREEDIT_UNDERLINE),
                );
                cur_x += pw;

                if !after.is_empty() {
                    let g_after = editor_painter.layout_no_wrap(after, font_id.clone(), TEXT_WHITE);
                    let w = g_after.size().x;
                    editor_painter.galley(Pos2::new(cur_x, text_y), g_after, TEXT_WHITE);
                    cur_x += w;
                }
                cur_x
            } else {
                let disp = expand_tabs_for_display(line);
                let galley = editor_painter.layout_no_wrap(disp, font_id.clone(), TEXT_WHITE);
                let end_x = text_origin_x + galley.size().x;
                editor_painter.galley(Pos2::new(text_origin_x, text_y), galley, TEXT_WHITE);
                end_x
            };

            if has_newline {
                editor_painter.text(
                    Pos2::new(end_x + 2.0, y + self.line_height * 0.5),
                    Align2::LEFT_CENTER,
                    "↵",
                    FontId::monospace(marker_font_size),
                    TEXT_CR_MARK,
                );
            }
        }

        // [EOF] marker
        let eof_y = editor_rect.min.y + top_pad + (total_rows as f64 * line_h - scroll_y) as f32;
        let eof_in_rows = !huge_wrap
            || huge_rows
                .last()
                .is_some_and(|r| r.last && r.line + 1 >= total_lines);
        if eof_in_rows
            && eof_y >= editor_rect.min.y - self.line_height
            && eof_y <= editor_rect.max.y
        {
            editor_painter.text(
                Pos2::new(
                    editor_rect.min.x + left_pad - scroll_x,
                    eof_y + self.line_height * 0.5,
                ),
                Align2::LEFT_CENTER,
                "[EOF]",
                FontId::monospace(marker_font_size),
                TEXT_EOF,
            );
        }

        // Cursor rendering (dynamically scaled to current font_size & line_height)
        let caret_x = editor_rect.min.x + left_pad + cursor_px_x + preedit_px_w - scroll_x;
        let caret_y = editor_rect.min.y + top_pad + (cursor_px_y - scroll_y) as f32;
        let caret_h = (self.line_height - 2.0).max(4.0);
        let caret_bar_w = (self.font_size / 8.0).clamp(1.5, 4.0);
        let cursor_rect = Rect::from_min_size(
            Pos2::new(caret_x, caret_y + 1.0),
            Vec2::new(cursor_w, caret_h),
        );

        if active && editor_rect.intersects(cursor_rect) && self.inline_prompt.is_none() {
            let t = ctx.input(|i| i.time);
            let blink_on = !self.ime_preedit.is_empty() || ((t * 1.8) as u64).is_multiple_of(2);
            if blink_on && self.overwrite_mode && self.ime_preedit.is_empty() {
                // 上書きモード: 文字の下半分を塗りつぶす
                let half = Rect::from_min_max(
                    Pos2::new(cursor_rect.min.x, cursor_rect.center().y),
                    cursor_rect.max,
                );
                editor_painter.rect_filled(half, CornerRadius::ZERO, CURSOR_FILL);
            } else if blink_on {
                editor_painter.rect_filled(cursor_rect, CornerRadius::ZERO, CURSOR_FILL);
                editor_painter.line_segment(
                    [
                        Pos2::new(caret_x, caret_y + 1.0),
                        Pos2::new(caret_x, caret_y + self.line_height - 1.0),
                    ],
                    Stroke::new(caret_bar_w, CURSOR_BAR),
                );
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(450));
        }

        // Report exact IME cursor area for Fcitx5 (tracks font_size & line_height accurately)
        if active
            && self.inline_prompt.is_none()
            && self.filer_modal.is_none()
            && self.open_menu.is_none()
            && !self.show_about
        {
            let preedit_start_x = editor_rect.min.x + left_pad + cursor_px_x - scroll_x;
            let max_ime_x = (editor_rect.max.x - 4.0).max(editor_rect.min.x);
            let max_ime_y = (editor_rect.max.y - self.line_height).max(editor_rect.min.y);
            let ime_x = preedit_start_x.clamp(editor_rect.min.x, max_ime_x);
            let ime_y = caret_y.clamp(editor_rect.min.y, max_ime_y);
            let ime_rect = Rect::from_min_size(
                Pos2::new(ime_x, ime_y),
                Vec2::new(preedit_px_w.max(cursor_w), self.line_height),
            );
            ctx.output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    rect: ime_rect,
                    cursor_rect: ime_rect,
                });
            });
        }

        // Vertical scrollbar
        let sb_rect = Rect::from_min_max(
            Pos2::new(avail_rect.max.x - scrollbar_w, avail_rect.min.y),
            avail_rect.max,
        );
        window_style::bg_painter(&painter).rect_filled(sb_rect, CornerRadius::ZERO, bg(BG_GUTTER));
        // 巨大ファイルの折り返しは論理行の位置、それ以外はピクセルの位置で、つまみを置く
        let huge_lines = total_lines.max(1) as f32;
        // つまみの (高さ, 動ける範囲, 上端)
        let thumb = if huge_wrap {
            let top_line = huge_rows.first().map_or(0, |r| r.line);
            let thumb_h = (sb_rect.height() * (visible_rows as f32 / huge_lines))
                .clamp(24.0, sb_rect.height());
            let track_h = (sb_rect.height() - thumb_h).max(1.0);
            Some((
                thumb_h,
                track_h,
                sb_rect.min.y + top_line as f32 / huge_lines * track_h,
            ))
        } else if max_scroll_y > 0.0 {
            let ratio = (f64::from(viewport_h) / total_content_h).clamp(0.06, 1.0) as f32;
            let thumb_h = (sb_rect.height() * ratio).max(24.0);
            let track_h = (sb_rect.height() - thumb_h).max(1.0);
            Some((
                thumb_h,
                track_h,
                sb_rect.min.y + (scroll_y / max_scroll_y) as f32 * track_h,
            ))
        } else {
            None
        };
        if let Some((thumb_h, track_h, thumb_y)) = thumb {
            let thumb_rect = Rect::from_min_size(
                Pos2::new(sb_rect.min.x + 2.0, thumb_y),
                Vec2::new(scrollbar_w - 4.0, thumb_h),
            );
            painter.rect_filled(thumb_rect, CornerRadius::same(3), BORDER_BLUE);
            let sb_resp = ui.interact(
                sb_rect,
                ui.id().with((id_salt, "vscroll")),
                Sense::click_and_drag(),
            );
            if sb_resp.dragged() || sb_resp.clicked() {
                if let Some(pos) = sb_resp.interact_pointer_pos() {
                    let norm = ((pos.y - sb_rect.min.y - thumb_h * 0.5) / track_h).clamp(0.0, 1.0);
                    if huge_wrap {
                        let line =
                            ((norm * huge_lines) as usize).min(total_lines.saturating_sub(1));
                        self.cur_slot_mut().huge.wrap_top = (line, 0);
                    } else {
                        scroll_y = f64::from(norm) * max_scroll_y;
                    }
                }
            }
        }

        let slot = self.cur_slot_mut();
        slot.scroll_x = scroll_x;
        slot.huge.horizontal_col = hcol;
        if huge_wrap {
            slot.scroll_y = slot.huge.wrap_top.0 as f64 * line_h;
        } else {
            if huge {
                // 折り返しにしたときに同じ辺りから表示する
                slot.huge.wrap_top = ((scroll_y / line_h) as usize, 0);
            }
            slot.scroll_y = scroll_y;
        }
        if wrap.is_some() {
            slot.wrap = wrap;
        }
        split::PaneLayout {
            pressed: response.is_pointer_button_down_on(),
            text_x: editor_rect.min.x + left_pad - scroll_x,
            cell_w,
            caret_x,
            caret_w: cursor_w,
            min_x: editor_rect.min.x,
            max_x: editor_rect.max.x,
        }
    }
}
