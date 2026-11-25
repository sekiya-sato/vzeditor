//! エディターと Markdown ビューの描画。

use super::*;

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
                let slot = &mut self.buffers[active_idx];
                let apply_offset = slot.md_scroll_dirty;
                let out = render_markdown_view(
                    ui,
                    active_idx,
                    &slot.md_blocks,
                    font_size,
                    slot.md_scroll_y,
                    apply_offset,
                );
                slot.md_scroll_y = out.scroll_y;
                slot.md_max_scroll_y = out.max_scroll_y;
                slot.md_viewport_h = out.viewport_h;
                slot.md_scroll_dirty = false;
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
                let response = ui.allocate_rect(avail_rect, Sense::click_and_drag());

                let font_id = FontId::monospace(self.font_size);
                let gutter_font_size = (self.font_size * 0.875).max(9.0);
                let marker_font_size = (self.font_size * 0.875).max(9.0);

                let scrollbar_w = 12.0;
                let total_lines = self.cur_buf().lines.len();
                let line_digits = total_lines.max(999).to_string().len();

                // Measure cursor X, preedit width, gutter width, and cursor cell width from actual font metrics
                let cursor_line = self.cur_buf().cursor_line;
                let cursor_col = self.cur_buf().cursor_col;
                let cursor_line_str = self.cur_buf().current_line_str().to_string();
                let cursor_byte = char_to_byte_idx(&cursor_line_str, cursor_col);
                let preedit_disp = expand_tabs_for_display(&self.ime_preedit);
                let char_under_cursor = cursor_line_str[cursor_byte..].chars().next();
                let cursor_sample = match char_under_cursor {
                    Some('\t') => "    ".to_string(),
                    Some(c) => c.to_string(),
                    None => "M".to_string(),
                };

                let (cell_w, preedit_px_w, gutter_text_w, cursor_w) = ui.fonts(|f| {
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
                    // 折り返し幅の計算用に、半角1文字の実際の幅を10文字分から求める
                    let cell_w = (f
                        .layout_no_wrap("M".repeat(10), font_id.clone(), TEXT_WHITE)
                        .size()
                        .x
                        / 10.0)
                        .max(1.0);
                    (cell_w, pw, gw, char_w.max(half_cell_w))
                });

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

                // 折り返し表示: 表示幅（セル数）で論理行を表示行に分ける。描画中は行割りをスロットから借りる
                let wrap: Option<WrapLayout> = if self.word_wrap {
                    let cols = ((viewport_w - cursor_w) / cell_w).floor().max(8.0) as usize;
                    self.wrap_cols = cols;
                    self.ensure_wrap_layout(cols);
                    self.cur_slot_mut().wrap.take()
                } else {
                    None
                };
                let total_rows = wrap.as_ref().map_or(total_lines, |w| w.total_rows());
                // 表示行 → (論理行, 行内の表示行番号)
                let row_to_line = |row: usize| -> (usize, usize) {
                    match &wrap {
                        Some(w) => w.line_of_row(row),
                        None => (row.min(total_lines.saturating_sub(1)), 0),
                    }
                };
                let seg_range = |line_idx: usize, seg: usize, len: usize| -> (usize, usize) {
                    match &wrap {
                        Some(w) => w.seg_range(line_idx, seg, len),
                        None => (0, len),
                    }
                };
                let is_last_seg =
                    |line_idx: usize, seg: usize| wrap.as_ref().is_none_or(|w| w.is_last_seg(line_idx, seg));
                let cursor_seg = wrap.as_ref().map_or(0, |w| w.seg_of(cursor_line, cursor_col));
                let cursor_row = wrap.as_ref().map_or(cursor_line, |w| w.row_of(cursor_line, cursor_col));
                let (cursor_seg_start, _) =
                    seg_range(cursor_line, cursor_seg, cursor_line_str.chars().count());
                let cursor_seg_byte = char_to_byte_idx(&cursor_line_str, cursor_seg_start);
                let before_cursor_disp =
                    expand_tabs_for_display(&cursor_line_str[cursor_seg_byte..cursor_byte]);
                let cursor_px_x = ui.fonts(|f| {
                    f.layout_no_wrap(before_cursor_disp, font_id.clone(), TEXT_WHITE)
                        .size()
                        .x
                });

                let mut scroll_y = self.cur_slot().scroll_y;
                let mut scroll_x = if wrap.is_some() {
                    0.0
                } else {
                    self.cur_slot().scroll_x
                };

                // Ctrl+マウスホイール: フォントサイズ（1ノッチ 1段階）
                if response.hovered() {
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
                }

                // Mouse wheel scrolling
                if response.hovered() {
                    let scroll_delta = ctx.input(|i| i.smooth_scroll_delta);
                    if scroll_delta.y.abs() > 0.0 {
                        scroll_y -= scroll_delta.y;
                    }
                    if scroll_delta.x.abs() > 0.0 && wrap.is_none() {
                        scroll_x -= scroll_delta.x;
                    }
                }

                // Auto-scroll to keep cursor inside viewport
                let cursor_px_y = cursor_row as f32 * self.line_height;
                if self.need_scroll_to_cursor {
                    if cursor_px_y < scroll_y {
                        scroll_y = cursor_px_y;
                    } else if cursor_px_y + self.line_height > scroll_y + viewport_h {
                        scroll_y = cursor_px_y + self.line_height - viewport_h;
                    }

                    let caret_end_x = cursor_px_x + preedit_px_w + cursor_w + 8.0;
                    if wrap.is_some() {
                        // 折り返し表示では横スクロールしない
                    } else if cursor_px_x < scroll_x {
                        scroll_x = (cursor_px_x - cursor_w * 2.0).max(0.0);
                    } else if caret_end_x > scroll_x + viewport_w {
                        scroll_x = (caret_end_x - viewport_w).max(0.0);
                    }
                    self.need_scroll_to_cursor = false;
                }

                let total_content_h = (total_rows as f32 + 2.0) * self.line_height;
                let max_scroll_y = (total_content_h - viewport_h).max(0.0);
                scroll_y = scroll_y.clamp(0.0, max_scroll_y);
                scroll_x = if wrap.is_some() { 0.0 } else { scroll_x.max(0.0) };

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
                        let rel_y = pos.y - editor_rect.min.y - top_pad + scroll_y;
                        let target_row = ((rel_y / this.line_height).floor().max(0.0) as usize)
                            .min(total_rows.saturating_sub(1));
                        let (target_line, seg) = row_to_line(target_row);
                        let click_x = (pos.x - editor_rect.min.x - left_pad + scroll_x).max(0.0);
                        let line_str = &this.cur_buf().lines[target_line];
                        let line_len = line_str.chars().count();
                        let (seg_start, seg_end) = seg_range(target_line, seg, line_len);
                        // 最後以外の表示行の終端は次の表示行の先頭になるため、1文字手前までにする
                        let seg_limit = if is_last_seg(target_line, seg) || seg_end == seg_start {
                            seg_end
                        } else {
                            seg_end - 1
                        };
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
                        (target_line, target_col)
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
                        if sakura && response.triple_clicked() {
                            let pos = hit(self, pointer);
                            self.cur_buf_mut().select_line(pos.0);
                        } else if sakura && response.double_clicked() {
                            let pos = hit(self, pointer);
                            let buf = self.cur_buf_mut();
                            buf.set_cursor(pos.0, pos.1);
                            buf.select_word_at_cursor();
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
                window_style::bg_painter(&painter).rect_filled(gutter_rect, CornerRadius::ZERO, bg(BG_GUTTER));
                painter.line_segment(
                    [
                        Pos2::new(gutter_rect.max.x, avail_rect.min.y),
                        Pos2::new(gutter_rect.max.x, avail_rect.max.y),
                    ],
                    Stroke::new(1.0_f32, GUTTER_SEP),
                );

                // Virtualized painting（表示行単位。折り返しなしなら 表示行 = 論理行）
                let first_row = (scroll_y / self.line_height).floor().max(0.0) as usize;
                let last_row = ((scroll_y + viewport_h) / self.line_height).ceil() as usize;
                let end_row = (last_row + 1).min(total_rows);

                let editor_painter = ui.painter_at(editor_rect);
                let gutter_painter = ui.painter_at(gutter_rect);
                let cur_line_now = self.cur_buf().cursor_line;
                let cur_col_now = self.cur_buf().cursor_col;
                let search_marks = self.cur_slot().search_marks.as_ref()
                    .filter(|(rev, _)| *rev == self.cur_buf().revision)
                    .map(|(_, lines)| lines);

                for row_idx in first_row..end_row {
                    let (line_idx, seg) = row_to_line(row_idx);
                    let y = editor_rect.min.y + top_pad + row_idx as f32 * self.line_height
                        - scroll_y;
                    let is_current = line_idx == cur_line_now;
                    let full_line = &self.cur_buf().lines[line_idx];
                    let line_len = full_line.chars().count();
                    let (seg_start, seg_end) = seg_range(line_idx, seg, line_len);
                    let last_seg = is_last_seg(line_idx, seg);
                    let line = &full_line[char_to_byte_idx(full_line, seg_start)
                        ..char_to_byte_idx(full_line, seg_end)];

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

                    // 範囲選択の背景（この表示行の範囲に切り詰める）
                    if let Some((from, to, newline)) = self.cur_buf().line_selection(line_idx) {
                        let from = from.clamp(seg_start, seg_end) - seg_start;
                        let to = to.clamp(seg_start, seg_end) - seg_start;
                        let newline = newline && last_seg;
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

                    let has_newline = last_seg && line_idx + 1 < total_lines;
                    let end_x = if is_current && seg == cursor_seg && !self.ime_preedit.is_empty() {
                        let byte_pos = char_to_byte_idx(line, cur_col_now.saturating_sub(seg_start));
                        let before = expand_tabs_for_display(&line[..byte_pos]);
                        let after = expand_tabs_for_display(&line[byte_pos..]);
                        let mut cur_x = text_origin_x;

                        if !before.is_empty() {
                            let g = editor_painter.layout_no_wrap(
                                before,
                                font_id.clone(),
                                TEXT_WHITE,
                            );
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
                            let g_after = editor_painter.layout_no_wrap(
                                after,
                                font_id.clone(),
                                TEXT_WHITE,
                            );
                            let w = g_after.size().x;
                            editor_painter.galley(Pos2::new(cur_x, text_y), g_after, TEXT_WHITE);
                            cur_x += w;
                        }
                        cur_x
                    } else {
                        let disp = expand_tabs_for_display(line);
                        let galley =
                            editor_painter.layout_no_wrap(disp, font_id.clone(), TEXT_WHITE);
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
                let eof_y = editor_rect.min.y
                    + top_pad
                    + total_rows as f32 * self.line_height
                    - scroll_y;
                if eof_y >= editor_rect.min.y - self.line_height && eof_y <= editor_rect.max.y {
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
                let caret_y = editor_rect.min.y + top_pad + cursor_px_y - scroll_y;
                let caret_h = (self.line_height - 2.0).max(4.0);
                let caret_bar_w = (self.font_size / 8.0).clamp(1.5, 4.0);
                let cursor_rect = Rect::from_min_size(
                    Pos2::new(caret_x, caret_y + 1.0),
                    Vec2::new(cursor_w, caret_h),
                );

                if editor_rect.intersects(cursor_rect) && self.inline_prompt.is_none() {
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
                if self.inline_prompt.is_none()
                    && self.filer_modal.is_none()
                    && self.open_menu.is_none()
                    && !self.show_about
                {
                    let preedit_start_x =
                        editor_rect.min.x + left_pad + cursor_px_x - scroll_x;
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
                if max_scroll_y > 0.0 {
                    let ratio = (viewport_h / total_content_h).clamp(0.06, 1.0);
                    let thumb_h = (sb_rect.height() * ratio).max(24.0);
                    let track_h = (sb_rect.height() - thumb_h).max(1.0);
                    let thumb_y = sb_rect.min.y + (scroll_y / max_scroll_y) * track_h;
                    let thumb_rect = Rect::from_min_size(
                        Pos2::new(sb_rect.min.x + 2.0, thumb_y),
                        Vec2::new(scrollbar_w - 4.0, thumb_h),
                    );
                    painter.rect_filled(thumb_rect, CornerRadius::same(3), BORDER_BLUE);

                    let sb_resp =
                        ui.interact(sb_rect, ui.id().with("vscroll"), Sense::click_and_drag());
                    if sb_resp.dragged() || sb_resp.clicked() {
                        if let Some(pos) = sb_resp.interact_pointer_pos() {
                            let norm =
                                ((pos.y - sb_rect.min.y - thumb_h * 0.5) / track_h).clamp(0.0, 1.0);
                            scroll_y = norm * max_scroll_y;
                        }
                    }
                }

                let slot = self.cur_slot_mut();
                slot.scroll_x = scroll_x;
                slot.scroll_y = scroll_y;
                if wrap.is_some() {
                    slot.wrap = wrap;
                }
            });
    }
}
