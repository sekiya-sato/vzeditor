//! VZ 互換のキー処理と、エディター・ファイラーの操作の実行。

use super::function_keys::FKeyLayer;
use super::*;

impl VzEditorApp {
    pub(super) fn execute_editor_action(&mut self, action: EditorAction) {
        self.prefix = PrefixKey::None;
        let in_md_view = self.is_markdown_view();
        match action {
            EditorAction::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            EditorAction::NewBuffer => {
                self.new_empty_buffer();
            }
            EditorAction::MoveLineStart => {
                if in_md_view {
                    self.scroll_markdown_to_top();
                } else {
                    self.cur_buf_mut().move_line_start();
                    self.need_scroll_to_cursor = true;
                }
            }
            EditorAction::MoveLineEnd => {
                if in_md_view {
                    self.scroll_markdown_to_bottom();
                } else {
                    self.cur_buf_mut().move_line_end();
                    self.need_scroll_to_cursor = true;
                }
            }
            EditorAction::DeleteLine => {
                if in_md_view {
                    self.status_message =
                        "ビューモード中は編集できません (^P / Alt+P で編集モードへ)".to_string();
                } else {
                    self.cur_buf_mut().delete_current_line();
                    self.need_scroll_to_cursor = true;
                    self.status_message = "現在行を削除しました (^Y)".to_string();
                }
            }
            EditorAction::MoveWordLeft => {
                if in_md_view {
                    let step =
                        (self.cur_slot().markdown.viewport_h * 0.5).max(self.line_height * 4.0);
                    self.scroll_markdown_by(-step);
                } else {
                    self.cur_buf_mut().move_word_left();
                    self.need_scroll_to_cursor = true;
                }
            }
            EditorAction::MoveWordRight => {
                if in_md_view {
                    let step =
                        (self.cur_slot().markdown.viewport_h * 0.5).max(self.line_height * 4.0);
                    self.scroll_markdown_by(step);
                } else {
                    self.cur_buf_mut().move_word_right();
                    self.need_scroll_to_cursor = true;
                }
            }
            EditorAction::SwitchToFiler => {
                self.enter_filer();
            }
            EditorAction::SaveFile => {
                self.trigger_save();
            }
            EditorAction::Cut => self.sakura_cut(),
            EditorAction::Copy => self.sakura_copy(),
            EditorAction::Paste => self.sakura_paste_from_clipboard(),
            EditorAction::BeginSelect => self.toggle_select_mode(),
            EditorAction::CloseBuffer => self.close_current_buffer(),
            EditorAction::RepeatSearch(up) => self.repeat_search(up),
            EditorAction::DuplicateLine => self.sakura_duplicate_line(),
            EditorAction::ToggleSearchHighlight => self.toggle_search_highlight(),
            EditorAction::Quit | EditorAction::Nop => {}
        }
    }

    // --- Filer Actions ---

    pub(super) fn filer_open_selected(&mut self) {
        // Space でマークしたファイルがあれば、まとめてバッファで開く
        let marked_files: Vec<PathBuf> = if self.filer.active_pane().marked.is_empty() {
            Vec::new()
        } else {
            self.filer
                .active_pane()
                .target_entries()
                .into_iter()
                .filter(|e| !e.is_dir && !e.is_parent)
                .map(|e| e.path)
                .collect()
        };
        if !marked_files.is_empty() {
            self.filer.active_pane_mut().marked.clear();
            self.open_paths(marked_files, false);
            return;
        }

        let Some(entry) = self.filer.active_pane().selected_entry().cloned() else {
            return;
        };
        if entry.is_parent {
            if let Err(e) = self.filer.active_pane_mut().go_parent() {
                self.status_message = format!("移動エラー: {e}");
            }
        } else if entry.is_dir {
            if let Err(e) = self.filer.active_pane_mut().enter_dir(entry.path) {
                self.status_message = format!("ディレクトリを開けません: {e}");
            }
        } else {
            self.open_file_in_buffer(entry.path);
        }
    }

