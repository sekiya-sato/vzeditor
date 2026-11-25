//! 組み込みターミナル（Alt+K）: 画面の切替、キー入力の送信、画面の描画。
//! シェルの起動と VT の解釈は vz-terminal。

use super::*;
use vz_terminal::keys::{encode_char, encode_key, Mods, TermKey};
use vz_terminal::{color, shell, Terminal};

/// マウスホイール 1 目盛りでスクロールする行数
const WHEEL_LINES: f32 = 3.0;

impl VzEditorApp {
    /// ターミナル・ブラウザ以外の画面から来たときだけ、戻り先として覚える。
    pub(super) fn remember_return_mode(&mut self) {
        if matches!(self.mode, AppMode::Editor | AppMode::Filer) {
            self.return_mode = self.mode;
        }
    }

    /// ターミナル・ブラウザから元の画面（エディター / ファイラー）へ戻る。
    pub(super) fn leave_tool_mode(&mut self) {
        self.ime_preedit.clear();
        self.mode = self.return_mode;
        if self.mode == AppMode::Editor {
            self.need_scroll_to_cursor = true;
        }
        self.status_message.clear();
    }

    /// 新しく開くターミナルの作業フォルダ: ファイラーの窓 → 編集中のファイルのフォルダ → ファイラーの窓。
    fn terminal_work_dir(&self) -> PathBuf {
        if self.mode != AppMode::Filer {
            if let Some(dir) = self.cur_buf().file_path.as_deref().and_then(Path::parent) {
                if dir.is_dir() {
                    return dir.to_path_buf();
                }
            }
        }
        self.filer.active_pane().current_dir.clone()
    }

    /// Alt+K: ターミナルを表示する（起動していなければ起動）。表示中なら元の画面へ戻る。
    pub(super) fn toggle_terminal(&mut self, ctx: &egui::Context) {
        if self.mode == AppMode::Terminal {
            self.leave_tool_mode();
            return;
        }
        let cwd = self.terminal_work_dir();
        self.remember_return_mode();
        self.show_help = false;
        self.prefix = PrefixKey::None;
        self.ime_preedit.clear();
        if !self.terminal.as_ref().is_some_and(Terminal::is_running)
            && !self.start_terminal(ctx, &cwd)
        {
            return;
        }
        self.mode = AppMode::Terminal;
        if let Some(term) = &self.terminal {
            self.status_message = format!(
                "ターミナル: {}  （Alt+K で戻る / Ctrl+Shift+C で画面をコピー / ホイールで履歴）",
                term.program()
            );
        }
    }

    /// シェルを起動する。失敗したらステータスバーに出して false。
    fn start_terminal(&mut self, ctx: &egui::Context, cwd: &Path) -> bool {
        let shell = shell::default_shell();
        let (rows, cols) = self
            .terminal
            .as_ref()
            .map_or((24, 80), |t| t.screen().size());
        let waker = ctx.clone();
        match Terminal::spawn(&shell, cwd, rows, cols, move || waker.request_repaint()) {
            Ok(term) => {
                self.terminal = Some(term);
                true
            }
            Err(e) => {
                self.status_message = format!("ターミナルの起動エラー: {e}");
                false
            }
        }
    }

    /// 復元したターミナルを起動する（`--restore` の後の最初のフレーム）。
    pub(super) fn start_pending_terminal(&mut self, ctx: &egui::Context) {
        let Some(cwd) = self.pending_terminal_cwd.take() else {
            return;
        };
        if self.terminal.is_none()
            && self.start_terminal(ctx, &cwd)
            && self.mode == AppMode::Terminal
        {
            self.status_message = format!("ターミナルを起動し直しました: {}", cwd.display());
        }
    }

    /// 届いた出力を画面モデルへ反映する（表示していなくても毎フレーム）。
    pub(super) fn poll_terminal(&mut self) {
        let Some(term) = self.terminal.as_mut() else {
            return;
        };
        let was_running = term.is_running();
        term.poll();
        if was_running && !term.is_running() {
            self.status_message = format!(
                "シェルが終了しました（終了コード {}）。Enter で再起動 / Alt+K で戻る",
                term.exit_code().unwrap_or(0)
            );
        }
    }

