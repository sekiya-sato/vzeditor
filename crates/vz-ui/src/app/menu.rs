//! メニューの項目・実行・キー操作・描画。

use super::*;

impl VzEditorApp {
    pub(super) fn menu_undo(&mut self) {
        if self.is_markdown_view() || self.mode != AppMode::Editor {
            return;
        }
        if self.cur_buf_mut().undo() {
            self.need_scroll_to_cursor = true;
            self.status_message = "元に戻しました (Undo)".to_string();
        } else {
            self.status_message = "元に戻せる履歴がありません".to_string();
        }
    }

    pub(super) fn menu_redo(&mut self) {
        if self.is_markdown_view() || self.mode != AppMode::Editor {
            return;
        }
        if self.cur_buf_mut().redo() {
            self.need_scroll_to_cursor = true;
            self.status_message = "やり直しました (Redo)".to_string();
        } else {
            self.status_message = "やり直せる履歴がありません".to_string();
        }
    }

    pub(super) fn menu_copy(&mut self, ctx: &egui::Context) {
        if self.mode != AppMode::Editor {
            return;
        }
        if self.is_sakura() {
            self.sakura_copy();
            return;
        }
        let text = if self.is_markdown_view() {
            self.cur_buf().text_lines().join(self.cur_buf().line_ending)
        } else {
            self.cur_buf().current_line_str().to_string()
        };
        self.internal_clipboard = text.clone();
        ctx.copy_text(text);
        self.status_message = if self.is_markdown_view() {
            "ドキュメント全体をクリップボードへコピーしました".to_string()
        } else {
            "現在行をクリップボードへコピーしました".to_string()
        };
    }

    pub(super) fn menu_cut(&mut self, ctx: &egui::Context) {
        if self.mode != AppMode::Editor || self.is_markdown_view() {
            return;
        }
        if self.is_sakura() {
            self.sakura_cut();
            return;
        }
        let line = self.cur_buf().current_line_str().to_string();
        self.internal_clipboard = line.clone();
        ctx.copy_text(line);
        self.cur_buf_mut().delete_current_line();
        self.need_scroll_to_cursor = true;
        self.status_message = "現在行を切り取りました".to_string();
    }

    pub(super) fn menu_paste(&mut self) {
        if self.mode != AppMode::Editor || self.is_markdown_view() {
            return;
        }
        let text = self.clipboard_text();
        if text.is_empty() {
            self.status_message =
                "クリップボードが空です（Ctrl+V での貼り付けも利用できます）".to_string();
        } else {
            self.cur_buf_mut().insert_text(&text);
            self.need_scroll_to_cursor = true;
            self.status_message = "クリップボードから貼り付けました".to_string();
        }
    }

    // --- Menu Bar Model & Actions ---

