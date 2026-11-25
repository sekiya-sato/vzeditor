//! ステータスバー、バッファ一覧、ファンクションキー表示。

use super::*;

impl VzEditorApp {
    pub(super) fn render_status_bar(&mut self, ctx: &egui::Context) {
        let mut switch_next_clicked = false;
        let mut toggle_md_clicked = false;
        let mut font_cmd: Option<i8> = None;
        let mut win_cmd: Option<MenuAction> = None;

        egui::TopBottomPanel::top("status_bar")
            .exact_height(30.0)
            .frame(
                egui::Frame::NONE
                    .fill(bg(STATUS_BG))
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .stroke(Stroke::new(1.0_f32, BORDER_BLUE)),
            )
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                ui.horizontal_centered(|ui| match self.mode {
                    AppMode::Editor => {
                        // LZ badge
                        egui::Frame::NONE
                            .fill(STATUS_BADGE_BG)
                            .corner_radius(CornerRadius::same(2))
                            .inner_margin(egui::Margin::symmetric(6, 1))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("VZ")
                                        .font(FontId::monospace(13.0))
                                        .strong()
                                        .color(STATUS_BADGE_FG),
                                );
                            });

                        ui.add_space(6.0);

                        // Multi-buffer count indicator `[1/3]` (clickable to cycle)
                        let buf_badge =
                            format!("[{}/{}]", self.active_buf + 1, self.buffers.len());
                        let badge_resp = ui.add(
                            egui::Button::new(
                                egui::RichText::new(buf_badge)
                                    .font(FontId::monospace(13.0))
                                    .strong()
                                    .color(if self.buffers.len() > 1 {
                                        TEXT_AMBER
                                    } else {
                                        TEXT_CYAN
                                    }),
                            )
                            .fill(BG_TAB_INACTIVE)
                            .stroke(Stroke::new(1.0_f32, BORDER_BLUE))
                            .corner_radius(CornerRadius::same(2)),
                        );
                        if badge_resp.clicked() {
                            switch_next_clicked = true;
                        }
                        badge_resp.on_hover_text("Ctrl+W またはクリックでバッファ切替");

                        ui.add_space(6.0);

                        // File name & dirty flag '*'
                        let name = self.cur_buf().display_name();
                        ui.label(
                            egui::RichText::new(name)
                                .font(FontId::monospace(14.0))
                                .strong()
                                .color(TEXT_WHITE),
                        );
                        if self.cur_buf().dirty {
                            ui.label(
                                egui::RichText::new("*")
                                    .font(FontId::monospace(15.0))
                                    .strong()
                                    .color(TEXT_AMBER),
                            );
                        }

                        // Markdown [EDIT] / [VIEW] clickable toggle badge
                        let is_md = self.cur_buf().is_markdown();
                        let in_view = self.cur_slot().markdown_view;
                        if is_md || in_view {
                            ui.add_space(6.0);
                            let (md_label, md_bg, md_fg, md_stroke) = if in_view {
                                (
                                    "[VIEW]",
                                    STATUS_BADGE_VIEW_BG,
                                    STATUS_BADGE_FG,
                                    STATUS_BADGE_VIEW_BG,
                                )
                            } else {
                                ("[EDIT]", BG_TAB_INACTIVE, TEXT_CYAN, BORDER_BLUE)
                            };
                            let md_resp = ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(md_label)
                                            .font(FontId::monospace(12.5))
                                            .strong()
                                            .color(md_fg),
                                    )
                                    .fill(md_bg)
                                    .stroke(Stroke::new(1.0_f32, md_stroke))
                                    .corner_radius(CornerRadius::same(2)),
                                )
                                .on_hover_text(
                                    "クリックまたは Ctrl+P / Alt+P / Ctrl+K+V で Markdown 編集/ビュー切替",
                                );
                            if md_resp.clicked() {
                                toggle_md_clicked = true;
                            }
                        }

                        // Prefix key badge (^Q / ^K)
                        if self.prefix != PrefixKey::None {
                            ui.add_space(8.0);
                            let ptext = match self.prefix {
                                PrefixKey::CtrlQ => "^Q",
                                PrefixKey::CtrlK => "^K",
                                PrefixKey::None => "",
                            };
                            egui::Frame::NONE
                                .fill(TEXT_AMBER)
                                .corner_radius(CornerRadius::same(2))
                                .inner_margin(egui::Margin::symmetric(5, 1))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(ptext)
                                            .font(FontId::monospace(13.0))
                                            .strong()
                                            .color(STATUS_BADGE_FG),
                                    );
                                });
                        }

                        // Status message (truncated so right-aligned controls never clip)
                        if !self.status_message.is_empty() {
                            let right_reserve = 380.0_f32;
                            let avail_msg_w = ui.available_width() - right_reserve;
                            if avail_msg_w > 48.0 {
                                ui.add_space(8.0);
                                let max_cols = ((avail_msg_w - 8.0) / 7.6).floor().max(6.0) as usize;
                                let clipped = truncate_display_right(&self.status_message, max_cols);
                                let msg_color = if self.status_message.contains("エラー") {
                                    TEXT_AMBER
                                } else {
                                    TEXT_CYAN
                                };
                                ui.label(
                                    egui::RichText::new(clipped)
                                        .font(FontId::monospace(12.5))
                                        .color(msg_color),
                                );
                            }
                        }

                        // Right-aligned encoding + line/column info + font size controls
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let buf = self.cur_buf();
                            let pos_text = if self.cur_slot().markdown_view {
                                format!("全{}行 [VIEW]", buf.lines.len())
                            } else {
                                format!(
                                    "Ln {:>4}/{:<4}  Col {:>3}",
                                    buf.display_line_num(),
                                    buf.lines.len(),
                                    buf.display_col_num()
                                )
                            };
                            ui.label(
                                egui::RichText::new(pos_text)
                                    .font(FontId::monospace(13.0))
                                    .color(TEXT_CYAN),
                            );

                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("|")
                                    .font(FontId::monospace(13.0))
                                    .color(BORDER_BLUE),
                            );
                            ui.add_space(6.0);

                            let eol = if buf.line_ending == "\r\n" {
                                "CRLF"
                            } else {
                                "LF"
                            };
                            ui.label(
                                egui::RichText::new(format!("{} {}", buf.encoding, eol))
                                    .font(FontId::monospace(13.0))
                                    .color(TEXT_WHITE),
                            );

                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("|")
                                    .font(FontId::monospace(13.0))
                                    .color(BORDER_BLUE),
                            );
                            ui.add_space(6.0);

                            if let Some(cmd) = Self::paint_font_size_controls(ui, self.font_size) {
                                font_cmd = Some(cmd);
                            }
                            ui.add_space(6.0);
                            if let Some(cmd) = Self::paint_window_style_controls(
                                ui,
                                self.bg_opacity,
                                self.text_opacity,
                                self.always_on_top,
                            ) {
                                win_cmd = Some(cmd);
                            }

                            if !self.cur_slot().markdown_view
                                && (self.ime_active || !self.ime_preedit.is_empty())
                            {
                                ui.add_space(8.0);
                                egui::Frame::NONE
                                    .fill(PREEDIT_BG)
                                    .corner_radius(CornerRadius::same(2))
                                    .inner_margin(egui::Margin::symmetric(5, 1))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new("IME")
                                                .font(FontId::monospace(12.0))
                                                .color(PREEDIT_FG),
                                        );
                                    });
                            }
                        });
                    }
                    AppMode::Filer => {
                        // FILER badge
                        egui::Frame::NONE
                            .fill(STATUS_BADGE_FILER_BG)
                            .corner_radius(CornerRadius::same(2))
                            .inner_margin(egui::Margin::symmetric(6, 1))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("LZ FILER")
                                        .font(FontId::monospace(13.0))
                                        .strong()
                                        .color(STATUS_BADGE_FG),
                                );
                            });

                        ui.add_space(8.0);

                        // Open buffers summary
                        let open_summary = format!(
                            "編集中:{}件 ({})",
                            self.buffers.len(),
                            self.cur_buf().display_name()
                        );
                        ui.label(
                            egui::RichText::new(open_summary)
                                .font(FontId::monospace(13.0))
                                .color(TEXT_WHITE),
                        );

                        // Marked items indicator on active pane
                        let marked_count = self.filer.active_pane().marked.len();
                        if marked_count > 0 {
                            ui.add_space(10.0);
                            egui::Frame::NONE
                                .fill(FILER_ROW_MARKED_CURSOR)
                                .corner_radius(CornerRadius::same(2))
                                .inner_margin(egui::Margin::symmetric(6, 1))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("マーク: {marked_count}件"))
                                            .font(FontId::monospace(12.5))
                                            .strong()
                                            .color(TEXT_AMBER),
                                    );
                                });
                        }

                        if !self.status_message.is_empty() {
                            let right_reserve = 250.0_f32;
                            let avail_msg_w = ui.available_width() - right_reserve;
                            if avail_msg_w > 48.0 {
                                ui.add_space(10.0);
                                let max_cols = ((avail_msg_w - 10.0) / 7.8).floor().max(6.0) as usize;
                                let clipped = truncate_display_right(&self.status_message, max_cols);
                                let msg_color = if self.status_message.contains("エラー")
                                    || self.status_message.contains("失敗")
                                {
                                    TEXT_AMBER
                                } else {
                                    TEXT_CYAN
                                };
                                ui.label(
                                    egui::RichText::new(clipped)
                                        .font(FontId::monospace(13.0))
                                        .color(msg_color),
                                );
                            }
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let sort_label = self.filer.active_pane().sort_mode.label();
                            let side = match self.filer.active {
                                ActivePane::Left => "左窓",
                                ActivePane::Right => "右窓",
                            };
                            ui.label(
                                egui::RichText::new(format!("{side} [{sort_label}]"))
                                    .font(FontId::monospace(13.0))
                                    .color(TEXT_CYAN),
                            );

                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("|")
                                    .font(FontId::monospace(13.0))
                                    .color(BORDER_BLUE),
                            );
                            ui.add_space(6.0);

                            if let Some(cmd) = Self::paint_font_size_controls(ui, self.font_size) {
                                font_cmd = Some(cmd);
                            }
                            ui.add_space(6.0);
                            if let Some(cmd) = Self::paint_window_style_controls(
                                ui,
                                self.bg_opacity,
                                self.text_opacity,
                                self.always_on_top,
                            ) {
                                win_cmd = Some(cmd);
                            }
                        });
                    }
                    AppMode::Terminal | AppMode::Browser => {
                        let (badge, badge_bg) = if self.mode == AppMode::Terminal {
                            ("TERM", STATUS_BADGE_VIEW_BG)
                        } else {
                            ("WEB", STATUS_BADGE_FILER_BG)
                        };
                        egui::Frame::NONE
                            .fill(badge_bg)
                            .corner_radius(CornerRadius::same(2))
                            .inner_margin(egui::Margin::symmetric(6, 1))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(badge)
                                        .font(FontId::monospace(13.0))
                                        .strong()
                                        .color(STATUS_BADGE_FG),
                                );
                            });
                        let avail_msg_w = ui.available_width() - 250.0;
                        if !self.status_message.is_empty() && avail_msg_w > 48.0 {
                            ui.add_space(10.0);
                            let max_cols = ((avail_msg_w - 10.0) / 7.8).floor().max(6.0) as usize;
                            let msg_color = if self.status_message.contains("エラー") {
                                TEXT_AMBER
                            } else {
                                TEXT_CYAN
                            };
                            ui.label(
                                egui::RichText::new(truncate_display_right(&self.status_message, max_cols))
                                    .font(FontId::monospace(13.0))
                                    .color(msg_color),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(cmd) = Self::paint_font_size_controls(ui, self.font_size) {
                                font_cmd = Some(cmd);
                            }
                            ui.add_space(6.0);
                            if let Some(cmd) = Self::paint_window_style_controls(
                                ui,
                                self.bg_opacity,
                                self.text_opacity,
                                self.always_on_top,
                            ) {
                                win_cmd = Some(cmd);
                            }
                        });
                    }
                });
            });

        if let Some(action) = win_cmd {
            self.execute_menu_action(ctx, action);
        }
        if switch_next_clicked {
            self.switch_buffer_step(1);
        }
        if toggle_md_clicked {
            self.toggle_markdown_view();
        }
        if let Some(cmd) = font_cmd {
            match cmd {
                1 => self.zoom_in_font(),
                -1 => self.zoom_out_font(),
                _ => self.reset_font_size(),
            }
        }
    }

    /// バッファ帯（ファイルが 2 件以上、またはターミナル・ブラウザを開いたとき）。
    /// ファイルのタブの後ろにターミナル・ブラウザのタブを並べる（各 1 つ）。
    pub(super) fn render_buffer_strip(&mut self, ctx: &egui::Context) {
        let has_terminal = self.terminal.is_some();
        let has_browser = self.browser_open;
        if self.mode == AppMode::Filer || (self.buffers.len() < 2 && !has_terminal && !has_browser) {
            return;
        }
        let in_editor = self.mode == AppMode::Editor;
        let mut selected_idx: Option<usize> = None;
        let mut tool_clicked: Option<AppMode> = None;

        egui::TopBottomPanel::top("buffer_strip")
            .exact_height(24.0)
            .frame(
                egui::Frame::NONE
                    .fill(bg(BG_TAB_BAR))
                    .inner_margin(egui::Margin::symmetric(8, 2))
                    .stroke(Stroke::new(1.0_f32, GUTTER_SEP)),
            )
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                ui.horizontal_centered(|ui| {
                    ui.label(
                        egui::RichText::new("^W バッファ:")
                            .font(FontId::monospace(12.0))
                            .color(TEXT_DIM_CYAN),
                    );
                    ui.add_space(4.0);

                    for (idx, slot) in self.buffers.iter().enumerate() {
                        let dirty_mark = if slot.buffer.dirty { "*" } else { "" };
                        let view_mark = if slot.markdown_view { "[V]" } else { "" };
                        let title = format!(
                            "{}:{}{}{}",
                            idx + 1,
                            slot.buffer.display_name(),
                            dirty_mark,
                            view_mark
                        );
                        if strip_tab(ui, title, in_editor && idx == self.active_buf).clicked() {
                            selected_idx = Some(idx);
                        }
                    }

                    if has_terminal || has_browser {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new("|").font(FontId::monospace(12.0)).color(GUTTER_SEP));
                        ui.add_space(2.0);
                    }
                    if let Some(term) = &self.terminal {
                        let label = if term.is_running() {
                            "ターミナル"
                        } else {
                            "ターミナル(終了)"
                        };
                        let resp = strip_tab(ui, label.to_string(), self.mode == AppMode::Terminal);
                        if resp.on_hover_text(format!("Alt+K  {}", term.program())).clicked() {
                            tool_clicked = Some(AppMode::Terminal);
                        }
                    }
                    if has_browser {
                        let name = if self.browser_title.is_empty() {
                            "ブラウザ".to_string()
                        } else {
                            format!("ブラウザ:{}", truncate_display_right(&self.browser_title, 24))
                        };
                        let resp = strip_tab(ui, name, self.mode == AppMode::Browser);
                        if resp.on_hover_text("Alt+B").clicked() {
                            tool_clicked = Some(AppMode::Browser);
                        }
                    }
                });
            });

        if let Some(idx) = selected_idx {
            if self.mode != AppMode::Editor {
                self.enter_editor();
            }
            self.switch_buffer_to(idx);
        }
        match tool_clicked {
            Some(AppMode::Terminal) if self.mode != AppMode::Terminal => self.toggle_terminal(ctx),
            Some(AppMode::Browser) if self.mode != AppMode::Browser => self.toggle_browser(),
            _ => {}
        }
    }

    // --- Function Key Guide Bar (F1..F10, 2 blocks of 5 keys) ---

    pub(super) fn render_fkey_bar(&mut self, ctx: &egui::Context) {
        match self.mode {
            AppMode::Editor => self.render_editor_fkey_bar(ctx),
            AppMode::Filer => self.render_filer_fkey_bar(ctx),
            // F キーはシェル・ページへ渡すので案内しない
            AppMode::Terminal | AppMode::Browser => {}
        }
    }

    pub(super) fn render_editor_fkey_bar(&mut self, ctx: &egui::Context) {
        let in_md_view = self.is_markdown_view();
        let keys_left: &[(&str, &str, EditorAction)] = if in_md_view {
            &[
                ("F1", "ヘルプ", EditorAction::ToggleHelp),
                ("F2", "新規", EditorAction::NewBuffer),
                ("F3", "開く", EditorAction::OpenFile),
                ("F4", "先頭", EditorAction::MoveLineStart),
                ("F5", "末尾", EditorAction::MoveLineEnd),
            ]
        } else {
            &[
                ("F1", "ヘルプ", EditorAction::ToggleHelp),
                ("F2", "新規", EditorAction::NewBuffer),
                ("F3", "開く", EditorAction::OpenFile),
                ("F4", "行頭", EditorAction::MoveLineStart),
                ("F5", "行末", EditorAction::MoveLineEnd),
            ]
        };
        let keys_right: &[(&str, &str, EditorAction)] = if in_md_view {
            &[
                ("F6", "行削除", EditorAction::DeleteLine),
                ("F7", "半頁上", EditorAction::MoveWordLeft),
                ("F8", "半頁下", EditorAction::MoveWordRight),
                ("F9", "ファイラ", EditorAction::SwitchToFiler),
                ("F10", "保存", EditorAction::SaveFile),
            ]
        } else {
            &[
                ("F6", "行削除", EditorAction::DeleteLine),
                ("F7", "単語左", EditorAction::MoveWordLeft),
                ("F8", "単語右", EditorAction::MoveWordRight),
                ("F9", "ファイラ", EditorAction::SwitchToFiler),
                ("F10", "保存", EditorAction::SaveFile),
            ]
        };

        // サクラ互換の F キー（F1〜F3 は VZ 互換と同じ。未対応のキーは空欄）
        let sakura_left: &[(&str, &str, EditorAction)] = &[
            ("F1", "ヘルプ", EditorAction::ToggleHelp),
            ("F2", "新規", EditorAction::NewBuffer),
            ("F3", "開く", EditorAction::OpenFile),
            ("F4", "閉じる", EditorAction::CloseBuffer),
            ("F5", "", EditorAction::Nop),
        ];
        let keys_left = if self.is_sakura() { sakura_left } else { keys_left };
        let sakura_right: &[(&str, &str, EditorAction)] = &[
            ("F6", "選択", EditorAction::BeginSelect),
            ("F7", "切取", EditorAction::Cut),
            ("F8", "コピー", EditorAction::Copy),
            ("F9", "貼付", EditorAction::Paste),
            ("F10", "", EditorAction::Nop),
        ];
        let keys_right = if self.is_sakura() { sakura_right } else { keys_right };

        if let Some(action) = Self::paint_fkey_bar(ctx, keys_left, keys_right, FKEY_NUM_BG) {
            self.execute_editor_action(action);
        }
    }

    pub(super) fn render_filer_fkey_bar(&mut self, ctx: &egui::Context) {
        let keys_left: &[(&str, &str, FilerAction)] = &[
            ("F1", "ヘルプ", FilerAction::ToggleHelp),
            ("F2", "名前変", FilerAction::Rename),
            ("F3", "新規作成", FilerAction::NewFile),
            ("F4", "コピー", FilerAction::Copy),
            ("F5", "移動", FilerAction::Move),
        ];
        let keys_right: &[(&str, &str, FilerAction)] = &[
            ("F6", "削除", FilerAction::Delete),
            ("F7", "フォルダ", FilerAction::NewDir),
            ("F8", "ソート", FilerAction::CycleSort),
            ("F9", "編集戻", FilerAction::BackToEditor),
            ("F10", "終了", FilerAction::Quit),
        ];

        if let Some(action) = Self::paint_fkey_bar(ctx, keys_left, keys_right, FKEY_NUM_FILER_BG) {
            self.execute_filer_action(ctx, action);
        }
    }

    /// F キーの案内バー（左右 5 個ずつ）を描き、押されたボタンの動作を返す。
    pub(super) fn paint_fkey_bar<A: Copy>(
        ctx: &egui::Context,
        keys_left: &[(&str, &str, A)],
        keys_right: &[(&str, &str, A)],
        badge_color: Color32,
    ) -> Option<A> {
        let mut clicked_action: Option<A> = None;

        egui::TopBottomPanel::bottom("fkey_guide_bar")
            .exact_height(34.0)
            .frame(
                egui::Frame::NONE
                    .fill(bg(FKEY_BAR_BG))
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .stroke(Stroke::new(1.0_f32, BORDER_BLUE)),
            )
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                let avail_w = ui.available_width();
                let block_gap = 18.0_f32;
                let key_gap = 4.0_f32;
                let btn_w = ((avail_w - block_gap - key_gap * 8.0) / 10.0).max(56.0);
                let btn_h = ui.available_height().max(24.0);

                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = key_gap;

                    for (i, keys) in [keys_left, keys_right].into_iter().enumerate() {
                        if i > 0 {
                            ui.add_space(block_gap - key_gap);
                        }
                        for &(fkey, label, action) in keys {
                            if Self::paint_fkey_button(
                                ui,
                                fkey,
                                label,
                                Vec2::new(btn_w, btn_h),
                                badge_color,
                            ) {
                                clicked_action = Some(action);
                            }
                        }
                    }
                });
            });

        clicked_action
    }

    pub(super) fn paint_fkey_button(
        ui: &mut egui::Ui,
        fkey: &str,
        label: &str,
        size: Vec2,
        badge_color: Color32,
    ) -> bool {
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let bg = if response.hovered() {
                FKEY_BTN_HOVER
            } else {
                FKEY_BTN_BG
            };
            painter.rect(
                rect,
                CornerRadius::same(2),
                bg,
                Stroke::new(1.0_f32, BORDER_BLUE),
                StrokeKind::Inside,
            );

            let badge_w = (rect.width() * 0.34).clamp(24.0, 34.0);
            let badge_rect =
                Rect::from_min_max(rect.min, Pos2::new(rect.min.x + badge_w, rect.max.y));
            painter.rect_filled(badge_rect, CornerRadius::same(1), badge_color);
            painter.text(
                badge_rect.center(),
                Align2::CENTER_CENTER,
                fkey,
                FontId::monospace(11.5),
                FKEY_NUM_FG,
            );

            let label_center = Pos2::new(
                badge_rect.max.x + (rect.max.x - badge_rect.max.x) * 0.5,
                rect.center().y,
            );
            painter.text(
                label_center,
                Align2::CENTER_CENTER,
                label,
                FontId::monospace(12.0),
                TEXT_WHITE,
            );
        }
        response.clicked()
    }

    // --- Main Center View (Editor, Markdown View, or 2-Pane Filer) ---
}

/// バッファ帯のタブ 1 つ。
fn strip_tab(ui: &mut egui::Ui, title: String, active: bool) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(title)
                .font(FontId::monospace(12.5))
                .color(if active { TEXT_WHITE } else { TEXT_DIM_CYAN }),
        )
        .fill(if active { BG_TAB_ACTIVE } else { BG_TAB_INACTIVE })
        .stroke(Stroke::new(1.0_f32, if active { TEXT_CYAN } else { GUTTER_SEP }))
        .corner_radius(CornerRadius::same(2)),
    )
}