    /// Shift+Enter: カーソル位置のファイルを OS の関連付けで実行する（フォルダは OS のファイル管理で開く）。
    fn filer_run_selected(&mut self) {
        let Some(entry) = self.filer.active_pane().selected_entry() else {
            return;
        };
        if entry.is_parent {
            self.status_message = "親ディレクトリ (..) は実行できません".to_string();
            return;
        }
        self.status_message = match vz_platform::shell::open_with_os(&entry.path) {
            Ok(()) => format!("実行しました: {}", entry.name),
            Err(e) => format!("実行できません: {}（{e}）", entry.name),
        };
    }

    pub(super) fn execute_filer_action(&mut self, ctx: &egui::Context, action: FilerAction) {
        match action {
            FilerAction::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            FilerAction::Rename => {
                if let Some(entry) = self.filer.active_pane().selected_entry().cloned() {
                    if entry.is_parent {
                        self.status_message =
                            "親ディレクトリ (..) はリネームできません".to_string();
                    } else {
                        self.filer_modal = Some(FilerModal::Rename {
                            src: entry.path,
                            input: entry.name,
                            just_opened: true,
                        });
                    }
                }
            }
            FilerAction::NewFile => {
                let dir = self.filer.active_pane().current_dir.clone();
                self.filer_modal = Some(FilerModal::NewFile {
                    dir,
                    input: String::new(),
                    just_opened: true,
                });
            }
            FilerAction::Copy | FilerAction::Move => {
                let is_move = action == FilerAction::Move;
                let targets = self.filer.active_pane().target_entries();
                if targets.is_empty() {
                    let what = if is_move { "移動" } else { "コピー" };
                    self.status_message = format!("{what}対象を選択してください");
                } else {
                    let dst_dir = self.filer.inactive_pane().current_dir.clone();
                    self.filer_modal = Some(FilerModal::ConfirmCopyOrMove {
                        is_move,
                        targets,
                        dst_dir,
                    });
                }
            }
            FilerAction::Delete => {
                let targets = self.filer.active_pane().target_entries();
                if targets.is_empty() {
                    self.status_message = "削除対象を選択してください".to_string();
                } else {
                    self.filer_modal = Some(FilerModal::ConfirmDelete { targets });
                }
            }
            FilerAction::NewDir => {
                let dir = self.filer.active_pane().current_dir.clone();
                self.filer_modal = Some(FilerModal::NewDir {
                    dir,
                    input: String::new(),
                    just_opened: true,
                });
            }
            FilerAction::CycleSort => {
                let mode = self.filer.active_pane_mut().cycle_sort();
                self.status_message = format!("ソート順を変更しました: {}", mode.label());
            }
            FilerAction::BackToEditor => {
                self.enter_editor();
            }
            FilerAction::Quit => {
                self.request_quit(ctx);
            }
            FilerAction::Nop => {}
        }
    }

    // --- Event Dispatch ---

    pub(super) fn handle_events(&mut self, ctx: &egui::Context) {
        if self.inline_prompt.is_some()
            || self.filer_modal.is_some()
            || self.search.is_open()
            || self.show_about
        {
            return;
        }
        self.handle_send_keys(ctx);
        self.handle_window_style_keys(ctx);
        if self.search.is_open() {
            return;
        }
        if self.open_menu.is_some() {
            self.handle_menu_events(ctx);
            return;
        }
        match self.mode {
            AppMode::Editor => self.handle_editor_events(ctx),
            AppMode::Filer => self.handle_filer_events(ctx),
            AppMode::Terminal => self.handle_terminal_events(ctx),
            AppMode::Browser => self.handle_browser_events(ctx),
        }
    }

