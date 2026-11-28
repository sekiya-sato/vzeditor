//! ステータスバー、バッファ一覧、ファンクションキー表示。

use super::function_keys::FKeyLayer;
use super::*;

impl VzEditorApp {
    pub(super) fn render_status_bar(&mut self, ctx: &egui::Context) {
        let mut switch_next_clicked = false;
        let mut toggle_md_clicked = false;
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
                        status_badge(ui, "VZ", STATUS_BADGE_BG);

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
                        let in_view = self.cur_slot().markdown.visible;
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
                                    "クリックまたは Alt+P（サクラ互換は Ctrl+Shift+V も、VZ 互換は Ctrl+K+V も）で Markdown 編集/ビュー切替",
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
                            let right_reserve = 260.0_f32;
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
                            let mut pos_text = if self.cur_slot().markdown.visible {
                                format!("全{}行 [VIEW]", buf.line_count())
                            } else {
                                format!(
                                    "Ln {:>4}/{:<4}  Col {:>3}",
                                    buf.display_line_num(),
                                    buf.line_count(),
                                    buf.display_col_num()
                                )
                            };
                            // 巨大ファイル: [巨大] と索引の作成中・保存中の進み具合
                            if let Some(state) = self.huge_status() {
                                pos_text = format!("{state}  {pos_text}");
                            }
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
                            let enc_resp = ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new(format!("{} {}", buf.encoding, eol))
                                            .font(FontId::monospace(13.0))
                                            .color(TEXT_WHITE),
                                    )
                                    .sense(Sense::click()),
                                )
                                .on_hover_text("クリックで文字コードを指定して保存");
                            if enc_resp.clicked() {
                                win_cmd = Some(MenuAction::SaveWithEncoding);
                            }

                            if !self.cur_slot().markdown.visible
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
                        status_badge(ui, "FILER", STATUS_BADGE_FILER_BG);

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
                            let right_reserve = 130.0_f32;
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
                        });
                    }
                    AppMode::Terminal | AppMode::Browser => {
                        let (badge, badge_bg) = if self.mode == AppMode::Terminal {
                            ("TERM", STATUS_BADGE_VIEW_BG)
                        } else {
                            ("WEB", STATUS_BADGE_FILER_BG)
                        };
                        status_badge(ui, badge, badge_bg);
                        let avail_msg_w = ui.available_width() - 10.0;
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
    }

    /// バッファ帯（ファイルが 2 件以上、またはターミナル・ブラウザを開いたとき）。
    /// ファイルのタブの後ろにターミナル（1 つ）・ブラウザ（タブごと）のタブを並べる。
    pub(super) fn render_buffer_strip(
        &mut self,
        ctx: &egui::Context,
        frame: Option<&eframe::Frame>,
    ) {
        let has_terminal = self.terminal.is_some();
        let has_browser = !self.browsers.is_empty();
        if self.mode == AppMode::Filer
            || (self.buffers.len() == 1
                && self.buffers[0].is_untouched_untitled()
                && !has_terminal
                && !has_browser)
        {
            return;
        }
        let in_editor = self.mode == AppMode::Editor;
        let mut selected_tab = None;
        let mut tab_move = None;
        let mut terminal_clicked = false;
        let mut browser_clicked: Option<usize> = None;
        let can_use_tab_gestures = self.filer_modal.is_none()
            && self.inline_prompt.is_none()
            && self.pending_dialog.is_none()
            && self.open_menu.is_none()
            && !self.show_help
            && !self.show_about;
        let file_tab_sense = if can_use_tab_gestures {
            Sense::click_and_drag()
        } else {
            Sense::click()
        };
        let primary = egui::PointerButton::Primary;
        let released = ctx.input(|i| i.pointer.button_released(primary));

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

                    let mut hovered_file_tab = None;
                    for (idx, slot) in self.buffers.iter().enumerate() {
                        let dirty_mark = if slot.buffer.dirty { "*" } else { "" };
                        let view_mark = if slot.markdown.visible { "[V]" } else { "" };
                        let name = slot.buffer.display_name();
                        let title = format!("{}:{name}{dirty_mark}{view_mark}", idx + 1);
                        let response = ui
                            .push_id(slot.tab_id, |ui| {
                                strip_tab(
                                    ui,
                                    title,
                                    in_editor && idx == self.active_buf,
                                    file_tab_sense,
                                )
                            })
                            .inner;
                        if response.clicked() {
                            let copy_name = can_use_tab_gestures
                                && response.double_clicked_by(primary)
                                && slot.buffer.file_path.is_some();
                            selected_tab = Some((idx, copy_name.then_some(name)));
                        }
                        if can_use_tab_gestures {
                            if response.drag_started_by(primary) {
                                egui::DragAndDrop::set_payload(ctx, FileTabDrag(slot.tab_id));
                            }
                            if response.contains_pointer() {
                                hovered_file_tab = Some((idx, response.rect));
                            }
                        }
                    }
                    if let Some((to, rect)) = hovered_file_tab {
                        let source = egui::DragAndDrop::payload::<FileTabDrag>(ctx)
                            .and_then(|drag| self.buffers.iter().position(|s| s.tab_id == drag.0));
                        if let Some(from) = source.filter(|&from| from != to) {
                            let x = if from < to { rect.right() } else { rect.left() };
                            ui.painter()
                                .vline(x, rect.y_range(), Stroke::new(2.0_f32, TEXT_CYAN));
                            if released {
                                tab_move = Some((from, to));
                            }
                        }
                    }

                    if has_terminal || has_browser {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new("|")
                                .font(FontId::monospace(12.0))
                                .color(GUTTER_SEP),
                        );
                        ui.add_space(2.0);
                    }
                    if let Some(term) = &self.terminal {
                        // シェルが終了したターミナルは poll_terminal が閉じるので、ここに来るのは動作中だけ
                        let resp = strip_tab(
                            ui,
                            "ターミナル".to_string(),
                            self.mode == AppMode::Terminal,
                            Sense::click(),
                        );
                        if resp
                            .on_hover_text(format!("Alt+K  {}", term.program()))
                            .clicked()
                        {
                            terminal_clicked = true;
                        }
                    }
                    let many = self.browsers.len() > 1;
                    for (i, tab) in self.browsers.iter().enumerate() {
                        // 複数あるときは番号を付ける。タイトルが無ければ URL
                        let head = if many {
                            format!("ブラウザ{}", i + 1)
                        } else {
                            "ブラウザ".to_string()
                        };
                        let label = tab.label();
                        let name = if label.is_empty() {
                            head
                        } else {
                            format!("{head}:{}", truncate_display_right(label, 24))
                        };
                        let active = self.mode == AppMode::Browser && i == self.active_browser;
                        let resp = strip_tab(ui, name, active, Sense::click());
                        if resp
                            .on_hover_text("Alt+B / Ctrl+Tab で切替、Ctrl+W で閉じる")
                            .clicked()
                        {
                            browser_clicked = Some(i);
                        }
                    }
                });
            });

        if can_use_tab_gestures {
            if let (Some(frame), Some(drag)) =
                (frame, egui::DragAndDrop::payload::<FileTabDrag>(ctx))
            {
                let outside = ctx.input(|input| {
                    input
                        .pointer
                        .latest_pos()
                        .is_none_or(|pos| !input.screen_rect().contains(pos))
                });
                let native_down = (!released && outside)
                    .then(|| vz_platform::tab_drop::primary_down(frame))
                    .flatten();
                if released || native_down == Some(false) {
                    egui::DragAndDrop::clear_payload(ctx);
                    let target = vz_platform::tab_drop::target_at_pointer(frame);
                    if matches!(target, vz_platform::tab_drop::DropTarget::Other(_)) {
                        self.send_tab(drag.0, target);
                        ctx.request_repaint();
                        return;
                    }
                } else {
                    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                    if native_down == Some(true) {
                        ctx.request_repaint_after(std::time::Duration::from_millis(30));
                    }
                }
            }
        }
        if let Some((from, to)) = tab_move {
            self.move_buffer_tab(from, to);
            ctx.request_repaint();
        } else if let Some((idx, copied_name)) = selected_tab {
            if self.mode != AppMode::Editor {
                self.enter_editor();
            }
            self.switch_buffer_to(idx);
            if let Some(name) = copied_name {
                self.status_message = format!("ファイル名をコピーしました: {name}");
                self.copy_to_clipboard(name);
            }
        }
        if terminal_clicked && self.mode != AppMode::Terminal {
            self.toggle_terminal(ctx);
        }
        if let Some(i) = browser_clicked {
            self.show_browser_tab(i);
        }
    }

    fn move_buffer_tab(&mut self, from: usize, to: usize) {
        let slot = self.buffers.remove(from);
        self.buffers.insert(to, slot);
        self.active_buf = match self.active_buf {
            active if active == from => to,
            active if from < active && active <= to => active - 1,
            active if to <= active && active < from => active + 1,
            active => active,
        };
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
}