    pub(super) fn handle_terminal_events(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        let modifiers_now = ctx.input(|i| i.modifiers);

        // 終了後は Enter で同じフォルダから起動し直す
        if !self.terminal.as_ref().is_some_and(Terminal::is_running) {
            let enter = events.iter().any(|e| {
                matches!(
                    e,
                    Event::Key {
                        key: Key::Enter,
                        pressed: true,
                        ..
                    }
                )
            });
            if enter {
                let cwd = self.terminal_work_dir();
                if self.start_terminal(ctx, &cwd) {
                    self.status_message = "ターミナルを再起動しました".to_string();
                }
            }
            return;
        }
        let Some(term) = self.terminal.as_mut() else {
            return;
        };
        let app_cursor = term.screen().application_cursor();
        let row_h = self.line_height;

        for event in events {
            match event {
                // Alt 付きの文字は Key 側で ESC + 文字として送る
                Event::Text(text) if !modifiers_now.alt => {
                    if self.ime_preedit.is_empty() {
                        term.send_input(text.as_bytes());
                    }
                }
                Event::Ime(ImeEvent::Preedit(text)) => self.ime_preedit = text,
                Event::Ime(ImeEvent::Commit(text)) => {
                    self.ime_preedit.clear();
                    term.send_input(text.as_bytes());
                }
                Event::Ime(ImeEvent::Enabled) => self.ime_active = true,
                Event::Ime(ImeEvent::Disabled) => {
                    self.ime_active = false;
                    self.ime_preedit.clear();
                }
                Event::Paste(text) => term.paste(&text),
                // egui は Ctrl+C / Ctrl+X（macOS は Cmd）をコピー・切り取りとして届ける
                Event::Copy => {
                    if modifiers_now.shift || cfg!(target_os = "macos") {
                        self.clipboard_out = Some(term.visible_text());
                        self.status_message = "ターミナルの画面をコピーしました".to_string();
                    } else {
                        term.send_input(b"\x03");
                    }
                }
                Event::Cut => {
                    if !cfg!(target_os = "macos") {
                        term.send_input(b"\x18");
                    }
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if !self.ime_preedit.is_empty() {
                        continue;
                    }
                    let mods = Mods {
                        ctrl: modifiers.ctrl,
                        alt: modifiers.alt,
                        shift: modifiers.shift,
                    };
                    if let Some(tk) = term_key(key) {
                        term.send_input(&encode_key(tk, mods, app_cursor));
                    } else if mods.ctrl || mods.alt {
                        if let Some(bytes) = key_char(key).and_then(|c| encode_char(c, mods)) {
                            term.send_input(&bytes);
                        }
                    }
                }
                Event::MouseWheel { unit, delta, .. } => {
                    let lines = match unit {
                        egui::MouseWheelUnit::Point => delta.y / row_h,
                        egui::MouseWheelUnit::Line => delta.y * WHEEL_LINES,
                        egui::MouseWheelUnit::Page => delta.y * f32::from(term.screen().size().0),
                    };
                    term.scroll_by(lines.round() as i32);
                }
                _ => {}
            }
        }
    }