    pub(super) fn handle_editor_events(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        let modifiers_now = ctx.input(|i| i.modifiers);
        let mut suppress_text_this_frame = false;

        for event in events {
            let in_md_view = self.is_markdown_view();
            match event {
                Event::Ime(ime_ev) => match ime_ev {
                    ImeEvent::Enabled => {
                        self.ime_active = true;
                    }
                    ImeEvent::Preedit(text) => {
                        if !in_md_view {
                            self.ime_preedit = text;
                            self.need_scroll_to_cursor = true;
                        }
                    }
                    ImeEvent::Commit(text) => {
                        self.ime_preedit.clear();
                        if !in_md_view && !text.is_empty() {
                            self.prefix = PrefixKey::None;
                            self.select_mode = false;
                            self.type_text(&text);
                            self.need_scroll_to_cursor = true;
                        }
                    }
                    ImeEvent::Disabled => {
                        self.ime_active = false;
                        self.ime_preedit.clear();
                    }
                },

                // egui_winit translates Ctrl+X to Event::Cut -> VZ diamond cursor DOWN (or MD scroll down)!
                Event::Cut => {
                    if !self.ime_preedit.is_empty() {
                        continue;
                    }
                    suppress_text_this_frame = true;
                    if self.is_sakura() {
                        // Ctrl+X / Shift+Del
                        self.sakura_cut();
                        continue;
                    }
                    if self.prefix == PrefixKey::None {
                        if in_md_view {
                            self.scroll_markdown_by(self.line_height * 2.5);
                        } else {
                            self.cursor_down();
                            self.need_scroll_to_cursor = true;
                        }
                    } else {
                        self.prefix = PrefixKey::None;
                        self.status_message.clear();
                    }
                }

                // egui_winit translates Ctrl+C to Event::Copy -> supports Ctrl+Q+C (doc end).
                Event::Copy => {
                    if !self.ime_preedit.is_empty() {
                        continue;
                    }
                    suppress_text_this_frame = true;
                    if self.is_sakura() {
                        // Ctrl+C / Ctrl+Ins（Ctrl+Shift+C はサクラでは C/C++ ヘッダを開く: 対象外）
                        if !modifiers_now.shift {
                            self.sakura_copy();
                        }
                        continue;
                    }
                    if self.prefix == PrefixKey::CtrlQ {
                        self.prefix = PrefixKey::None;
                        if in_md_view {
                            self.scroll_markdown_to_bottom();
                        } else {
                            self.cur_buf_mut().move_doc_end();
                            self.need_scroll_to_cursor = true;
                        }
                        self.status_message = "ファイル末尾へ移動 (^Q^C)".to_string();
                    } else {
                        self.prefix = PrefixKey::None;
                    }
                }

                Event::Paste(text) => {
                    if !self.ime_preedit.is_empty() {
                        continue;
                    }
                    suppress_text_this_frame = true;
                    if self.is_sakura() {
                        if (modifiers_now.ctrl || modifiers_now.command) && modifiers_now.shift {
                            // Ctrl+Shift+V: Markdown 編集 / ビュー切替（egui は貼り付けとして届ける）
                            self.toggle_markdown_view();
                        } else {
                            // Ctrl+V / Shift+Ins（選択中は置き換え）
                            self.sakura_paste_text(&text);
                        }
                        continue;
                    }
                    self.prefix = PrefixKey::None;
                    if !in_md_view && !text.is_empty() {
                        self.cur_buf_mut().insert_text(&text);
                        self.need_scroll_to_cursor = true;
                    }
                }

                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    repeat,
                    ..
                } => {
                    if !self.ime_preedit.is_empty() {
                        continue;
                    }

                    let ctrl = modifiers.ctrl || modifiers.command;

                    if key == Key::F3 && !modifiers.alt && !(ctrl && modifiers.shift) {
                        suppress_text_this_frame = true;
                        if ctrl && repeat {
                            continue;
                        }
                        let layer = FKeyLayer::from_modifiers(modifiers);
                        if let Some(action) =
                            Self::editor_fkey_action(key, self.is_sakura(), in_md_view, layer)
                        {
                            self.execute_editor_action(action);
                        }
                        continue;
                    }
                    // Ctrl+F4 以外はエディターの選択・カーソルを動かさない。
                    if key == Key::F4 && (!ctrl || modifiers.alt || modifiers.shift) {
                        continue;
                    }

                    // 0. サクラ互換のキー割り当て（VZ の処理は通らない）
                    if self.is_sakura() {
                        if self.handle_sakura_key(key, modifiers, in_md_view) {
                            suppress_text_this_frame = true;
                            if self.mode == AppMode::Filer || self.search.is_open() {
                                return;
                            }
                        }
                        continue;
                    }

                    // VZ の通常キー操作を再開するときは検索結果の選択を解除する。
                    self.cur_buf_mut().clear_selection();

                    // 1. Two-stroke VZ prefix keys (^Q, ^K)
                    match self.prefix {
                        PrefixKey::CtrlQ => {
                            suppress_text_this_frame = true;
                            self.prefix = PrefixKey::None;
                            match key {
                                Key::S => {
                                    if !in_md_view {
                                        self.cur_buf_mut().move_line_start();
                                        self.need_scroll_to_cursor = true;
                                        self.status_message = "行頭へ移動 (^Q^S)".to_string();
                                    }
                                }
                                Key::D => {
                                    if !in_md_view {
                                        self.cur_buf_mut().move_line_end();
                                        self.need_scroll_to_cursor = true;
                                        self.status_message = "行末へ移動 (^Q^D)".to_string();
                                    }
                                }
                                Key::R => {
                                    if in_md_view {
                                        self.scroll_markdown_to_top();
                                    } else {
                                        self.cur_buf_mut().move_doc_start();
                                        self.need_scroll_to_cursor = true;
                                    }
                                    self.status_message = "ファイル先頭へ移動 (^Q^R)".to_string();
                                }
                                Key::C => {
                                    if in_md_view {
                                        self.scroll_markdown_to_bottom();
                                    } else {
                                        self.cur_buf_mut().move_doc_end();
                                        self.need_scroll_to_cursor = true;
                                    }
                                    self.status_message = "ファイル末尾へ移動 (^Q^C)".to_string();
                                }
                                Key::Escape => {
                                    self.status_message =
                                        "コマンドをキャンセルしました".to_string();
                                }
                                _ => {
                                    self.status_message = format!("未定義の ^Q コマンド: {key:?}");
                                }
                            }
                            continue;
                        }
                        PrefixKey::CtrlK => {
                            suppress_text_this_frame = true;
                            self.prefix = PrefixKey::None;
                            match key {
                                Key::D => {
                                    self.trigger_save();
                                }
                                Key::V => {
                                    self.toggle_markdown_view();
                                }
                                Key::Q => {
                                    self.enter_filer();
                                    return;
                                }
                                Key::Escape => {
                                    self.status_message =
                                        "コマンドをキャンセルしました".to_string();
                                }
                                _ => {
                                    self.status_message = format!("未定義の ^K コマンド: {key:?}");
                                }
                            }
                            continue;
                        }
                        PrefixKey::None => {}
                    }

                    // F3 と Ctrl+F4 以外の VZ のファンクションキーは修飾キーを区別しない。
                    let layer = if key == Key::F4 {
                        FKeyLayer::Ctrl
                    } else {
                        FKeyLayer::Normal
                    };
                    let fkey_action = if key == Key::F3 {
                        None
                    } else {
                        Self::editor_fkey_action(key, false, in_md_view, layer)
                    };
                    if let Some(action) = fkey_action {
                        suppress_text_this_frame = true;
                        self.execute_editor_action(action);
                        if self.mode == AppMode::Filer {
                            return;
                        }
                        continue;
                    }

                    // 3. ESC: instant switch to 2-pane Filer! (Closes help first if open)
                    if key == Key::Escape {
                        suppress_text_this_frame = true;
                        if self.cancel_huge_task() {
                            // 巨大ファイルの検索・置換の実行中は中止だけ
                        } else if self.show_help {
                            self.show_help = false;
                        } else {
                            self.enter_filer();
                            return;
                        }
                        continue;
                    }

                    // 3.5. Font size zoom (Ctrl++ / Ctrl+= / Ctrl+- / Ctrl+0)
                    if self.handle_font_zoom_key(key, modifiers) {
                        suppress_text_this_frame = true;
                        continue;
                    }

                    // 3.6. Alt+P: Markdown edit/view toggle
                    if modifiers.alt && !ctrl && key == Key::P {
                        suppress_text_this_frame = true;
                        self.toggle_markdown_view();
                        continue;
                    }

                    // 4. VZ Ctrl-based bindings
                    if ctrl && !modifiers.alt {
                        let handled = match key {
                            // Markdown Edit / View toggle: Ctrl+P
                            Key::P => {
                                self.toggle_markdown_view();
                                true
                            }
                            // Diamond cursor: Ctrl+E / S / D / X (scroll in MD View)
                            Key::E => {
                                if in_md_view {
                                    self.scroll_markdown_by(-self.line_height * 2.5);
                                } else {
                                    self.cursor_up();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            Key::S => {
                                if !in_md_view {
                                    self.cur_buf_mut().move_left();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            Key::D => {
                                if !in_md_view {
                                    self.cur_buf_mut().move_right();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            Key::X => {
                                if in_md_view {
                                    self.scroll_markdown_by(self.line_height * 2.5);
                                } else {
                                    self.cursor_down();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            // Word left / right: Ctrl+A / Ctrl+F
                            Key::A => {
                                if !in_md_view {
                                    self.cur_buf_mut().move_word_left();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            Key::F => {
                                if !in_md_view {
                                    self.cur_buf_mut().move_word_right();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            // Multi-buffer switch (裏画面切替): Ctrl+W / Ctrl+Shift+W
                            Key::W => {
                                if modifiers.shift {
                                    self.switch_buffer_step(-1);
                                } else {
                                    self.switch_buffer_step(1);
                                }
                                true
                            }
                            // Delete current line: Ctrl+Y
                            Key::Y => {
                                if !in_md_view {
                                    self.cur_buf_mut().delete_current_line();
                                    self.need_scroll_to_cursor = true;
                                    self.status_message = "現在行を削除しました (^Y)".to_string();
                                }
                                true
                            }
                            // Delete char at cursor: Ctrl+G
                            Key::G => {
                                if !in_md_view {
                                    self.cur_buf_mut().delete_char_at_cursor();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            // BackSpace: Ctrl+H
                            Key::H => {
                                if !in_md_view {
                                    self.cur_buf_mut().backspace();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            // Enter: Ctrl+M（Enter と同じく自動インデント）
                            Key::M => {
                                if !in_md_view {
                                    self.cur_buf_mut().insert_newline_with_indent();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            // Prefix keys: Ctrl+Q, Ctrl+K
                            Key::Q => {
                                self.prefix = PrefixKey::CtrlQ;
                                self.status_message =
                                    "^Q  [S:行頭  D:行末  R:先頭  C:末尾  ESC:取消]".to_string();
                                true
                            }
                            Key::K => {
                                self.prefix = PrefixKey::CtrlK;
                                self.status_message =
                                    "^K  [D:保存  V:MDビュー切替  Q:ファイラー  ESC:取消]"
                                        .to_string();
                                true
                            }
                            // Page up: Ctrl+R
                            Key::R => {
                                if in_md_view {
                                    let step = self.md_page_step();
                                    self.scroll_markdown_by(-step);
                                } else {
                                    let lines = self.last_visible_lines;
                                    self.cur_buf_mut().move_page_up(lines);
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            _ => false,
                        };
                        if handled {
                            suppress_text_this_frame = true;
                            continue;
                        }
                    }

                    // 5. Standard navigation & editing keys
                    if !ctrl && !modifiers.alt {
                        if in_md_view {
                            // サクラ互換と同じスクロール（Ctrl なし）
                            self.md_view_key(key, false);
                        } else {
                            match key {
                                Key::ArrowUp => {
                                    self.cursor_up();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::ArrowDown => {
                                    self.cursor_down();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::ArrowLeft => {
                                    self.cur_buf_mut().move_left();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::ArrowRight => {
                                    self.cur_buf_mut().move_right();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::Home => {
                                    self.cur_buf_mut().move_line_start();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::End => {
                                    self.cur_buf_mut().move_line_end();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::PageUp => {
                                    let lines = self.last_visible_lines;
                                    self.cur_buf_mut().move_page_up(lines);
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::PageDown => {
                                    let lines = self.last_visible_lines;
                                    self.cur_buf_mut().move_page_down(lines);
                                    self.need_scroll_to_cursor = true;
                                }
                                // Enter は自動インデント、Shift+Enter は行頭から
                                Key::Enter if modifiers.shift => {
                                    self.cur_buf_mut().insert_newline();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::Enter => {
                                    self.cur_buf_mut().insert_newline_with_indent();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::Backspace => {
                                    self.cur_buf_mut().backspace();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::Delete => {
                                    self.cur_buf_mut().delete_char_at_cursor();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::Tab => {
                                    self.cur_buf_mut().insert_text("\t");
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::Insert => {
                                    self.toggle_overwrite_mode();
                                }
                                _ => {}
                            }
                        }
                    }
                }

                Event::Text(text) => {
                    if in_md_view
                        || suppress_text_this_frame
                        || !self.ime_preedit.is_empty()
                        || self.prefix != PrefixKey::None
                        || modifiers_now.ctrl
                        || modifiers_now.command
                        || modifiers_now.alt
                    {
                        continue;
                    }
                    let filtered: String = text.chars().filter(|c| !c.is_control()).collect();
                    if !filtered.is_empty() {
                        // 選択中は置き換え（サクラ互換）。F6 の選択モードも終える
                        self.select_mode = false;
                        self.type_text(&filtered);
                        self.need_scroll_to_cursor = true;
                    }
                }

                _ => {}
            }
        }
    }

    pub(super) fn handle_filer_events(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());

        for event in events {
            match event {
                // Ctrl+X is converted to Event::Cut by egui_winit -> Filer cursor DOWN!
                Event::Cut => {
                    self.filer.active_pane_mut().move_down();
                }

                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let ctrl = modifiers.ctrl || modifiers.command;

                    // Function keys F1..F10
                    if let Some(action) = Self::filer_fkey_action(key) {
                        self.execute_filer_action(ctx, action);
                        if self.mode == AppMode::Editor || self.filer_modal.is_some() {
                            return;
                        }
                        continue;
                    }

                    // ESC: close help if open, else return immediately to Editor!
                    if key == Key::Escape {
                        if self.show_help {
                            self.show_help = false;
                        } else {
                            self.enter_editor();
                            return;
                        }
                        continue;
                    }

                    // Font size zoom (Ctrl++ / Ctrl+= / Ctrl+- / Ctrl+0)
                    if self.handle_font_zoom_key(key, modifiers) {
                        continue;
                    }

                    if ctrl && !modifiers.alt {
                        match key {
                            Key::E => self.filer.active_pane_mut().move_up(),
                            Key::X => self.filer.active_pane_mut().move_down(),
                            Key::S => {
                                self.filer.active = ActivePane::Left;
                            }
                            Key::D => {
                                self.filer.active = ActivePane::Right;
                            }
                            Key::H => {
                                if let Err(e) = self.filer.active_pane_mut().go_parent() {
                                    self.status_message = format!("移動エラー: {e}");
                                }
                            }
                            Key::M => {
                                self.filer_open_selected();
                                if self.mode == AppMode::Editor {
                                    return;
                                }
                            }
                            Key::R => {
                                let rows = self.last_filer_visible_rows;
                                self.filer.active_pane_mut().move_page_up(rows);
                            }
                            _ => {}
                        }
                        continue;
                    }

                    if !ctrl && !modifiers.alt {
                        match key {
                            Key::Tab => {
                                self.filer.active = self.filer.active.toggled();
                            }
                            Key::ArrowLeft => {
                                self.filer.active = ActivePane::Left;
                            }
                            Key::ArrowRight => {
                                self.filer.active = ActivePane::Right;
                            }
                            Key::ArrowUp => {
                                self.filer.active_pane_mut().move_up();
                            }
                            Key::ArrowDown => {
                                self.filer.active_pane_mut().move_down();
                            }
                            Key::PageUp => {
                                let rows = self.last_filer_visible_rows;
                                self.filer.active_pane_mut().move_page_up(rows);
                            }
                            Key::PageDown => {
                                let rows = self.last_filer_visible_rows;
                                self.filer.active_pane_mut().move_page_down(rows);
                            }
                            Key::Home => {
                                self.filer.active_pane_mut().move_top();
                            }
                            Key::End => {
                                self.filer.active_pane_mut().move_bottom();
                            }
                            Key::Space => {
                                self.filer.active_pane_mut().toggle_mark_and_advance();
                            }
                            Key::Backspace => {
                                if let Err(e) = self.filer.active_pane_mut().go_parent() {
                                    self.status_message = format!("移動エラー: {e}");
                                }
                            }
                            Key::Enter if modifiers.shift => self.filer_run_selected(),
                            Key::Enter => {
                                self.filer_open_selected();
                                if self.mode == AppMode::Editor {
                                    return;
                                }
                            }
                            _ => {}
                        }
                    }
                }

                _ => {}
            }
        }
    }

    // --- Compact VZ-Styled Top Menu Bar (VZ Keybinding Safe) ---
}
