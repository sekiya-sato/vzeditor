//! サクラ互換のキー処理と、それが使う編集操作（選択・切り取り・貼り付けなど）。

use super::function_keys::FKeyLayer;
use super::*;

impl VzEditorApp {
    pub(super) fn toggle_key_scheme(&mut self) {
        self.key_scheme = match self.key_scheme {
            KeyScheme::Sakura => KeyScheme::Vz,
            KeyScheme::Vz => KeyScheme::Sakura,
        };
        // VZ 互換には範囲選択が無いので解除する
        for slot in &mut self.buffers {
            slot.buffer.clear_selection();
        }
        self.select_mode = false;
        self.prefix = PrefixKey::None;
        self.status_message = format!(
            "キー割り当てを {} にしました（ツール→キー割り当てで切替）",
            self.key_scheme.label()
        );
        self.save_settings();
    }

    pub(super) fn is_sakura(&self) -> bool {
        self.key_scheme == KeyScheme::Sakura
    }

    /// フレームの最後にクリップボードへ書き出す（内部クリップボードにも保持）。
    pub(super) fn copy_to_clipboard(&mut self, text: String) {
        self.internal_clipboard = text.clone();
        self.clipboard_out = Some(text);
    }

    pub(super) fn view_mode_guard(&mut self) -> bool {
        if self.is_markdown_view() {
            self.status_message =
                "ビューモード中は編集できません (Ctrl+Shift+V で編集モードへ)".to_string();
            return true;
        }
        false
    }

    /// 選択範囲、選択が無ければ現在行（改行を含む）を返す。
    pub(super) fn selection_or_line_range(&self) -> (TextPos, TextPos) {
        let buf = self.cur_buf();
        if let Some(range) = buf.selection_range() {
            return range;
        }
        buf.line_range(buf.cursor_line)
    }

    /// サクラの切り取り（Ctrl+X / Shift+Del / F7）。選択が無ければ現在行を切り取る。
    pub(super) fn sakura_cut(&mut self) {
        if self.view_mode_guard() {
            return;
        }
        let had_selection = self.cur_buf().has_selection();
        let (start, end) = self.selection_or_line_range();
        if let Some(text) = self.cur_buf_mut().delete_range(start, end) {
            self.copy_to_clipboard(text);
            self.status_message = if had_selection {
                "選択範囲を切り取りました".to_string()
            } else {
                "現在行を切り取りました".to_string()
            };
        }
        self.select_mode = false;
        self.need_scroll_to_cursor = true;
    }

    /// サクラのコピー（Ctrl+C / Ctrl+Ins / F8）。選択が無ければ現在行、ビューモードでは全文。
    pub(super) fn sakura_copy(&mut self) {
        let (text, msg) = if self.is_markdown_view() {
            (
                self.cur_buf().text_lines().join(self.cur_buf().line_ending),
                "ドキュメント全体をコピーしました",
            )
        } else if let Some(text) = self.cur_buf().selected_text() {
            (text, "選択範囲をコピーしました")
        } else {
            let (start, end) = self.selection_or_line_range();
            (
                self.cur_buf().text_in_range(start, end),
                "現在行をコピーしました",
            )
        };
        self.copy_to_clipboard(text);
        self.select_mode = false;
        self.status_message = msg.to_string();
    }

    /// 文字入力・IME 確定。上書きモードならカーソル位置の文字を置き換える（貼り付けは常に挿入）。
    pub(super) fn type_text(&mut self, text: &str) {
        if self.overwrite_mode {
            self.cur_buf_mut().overwrite_text(text);
        } else {
            self.cur_buf_mut().insert_text(text);
        }
    }

    /// Ins: 挿入 / 上書きを切り替える（両方式共通）。
    pub(super) fn toggle_overwrite_mode(&mut self) {
        self.overwrite_mode = !self.overwrite_mode;
        self.status_message = if self.overwrite_mode {
            "上書きモード (Ins で挿入に戻す)".to_string()
        } else {
            "挿入モード".to_string()
        };
    }