    pub(super) fn render_terminal_canvas(&mut self, ctx: &egui::Context) {
        let font_id = FontId::monospace(self.font_size);
        let row_h = self.line_height;
        let preedit = self.ime_preedit.clone();
        let mut ime_rect: Option<Rect> = None;

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                let avail = ui.available_rect_before_wrap();
                ui.allocate_rect(avail, Sense::click());
                let painter = ui.painter_at(avail);
                window_style::bg_painter(&painter).rect_filled(
                    avail,
                    CornerRadius::ZERO,
                    bg(BG_NAVY),
                );

                let Some(term) = self.terminal.as_mut() else {
                    return;
                };
                let cell_w = ui.fonts(|f| {
                    (f.layout_no_wrap("M".repeat(10), font_id.clone(), TEXT_WHITE)
                        .size()
                        .x
                        / 10.0)
                        .max(1.0)
                });
                let pad = 6.0;
                let cols = ((avail.width() - pad * 2.0) / cell_w).floor().max(10.0) as u16;
                let rows = ((avail.height() - pad * 2.0) / row_h).floor().max(2.0) as u16;
                term.resize(rows, cols);

                let origin = avail.min + Vec2::splat(pad);
                let screen = term.screen();
                let (rows, cols) = screen.size();
                let cell_pos = |row: u16, col: u16| {
                    origin + Vec2::new(f32::from(col) * cell_w, f32::from(row) * row_h)
                };
                // 文字の上端を行の中で上下中央に寄せる
                let text_dy = ((row_h - self.font_size * 1.2) * 0.5).max(0.0);

                for row in 0..rows {
                    let mut col = 0u16;
                    while col < cols {
                        let Some(cell) = screen.cell(row, col) else {
                            break;
                        };
                        if cell.is_wide_continuation() {
                            col += 1;
                            continue;
                        }
                        let style = CellStyle::of(cell);
                        // 同じ見た目の半角文字をまとめて描く（全角は位置がずれないよう 1 文字ずつ）
                        let start = col;
                        let mut text = String::new();
                        let mut width = 0u16;
                        while let Some(c) = screen.cell(row, col) {
                            if col >= cols
                                || c.is_wide_continuation()
                                || CellStyle::of(c) != style
                                || (width > 0 && c.is_wide())
                            {
                                break;
                            }
                            let s = c.contents();
                            text.push_str(if s.is_empty() { " " } else { s });
                            let w = if c.is_wide() { 2 } else { 1 };
                            width += w;
                            col += w;
                            if c.is_wide() {
                                break;
                            }
                        }
                        if width == 0 {
                            col += 1;
                            continue;
                        }
                        let pos = cell_pos(row, start);
                        let run_rect =
                            Rect::from_min_size(pos, Vec2::new(f32::from(width) * cell_w, row_h));
                        if let Some(bg_color) = style.bg {
                            painter.rect_filled(run_rect, CornerRadius::ZERO, bg_color);
                        }
                        if !text.trim().is_empty() {
                            painter.text(
                                pos + Vec2::new(0.0, text_dy),
                                Align2::LEFT_TOP,
                                &text,
                                font_id.clone(),
                                style.fg,
                            );
                        }
                        if style.underline {
                            let y = run_rect.max.y - 2.0;
                            painter.line_segment(
                                [Pos2::new(run_rect.min.x, y), Pos2::new(run_rect.max.x, y)],
                                Stroke::new(1.0_f32, style.fg),
                            );
                        }
                    }
                }

                // カーソルと IME の変換中の文字（履歴を見ている間は出さない）
                if screen.scrollback() == 0 && term.is_running() {
                    let (crow, ccol) = screen.cursor_position();
                    let pos = cell_pos(crow, ccol);
                    if !preedit.is_empty() {
                        let galley = ui.fonts(|f| {
                            f.layout_no_wrap(preedit.clone(), font_id.clone(), PREEDIT_FG)
                        });
                        let rect =
                            Rect::from_min_size(pos, Vec2::new(galley.size().x.max(cell_w), row_h));
                        painter.rect_filled(rect, CornerRadius::ZERO, PREEDIT_BG);
                        painter.galley(pos + Vec2::new(0.0, text_dy), galley, PREEDIT_FG);
                        painter.line_segment(
                            [
                                Pos2::new(rect.min.x, rect.max.y - 1.0),
                                Pos2::new(rect.max.x, rect.max.y - 1.0),
                            ],
                            Stroke::new(1.5_f32, PREEDIT_UNDERLINE),
                        );
                        ime_rect = Some(rect);
                    } else {
                        let wide = screen.cell(crow, ccol).is_some_and(|c| c.is_wide());
                        let w = if wide { cell_w * 2.0 } else { cell_w };
                        let rect = Rect::from_min_size(pos, Vec2::new(w, row_h));
                        if !screen.hide_cursor() {
                            painter.rect_filled(rect, CornerRadius::ZERO, CURSOR_FILL);
                        }
                        ime_rect = Some(rect);
                    }
                } else if screen.scrollback() > 0 {
                    painter.text(
                        Pos2::new(avail.max.x - pad, avail.min.y + pad),
                        Align2::RIGHT_TOP,
                        format!("履歴 {} 行前（キー入力で最新へ）", screen.scrollback()),
                        FontId::monospace(12.5),
                        TEXT_AMBER,
                    );
                }
            });

        // IME の候補ウィンドウをカーソル位置に出す（メニュー・ダイアログ表示中は出さない）
        let overlay = self.open_menu.is_some() || self.filer_modal.is_some() || self.show_about;
        ctx.output_mut(|o| {
            o.ime = match ime_rect {
                Some(rect) if !overlay => Some(egui::output::IMEOutput {
                    rect,
                    cursor_rect: rect,
                }),
                _ => None,
            };
        });
    }
}