/// ステータスバー左端の画面名のバッジ（VZ・FILER・TERM・WEB）。
fn status_badge(ui: &mut egui::Ui, text: &str, fill: Color32) {
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(CornerRadius::same(2))
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .font(FontId::monospace(13.0))
                    .strong()
                    .color(STATUS_BADGE_FG),
            );
        });
}

/// バッファ帯のタブ 1 つ。
fn strip_tab(ui: &mut egui::Ui, title: String, active: bool, sense: Sense) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(title)
                .font(FontId::monospace(12.5))
                .color(if active { TEXT_WHITE } else { TEXT_DIM_CYAN }),
        )
        .sense(sense)
        .fill(if active {
            BG_TAB_ACTIVE
        } else {
            BG_TAB_INACTIVE
        })
        .stroke(Stroke::new(
            1.0_f32,
            if active { TEXT_CYAN } else { GUTTER_SEP },
        ))
        .corner_radius(CornerRadius::same(2)),
    )
}

struct FileTabDrag(usize);

impl FKeyLayer {
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
            vec![(2, RepeatSearch(true)), (7, Copy), (9, DuplicateLine)]
        );
        assert_eq!(
            assigned(&sakura(FKeyLayer::Ctrl)),
            vec![(2, ToggleSearchHighlight), (3, CloseBuffer)]
        );
        assert_eq!(assigned(&sakura(FKeyLayer::Alt)), vec![(3, Quit)]);
        for md in [false, true] {
            let vz = |layer| VzEditorApp::editor_fkeys(false, md, layer);
            let normal = vz(FKeyLayer::Normal);
            assert_eq!(normal[2].1, RepeatSearch(false));
            assert_eq!(normal[3].1, ToggleSplit);
            assert_eq!(vz(FKeyLayer::Shift)[2].1, RepeatSearch(true));
            assert_eq!(vz(FKeyLayer::Shift)[3].1, Nop);
            assert_eq!(vz(FKeyLayer::Ctrl)[2].1, ToggleSearchHighlight);
            assert_eq!(vz(FKeyLayer::Ctrl)[3].1, CloseBuffer);
            assert_eq!(vz(FKeyLayer::Ctrl)[4], normal[4]);
            assert_eq!(vz(FKeyLayer::Alt)[3].1, Quit);
        }
        assert_eq!(sakura(FKeyLayer::Normal)[2].1, RepeatSearch(false));
        assert_eq!(sakura(FKeyLayer::Normal)[3].1, ToggleSplit);
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