    pub(super) fn build_menu_entries(&self, menu: TopMenu) -> Vec<MenuEntry> {
        let in_editor = self.mode == AppMode::Editor;
        let in_md_view = self.is_markdown_view();
        let can_edit = in_editor && !in_md_view;
        let sakura = self.is_sakura();
        let sc = |sakura_key: &'static str, vz_key: &'static str| {
            if sakura {
                sakura_key
            } else {
                vz_key
            }
        };

        match menu {
            TopMenu::File => vec![
                MenuEntry::Item {
                    label: "新規作成",
                    shortcut: sc("Ctrl+N", "F2"),
                    action: MenuAction::NewBuffer,
                    enabled: true,
                },
                MenuEntry::Item {
                    label: "ファイルを開く...",
                    shortcut: sc("Ctrl+O", ""),
                    action: MenuAction::OpenFile,
                    enabled: true,
                },
                MenuEntry::Item {
                    label: "上書き保存",
                    shortcut: sc("Ctrl+S", "^K^D / F10"),
                    action: MenuAction::SaveFile,
                    enabled: true,
                },
                MenuEntry::Item {
                    label: "名前を付けて保存...",
                    shortcut: sc("Ctrl+Shift+S", ""),
                    action: MenuAction::SaveAs,
                    enabled: true,
                },
                MenuEntry::Item {
                    label: "文字コードを指定して保存...",
                    shortcut: "",
                    action: MenuAction::SaveWithEncoding,
                    enabled: in_editor,
                },
                MenuEntry::Item {
                    label: "印刷...",
                    shortcut: "Ctrl+P",
                    action: MenuAction::Print,
                    enabled: in_editor,
                },
                MenuEntry::Item {
                    label: "現在のバッファを閉じる",
                    shortcut: "Ctrl+F4",
                    action: MenuAction::CloseBuffer,
                    enabled: true,
                },
                MenuEntry::Separator,
                MenuEntry::Submenu {
                    label: "最近使ったファイル",
                    items: self.recent_menu_items(false),
                },
                MenuEntry::Submenu {
                    label: "最近使ったフォルダー",
                    items: self.recent_menu_items(true),
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: if in_editor {
                        "2画面ファイラーへ切替"
                    } else {
                        "エディタ画面へ戻る"
                    },
                    shortcut: sc("Esc", "ESC / F9"),
                    action: MenuAction::ToggleFiler,
                    enabled: true,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: "状態を記録して終了",
                    shortcut: "Alt+Shift+F4",
                    action: MenuAction::SaveStateAndQuit,
                    enabled: true,
                },
                MenuEntry::Item {
                    label: "終了",
                    shortcut: sc("Alt+F4", "^K^Q"),
                    action: MenuAction::Quit,
                    enabled: true,
                },
            ],
            TopMenu::Edit => vec![
                MenuEntry::Item {
                    label: "元に戻す (Undo)",
                    shortcut: sc("Ctrl+Z", ""),
                    action: MenuAction::Undo,
                    enabled: can_edit && self.cur_buf().can_undo(),
                },
                MenuEntry::Item {
                    label: "やり直し (Redo)",
                    shortcut: sc("Ctrl+Y", ""),
                    action: MenuAction::Redo,
                    enabled: can_edit && self.cur_buf().can_redo(),
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: if sakura {
                        "切り取り"
                    } else {
                        "切り取り (現在行)"
                    },
                    shortcut: sc("Ctrl+X", ""),
                    action: MenuAction::Cut,
                    enabled: can_edit,
                },
                MenuEntry::Item {
                    label: if in_md_view {
                        "コピー (全文)"
                    } else if sakura {
                        "コピー"
                    } else {
                        "コピー (現在行)"
                    },
                    shortcut: sc("Ctrl+C", ""),
                    action: MenuAction::Copy,
                    enabled: in_editor,
                },
                MenuEntry::Item {
                    label: "貼り付け",
                    shortcut: "Ctrl+V",
                    action: MenuAction::Paste,
                    enabled: can_edit,
                },
                MenuEntry::Item {
                    label: "すべて選択",
                    shortcut: sc("Ctrl+A", ""),
                    action: MenuAction::SelectAll,
                    enabled: can_edit && sakura,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: "現在行の削除",
                    shortcut: sc("Ctrl+Shift+E", "Ctrl+Y"),
                    action: MenuAction::DeleteLine,
                    enabled: can_edit,
                },
                MenuEntry::Item {
                    label: if in_md_view {
                        "ドキュメント先頭へ"
                    } else {
                        "行頭へ移動"
                    },
                    shortcut: sc("Home", "^Q^S"),
                    action: MenuAction::MoveLineStart,
                    enabled: in_editor,
                },
                MenuEntry::Item {
                    label: if in_md_view {
                        "ドキュメント末尾へ"
                    } else {
                        "行末へ移動"
                    },
                    shortcut: sc("End", "^Q^D / F5"),
                    action: MenuAction::MoveLineEnd,
                    enabled: in_editor,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: if self.overwrite_mode {
                        "挿入モードに戻す"
                    } else {
                        "上書きモードにする"
                    },
                    shortcut: "Ins",
                    action: MenuAction::ToggleOverwrite,
                    enabled: can_edit,
                },
            ],
            TopMenu::Search => vec![
                MenuEntry::Item {
                    label: "検索…",
                    shortcut: sc("Ctrl+F", "Alt+F"),
                    action: MenuAction::OpenSearch(SearchDialog::Find),
                    enabled: in_editor,
                },
                MenuEntry::Item {
                    label: "次を検索",
                    shortcut: "F3 / Alt+N",
                    action: MenuAction::RepeatSearch(false),
                    enabled: in_editor,
                },
                MenuEntry::Item {
                    label: "前を検索",
                    shortcut: "Shift+F3 / Alt+Shift+N",
                    action: MenuAction::RepeatSearch(true),
                    enabled: in_editor,
                },
                MenuEntry::Item {
                    label: if self.search.highlight_enabled() {
                        "検索ハイライトをオフ"
                    } else {
                        "検索ハイライトをオン"
                    },
                    shortcut: "Ctrl+F3",
                    action: MenuAction::ToggleSearchHighlight,
                    enabled: in_editor,
                },
                MenuEntry::Item {
                    label: "行マークの切替",
                    shortcut: "",
                    action: MenuAction::ToggleSearchMarks,
                    enabled: in_editor,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: "置換…",
                    shortcut: sc("Ctrl+R", "Alt+R"),
                    action: MenuAction::OpenSearch(SearchDialog::Replace),
                    enabled: in_editor,
                },
                MenuEntry::Item {
                    label: "Grep…",
                    shortcut: sc("Ctrl+G", "Alt+G"),
                    action: MenuAction::OpenSearch(SearchDialog::Grep),
                    enabled: in_editor,
                },
                MenuEntry::Item {
                    label: "指定行へジャンプ…",
                    shortcut: sc("Ctrl+J", "Alt+J"),
                    action: MenuAction::OpenSearch(SearchDialog::Jump),
                    enabled: in_editor,
                },
            ],
            TopMenu::Tools => vec![
                MenuEntry::Item {
                    label: if in_md_view {
                        "テキスト編集モードへ"
                    } else {
                        "Markdown Viewモードへ"
                    },
                    shortcut: sc("Ctrl+Shift+V / Alt+P", "Alt+P / ^K^V"),
                    action: MenuAction::ToggleMarkdownView,
                    enabled: in_editor,
                },
                MenuEntry::Item {
                    label: if self.mode == AppMode::Terminal {
                        "ターミナルから戻る"
                    } else {
                        "ターミナル"
                    },
                    shortcut: "Alt+K",
                    action: MenuAction::ToggleTerminal,
                    enabled: true,
                },
                MenuEntry::Submenu {
                    label: "ブラウザ",
                    items: vec![
                        MenuEntry::Item {
                            label: if self.mode == AppMode::Browser {
                                "ブラウザから戻る"
                            } else {
                                "編集中の HTML を表示"
                            },
                            shortcut: "Alt+B",
                            action: MenuAction::ToggleBrowser,
                            enabled: true,
                        },
                        MenuEntry::Item {
                            label: "新しいタブ",
                            shortcut: "Ctrl+T",
                            action: MenuAction::NewBrowserTab,
                            enabled: true,
                        },
                        MenuEntry::Item {
                            label: "このタブを閉じる",
                            shortcut: "Ctrl+W",
                            action: MenuAction::CloseBrowserTab,
                            enabled: self.mode == AppMode::Browser,
                        },
                    ],
                },
                MenuEntry::Separator,
                MenuEntry::Submenu {
                    label: "フォント",
                    items: vec![
                        MenuEntry::Item {
                            label: "サイズ拡大",
                            shortcut: "Ctrl++",
                            action: MenuAction::FontZoomIn,
                            enabled: self.font_size < MAX_FONT_SIZE,
                        },
                        MenuEntry::Item {
                            label: "サイズ縮小",
                            shortcut: "Ctrl+-",
                            action: MenuAction::FontZoomOut,
                            enabled: self.font_size > MIN_FONT_SIZE,
                        },
                        MenuEntry::Item {
                            label: "サイズリセット (16pt)",
                            shortcut: "Ctrl+0",
                            action: MenuAction::FontReset,
                            enabled: true,
                        },
                    ],
                },
                MenuEntry::Item {
                    label: if self.word_wrap {
                        "折り返し表示を解除"
                    } else {
                        "長い行を折り返して表示"
                    },
                    shortcut: "Alt+W",
                    action: MenuAction::ToggleWordWrap,
                    enabled: true,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: "次のバッファへ切替",
                    shortcut: sc("Ctrl+Tab", "Ctrl+W"),
                    action: MenuAction::NextBuffer,
                    enabled: self.buffers.len() > 1,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: if self.always_on_top {
                        "最前面固定を解除"
                    } else {
                        "最前面に固定"
                    },
                    shortcut: "Alt+T",
                    action: MenuAction::ToggleAlwaysOnTop,
                    enabled: true,
                },
                MenuEntry::Submenu {
                    label: "背景透過",
                    items: vec![
                        MenuEntry::Item {
                            label: "濃くする (+10%)",
                            shortcut: "Alt+↑",
                            action: MenuAction::OpacityUp,
                            enabled: self.bg_opacity < window_style::MAX_OPACITY,
                        },
                        MenuEntry::Item {
                            label: "透過する (-10%)",
                            shortcut: "Alt+↓",
                            action: MenuAction::OpacityDown,
                            enabled: self.bg_opacity > window_style::MIN_OPACITY,
                        },
                        MenuEntry::Item {
                            label: "解除 (100%)",
                            shortcut: "Alt+0",
                            action: MenuAction::OpacityReset,
                            enabled: self.bg_opacity < window_style::MAX_OPACITY,
                        },
                    ],
                },
                MenuEntry::Submenu {
                    label: "文字透過",
                    items: vec![
                        MenuEntry::Item {
                            label: "濃くする (+10%)",
                            shortcut: "Alt+Shift++",
                            action: MenuAction::TextOpacityUp,
                            enabled: self.text_opacity < window_style::MAX_OPACITY,
                        },
                        MenuEntry::Item {
                            label: "透過する (-10%)",
                            shortcut: "Alt+Shift+-",
                            action: MenuAction::TextOpacityDown,
                            enabled: self.text_opacity > window_style::MIN_OPACITY,
                        },
                        MenuEntry::Item {
                            label: "解除 (100%)",
                            shortcut: "Alt+Shift+0",
                            action: MenuAction::TextOpacityReset,
                            enabled: self.text_opacity < window_style::MAX_OPACITY,
                        },
                    ],
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: if sakura {
                        "キー割り当て: サクラ互換 → VZ互換へ切替"
                    } else {
                        "キー割り当て: VZ互換 → サクラ互換へ切替"
                    },
                    shortcut: "",
                    action: MenuAction::ToggleKeyScheme,
                    enabled: true,
                },
                MenuEntry::Separator,
                MenuEntry::Submenu {
                    label: "他アプリ連携",
                    items: vec![
                        MenuEntry::Item {
                            label: "Claude Code へ送信",
                            shortcut: "Alt+C → C",
                            action: MenuAction::SendToClaudeCode,
                            enabled: self.pending_send.is_none(),
                        },
                        MenuEntry::Item {
                            label: "Codex へ送信",
                            shortcut: "Alt+C → X",
                            action: MenuAction::SendToCodex,
                            enabled: self.pending_send.is_none(),
                        },
                        MenuEntry::Item {
                            label: "Claude Code で新規セッション",
                            shortcut: "Alt+C → N",
                            action: MenuAction::NewSessionClaudeCode,
                            enabled: self.pending_send.is_none(),
                        },
                        MenuEntry::Item {
                            label: "Codex で新規セッション",
                            shortcut: "Alt+C → M",
                            action: MenuAction::NewSessionCodex,
                            enabled: self.pending_send.is_none(),
                        },
                        MenuEntry::Item {
                            label: "GitHub Desktop で Push",
                            shortcut: "Alt+C → P",
                            action: MenuAction::PushGitHubDesktop,
                            enabled: self.pending_send.is_none(),
                        },
                    ],
                },
            ],
            TopMenu::About => vec![
                MenuEntry::Item {
                    label: "キーバインド・操作ヘルプ",
                    shortcut: "F1",
                    action: MenuAction::ShowHelp,
                    enabled: true,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: "バージョン情報...",
                    shortcut: "",
                    action: MenuAction::ShowAbout,
                    enabled: true,
                },
            ],
        }
    }

