//! ファイラーの描画。

use super::*;

impl VzEditorApp {
    pub(super) fn render_filer_canvas(&mut self, ctx: &egui::Context) {
        // When browsing in Filer (no modal open), disable IME so Space/Tab/ESC/VZ keys work cleanly.
        if self.filer_modal.is_none() {
            ctx.output_mut(|o| {
                o.ime = None;
            });
        }

        let mut open_double_clicked = false;

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(bg(BG_NAVY))
                    .inner_margin(egui::Margin::same(6)),
            )
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                let avail = ui.available_rect_before_wrap();
                let gap = 6.0_f32;
                let half_w = ((avail.width() - gap) * 0.5).max(200.0);

                let left_rect = Rect::from_min_size(avail.min, Vec2::new(half_w, avail.height()));
                let right_rect = Rect::from_min_size(
                    Pos2::new(avail.min.x + half_w + gap, avail.min.y),
                    Vec2::new(avail.width() - half_w - gap, avail.height()),
                );

                if self.paint_filer_pane(ui, ctx, ActivePane::Left, left_rect) {
                    open_double_clicked = true;
                }
                if self.paint_filer_pane(ui, ctx, ActivePane::Right, right_rect) {
                    open_double_clicked = true;
                }
            });

        if open_double_clicked {
            self.filer_open_selected();
        }
    }

    /// Paint one side (Left or Right) of the 2-pane VZ Filer. Returns true if an item was double-clicked.
    pub(super) fn paint_filer_pane(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        side: ActivePane,
        rect: Rect,
    ) -> bool {
        let is_active = self.filer.active == side;
        let pane_id = ui.id().with(match side {
            ActivePane::Left => "filer_pane_left",
            ActivePane::Right => "filer_pane_right",
        });

        // Scale all filer typography & row geometry proportionally from self.font_size
        let scale = self.font_size / DEFAULT_FONT_SIZE;
        let name_font_size = (14.0 * scale).clamp(9.0, 34.0);
        let hdr_font_size = (13.0 * scale).clamp(9.0, 28.0);
        let size_font_size = (12.5 * scale).clamp(8.5, 26.0);
        let date_font_size = (12.0 * scale).clamp(8.5, 24.0);
        let col_font_size = (12.0 * scale).clamp(8.5, 24.0);

        let header_h = (hdr_font_size * 2.0).round().max(24.0);
        let col_hdr_h = (col_font_size * 1.85).round().max(20.0);
        let row_h = (name_font_size * 1.55).round().max(name_font_size + 6.0);
        let sb_w = 10.0_f32;

        // Measure monospace glyph widths at the active font scale for accurate column layout & clipping
        let (hdr_char_w, name_char_w, size_col_w, date_col_w) = ui.fonts(|f| {
            let hc = f
                .layout_no_wrap("M".into(), FontId::monospace(hdr_font_size), TEXT_WHITE)
                .size()
                .x
                .max(hdr_font_size * 0.55);
            let nc = f
                .layout_no_wrap("M".into(), FontId::monospace(name_font_size), TEXT_WHITE)
                .size()
                .x
                .max(name_font_size * 0.55);
            let sc =
                f.layout_no_wrap(
                    "999.9 M".into(),
                    FontId::monospace(size_font_size),
                    TEXT_WHITE,
                )
                .size()
                .x + 10.0;
            let dc =
                f.layout_no_wrap(
                    "2026-11-23 00:00".into(),
                    FontId::monospace(date_font_size),
                    TEXT_DIM_CYAN,
                )
                .size()
                .x + 10.0;
            (hc, nc, sc, dc)
        });

        let border_color = if is_active {
            BORDER_ACTIVE_PANE
        } else {
            BORDER_BLUE
        };
        let hdr_bg = if is_active {
            FILER_HDR_ACTIVE
        } else {
            FILER_HDR_INACTIVE
        };

        let painter = ui.painter_at(rect);
        painter.rect(
            rect,
            CornerRadius::same(2),
            Color32::TRANSPARENT,
            Stroke::new(if is_active { 1.5_f32 } else { 1.0_f32 }, border_color),
            StrokeKind::Inside,
        );

        // Directory Header Bar
        let hdr_rect = Rect::from_min_size(
            Pos2::new(rect.min.x + 1.0, rect.min.y + 1.0),
            Vec2::new(rect.width() - 2.0, header_h),
        );
        painter.rect_filled(hdr_rect, CornerRadius::ZERO, hdr_bg);

        let pane = match side {
            ActivePane::Left => &mut self.filer.left,
            ActivePane::Right => &mut self.filer.right,
        };

        let side_tag = match side {
            ActivePane::Left => "[L]",
            ActivePane::Right => "[R]",
        };
        let entry_count = pane.entries.iter().filter(|e| !e.is_parent).count();
        let marked_count = pane.marked.len();
        let right_info = if marked_count > 0 {
            format!("*{marked_count}/{entry_count}")
        } else {
            format!("{entry_count}件")
        };
        let right_info_w = (right_info.chars().count() as f32 * hdr_char_w * 1.2).max(56.0);
        let max_dir_cols = ((hdr_rect.width() - right_info_w - hdr_char_w * 6.0) / hdr_char_w)
            .floor()
            .max(8.0) as usize;
        let dir_full = filer_dir_label(&pane.current_dir);
        let dir_str = truncate_path_left(&dir_full, max_dir_cols);

        painter.text(
            Pos2::new(hdr_rect.min.x + 8.0, hdr_rect.center().y),
            Align2::LEFT_CENTER,
            format!("{side_tag} {dir_str}"),
            FontId::monospace(hdr_font_size),
            if is_active { TEXT_CYAN } else { TEXT_WHITE },
        );

        painter.text(
            Pos2::new(hdr_rect.max.x - 8.0, hdr_rect.center().y),
            Align2::RIGHT_CENTER,
            right_info,
            FontId::monospace(size_font_size),
            if marked_count > 0 {
                TEXT_AMBER
            } else {
                TEXT_DIM_CYAN
            },
        );

        // Column Header Row
        let col_rect = Rect::from_min_size(
            Pos2::new(rect.min.x + 1.0, hdr_rect.max.y),
            Vec2::new(rect.width() - 2.0, col_hdr_h),
        );
        painter.rect_filled(col_rect, CornerRadius::ZERO, FILER_COL_HDR);
        painter.line_segment(
            [
                Pos2::new(col_rect.min.x, col_rect.max.y),
                Pos2::new(col_rect.max.x, col_rect.max.y),
            ],
            Stroke::new(1.0_f32, GUTTER_SEP),
        );

        let list_rect = Rect::from_min_max(
            Pos2::new(rect.min.x + 1.0, col_rect.max.y + 1.0),
            Pos2::new(rect.max.x - sb_w - 1.0, rect.max.y - 1.0),
        );

        let mark_x = list_rect.min.x + (8.0 * scale).clamp(6.0, 14.0);
        let name_x = list_rect.min.x + (22.0 * scale).clamp(16.0, 38.0);
        let min_name_w = (name_char_w * 12.0).max(96.0);
        let show_date = list_rect.width()
            >= (name_x - list_rect.min.x) + min_name_w + size_col_w + date_col_w + 24.0;
        let show_size =
            list_rect.width() >= (name_x - list_rect.min.x) + min_name_w + size_col_w + 16.0;

        let date_x = list_rect.max.x - 8.0;
        let size_x = if show_date {
            date_x - date_col_w - 8.0
        } else {
            list_rect.max.x - 8.0
        };
        let name_right_limit = if show_size {
            size_x - size_col_w - 6.0
        } else {
            list_rect.max.x - 8.0
        };
        let name_max_w = (name_right_limit - name_x).max(60.0);

        painter.text(
            Pos2::new(name_x, col_rect.center().y),
            Align2::LEFT_CENTER,
            "名前",
            FontId::monospace(col_font_size),
            TEXT_DIM_CYAN,
        );
        if show_size {
            painter.text(
                Pos2::new(size_x, col_rect.center().y),
                Align2::RIGHT_CENTER,
                "サイズ",
                FontId::monospace(col_font_size),
                TEXT_DIM_CYAN,
            );
        }
        if show_date {
            painter.text(
                Pos2::new(date_x, col_rect.center().y),
                Align2::RIGHT_CENTER,
                "更新日時",
                FontId::monospace(col_font_size),
                TEXT_DIM_CYAN,
            );
        }

        // Interaction area for the pane
        let resp = ui.interact(list_rect, pane_id, Sense::click());
        let visible_rows = (list_rect.height() / row_h).floor().max(1.0) as usize;
        if is_active {
            self.last_filer_visible_rows = visible_rows;
        }

        // Scroll handling
        let pane = match side {
            ActivePane::Left => &mut self.filer.left,
            ActivePane::Right => &mut self.filer.right,
        };
        if resp.hovered() {
            let delta = ctx.input(|i| i.smooth_scroll_delta);
            if delta.y.abs() > 0.0 {
                pane.scroll_y -= delta.y;
            }
        }

        if pane.need_scroll_to_cursor {
            let cy = pane.cursor as f32 * row_h;
            if cy < pane.scroll_y {
                pane.scroll_y = cy;
            } else if cy + row_h > pane.scroll_y + list_rect.height() {
                pane.scroll_y = cy + row_h - list_rect.height();
            }
            pane.need_scroll_to_cursor = false;
        }

        let total_h = pane.entries.len() as f32 * row_h;
        let max_scroll = (total_h - list_rect.height()).max(0.0);
        pane.scroll_y = pane.scroll_y.clamp(0.0, max_scroll);

        // Mouse click / double-click on rows
        let mut double_clicked = false;
        // ヘルプ・About 表示中のクリックはそれを閉じるだけにする
        let overlay_open = self.show_help || self.show_about;
        if !overlay_open && (resp.clicked() || resp.double_clicked()) {
            if let Some(pos) = resp.interact_pointer_pos() {
                let clicked_row = ((pos.y - list_rect.min.y + pane.scroll_y) / row_h)
                    .floor()
                    .max(0.0) as usize;
                if clicked_row < pane.entries.len() {
                    pane.cursor = clicked_row;
                    if resp.double_clicked() {
                        double_clicked = true;
                    }
                }
            }
            self.filer.active = side;
        }

        // Re-borrow pane immutably for painting
        let pane = match side {
            ActivePane::Left => &self.filer.left,
            ActivePane::Right => &self.filer.right,
        };
        let is_active_now = self.filer.active == side;
        let list_painter = ui.painter_at(list_rect);

        let first_row = (pane.scroll_y / row_h).floor().max(0.0) as usize;
        let last_row = ((pane.scroll_y + list_rect.height()) / row_h).ceil() as usize;
        let end_row = (last_row + 1).min(pane.entries.len());

        let cursor_bar_w = (3.0 * scale).clamp(2.5, 5.0);

        for idx in first_row..end_row {
            let entry = &pane.entries[idx];
            let y = list_rect.min.y + idx as f32 * row_h - pane.scroll_y;
            let row_rect = Rect::from_min_size(
                Pos2::new(list_rect.min.x, y),
                Vec2::new(list_rect.width(), row_h),
            );

            let is_cursor = idx == pane.cursor;
            let is_marked = !entry.is_parent && pane.marked.contains(&entry.path);

            let row_bg = match (is_cursor, is_marked, is_active_now) {
                (true, true, true) => Some(FILER_ROW_MARKED_CURSOR),
                (true, false, true) => Some(FILER_ROW_CURSOR_ACTIVE),
                (true, _, false) => Some(FILER_ROW_CURSOR_INACTIVE),
                (false, true, _) => Some(FILER_ROW_MARKED),
                (false, false, _) => None,
            };
            if let Some(bg) = row_bg {
                list_painter.rect_filled(row_rect, CornerRadius::ZERO, bg);
            }
            if is_cursor && is_active_now {
                list_painter.rect_filled(
                    Rect::from_min_size(row_rect.min, Vec2::new(cursor_bar_w, row_h)),
                    CornerRadius::ZERO,
                    CURSOR_BAR,
                );
            }

            // Mark asterisk
            if is_marked {
                list_painter.text(
                    Pos2::new(mark_x, row_rect.center().y),
                    Align2::LEFT_CENTER,
                    "*",
                    FontId::monospace(name_font_size),
                    TEXT_AMBER,
                );
            }

            // Name display
            let display_name: Cow<str> = if entry.is_parent {
                Cow::Borrowed("..")
            } else if entry.is_dir {
                Cow::Owned(format!("{}/", entry.name))
            } else {
                Cow::Borrowed(&entry.name)
            };
            let max_chars = (name_max_w / name_char_w).floor().max(8.0) as usize;
            let clipped_name = truncate_display_right(&display_name, max_chars);

            let name_color = if is_marked {
                TEXT_AMBER
            } else if entry.is_dir {
                TEXT_CYAN
            } else {
                TEXT_WHITE
            };
            list_painter.text(
                Pos2::new(name_x, row_rect.center().y),
                Align2::LEFT_CENTER,
                clipped_name,
                FontId::monospace(name_font_size),
                name_color,
            );

            // Size column
            if show_size {
                let size_text: Cow<str> = if entry.is_parent {
                    Cow::Borrowed("<UP>")
                } else if entry.is_dir {
                    Cow::Borrowed("<DIR>")
                } else {
                    Cow::Owned(format_size(entry.size))
                };
                list_painter.text(
                    Pos2::new(size_x, row_rect.center().y),
                    Align2::RIGHT_CENTER,
                    size_text,
                    FontId::monospace(size_font_size),
                    if entry.is_dir {
                        TEXT_DIM_CYAN
                    } else {
                        TEXT_WHITE
                    },
                );
            }

            // Modified timestamp
            if show_date && !entry.modified_str.is_empty() {
                list_painter.text(
                    Pos2::new(date_x, row_rect.center().y),
                    Align2::RIGHT_CENTER,
                    &entry.modified_str,
                    FontId::monospace(date_font_size),
                    TEXT_DIM_CYAN,
                );
            }
        }

        // Scrollbar for pane
        let sb_rect = Rect::from_min_max(
            Pos2::new(rect.max.x - sb_w - 1.0, col_rect.max.y + 1.0),
            Pos2::new(rect.max.x - 1.0, rect.max.y - 1.0),
        );
        window_style::bg_painter(&painter).rect_filled(sb_rect, CornerRadius::ZERO, bg(BG_GUTTER));
        if max_scroll > 0.0 {
            let ratio = (list_rect.height() / total_h).clamp(0.08, 1.0);
            let thumb_h = (sb_rect.height() * ratio).max(20.0);
            let track_h = (sb_rect.height() - thumb_h).max(1.0);
            let thumb_y = sb_rect.min.y + (pane.scroll_y / max_scroll) * track_h;
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(sb_rect.min.x + 2.0, thumb_y),
                    Vec2::new(sb_w - 4.0, thumb_h),
                ),
                CornerRadius::same(2),
                if is_active_now {
                    BORDER_ACTIVE_PANE
                } else {
                    BORDER_BLUE
                },
            );
        }

        double_clicked
    }
}