    /// 貼り付け（選択中は置き換え）。
    pub(super) fn sakura_paste_text(&mut self, text: &str) {
        if self.view_mode_guard() || text.is_empty() {
            return;
        }
        self.cur_buf_mut().insert_text(text);
        self.select_mode = false;
        self.need_scroll_to_cursor = true;
    }

    /// 行の複製（Shift+F10 / Ctrl+I）。
    pub(super) fn sakura_duplicate_line(&mut self) {
        if !self.view_mode_guard() {
            self.cur_buf_mut().duplicate_line();
            self.need_scroll_to_cursor = true;
        }
    }

    /// 貼り付ける文字列（システムのクリップボード、取れなければ内部クリップボード）。
    pub(super) fn clipboard_text(&self) -> String {
        clipboard::read_text().unwrap_or_else(|| self.internal_clipboard.clone())
    }

    /// F9 の貼り付け（システムのクリップボード、無ければ内部クリップボード）。
    pub(super) fn sakura_paste_from_clipboard(&mut self) {
        let text = self.clipboard_text();
        if text.is_empty() {
            self.status_message = "クリップボードが空です".to_string();
        } else {
            self.sakura_paste_text(&text);
        }
    }

    /// 範囲を切り取る（delete_only なら削除のみ）。
    pub(super) fn sakura_cut_range(
        &mut self,
        start: TextPos,
        end: TextPos,
        delete_only: bool,
        what: &str,
    ) {
        if self.view_mode_guard() {
            return;
        }
        if let Some(text) = self.cur_buf_mut().delete_range(start, end) {
            if delete_only {
                self.status_message = format!("{what}を削除しました");
            } else {
                self.copy_to_clipboard(text);
                self.status_message = format!("{what}を切り取りました");
            }
        }
        self.select_mode = false;
        self.need_scroll_to_cursor = true;
    }

    /// 単語単位で移動した先の位置（カーソルは動かさない）。
    pub(super) fn word_boundary_pos(&mut self, right: bool) -> TextPos {
        let buf = self.cur_buf_mut();
        let cur = buf.cursor_pos();
        if right {
            buf.move_word_right();
        } else {
            buf.move_word_left();
        }
        let pos = buf.cursor_pos();
        buf.set_cursor(cur.0, cur.1);
        pos
    }

    pub(super) fn toggle_select_mode(&mut self) {
        if self.view_mode_guard() {
            return;
        }
        self.select_mode = !self.select_mode;
        if self.select_mode {
            let buf = self.cur_buf_mut();
            let pos = buf.cursor_pos();
            buf.anchor = Some(pos);
            self.status_message = "範囲選択開始（移動キーで選択、Esc で解除）".to_string();
        } else {
            self.status_message = "範囲選択モードを終了しました".to_string();
        }
    }

    /// Markdown ビューの 1 ページ分のスクロール量。
    pub(super) fn md_page_step(&self) -> f32 {
        (self.cur_slot().markdown.viewport_h * 0.85).max(self.line_height * 4.0)
    }

    /// Markdown ビューでのスクロールキー（両方のキー割り当て共通。Ctrl+P はサクラ互換のみ）。
    pub(super) fn md_view_key(&mut self, key: Key, ctrl: bool) -> bool {
        let page = self.md_page_step();
        match (key, ctrl) {
            (Key::ArrowUp, _) => self.scroll_markdown_by(-self.line_height * 2.5),
            (Key::ArrowDown, _) => self.scroll_markdown_by(self.line_height * 2.5),
            (Key::PageUp, _) => self.scroll_markdown_by(-page),
            (Key::PageDown, _) | (Key::Space, false) => self.scroll_markdown_by(page),
            (Key::Home, _) => self.scroll_markdown_to_top(),
            (Key::End, _) => self.scroll_markdown_to_bottom(),
            (Key::P, true) => self.toggle_markdown_view(),
            _ => return ctrl,
        }
        true
    }