    /// 最近使ったファイル／フォルダーのサブメニュー（新しい順に先頭 36 件。0-9・A-Z をアクセスキーにする）。
    /// ファイルは文字コードを添え、開いているものに印を付ける。
    fn recent_menu_items(&self, folders: bool) -> Vec<MenuEntry> {
        let items: Vec<MenuEntry> = if folders {
            self.history
                .folders
                .iter()
                .take(RECENT_MENU_MAX)
                .enumerate()
                .map(|(i, dir)| MenuEntry::Recent {
                    label: format!("{} {}", recent_access_key(i), dir.display()),
                    action: MenuAction::OpenRecentFolder(i),
                    checked: false,
                })
                .collect()
        } else {
            let open: Vec<PathBuf> = self
                .buffers
                .iter()
                .filter_map(|slot| slot.buffer.file_path.as_deref())
                .filter_map(history::normalize)
                .collect();
            self.history
                .files
                .iter()
                .take(RECENT_MENU_MAX)
                .enumerate()
                .map(|(i, f)| MenuEntry::Recent {
                    label: format!(
                        "{} {}  [{}]",
                        recent_access_key(i),
                        f.path.display(),
                        f.encoding
                    ),
                    action: MenuAction::OpenRecentFile(i),
                    checked: open.iter().any(|p| history::same_path(p, &f.path)),
                })
                .collect()
        };
        if items.is_empty() {
            return vec![MenuEntry::Item {
                label: "（履歴はありません）",
                shortcut: "",
                action: MenuAction::NewBuffer,
                enabled: false,
            }];
        }
        items
    }

