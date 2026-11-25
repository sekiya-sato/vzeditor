//! 文字サイズ、透過・最前面、ウィンドウの位置、設定の保存。

use super::*;

impl VzEditorApp {
    pub(super) fn set_font_size(&mut self, new_size: f32) {
        let clamped = new_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE).round();
        self.font_size = clamped;
        self.line_height = calc_line_height(clamped);
        self.need_scroll_to_cursor = true;
        self.filer.left.need_scroll_to_cursor = true;
        self.filer.right.need_scroll_to_cursor = true;
        self.status_message = format!("フォントサイズ: {}pt", clamped as i32);
        self.save_settings();
    }

    // --- Window style (背面透過・最前面固定) ---

    pub(super) fn save_settings(&mut self) {
        if !self.persist_settings {
            return;
        }
        let settings = Settings {
            opacity: self.bg_opacity,
            text_opacity: self.text_opacity,
            always_on_top: self.always_on_top,
            font_size: self.font_size,
            key_scheme: self.key_scheme,
            word_wrap: self.word_wrap,
            window: self.window_geometry,
        };
        if let Err(e) = settings.save() {
            self.status_message = format!("設定の保存に失敗しました: {e}");
        }
    }

    // --- Window geometry (前回のモニター・位置・サイズで起動) ---

    /// 起動直後に前回の位置へ合わせる。保存した位置がどのモニターにも見えなければ主モニターの中央に置く。
    pub(super) fn restore_window_position(&mut self, ctx: &egui::Context) {
        let Some(g) = self.pending_window_restore else {
            return;
        };
        let (native_ppp, outer) = ctx.input(|i| {
            (
                i.viewport().native_pixels_per_point,
                i.viewport().outer_rect,
            )
        });
        // ウィンドウの情報が届くまで待つ
        let (Some(native_ppp), Some(outer)) = (native_ppp, outer) else {
            return;
        };
        self.pending_window_restore = None;
        if window_title_bar_visible(&g) {
            // 作成時は主モニターの拡大率で換算されるので、物理ピクセルで合わせ直す
            let ppp = native_ppp * ctx.zoom_factor();
            let target = egui::pos2(g.x as f32 / ppp, g.y as f32 / ppp);
            if (outer.min - target).length() > 0.5 {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(target));
            }
        } else if let Some(cmd) = egui::ViewportCommand::center_on_screen(ctx) {
            ctx.send_viewport_cmd(cmd);
        }
        if g.maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
    }

    /// 現在のウィンドウの位置とサイズを覚える（最小化・全画面中は前の値を残す）。
    pub(super) fn track_window_geometry(&mut self, ctx: &egui::Context) {
        if self.pending_window_restore.is_some() {
            return;
        }
        let zoom = ctx.zoom_factor();
        let next = ctx.input(|i| next_window_geometry(self.window_geometry, i.viewport(), zoom));
        if next.is_some() {
            self.window_geometry = next;
        }
    }

    pub(super) fn set_bg_opacity(&mut self, percent: u8) {
        self.set_opacity(false, percent);
    }

    pub(super) fn change_bg_opacity(&mut self, delta: i16) {
        self.set_bg_opacity(Self::step_opacity(self.bg_opacity, delta));
    }

    pub(super) fn set_text_opacity(&mut self, percent: u8) {
        self.set_opacity(true, percent);
    }

    pub(super) fn change_text_opacity(&mut self, delta: i16) {
        self.set_text_opacity(Self::step_opacity(self.text_opacity, delta));
    }

    pub(super) fn step_opacity(current: u8, delta: i16) -> u8 {
        (current as i16 + delta).clamp(0, 100) as u8
    }

    /// 背景（text = false）または文字（text = true）の不透明度を設定する。
    pub(super) fn set_opacity(&mut self, text: bool, percent: u8) {
        let p = window_style::clamp_opacity(percent);
        if self.persist_settings {
            if text {
                window_style::set_text_opacity_percent(p);
            } else {
                window_style::set_opacity_percent(p);
            }
        }
        let (current, what) = if text {
            (&mut self.text_opacity, "文字")
        } else {
            (&mut self.bg_opacity, "背景")
        };
        if p == *current {
            self.status_message = format!(
                "{what}の不透明度は {p}% です ({}〜{}%)",
                window_style::MIN_OPACITY,
                window_style::MAX_OPACITY
            );
            return;
        }
        *current = p;
        self.status_message = if text {
            format!("文字の不透明度: {p}% (Alt+Shift++ 濃く / Alt+Shift+- 薄く / Alt+Shift+0 戻す)")
        } else {
            format!("背景の不透明度: {p}% (Alt+↑ 濃く / Alt+↓ 薄く / Alt+0 戻す)")
        };
        self.save_settings();
    }

    // --- Claude Code / Codex への送信 ---

    pub(super) fn toggle_always_on_top(&mut self, ctx: &egui::Context) {
        self.always_on_top = !self.always_on_top;
        let level = if self.always_on_top {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
        self.status_message = if self.always_on_top {
            "最前面に固定しました (Alt+T で解除)".to_string()
        } else {
            "最前面固定を解除しました (Alt+T で固定)".to_string()
        };
        self.save_settings();
    }

    /// ウィンドウスタイルのキーをモードに関係なく処理し、処理したキーは入力から取り除く。
    ///
    /// - Alt+T / Alt+↑ / Alt+↓ / Alt+0: 最前面固定・背景の不透明度
    /// - Alt+Shift+「+」 / Alt+Shift+「-」 / Alt+Shift+0: 文字の不透明度
    ///
    /// 「+」「-」は配列差（US: `=+` / JIS: `;+`、Shift+「-」が `=` になる JIS）を吸収するため
    /// 物理キーを優先して判定する。既存の Ctrl+± / Ctrl+0（フォントサイズ）は Alt 併用時に
    /// 反応しないため重複しない。
    pub(super) fn handle_window_style_keys(&mut self, ctx: &egui::Context) {
        let mut commands: Vec<MenuAction> = Vec::new();
        let in_editor = self.mode == AppMode::Editor && self.ime_preedit.is_empty();
        ctx.input_mut(|i| {
            i.events.retain(|ev| {
                if let Event::Key {
                    key,
                    physical_key,
                    pressed,
                    modifiers,
                    ..
                } = ev
                {
                    if let Some(action) = window_style_key_action(*key, *physical_key, *modifiers) {
                        if !in_editor
                            && matches!(
                                action,
                                MenuAction::OpenSearch(_) | MenuAction::RepeatSearch(_)
                            )
                        {
                            return true;
                        }
                        if *pressed {
                            commands.push(action);
                        }
                        return false;
                    }
                }
                true
            });
            // Option/Alt+キーで生じる文字（macOS の Option+T → † など）を本文に入れない
            if !commands.is_empty() {
                i.events.retain(|ev| !matches!(ev, Event::Text(_)));
            }
        });
        for action in commands {
            self.execute_menu_action(ctx, action);
        }
    }

    pub(super) fn zoom_in_font(&mut self) {
        self.set_font_size(self.font_size + FONT_SIZE_STEP);
    }

    pub(super) fn zoom_out_font(&mut self) {
        self.set_font_size(self.font_size - FONT_SIZE_STEP);
    }

    pub(super) fn reset_font_size(&mut self) {
        self.set_font_size(DEFAULT_FONT_SIZE);
    }

    pub(super) fn handle_font_zoom_key(&mut self, key: Key, modifiers: egui::Modifiers) -> bool {
        let ctrl = modifiers.ctrl || modifiers.command;
        if !ctrl || modifiers.alt {
            return false;
        }
        match key {
            Key::Plus | Key::Equals => {
                self.zoom_in_font();
                true
            }
            // JIS keyboard fallback where Shift+; is '+'
            Key::Semicolon if modifiers.shift => {
                self.zoom_in_font();
                true
            }
            Key::Minus => {
                self.zoom_out_font();
                true
            }
            Key::Num0 => {
                self.reset_font_size();
                true
            }
            _ => false,
        }
    }

    // --- Markdown View Mode ---
}