    /// サクラエディタ互換のキー処理（エディター画面）。処理したら true。
    /// Ctrl+X / C / V、Shift+Del、Ctrl+Ins、Shift+Ins は egui が Cut / Copy / Paste イベントに
    /// 変換するため、ここではなく handle_editor_events で扱う。
    pub(super) fn handle_sakura_key(
        &mut self,
        key: Key,
        modifiers: egui::Modifiers,
        in_md_view: bool,
    ) -> bool {
        let ctrl = modifiers.ctrl || modifiers.command;
        let shift = modifiers.shift;
        if modifiers.alt {
            // Alt 系は透過・最前面・送信（handle_events で処理済み）。サクラの Alt+P などは未対応
            return false;
        }

        if ctrl && !shift {
            let dialog = match key {
                Key::F => Some(SearchDialog::Find),
                Key::R => Some(SearchDialog::Replace),
                Key::G => Some(SearchDialog::Grep),
                Key::J => Some(SearchDialog::Jump),
                _ => None,
            };
            if let Some(dialog) = dialog {
                self.open_search_dialog(dialog);
                return true;
            }
        }
        // Ctrl+Shift+F1 は表示欄の修飾キーの段とは別に扱う。
        if key == Key::F1 && ctrl && shift {
            self.show_help = false;
            self.show_about = true;
            return true;
        }
        if !(ctrl && shift) {
            let layer = FKeyLayer::from_modifiers(modifiers);
            if let Some(action) = Self::editor_fkey_action(key, true, in_md_view, layer) {
                self.execute_editor_action(action);
                return true;
            }
        }
        if matches!(
            key,
            Key::F2
                | Key::F3
                | Key::F5
                | Key::F6
                | Key::F7
                | Key::F9
                | Key::F10
                | Key::F11
                | Key::F12
        ) {
            self.status_message = format!("{key:?} はサクラ互換でまだ対応していません");
            return true;
        }

        if key == Key::Escape {
            if self.cancel_huge_task() {
                // 巨大ファイルの検索・置換の実行中は中止だけ
            } else if self.cur_buf().anchor.is_some() || self.select_mode {
                self.cur_buf_mut().clear_selection();
                self.select_mode = false;
                self.status_message = "範囲選択を解除しました".to_string();
            } else if self.show_help {
                self.show_help = false;
            } else {
                self.enter_filer();
            }
            return true;
        }

        if self.handle_font_zoom_key(key, modifiers) {
            return true;
        }

        // バッファ切替は Markdown ビューでも使えるよう、ビューのキー処理より先に扱う
        if ctrl && key == Key::Tab {
            self.switch_buffer_step(if shift { -1 } else { 1 });
            return true;
        }

        if in_md_view {
            return self.md_view_key(key, ctrl);
        }

        // --- 移動（Shift または F6 の選択モードで範囲選択） ---
        let page = self.last_visible_lines;
        let is_move = matches!(
            key,
            Key::ArrowLeft
                | Key::ArrowRight
                | Key::ArrowUp
                | Key::ArrowDown
                | Key::Home
                | Key::End
                | Key::PageUp
                | Key::PageDown
        );
        if is_move {
            // Ctrl+↑ / ↓（Shift なし）は1行スクロール
            if ctrl && !shift && matches!(key, Key::ArrowUp | Key::ArrowDown) {
                let dy = if key == Key::ArrowUp {
                    -self.line_height
                } else {
                    self.line_height
                };
                let slot = self.cur_slot_mut();
                slot.scroll_y = (slot.scroll_y + f64::from(dy)).max(0.0);
                return true;
            }
            let extend = shift || self.select_mode;
            let buf = self.cur_buf_mut();
            if extend {
                buf.begin_selection_if_none();
            } else {
                buf.clear_selection();
            }
            // 折り返し表示では ↑↓ は表示行単位（Ctrl 付きは2行）
            let row_delta = match key {
                Key::ArrowUp => -1,
                Key::ArrowDown => 1,
                _ => 0,
            } * if ctrl { 2 } else { 1 };
            if row_delta != 0 && self.move_cursor_row(row_delta) {
                self.need_scroll_to_cursor = true;
                return true;
            }
            let buf = self.cur_buf_mut();
            match (key, ctrl) {
                (Key::ArrowLeft, false) => buf.move_left(),
                (Key::ArrowLeft, true) => buf.move_word_left(),
                (Key::ArrowRight, false) => buf.move_right(),
                (Key::ArrowRight, true) => buf.move_word_right(),
                (Key::ArrowUp, false) => buf.move_up(),
                (Key::ArrowUp, true) => {
                    buf.move_up();
                    buf.move_up();
                }
                (Key::ArrowDown, false) => buf.move_down(),
                (Key::ArrowDown, true) => {
                    buf.move_down();
                    buf.move_down();
                }
                (Key::Home, false) => buf.move_line_start(),
                (Key::Home, true) => buf.move_doc_start(),
                (Key::End, false) => buf.move_line_end(),
                (Key::End, true) => buf.move_doc_end(),
                (Key::PageUp, false) => buf.move_page_up(page),
                (Key::PageUp, true) => buf.move_page_up((page / 2).max(1)),
                (Key::PageDown, false) => buf.move_page_down(page),
                (Key::PageDown, true) => buf.move_page_down((page / 2).max(1)),
                _ => {}
            }
            self.need_scroll_to_cursor = true;
            return true;
        }

        if ctrl {
            let cur = self.cur_buf().cursor_pos();
            match key {
                Key::N => self.new_empty_buffer(),
                Key::O => self.trigger_open(),
                Key::S if shift => self.trigger_save_as(),
                Key::S => self.trigger_save(),
                Key::Z => self.menu_undo(),
                Key::Y => self.menu_redo(),
                Key::A => {
                    self.cur_buf_mut().select_all();
                    self.status_message = "すべて選択しました".to_string();
                }
                Key::W => self.cur_buf_mut().select_word_at_cursor(),
                Key::P => self.toggle_markdown_view(),
                Key::E if shift => {
                    if !self.view_mode_guard() {
                        self.cur_buf_mut().delete_current_line();
                        self.need_scroll_to_cursor = true;
                        self.status_message = "行を削除しました".to_string();
                    }
                }
                Key::E => {
                    let (start, end) = self.cur_buf().line_range(cur.0);
                    self.sakura_cut_range(start, end, false, "行");
                }
                Key::K => {
                    let buf = self.cur_buf();
                    let len = buf.line_char_len(cur.0);
                    // 行末にいるときは改行を対象にする
                    let end = if cur.1 >= len && cur.0 + 1 < buf.line_count() {
                        (cur.0 + 1, 0)
                    } else {
                        (cur.0, len)
                    };
                    self.sakura_cut_range(cur, end, shift, "行末まで");
                }
                Key::U => self.sakura_cut_range((cur.0, 0), cur, shift, "行頭まで"),
                Key::D => {
                    let (s, e) = self.cur_buf().word_range_at(cur);
                    self.sakura_cut_range((cur.0, s), (cur.0, e), shift, "単語");
                }
                Key::I => self.sakura_duplicate_line(),
                Key::Backspace => {
                    let start = self.word_boundary_pos(false);
                    self.sakura_cut_range(start, cur, true, "単語の左側");
                }
                Key::Delete => {
                    let end = self.word_boundary_pos(true);
                    self.sakura_cut_range(cur, end, true, "単語の右側");
                }
                _ => {}
            }
            // Ctrl の組み合わせは文字入力に流さない
            return true;
        }

        // --- 修飾キーなしの編集キー ---
        match key {
            // Enter は自動インデント、Shift+Enter は行頭から
            Key::Enter if shift => {
                self.cur_buf_mut().insert_newline();
            }
            Key::Enter => {
                self.cur_buf_mut().insert_newline_with_indent();
            }
            Key::Backspace => {
                self.cur_buf_mut().backspace();
            }
            Key::Delete => {
                self.cur_buf_mut().delete_char_at_cursor();
            }
            Key::Tab if !shift => {
                self.cur_buf_mut().insert_text("\t");
            }
            Key::Insert => {
                self.toggle_overwrite_mode();
                return true;
            }
            _ => return false,
        }
        self.select_mode = false;
        self.need_scroll_to_cursor = true;
        true
    }
}