    pub(super) fn first_selectable_menu_idx(entries: &[MenuEntry]) -> usize {
        entries
            .iter()
            .position(|e| {
                matches!(
                    e,
                    MenuEntry::Item { enabled: true, .. }
                        | MenuEntry::Recent { .. }
                        | MenuEntry::Submenu { .. }
                )
            })
            .or_else(|| {
                entries
                    .iter()
                    .position(|e| !matches!(e, MenuEntry::Separator))
            })
            .unwrap_or(0)
    }

    /// 区切り線を飛ばして `delta` 方向の次の項目の位置を返す（端で反対へ回る）。
    fn step_menu_idx(entries: &[MenuEntry], current: usize, delta: isize) -> usize {
        let len = entries.len();
        if len == 0 {
            return current;
        }
        let mut idx = current.min(len - 1);
        for _ in 0..len {
            idx = ((idx as isize + delta).rem_euclid(len as isize)) as usize;
            if !matches!(entries[idx], MenuEntry::Separator) {
                return idx;
            }
        }
        current
    }

    /// 開いているサブメニューの項目。親が範囲外やサブメニューでなければ None。
    fn open_submenu_items(&self, entries: &[MenuEntry]) -> Option<Vec<MenuEntry>> {
        match entries.get(self.open_submenu?) {
            Some(MenuEntry::Submenu { items, .. }) => Some(items.clone()),
            _ => None,
        }
    }

    fn open_submenu_at(&mut self, entries: &[MenuEntry], idx: usize) {
        if let Some(MenuEntry::Submenu { items, .. }) = entries.get(idx) {
            self.menu_selected_idx = idx;
            self.open_submenu = Some(idx);
            self.submenu_selected_idx = Self::first_selectable_menu_idx(items);
        }
    }

    pub(super) fn open_top_menu(&mut self, menu: TopMenu) {
        self.prefix = PrefixKey::None;
        if menu == TopMenu::File {
            self.reload_history();
        }
        let entries = self.build_menu_entries(menu);
        self.menu_selected_idx = Self::first_selectable_menu_idx(&entries);
        self.open_submenu = None;
        self.open_menu = Some(menu);
    }

    pub(super) fn execute_menu_action(&mut self, ctx: &egui::Context, action: MenuAction) {
        self.open_menu = None;
        self.open_submenu = None;
        self.prefix = PrefixKey::None;
        match action {
            MenuAction::NewBuffer => self.new_empty_buffer(),
            MenuAction::OpenFile => self.trigger_open(),
            MenuAction::SaveFile => self.trigger_save(),
            MenuAction::SaveAs => self.trigger_save_as(),
            MenuAction::SaveWithEncoding => self.open_encoding_dialog(),
            MenuAction::Print => self.open_print_dialog(),
            MenuAction::CloseBuffer => self.close_current_buffer(),
            MenuAction::OpenRecentFile(idx) => self.open_recent_file(idx),
            MenuAction::OpenRecentFolder(idx) => self.open_recent_folder(idx),
            MenuAction::ToggleFiler => match self.mode {
                AppMode::Filer => self.enter_editor(),
                _ => self.enter_filer(),
            },
            MenuAction::Quit => self.request_quit(ctx),
            MenuAction::SaveStateAndQuit => self.save_state_and_quit(ctx),
            MenuAction::Undo => self.menu_undo(),
            MenuAction::Redo => self.menu_redo(),
            MenuAction::Cut => self.menu_cut(ctx),
            MenuAction::Copy => self.menu_copy(ctx),
            MenuAction::Paste => self.menu_paste(),
            MenuAction::DeleteLine => self.execute_editor_action(EditorAction::DeleteLine),
            MenuAction::MoveLineStart => self.execute_editor_action(EditorAction::MoveLineStart),
            MenuAction::MoveLineEnd => self.execute_editor_action(EditorAction::MoveLineEnd),
            MenuAction::ToggleOverwrite => self.toggle_overwrite_mode(),
            MenuAction::OpenSearch(dialog) => self.open_search_dialog(dialog),
            MenuAction::RepeatSearch(up) => self.repeat_search(up),
            MenuAction::ToggleSearchHighlight => {
                self.execute_editor_action(EditorAction::ToggleSearchHighlight);
            }
            MenuAction::ToggleSearchMarks => {
                if let Err(message) = self.toggle_search_marks() {
                    self.status_message = message;
                }
            }
            MenuAction::ToggleMarkdownView => {
                if self.mode == AppMode::Editor {
                    self.toggle_markdown_view();
                }
            }
            MenuAction::FontZoomIn => self.zoom_in_font(),
            MenuAction::FontZoomOut => self.zoom_out_font(),
            MenuAction::FontReset => self.reset_font_size(),
            MenuAction::NextBuffer => {
                if self.mode != AppMode::Editor {
                    self.enter_editor();
                }
                self.switch_buffer_step(1);
            }
            MenuAction::ToggleAlwaysOnTop => self.toggle_always_on_top(ctx),
            MenuAction::OpacityUp => self.change_bg_opacity(window_style::OPACITY_STEP as i16),
            MenuAction::OpacityDown => self.change_bg_opacity(-(window_style::OPACITY_STEP as i16)),
            MenuAction::OpacityReset => self.set_bg_opacity(window_style::MAX_OPACITY),
            MenuAction::TextOpacityUp => {
                self.change_text_opacity(window_style::OPACITY_STEP as i16)
            }
            MenuAction::TextOpacityDown => {
                self.change_text_opacity(-(window_style::OPACITY_STEP as i16))
            }
            MenuAction::TextOpacityReset => self.set_text_opacity(window_style::MAX_OPACITY),
            MenuAction::SendToClaudeCode => self.start_send(SendTarget::ClaudeCode),
            MenuAction::SendToCodex => self.start_send(SendTarget::Codex),
            MenuAction::PushGitHubDesktop => self.start_send(SendTarget::GitHubDesktop),
            MenuAction::NewSessionClaudeCode => self.start_send(SendTarget::ClaudeCodeNewSession),
            MenuAction::NewSessionCodex => self.start_send(SendTarget::CodexNewSession),
            MenuAction::ToggleKeyScheme => self.toggle_key_scheme(),
            MenuAction::ToggleWordWrap => self.toggle_word_wrap(),
            MenuAction::ToggleTerminal => self.toggle_terminal(ctx),
            MenuAction::ToggleBrowser => self.toggle_browser(),
            MenuAction::NewBrowserTab => {
                // エディターなどからは新しいタブを開いてブラウザを表示する
                self.new_browser_tab(None);
                let idx = self.active_browser;
                self.show_browser_tab(idx);
            }
            MenuAction::CloseBrowserTab => self.close_browser_tab(self.active_browser),
            MenuAction::SelectAll => {
                if self.mode == AppMode::Editor && !self.is_markdown_view() {
                    self.cur_buf_mut().select_all();
                }
            }
            MenuAction::ShowHelp => {
                self.show_help = !self.show_help;
            }
            MenuAction::ShowAbout => {
                self.show_help = false;
                self.show_about = true;
            }
        }
    }

