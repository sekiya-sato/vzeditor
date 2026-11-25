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
                                    egui::RichText::new("FILER")
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
    /// ファイルのタブの後ろにターミナル（1 つ）・ブラウザ（タブごと）のタブを並べる。
    pub(super) fn render_buffer_strip(&mut self, ctx: &egui::Context) {
        let has_terminal = self.terminal.is_some();
        let has_browser = !self.browsers.is_empty();
        if self.mode == AppMode::Filer || (self.buffers.len() < 2 && !has_terminal && !has_browser) {
            return;
        }
        let in_editor = self.mode == AppMode::Editor;
        let mut selected_idx: Option<usize> = None;
        let mut terminal_clicked = false;
        let mut browser_clicked: Option<usize> = None;

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
                        // シェルが終了したターミナルは poll_terminal が閉じるので、ここに来るのは動作中だけ
                        let resp = strip_tab(ui, "ターミナル".to_string(), self.mode == AppMode::Terminal);
                        if resp.on_hover_text(format!("Alt+K  {}", term.program())).clicked() {
                            terminal_clicked = true;
                        }
                    }
                    let many = self.browsers.len() > 1;
                    for (i, tab) in self.browsers.iter().enumerate() {
                        // 複数あるときは番号を付ける。タイトルが無ければ URL
                        let head = if many { format!("ブラウザ{}", i + 1) } else { "ブラウザ".to_string() };
                        let label = tab.label();
                        let name = if label.is_empty() {
                            head
                        } else {
                            format!("{head}:{}", truncate_display_right(label, 24))
                        };
                        let active = self.mode == AppMode::Browser && i == self.active_browser;
                        let resp = strip_tab(ui, name, active);
                        if resp.on_hover_text("Alt+B / Ctrl+Tab で切替、Ctrl+W で閉じる").clicked() {
                            browser_clicked = Some(i);
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
        if terminal_clicked && self.mode != AppMode::Terminal {
            self.toggle_terminal(ctx);
        }
        if let Some(i) = browser_clicked {
            self.show_browser_tab(i);
        }
    }

    // --- Function Key Guide Bar (F1..F10 / F1..F12, blocks of 5+5(+2) keys) ---

    pub(super) fn render_fkey_bar(&mut self, ctx: &egui::Context) {
        match self.mode {
            AppMode::Editor => self.render_editor_fkey_bar(ctx),
            AppMode::Filer => self.render_filer_fkey_bar(ctx),
            // F キーはシェル・ページへ渡すので案内しない
            AppMode::Terminal | AppMode::Browser => {}
        }
    }

    /// エディターの F キー欄（F1〜F12 の順のラベルと動作）。`layer` は押している修飾キー。
    pub(super) fn editor_fkeys(
        sakura: bool,
        in_md_view: bool,
        layer: FKeyLayer,
    ) -> [(&'static str, EditorAction); 12] {
        use EditorAction::*;
        const NONE: (&str, EditorAction) = ("", Nop);
        if sakura {
            // サクラ互換（F1〜F3 は VZ 互換と同じ。未対応のキーは空欄）
            let mut keys = [NONE; 12];
            match layer {
                FKeyLayer::Normal => {
                    keys[..9].copy_from_slice(&[
                        ("ヘルプ", ToggleHelp),
                        ("新規", NewBuffer),
                        ("開く", OpenFile),
                        ("閉じる", CloseBuffer),
                        NONE,
                        ("選択", BeginSelect),
                        ("切取", Cut),
                        ("コピー", Copy),
                        ("貼付", Paste),
                    ]);
                }
                FKeyLayer::Shift => {
                    keys[2] = ("前検索", FindPrev);
                    keys[7] = ("コピー", Copy);
                    keys[9] = ("行複製", DuplicateLine);
                }
                FKeyLayer::Ctrl => {
                    keys[2] = ("行マーク", ToggleSearchMarks);
                    keys[3] = ("閉じる", CloseBuffer);
                }
                FKeyLayer::Alt => keys[3] = ("終了", Quit),
            }
            return keys;
        }
        // VZ 互換は修飾キーを区別しない（Ctrl+F4 の「閉じる」と、OS の Alt+F4 の終了を除く）
        let (f4, f5, f7, f8) = if in_md_view {
            ("先頭", "末尾", "半頁上", "半頁下")
        } else {
            ("行頭", "行末", "単語左", "単語右")
        };
        let mut keys = [
            ("ヘルプ", ToggleHelp),
            ("新規", NewBuffer),
            ("開く", OpenFile),
            (f4, MoveLineStart),
            (f5, MoveLineEnd),
            ("行削除", DeleteLine),
            (f7, MoveWordLeft),
            (f8, MoveWordRight),
            ("ファイラ", SwitchToFiler),
            ("保存", SaveFile),
            NONE,
            NONE,
        ];
        match layer {
            FKeyLayer::Ctrl => keys[3] = ("閉じる", CloseBuffer),
            FKeyLayer::Alt => keys[3] = ("終了", Quit),
            FKeyLayer::Normal | FKeyLayer::Shift => {}
        }
        keys
    }

    /// ファイラーの F キー欄。修飾キーは区別しない（OS の Alt+F4 の終了を除く）。
    pub(super) fn filer_fkeys(layer: FKeyLayer) -> [(&'static str, FilerAction); 12] {
        use FilerAction::*;
        let mut keys = [
            ("ヘルプ", ToggleHelp),
            ("名前変", Rename),
            ("新規作成", NewFile),
            ("コピー", Copy),
            ("移動", Move),
            ("削除", Delete),
            ("フォルダ", NewDir),
            ("ソート", CycleSort),
            ("編集戻", BackToEditor),
            ("終了", Quit),
            ("", Nop),
            ("", Nop),
        ];
        if layer == FKeyLayer::Alt {
            keys[3] = ("終了", Quit);
        }
        keys
    }

    pub(super) fn render_editor_fkey_bar(&mut self, ctx: &egui::Context) {
        let layer = FKeyLayer::current(ctx);
        let keys = Self::editor_fkeys(self.is_sakura(), self.is_markdown_view(), layer);
        match Self::paint_fkey_bar(ctx, &keys, layer, FKEY_NUM_BG) {
            Some(EditorAction::Quit) => self.request_quit(ctx),
            Some(action) => self.execute_editor_action(action),
            None => {}
        }
    }

    pub(super) fn render_filer_fkey_bar(&mut self, ctx: &egui::Context) {
        let layer = FKeyLayer::current(ctx);
        let keys = Self::filer_fkeys(layer);
        if let Some(action) = Self::paint_fkey_bar(ctx, &keys, layer, FKEY_NUM_FILER_BG) {
            self.execute_filer_action(ctx, action);
        }
    }

    /// F キーの案内バー（F1〜F5・F6〜F10・F11〜F12 の 3 ブロック）を描き、押されたボタンの動作を返す。
    /// F11〜F12 は 12 個がボタンの最小幅で並ぶときだけ表示する。キー名は `layer` に応じて `SF1` などにする。
    pub(super) fn paint_fkey_bar<A: Copy>(
        ctx: &egui::Context,
        keys: &[(&str, A); 12],
        layer: FKeyLayer,
        badge_color: Color32,
    ) -> Option<A> {
        let mut clicked_action: Option<A> = None;
        let names = layer.key_names();

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
                let (block_count, btn_w) = fkey_bar_layout(ui.available_width());
                let btn_h = ui.available_height().max(24.0);
                let shown = if block_count == 3 { 12 } else { 10 };

                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = FKEY_KEY_GAP;

                    for (i, (&(label, action), fkey)) in
                        keys.iter().zip(names).take(shown).enumerate()
                    {
                        if i == 5 || i == 10 {
                            ui.add_space(FKEY_BLOCK_GAP - FKEY_KEY_GAP);
                        }
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

            // SF10 などの長いキー名も収まるように広げる
            let badge_w = (rect.width() * 0.34)
                .clamp(24.0, 34.0)
                .max(fkey.len() as f32 * 7.0 + 6.0);
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

/// F キー欄に表示する修飾キーの段（Shift・Ctrl・Alt のどれか 1 つだけを押している間）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FKeyLayer {
    Normal,
    Shift,
    Ctrl,
    Alt,
}

impl FKeyLayer {
    /// 修飾キーから段を決める。2 つ以上の組み合わせ（Ctrl+Shift など）は通常の段。
    fn from_modifiers(m: egui::Modifiers) -> Self {
        match (m.shift, m.ctrl || m.command, m.alt) {
            (true, false, false) => Self::Shift,
            (false, true, false) => Self::Ctrl,
            (false, false, true) => Self::Alt,
            _ => Self::Normal,
        }
    }

    fn current(ctx: &egui::Context) -> Self {
        ctx.input(|i| Self::from_modifiers(i.modifiers))
    }

    fn key_names(self) -> &'static [&'static str; 12] {
        match self {
            Self::Normal => &[
                "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
            ],
            Self::Shift => &[
                "SF1", "SF2", "SF3", "SF4", "SF5", "SF6", "SF7", "SF8", "SF9", "SF10", "SF11",
                "SF12",
            ],
            Self::Ctrl => &[
                "CF1", "CF2", "CF3", "CF4", "CF5", "CF6", "CF7", "CF8", "CF9", "CF10", "CF11",
                "CF12",
            ],
            Self::Alt => &[
                "AF1", "AF2", "AF3", "AF4", "AF5", "AF6", "AF7", "AF8", "AF9", "AF10", "AF11",
                "AF12",
            ],
        }
    }
}

/// F キー案内バーのボタン間隔・ブロック間隔・ボタンの最小幅。
const FKEY_KEY_GAP: f32 = 4.0;
const FKEY_BLOCK_GAP: f32 = 18.0;
const FKEY_BTN_MIN_W: f32 = 56.0;

/// 幅 `avail_w` に並べる F キーのブロック数（2 = F1〜F10、3 = F1〜F12）とボタン幅。
/// 12 個が最小幅以上で収まるなら F12 まで並べて幅いっぱいに広げ、収まらなければ F10 まで。
fn fkey_bar_layout(avail_w: f32) -> (usize, f32) {
    // 12 個: ブロック間 2 か所、ブロック内のすき間 4 + 4 + 1 = 9 か所
    let w12 = (avail_w - FKEY_BLOCK_GAP * 2.0 - FKEY_KEY_GAP * 9.0) / 12.0;
    if w12 >= FKEY_BTN_MIN_W {
        return (3, w12);
    }
    // 10 個: ブロック間 1 か所、ブロック内のすき間 4 + 4 = 8 か所
    let w10 = (avail_w - FKEY_BLOCK_GAP - FKEY_KEY_GAP * 8.0) / 10.0;
    (2, w10.max(FKEY_BTN_MIN_W))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fkey_bar_layout_switches_to_f12_when_wide_enough() {
        // F12 まで並ぶ最小の幅: 56 * 12 + 18 * 2 + 4 * 9 = 744
        let threshold = FKEY_BTN_MIN_W * 12.0 + FKEY_BLOCK_GAP * 2.0 + FKEY_KEY_GAP * 9.0;
        assert_eq!(fkey_bar_layout(threshold), (3, FKEY_BTN_MIN_W));
        let (blocks, w) = fkey_bar_layout(threshold - 1.0);
        assert_eq!(blocks, 2);
        assert!(w > FKEY_BTN_MIN_W);
        // 広いほどボタンも広がる
        let (blocks, w) = fkey_bar_layout(1200.0);
        assert_eq!(blocks, 3);
        assert!((w * 12.0 + FKEY_BLOCK_GAP * 2.0 + FKEY_KEY_GAP * 9.0 - 1200.0).abs() < 0.01);
        // 狭いときは F10 まで、最小幅で
        assert_eq!(fkey_bar_layout(300.0), (2, FKEY_BTN_MIN_W));
    }

    #[test]
    fn test_editor_fkeys_layers() {
        use EditorAction::*;
        let assigned = |keys: &[(&str, EditorAction); 12]| -> Vec<(usize, EditorAction)> {
            keys.iter()
                .enumerate()
                .filter(|(_, (_, a))| *a != Nop)
                .map(|(i, (_, a))| (i, *a))
                .collect()
        };
        let sakura = |layer| VzEditorApp::editor_fkeys(true, false, layer);
        assert_eq!(
            assigned(&sakura(FKeyLayer::Shift)),
            vec![(2, FindPrev), (7, Copy), (9, DuplicateLine)]
        );
        assert_eq!(
            assigned(&sakura(FKeyLayer::Ctrl)),
            vec![(2, ToggleSearchMarks), (3, CloseBuffer)]
        );
        assert_eq!(assigned(&sakura(FKeyLayer::Alt)), vec![(3, Quit)]);
        // VZ 互換は Shift を区別せず、Ctrl+F4 は閉じる、Alt+F4 は終了
        for md in [false, true] {
            let vz = |layer| VzEditorApp::editor_fkeys(false, md, layer);
            let normal = vz(FKeyLayer::Normal);
            assert_eq!(vz(FKeyLayer::Shift), normal);
            assert_eq!(vz(FKeyLayer::Ctrl)[3].1, CloseBuffer);
            assert_eq!(vz(FKeyLayer::Ctrl)[4], normal[4]);
            assert_eq!(vz(FKeyLayer::Alt)[3].1, Quit);
        }
        let filer_alt = VzEditorApp::filer_fkeys(FKeyLayer::Alt);
        assert_eq!(filer_alt[3].1, FilerAction::Quit);
    }

    #[test]
    fn test_fkey_layer_from_modifiers() {
        let layer = |shift, ctrl, alt| {
            FKeyLayer::from_modifiers(egui::Modifiers {
                shift,
                ctrl,
                alt,
                ..Default::default()
            })
        };
        assert_eq!(layer(false, false, false), FKeyLayer::Normal);
        assert_eq!(layer(true, false, false), FKeyLayer::Shift);
        assert_eq!(layer(false, true, false), FKeyLayer::Ctrl);
        assert_eq!(layer(false, false, true), FKeyLayer::Alt);
        // Ctrl+Shift などの組み合わせは通常の段
        assert_eq!(layer(true, true, false), FKeyLayer::Normal);
    }
}