/// セル 1 つの見た目（前景・背景・下線）。
#[derive(Debug, Clone, Copy, PartialEq)]
struct CellStyle {
    fg: Color32,
    /// None は既定の背景（パネルの背景をそのまま見せる）
    bg: Option<Color32>,
    underline: bool,
}

impl CellStyle {
    fn of(cell: &vz_terminal::vt100::Cell) -> Self {
        let to_c32 = |(r, g, b): (u8, u8, u8)| Color32::from_rgb(r, g, b);
        let mut fg = color::rgb(cell.fgcolor(), cell.bold()).map_or(TEXT_WHITE, to_c32);
        let mut bg = color::rgb(cell.bgcolor(), false).map(to_c32);
        if cell.inverse() {
            let new_bg = fg;
            fg = bg.unwrap_or(BG_NAVY);
            bg = Some(new_bg);
        }
        if cell.dim() {
            fg = fg.gamma_multiply(0.6);
        }
        Self {
            fg,
            bg,
            underline: cell.underline(),
        }
    }
}

/// 文字以外のキー。
fn term_key(key: Key) -> Option<TermKey> {
    Some(match key {
        Key::Enter => TermKey::Enter,
        Key::Backspace => TermKey::Backspace,
        Key::Tab => TermKey::Tab,
        Key::Escape => TermKey::Escape,
        Key::ArrowUp => TermKey::Up,
        Key::ArrowDown => TermKey::Down,
        Key::ArrowLeft => TermKey::Left,
        Key::ArrowRight => TermKey::Right,
        Key::Home => TermKey::Home,
        Key::End => TermKey::End,
        Key::PageUp => TermKey::PageUp,
        Key::PageDown => TermKey::PageDown,
        Key::Insert => TermKey::Insert,
        Key::Delete => TermKey::Delete,
        Key::F1 => TermKey::F(1),
        Key::F2 => TermKey::F(2),
        Key::F3 => TermKey::F(3),
        Key::F4 => TermKey::F(4),
        Key::F5 => TermKey::F(5),
        Key::F6 => TermKey::F(6),
        Key::F7 => TermKey::F(7),
        Key::F8 => TermKey::F(8),
        Key::F9 => TermKey::F(9),
        Key::F10 => TermKey::F(10),
        Key::F11 => TermKey::F(11),
        Key::F12 => TermKey::F(12),
        _ => return None,
    })
}

/// Ctrl / Alt と組み合わせる文字キー（英字は小文字）。
fn key_char(key: Key) -> Option<char> {
    if key == Key::Space {
        return Some(' ');
    }
    let name = key.symbol_or_name();
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_graphic() => Some(c.to_ascii_lowercase()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_char_maps_letters_and_symbols() {
        assert_eq!(key_char(Key::C), Some('c'));
        assert_eq!(key_char(Key::OpenBracket), Some('['));
        assert_eq!(key_char(Key::Space), Some(' '));
        assert_eq!(key_char(Key::Num2), Some('2'));
        assert_eq!(key_char(Key::ArrowUp), None);
        assert_eq!(key_char(Key::F1), None);
    }

    #[test]
    fn test_term_key_covers_function_keys() {
        assert_eq!(term_key(Key::F12), Some(TermKey::F(12)));
        assert_eq!(term_key(Key::A), None);
    }
}