    pub(super) fn handle_menu_events(&mut self, ctx: &egui::Context) {
        let Some(menu) = self.open_menu else {
            return;
        };
        let events = ctx.input(|i| i.events.clone());
        let mut active_menu = menu;

        for event in events {
            if let Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                let ctrl = modifiers.ctrl || modifiers.command;
                if ctrl || modifiers.alt {
                    continue;
                }
                let entries = self.build_menu_entries(active_menu);
                if let Some(items) = self.open_submenu_items(&entries) {
                    // サブメニューを開いている間: 上下で移動、← / Esc で閉じる、→ で次のメニュー
                    match key {
                        Key::Escape | Key::ArrowLeft => self.open_submenu = None,
                        Key::ArrowDown | Key::ArrowUp => {
                            let delta = if key == Key::ArrowDown { 1 } else { -1 };
                            self.submenu_selected_idx =
                                Self::step_menu_idx(&items, self.submenu_selected_idx, delta);
                        }
                        Key::ArrowRight => {
                            active_menu = active_menu.next();
                            self.open_top_menu(active_menu);
                        }
                        Key::Enter => {
                            if let Some(
                                MenuEntry::Item {
                                    action,
                                    enabled: true,
                                    ..
                                }
                                | MenuEntry::Recent { action, .. },
                            ) = items.get(self.submenu_selected_idx)
                            {
                                self.execute_menu_action(ctx, *action);
                                return;
                            }
                        }
                        _ => {
                            // 履歴の 0-9・A-Z で直接開く
                            if let Some(action) = recent_action_for_key(&items, key) {
                                self.execute_menu_action(ctx, action);
                                return;
                            }
                        }
                    }
                    continue;
                }
                match key {
                    Key::Escape => {
                        self.open_menu = None;
                        return;
                    }
                    Key::ArrowDown | Key::ArrowUp => {
                        let delta = if key == Key::ArrowDown { 1 } else { -1 };
                        self.menu_selected_idx =
                            Self::step_menu_idx(&entries, self.menu_selected_idx, delta);
                    }
                    Key::ArrowRight => {
                        if matches!(
                            entries.get(self.menu_selected_idx),
                            Some(MenuEntry::Submenu { .. })
                        ) {
                            self.open_submenu_at(&entries, self.menu_selected_idx);
                        } else {
                            active_menu = active_menu.next();
                            self.open_top_menu(active_menu);
                        }
                    }
                    Key::ArrowLeft => {
                        active_menu = active_menu.prev();
                        self.open_top_menu(active_menu);
                    }
                    Key::Enter => match entries.get(self.menu_selected_idx) {
                        Some(MenuEntry::Item {
                            action,
                            enabled: true,
                            ..
                        }) => {
                            self.execute_menu_action(ctx, *action);
                            return;
                        }
                        Some(MenuEntry::Submenu { .. }) => {
                            self.open_submenu_at(&entries, self.menu_selected_idx);
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }

    // --- Editor Actions ---

    pub(super) fn render_menu_bar(&mut self, ctx: &egui::Context) {
        if self.open_menu.is_some() {
            // Keep IME off while navigating an open menu so arrows / Enter / ESC stay crisp
            ctx.output_mut(|o| {
                o.ime = None;
            });
        }

        let mut header_rects = TopMenu::ALL.map(|menu| (menu, Rect::NOTHING));
        let mut bar_rect = Rect::NOTHING;
        let mut clicked_header: Option<TopMenu> = None;
        let mut hovered_header: Option<TopMenu> = None;
        let mut font_cmd: Option<i8> = None;
        let mut win_cmd: Option<MenuAction> = None;

        egui::TopBottomPanel::top("menu_bar")
            .exact_height(24.0)
            .frame(
                egui::Frame::NONE
                    .fill(bg(MENU_BAR_BG))
                    .inner_margin(egui::Margin::symmetric(6, 1))
                    .stroke(Stroke::new(1.0_f32, GUTTER_SEP)),
            )
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                bar_rect = ui.max_rect();
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;

                    for (slot_idx, &menu) in TopMenu::ALL.iter().enumerate() {
                        let label = menu.label();
                        let font = FontId::monospace(12.5);
                        let text_w = ui.fonts(|f| {
                            f.layout_no_wrap(label.to_string(), font.clone(), TEXT_WHITE)
                                .size()
                                .x
                        });
                        let btn_size = Vec2::new(text_w + 16.0, 20.0);
                        // Non-focusable click region so keyboard focus is NEVER stolen from editor/filer
                        let (rect, resp) = ui.allocate_exact_size(btn_size, Sense::click());
                        header_rects[slot_idx] = (menu, rect);

                        let is_open = self.open_menu == Some(menu);
                        if resp.hovered() {
                            hovered_header = Some(menu);
                        }
                        if resp.clicked() {
                            clicked_header = Some(menu);
                        }

                        if ui.is_rect_visible(rect) {
                            let painter = ui.painter();
                            if is_open || resp.hovered() {
                                let fill = if is_open {
                                    MENU_ITEM_HOVER
                                } else {
                                    MENU_BAR_HOVER
                                };
                                painter.rect(
                                    rect,
                                    CornerRadius::same(2),
                                    fill,
                                    Stroke::new(
                                        1.0_f32,
                                        if is_open { TEXT_CYAN } else { BORDER_BLUE },
                                    ),
                                    StrokeKind::Inside,
                                );
                            }
                            painter.text(
                                rect.center(),
                                Align2::CENTER_CENTER,
                                label,
                                font,
                                if is_open { TEXT_CYAN } else { TEXT_WHITE },
                            );
                        }
                    }

                    // 右端: 文字・背景の透過、最前面、フォントサイズ（どの画面でも同じ位置）
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        font_cmd = Self::paint_font_size_controls(ui, self.font_size);
                        ui.add_space(6.0);
                        win_cmd = Self::paint_window_style_controls(
                            ui,
                            self.bg_opacity,
                            self.text_opacity,
                            self.always_on_top,
                        );
                    });
                });
            });

        if win_cmd.is_some() || font_cmd.is_some() {
            self.open_menu = None;
            self.open_submenu = None;
        }
        if let Some(action) = win_cmd {
            self.execute_menu_action(ctx, action);
        }
        match font_cmd {
            Some(1) => self.zoom_in_font(),
            Some(-1) => self.zoom_out_font(),
            Some(_) => self.reset_font_size(),
            None => {}
        }

        if let Some(menu) = clicked_header {
            if self.open_menu == Some(menu) {
                self.open_menu = None;
            } else {
                self.open_top_menu(menu);
            }
        } else if let (Some(open), Some(hovered)) = (self.open_menu, hovered_header) {
            if open != hovered {
                self.open_top_menu(hovered);
            }
        }

        // Dropdown popup rendered on Foreground layer
        let mut triggered_action: Option<MenuAction> = None;
        let mut popup_rect = Rect::NOTHING;
        let mut submenu_rect = Rect::NOTHING;
        let mut submenu_anchor: Option<(usize, Rect)> = None;

        if let Some(active_menu) = self.open_menu {
            let anchor_rect = header_rects
                .iter()
                .find(|(m, _)| *m == active_menu)
                .map(|(_, r)| *r)
                .unwrap_or(bar_rect);
            let entries = self.build_menu_entries(active_menu);
            let popup_w = 302.0_f32;
            let row_h = 24.0_f32;
            let sep_h = 9.0_f32;
            let selected_idx = self.menu_selected_idx;

            let area_resp = egui::Area::new(egui::Id::new("lz_menu_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(Pos2::new(anchor_rect.min.x, bar_rect.max.y))
                .show(ctx, |ui| {
                    let frame_resp = Self::menu_popup_frame().show(ui, |ui| {
                        ui.set_width(popup_w);
                        ui.spacing_mut().item_spacing.y = 1.0;

                        for (idx, entry) in entries.iter().enumerate() {
                            match entry {
                                MenuEntry::Separator => {
                                    Self::paint_menu_separator(ui, popup_w, sep_h);
                                }
                                MenuEntry::Item {
                                    label,
                                    shortcut,
                                    action,
                                    enabled,
                                } => {
                                    let (rect, resp) = ui.allocate_exact_size(
                                        Vec2::new(popup_w, row_h),
                                        Sense::click(),
                                    );
                                    if resp.hovered() {
                                        self.menu_selected_idx = idx;
                                        self.open_submenu = None;
                                    }
                                    let is_selected = idx == selected_idx || resp.hovered();
                                    Self::paint_menu_row(
                                        ui,
                                        rect,
                                        label,
                                        shortcut,
                                        *enabled,
                                        is_selected,
                                    );
                                    if *enabled && resp.clicked() {
                                        triggered_action = Some(*action);
                                    }
                                }
                                MenuEntry::Recent { .. } => {}
                                MenuEntry::Submenu { label, .. } => {
                                    let (rect, resp) = ui.allocate_exact_size(
                                        Vec2::new(popup_w, row_h),
                                        Sense::click(),
                                    );
                                    if (resp.hovered() || resp.clicked())
                                        && self.open_submenu != Some(idx)
                                    {
                                        self.open_submenu_at(&entries, idx);
                                    }
                                    let is_open = self.open_submenu == Some(idx);
                                    if is_open {
                                        submenu_anchor = Some((idx, rect));
                                    }
                                    let is_selected =
                                        idx == selected_idx || resp.hovered() || is_open;
                                    Self::paint_menu_row(ui, rect, label, "▶", true, is_selected);
                                }
                            }
                        }
                    });
                    popup_rect = frame_resp.response.rect;
                });
            if popup_rect == Rect::NOTHING {
                popup_rect = area_resp.response.rect;
            }

            // サブメニュー: 親項目の右に開く（画面の右端を越えるなら左に開く）
            if let Some((parent_idx, anchor)) = submenu_anchor {
                if let Some(MenuEntry::Submenu { items, .. }) = entries.get(parent_idx) {
                    let frame_pad = 4.0_f32;
                    let screen = ctx.screen_rect();
                    // 履歴はパスが長いので、最も長い行に合わせて広げる（画面幅まで）
                    let text_w = items
                        .iter()
                        .filter_map(|e| match e {
                            MenuEntry::Recent { label, .. } => Some(ctx.fonts(|f| {
                                f.layout_no_wrap(
                                    recent_row_text(label),
                                    FontId::monospace(12.5),
                                    TEXT_WHITE,
                                )
                                .size()
                                .x
                            })),
                            _ => None,
                        })
                        .fold(0.0_f32, f32::max);
                    let sub_w = (text_w + 24.0)
                        .max(326.0)
                        .min(screen.width() - frame_pad * 2.0 - 8.0);
                    let mut x = popup_rect.max.x - 2.0;
                    if x + sub_w + frame_pad * 2.0 > screen.max.x {
                        x = (popup_rect.min.x - sub_w - frame_pad * 2.0 + 2.0).max(screen.min.x);
                    }
                    let sub_selected = self.submenu_selected_idx;
                    let sub_resp = egui::Area::new(egui::Id::new("lz_menu_submenu"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(Pos2::new(x, anchor.min.y - frame_pad))
                        .show(ctx, |ui| {
                            Self::menu_popup_frame().show(ui, |ui| {
                                ui.set_width(sub_w);
                                ui.spacing_mut().item_spacing.y = 1.0;
                                for (idx, entry) in items.iter().enumerate() {
                                    match entry {
                                        MenuEntry::Item {
                                            label,
                                            shortcut,
                                            action,
                                            enabled,
                                        } => {
                                            let (rect, resp) = ui.allocate_exact_size(
                                                Vec2::new(sub_w, row_h),
                                                Sense::click(),
                                            );
                                            if resp.hovered() {
                                                self.submenu_selected_idx = idx;
                                            }
                                            let is_selected = idx == sub_selected || resp.hovered();
                                            Self::paint_menu_row(
                                                ui,
                                                rect,
                                                label,
                                                shortcut,
                                                *enabled,
                                                is_selected,
                                            );
                                            if *enabled && resp.clicked() {
                                                triggered_action = Some(*action);
                                            }
                                        }
                                        MenuEntry::Recent {
                                            label,
                                            action,
                                            checked,
                                        } => {
                                            let (rect, resp) = ui.allocate_exact_size(
                                                Vec2::new(sub_w, row_h),
                                                Sense::click(),
                                            );
                                            if resp.hovered() {
                                                self.submenu_selected_idx = idx;
                                            }
                                            let is_selected = idx == sub_selected || resp.hovered();
                                            Self::paint_menu_row(
                                                ui,
                                                rect,
                                                &recent_row_text(label),
                                                "",
                                                true,
                                                is_selected,
                                            );
                                            if *checked {
                                                Self::paint_menu_check(ui, rect);
                                            }
                                            if resp.clicked() {
                                                triggered_action = Some(*action);
                                            }
                                        }
                                        _ => Self::paint_menu_separator(ui, sub_w, sep_h),
                                    }
                                }
                            });
                        });
                    submenu_rect = sub_resp.response.rect;
                }
            }
        }

        if let Some(action) = triggered_action {
            self.execute_menu_action(ctx, action);
        } else if self.open_menu.is_some() && clicked_header.is_none() {
            // Close menu if the user clicked anywhere outside the menu bar and the popups
            let clicked_outside = ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|pos| {
                        !bar_rect.contains(pos)
                            && !popup_rect.contains(pos)
                            && !submenu_rect.contains(pos)
                    })
            });
            if clicked_outside {
                self.open_menu = None;
            }
        }
    }

    fn menu_popup_frame() -> egui::Frame {
        egui::Frame::NONE
            .fill(MENU_POPUP_BG)
            .stroke(Stroke::new(1.5_f32, TEXT_CYAN))
            .corner_radius(CornerRadius::same(2))
            .inner_margin(egui::Margin::symmetric(4, 4))
    }

    fn paint_menu_separator(ui: &mut egui::Ui, width: f32, height: f32) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        let y = rect.center().y;
        ui.painter().line_segment(
            [
                Pos2::new(rect.min.x + 6.0, y),
                Pos2::new(rect.max.x - 6.0, y),
            ],
            Stroke::new(1.0_f32, GUTTER_SEP),
        );
    }

    /// 履歴の行の左端の印（開いているファイル）。フォントに頼らず線で描く。
    fn paint_menu_check(ui: &egui::Ui, rect: Rect) {
        let x = rect.min.x + 6.0;
        let y = rect.center().y;
        ui.painter().add(egui::Shape::line(
            vec![
                Pos2::new(x, y),
                Pos2::new(x + 3.5, y + 3.5),
                Pos2::new(x + 10.0, y - 4.5),
            ],
            Stroke::new(2.0_f32, TEXT_CYAN),
        ));
    }

    /// メニューの 1 行（左に項目名、右にショートカットかサブメニューの ▶）。
    fn paint_menu_row(
        ui: &egui::Ui,
        rect: Rect,
        label: &str,
        shortcut: &str,
        enabled: bool,
        selected: bool,
    ) {
        if !ui.is_rect_visible(rect) {
            return;
        }
        let p = ui.painter();
        if selected && enabled {
            p.rect_filled(rect, CornerRadius::same(2), MENU_ITEM_HOVER);
        }
        p.text(
            Pos2::new(rect.min.x + 10.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::monospace(12.5),
            if enabled {
                TEXT_WHITE
            } else {
                MENU_TEXT_DISABLED
            },
        );
        if !shortcut.is_empty() {
            let sc_color = if !enabled {
                MENU_TEXT_DISABLED
            } else if selected {
                TEXT_CYAN
            } else {
                TEXT_DIM_CYAN
            };
            p.text(
                Pos2::new(rect.max.x - 10.0, rect.center().y),
                Align2::RIGHT_CENTER,
                shortcut,
                FontId::monospace(11.5),
                sc_color,
            );
        }
    }

    // --- Status Bar & Multi-Buffer Strip ---

    /// メニューバー右端の [最前面] [透過 n%] 表示。クリックで切替、透過表示は右クリックで濃くする。
    pub(super) fn paint_window_style_controls(
        ui: &mut egui::Ui,
        opacity: u8,
        text_opacity: u8,
        always_on_top: bool,
    ) -> Option<MenuAction> {
        let mut cmd = None;
        let top_resp = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(if always_on_top { "TOP" } else { "top" })
                        .font(FontId::monospace(12.5))
                        .strong()
                        .color(if always_on_top {
                            TEXT_AMBER
                        } else {
                            TEXT_DIM_CYAN
                        }),
                )
                .fill(BG_TAB_INACTIVE)
                .stroke(Stroke::new(1.0_f32, BORDER_BLUE)),
            )
            .on_hover_text("最前面固定の切替 (Alt+T)");
        if top_resp.clicked() {
            cmd = Some(MenuAction::ToggleAlwaysOnTop);
        }
        let op_resp = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(format!("背{:>3}%", opacity))
                        .font(FontId::monospace(12.5))
                        .color(if opacity < window_style::MAX_OPACITY {
                            TEXT_AMBER
                        } else {
                            TEXT_DIM_CYAN
                        }),
                )
                .fill(BG_TAB_INACTIVE)
                .stroke(Stroke::new(1.0_f32, BORDER_BLUE)),
            )
            .on_hover_text("背景の不透明度: クリックで薄く / 右クリック・Shift+クリックで濃く (Alt+↓ / Alt+↑ / Alt+0)");
        // Shift+クリックは右クリックと同じく濃くする
        let shift = ui.input(|i| i.modifiers.shift);
        if op_resp.secondary_clicked() || (shift && op_resp.clicked()) {
            cmd = Some(MenuAction::OpacityUp);
        } else if op_resp.clicked() {
            cmd = Some(if opacity <= window_style::MIN_OPACITY {
                MenuAction::OpacityReset
            } else {
                MenuAction::OpacityDown
            });
        }
        let text_resp = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(format!("字{:>3}%", text_opacity))
                        .font(FontId::monospace(12.5))
                        .color(if text_opacity < window_style::MAX_OPACITY {
                            TEXT_AMBER
                        } else {
                            TEXT_DIM_CYAN
                        }),
                )
                .fill(BG_TAB_INACTIVE)
                .stroke(Stroke::new(1.0_f32, BORDER_BLUE)),
            )
            .on_hover_text(
                "文字の不透明度: クリックで薄く / 右クリック・Shift+クリックで濃く (Alt+Shift+- / Alt+Shift++ / Alt+Shift+0)",
            );
        if text_resp.secondary_clicked() || (shift && text_resp.clicked()) {
            cmd = Some(MenuAction::TextOpacityUp);
        } else if text_resp.clicked() {
            cmd = Some(if text_opacity <= window_style::MIN_OPACITY {
                MenuAction::TextOpacityReset
            } else {
                MenuAction::TextOpacityDown
            });
        }
        cmd
    }

    pub(super) fn paint_font_size_controls(ui: &mut egui::Ui, font_size: f32) -> Option<i8> {
        // Returns Some(-1) for shrink, Some(0) for reset, Some(1) for enlarge.
        // Rendered inside a right_to_left layout: add '+', then '16pt', then '-' so visually: [-] 16pt [+]
        let mut cmd: Option<i8> = None;
        let is_default = (font_size - DEFAULT_FONT_SIZE).abs() < 0.1;

        let plus_resp = ui
            .add(
                egui::Button::new(
                    egui::RichText::new("+")
                        .font(FontId::monospace(12.5))
                        .strong()
                        .color(if font_size < MAX_FONT_SIZE {
                            TEXT_CYAN
                        } else {
                            TEXT_DIM_CYAN
                        }),
                )
                .fill(BG_TAB_INACTIVE)
                .stroke(Stroke::new(1.0_f32, BORDER_BLUE))
                .corner_radius(CornerRadius::same(2))
                .min_size(Vec2::new(18.0, 18.0)),
            )
            .on_hover_text("フォント拡大 (Ctrl++ / Ctrl+=)");
        if plus_resp.clicked() {
            cmd = Some(1);
        }

        let pt_label = format!("{}pt", font_size.round() as i32);
        let pt_resp = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(pt_label)
                        .font(FontId::monospace(12.5))
                        .strong()
                        .color(if is_default { TEXT_WHITE } else { TEXT_AMBER }),
                )
                .fill(BG_TAB_INACTIVE)
                .stroke(Stroke::new(
                    1.0_f32,
                    if is_default { BORDER_BLUE } else { TEXT_AMBER },
                ))
                .corner_radius(CornerRadius::same(2))
                .min_size(Vec2::new(40.0, 18.0)),
            )
            .on_hover_text("クリックでデフォルトサイズ (16pt) へリセット (Ctrl+0)");
        if pt_resp.clicked() {
            cmd = Some(0);
        }

        let minus_resp = ui
            .add(
                egui::Button::new(
                    egui::RichText::new("-")
                        .font(FontId::monospace(12.5))
                        .strong()
                        .color(if font_size > MIN_FONT_SIZE {
                            TEXT_CYAN
                        } else {
                            TEXT_DIM_CYAN
                        }),
                )
                .fill(BG_TAB_INACTIVE)
                .stroke(Stroke::new(1.0_f32, BORDER_BLUE))
                .corner_radius(CornerRadius::same(2))
                .min_size(Vec2::new(18.0, 18.0)),
            )
            .on_hover_text("フォント縮小 (Ctrl+-)");
        if minus_resp.clicked() {
            cmd = Some(-1);
        }

        cmd
    }
}

/// 履歴のサブメニューに出す件数（アクセスキー 0-9・A-Z の数）。
const RECENT_MENU_MAX: usize = 36;

/// 履歴の行の印の幅を空けた表示文字列。
fn recent_row_text(label: &str) -> String {
    format!("  {label}")
}

/// 履歴の `idx` 番目のアクセスキー（0-9、続けて A-Z）。
fn recent_access_key(idx: usize) -> char {
    match idx {
        0..=9 => char::from(b'0' + idx as u8),
        _ => char::from(b'A' + (idx - 10) as u8),
    }
}

/// サブメニューで押されたキーに当たる履歴の項目。
fn recent_action_for_key(items: &[MenuEntry], key: Key) -> Option<MenuAction> {
    let mut chars = key.name().chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    if !c.is_ascii_alphanumeric() {
        return None;
    }
    items.iter().find_map(|e| match e {
        MenuEntry::Recent { label, action, .. } if label.starts_with(c) => Some(*action),
        _ => None,
    })
}