fn filer_dir_label(path: &Path) -> Cow<'_, str> {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};

        // 日本語フォントでバックスラッシュが円記号に見えるため、通常のパスは / で区切る。
        if let Some(Component::Prefix(prefix)) = path.components().next() {
            match prefix.kind() {
                Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(..) => {
                    let plain = vz_platform::shell::plain_path(path);
                    return Cow::Owned(plain.to_string_lossy().replace('\\', "/"));
                }
                Prefix::Verbatim(_) | Prefix::DeviceNS(_) => return path.to_string_lossy(),
                _ => {}
            }
        }
        let label = path.to_string_lossy();
        if label.contains('\\') {
            return Cow::Owned(label.replace('\\', "/"));
        }
        label
    }
    #[cfg(not(windows))]
    path.to_string_lossy()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn filer_dir_label_displays_windows_paths() {
        for (path, expected) in [
            (r"\\?\C:\gitroot\日本語", "C:/gitroot/日本語"),
            (r"C:\", "C:/"),
            (r"\\?\UNC\server\share\dir", "//server/share/dir"),
            (r"\\server\share", "//server/share"),
            (r"\\?\Volume{guid}\dir", r"\\?\Volume{guid}\dir"),
        ] {
            assert_eq!(filer_dir_label(Path::new(path)), expected);
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn filer_dir_label_preserves_unix_paths() {
        let path = r"/tmp/dir\name";
        assert_eq!(filer_dir_label(Path::new(path)), path);
    }
}
