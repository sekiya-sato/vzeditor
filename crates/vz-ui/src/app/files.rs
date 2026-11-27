//! Markdown ビューとファイラーの切替、バッファの追加・切替、開く・保存・閉じる・終了。

use super::*;

enum SaveProgress {
    Saved,
    WaitingForHugeSave,
}

impl VzEditorApp {
    fn save_buffer(
        &mut self,
        idx: usize,
        path: &Path,
        encoding: TextEncoding,
    ) -> std::io::Result<SaveProgress> {
        if self.buffers[idx].buffer.is_huge() {
            // ハードリンクの保存確認は表示中のバッファを対象とする。
            debug_assert_eq!(idx, self.active_buf);
            self.request_huge_save(path.to_path_buf(), encoding);
            self.record_file_history(path, encoding);
            return Ok(SaveProgress::WaitingForHugeSave);
        }
        self.buffers[idx].buffer.save_to_path_as(path, encoding)?;
        self.record_file_history(path, encoding);
        Ok(SaveProgress::Saved)
    }

    // --- File / Folder History ---

    /// 履歴に記録する（状態の復元中は記録しない）。共通の履歴ファイルを読み直して書き戻し、
    /// 書けなければこのプロセスの履歴にだけ加えてステータスに出す。
    pub(super) fn record_history(&mut self, visits: &[Visit]) {
        if self.history_paused {
            return;
        }
        if let Some(file) = &self.history_file {
            match History::record(file, visits) {
                Ok(history) => {
                    self.history = history;
                    return;
                }
                Err(e) => self.status_message = format!("履歴の保存に失敗しました: {e}"),
            }
        }
        for &visit in visits {
            self.history.apply(visit);
        }
    }

    /// ファイルと、それを含むフォルダを履歴に記録する。
    pub(super) fn record_file_history(&mut self, path: &Path, encoding: TextEncoding) {
        let mut visits = vec![Visit::File(path, encoding.label())];
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            visits.push(Visit::Folder(dir));
        }
        self.record_history(&visits);
    }

    /// 他のプロセスが記録した分を含めて読み直す（ファイルメニューを開いたとき）。
    pub(super) fn reload_history(&mut self) {
        if let Some(file) = &self.history_file {
            self.history = History::load_from(file);
        }
    }

    /// ファイラーの表示中の窓でフォルダへ入り、履歴に記録する。
    pub(super) fn filer_enter_dir(&mut self, dir: PathBuf) -> std::io::Result<()> {
        self.filer.active_pane_mut().enter_dir(dir)?;
        let current = self.filer.active_pane().current_dir.clone();
        self.record_history(&[Visit::Folder(&current)]);
        Ok(())
    }

    /// ファイラーの表示中の窓で親フォルダへ移り、履歴に記録する。失敗はステータスに出す。
    pub(super) fn filer_go_parent(&mut self) {
        match self.filer.active_pane_mut().go_parent() {
            Ok(true) => {
                let current = self.filer.active_pane().current_dir.clone();
                self.record_history(&[Visit::Folder(&current)]);
            }
            Ok(false) => {}
            Err(e) => self.status_message = format!("移動エラー: {e}"),
        }
    }

    /// ファイル→最近使ったファイル。
    pub(super) fn open_recent_file(&mut self, idx: usize) {
        let Some(path) = self.history.files.get(idx).map(|f| f.path.clone()) else {
            return;
        };
        if !path.is_file() {
            self.status_message = format!("ファイルが見つかりません: {}", path.display());
            return;
        }
        self.open_file_in_buffer(path);
    }

    /// ファイル→最近使ったフォルダー。ファイラーでそのフォルダを開く。
    pub(super) fn open_recent_folder(&mut self, idx: usize) {
        let Some(dir) = self.history.folders.get(idx).cloned() else {
            return;
        };
        self.open_folder_in_filer(dir);
    }

    fn open_folder_in_filer(&mut self, dir: PathBuf) {
        match self.filer_enter_dir(dir.clone()) {
            Ok(()) => {
                self.enter_filer();
                self.status_message = format!("フォルダを開きました: {}", dir.display());
            }
            Err(e) => self.status_message = format!("フォルダを開けません: {e}"),
        }
    }

    pub(super) fn toggle_markdown_view(&mut self) {
        if self.cur_buf().is_huge() {
            self.status_message = "巨大ファイルでは Markdown ビューを使えません".into();
            return;
        }
        self.prefix = PrefixKey::None;
        self.ime_preedit.clear();
        let view = &mut self.cur_slot_mut().markdown;
        view.visible = !view.visible;
        let now_view = view.visible;
        if now_view {
            view.scroll_dirty = true;
            self.status_message = "Markdown ビューモード [VIEW] (Alt+P で編集へ戻る)".to_string();
        } else {
            self.need_scroll_to_cursor = true;
            self.status_message = "テキスト編集モード [EDIT] (Alt+P でプレビューへ)".to_string();
        }
    }

    pub(super) fn scroll_markdown_by(&mut self, delta_y: f32) {
        let view = &mut self.cur_slot_mut().markdown;
        view.scroll_y = (view.scroll_y + delta_y).clamp(0.0, view.max_scroll_y);
        view.scroll_dirty = true;
    }

    pub(super) fn scroll_markdown_to_top(&mut self) {
        let view = &mut self.cur_slot_mut().markdown;
        view.scroll_y = 0.0;
        view.scroll_dirty = true;
    }

    pub(super) fn scroll_markdown_to_bottom(&mut self) {
        let view = &mut self.cur_slot_mut().markdown;
        view.scroll_y = view.max_scroll_y;
        view.scroll_dirty = true;
    }

    // --- Mode Switching ---

    pub(super) fn enter_filer(&mut self) {
        self.prefix = PrefixKey::None;
        self.ime_preedit.clear();
        self.show_help = false;
        self.filer.refresh_both();
        self.mode = AppMode::Filer;
        self.status_message =
            "2画面ファイラー (Tab/←→:窓切替  Space:マーク  Enter:開く  ESC:エディタ戻)".to_string();
    }

    pub(super) fn enter_editor(&mut self) {
        self.prefix = PrefixKey::None;
        self.show_help = false;
        self.filer_modal = None;
        self.mode = AppMode::Editor;
        self.need_scroll_to_cursor = true;
        self.mark_md_scroll_dirty();
        self.status_message = format!(
            "エディタ [{}/{}]: {}{}",
            self.active_buf + 1,
            self.buffers.len(),
            self.cur_buf().display_name(),
            self.view_tag()
        );
    }

    /// 表示中のバッファが Markdown ビューなら、スクロール位置の再計算を要求する。
    pub(super) fn mark_md_scroll_dirty(&mut self) {
        let slot = self.cur_slot_mut();
        if slot.markdown.visible {
            slot.markdown.scroll_dirty = true;
        }
    }

    /// 表示中のバッファが Markdown ビューなら " [VIEW]"。
    pub(super) fn view_tag(&self) -> &'static str {
        if self.cur_slot().markdown.visible {
            " [VIEW]"
        } else {
            ""
        }
    }

    // --- Multi-Buffer Management ---

    /// 未編集の [無題] だけなら置き換え、それ以外は末尾に追加してアクティブにする。
    pub(super) fn add_buffer(&mut self, buf: TextBuffer) {
        self.add_buffer_slot(BufferSlot::new(buf));
    }

    pub(super) fn add_buffer_slot(&mut self, slot: BufferSlot) {
        if self.buffers.len() == 1 && self.buffers[0].is_untouched_untitled() {
            self.buffers[0] = slot;
            self.active_buf = 0;
        } else {
            self.buffers.push(slot);
            self.active_buf = self.buffers.len() - 1;
        }
        self.mode = AppMode::Editor;
        self.need_scroll_to_cursor = true;
    }

    /// 複数のファイル・フォルダを開く（ドラッグ＆ドロップ、起動引数、開くダイアログ、ファイラーのマーク）。
    ///
    /// - ファイルはそれぞれバッファで開き、最初に開いたファイルを表示する。既に開いていれば切り替えるだけ。
    /// - フォルダだけが渡された場合は、ファイラーでそのフォルダへ移動する。
    /// - `create_missing` が true なら存在しないパスを新規ファイルとして開く。
    pub(super) fn open_paths(&mut self, paths: Vec<PathBuf>, create_missing: bool) {
        let mut first_opened: Option<usize> = None;
        let mut opened = 0usize;
        let mut failed: Vec<String> = Vec::new();
        let mut first_dir: Option<PathBuf> = None;

        for path in paths {
            if path.is_dir() {
                if first_dir.is_none() {
                    first_dir = Some(path);
                }
                continue;
            }
            let ok = if !path.exists() && create_missing {
                let mut buf = TextBuffer::new();
                buf.file_path = Some(path.clone());
                self.add_buffer(buf);
                self.status_message = format!("新規ファイル: {}", path.display());
                true
            } else {
                self.open_file_in_buffer(path.clone())
            };
            if ok {
                opened += 1;
                first_opened.get_or_insert(self.active_buf);
            } else {
                failed.push(
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string()),
                );
            }
        }

        if let Some(idx) = first_opened {
            self.switch_buffer_to(idx);
            self.mode = AppMode::Editor;
            if opened > 1 || !failed.is_empty() {
                let mut msg = format!(
                    "{opened} 個のファイルを開きました [{}/{}]: {}",
                    self.active_buf + 1,
                    self.buffers.len(),
                    self.cur_buf().display_name()
                );
                if !failed.is_empty() {
                    msg.push_str(&format!("  (開けなかったファイル: {})", failed.join(", ")));
                }
                self.status_message = msg;
            }
        } else if let Some(dir) = first_dir {
            self.open_folder_in_filer(dir);
        } else if !failed.is_empty() {
            self.status_message = format!("開けなかったファイル: {}", failed.join(", "));
        }
    }

    /// ドロップされたファイルを開く。
    pub(super) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let paths: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if paths.is_empty() {
            return;
        }
        if self.filer_modal.is_some()
            || self.search.is_open()
            || self.inline_prompt.is_some()
            || self.pending_dialog.is_some()
            || self.quit_after_save
        {
            self.status_message = "ダイアログを閉じてからドロップしてください".to_string();
            return;
        }
        self.open_menu = None;
        self.show_help = false;
        self.show_about = false;
        self.prefix = PrefixKey::None;
        self.open_paths(paths, false);
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// ファイルをウィンドウ上にドラッグしている間の案内表示。
    pub(super) fn render_drop_overlay(&self, ctx: &egui::Context) {
        let count = ctx.input(|i| i.raw.hovered_files.len());
        if count == 0 {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("vz_drop_overlay"),
        ));
        let rect = ctx.screen_rect();
        painter.rect_filled(
            rect,
            CornerRadius::ZERO,
            Color32::from_rgba_unmultiplied(8, 24, 56, 210),
        );
        painter.rect_stroke(
            rect.shrink(10.0),
            CornerRadius::same(6),
            Stroke::new(3.0_f32, TEXT_CYAN),
            StrokeKind::Inside,
        );
        let text = if count > 1 {
            format!("ドロップして {count} 個のファイルを開く")
        } else {
            "ドロップしてファイルを開く".to_string()
        };
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::monospace(22.0),
            TEXT_CYAN,
        );
    }

    /// `path` を開いているバッファ（無題を除く）。
    pub(super) fn find_buffer_by_path(&self, path: &Path) -> Option<usize> {
        let target = canonical_or_self(path);
        self.buffers.iter().position(|slot| {
            slot.buffer
                .file_path
                .as_deref()
                .is_some_and(|p| same_file(p, path, &target))
        })
    }

    /// ファイルをバッファで開く。開けた（または既存バッファへ切り替えた）ら true。
    pub(super) fn open_file_in_buffer(&mut self, path: PathBuf) -> bool {
        // If already open in an existing buffer, switch directly to it.
        if let Some(existing_idx) = self.find_buffer_by_path(&path) {
            let encoding = self.buffers[existing_idx].buffer.encoding;
            self.record_file_history(&path, encoding);
            self.active_buf = existing_idx;
            self.mode = AppMode::Editor;
            self.need_scroll_to_cursor = true;
            self.mark_md_scroll_dirty();
            self.status_message = format!(
                "既存バッファへ切替 [{}/{}]: {}",
                self.active_buf + 1,
                self.buffers.len(),
                self.cur_buf().display_name()
            );
            return true;
        }

        match TextBuffer::from_file(&path) {
            Ok(buf) => {
                let hint = if buf.is_markdown() {
                    " (^PでMDビュー切替)"
                } else {
                    ""
                };
                self.record_file_history(&path, buf.encoding);
                self.add_buffer(buf);
                self.status_message = format!(
                    "開きました [{}/{}]: {}{hint}",
                    self.active_buf + 1,
                    self.buffers.len(),
                    path.display()
                );
                true
            }
            Err(e) => {
                self.status_message = format!("読み込みエラー: {e}");
                false
            }
        }
    }

    pub(super) fn new_empty_buffer(&mut self) {
        self.buffers.push(BufferSlot::new(TextBuffer::new()));
        self.active_buf = self.buffers.len() - 1;
        self.mode = AppMode::Editor;
        self.need_scroll_to_cursor = true;
        self.status_message = format!(
            "新規バッファ作成 [{}/{}]",
            self.active_buf + 1,
            self.buffers.len()
        );
    }

    pub(super) fn switch_buffer_step(&mut self, delta: isize) {
        let count = self.buffers.len();
        if count <= 1 {
            self.status_message =
                "開いているバッファは1件です（ESCファイラーから複数ファイルを開けます）"
                    .to_string();
            return;
        }
        let next = ((self.active_buf as isize + delta).rem_euclid(count as isize)) as usize;
        self.switch_buffer_to(next);
    }

    pub(super) fn switch_buffer_to(&mut self, idx: usize) {
        if idx < self.buffers.len() {
            self.active_buf = idx;
            self.prefix = PrefixKey::None;
            self.ime_preedit.clear();
            self.need_scroll_to_cursor = true;
            self.mark_md_scroll_dirty();
            self.status_message = format!(
                "バッファ切替 [{}/{}]: {}{}",
                idx + 1,
                self.buffers.len(),
                self.cur_buf().display_name(),
                self.view_tag()
            );
        }
    }

    pub(super) fn update_open_buffer_path(&mut self, old_path: &Path, new_path: &Path) {
        let canon_old = canonical_or_self(old_path);
        for slot in &mut self.buffers {
            if slot
                .buffer
                .file_path
                .as_deref()
                .is_some_and(|p| same_file(p, old_path, &canon_old))
            {
                slot.buffer.file_path = Some(new_path.to_path_buf());
            }
        }
    }

    // --- Native Dialogs (Editor) ---

    pub(super) fn poll_dialog(&mut self) {
        let result = match &self.pending_dialog {
            Some(rx) => match rx.try_recv() {
                Ok(res) => Some(res),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.pending_dialog = None;
                    None
                }
            },
            None => None,
        };

        if let Some(res) = result {
            self.pending_dialog = None;
            match res {
                DialogResult::SavePath(Some(path)) => self.perform_save(path),
                DialogResult::SavePath(None) => {
                    self.status_message = if self.quit_after_save {
                        "保存をキャンセルしたため終了を中止しました".to_string()
                    } else {
                        "保存をキャンセルしました".to_string()
                    };
                    self.quit_after_save = false;
                }
                DialogResult::OpenPaths(paths) if !paths.is_empty() => {
                    self.open_paths(paths, false)
                }
                DialogResult::OpenPaths(_) => {
                    self.status_message = "ファイルを開くのをキャンセルしました".to_string();
                }
                DialogResult::Unavailable { is_save } => {
                    let initial = self
                        .cur_buf()
                        .file_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| {
                            std::env::current_dir()
                                .map(|d| d.join("untitled.txt").display().to_string())
                                .unwrap_or_else(|_| "untitled.txt".to_string())
                        });
                    self.inline_prompt = Some(InlinePathPrompt {
                        is_save,
                        path_input: initial,
                        just_opened: true,
                    });
                }
            }
        }
    }

    // --- Dynamic Window Title ---

    pub(super) fn compute_window_title(&self) -> String {
        match self.mode {
            AppMode::Editor => {
                let buf = self.cur_buf();
                let dirty_prefix = if buf.dirty { "*" } else { "" };
                format!("{}{} - VZ Editor", dirty_prefix, buf.display_name())
            }
            AppMode::Filer => {
                let dir = self.filer.active_pane().current_dir.display();
                format!("[2画面ファイラー] {} - VZ Editor", dir)
            }
            AppMode::Terminal => {
                let name = self
                    .terminal
                    .as_ref()
                    .map(|t| t.title().unwrap_or(t.program()).to_string())
                    .unwrap_or_default();
                format!("[ターミナル] {name} - VZ Editor")
            }
            AppMode::Browser => {
                let name = self
                    .active_browser_tab()
                    .map(|t| t.label())
                    .unwrap_or_default();
                format!("[ブラウザ] {name} - VZ Editor")
            }
        }
    }

    pub(super) fn sync_window_title(&mut self, ctx: &egui::Context) {
        let title = self.compute_window_title();
        if title != self.last_window_title {
            self.last_window_title = title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
    }

    pub(super) fn perform_save(&mut self, path: PathBuf) {
        let encoding = self.cur_buf().encoding;
        match self.save_buffer(self.active_buf, &path, encoding) {
            Ok(SaveProgress::Saved) => {
                self.status_message = format!("保存しました: {}", path.display());
            }
            Ok(SaveProgress::WaitingForHugeSave) => {}
            Err(e) => {
                self.status_message = format!("保存エラー: {e}");
                self.quit_after_save = false;
            }
        }
    }

    /// 文字コードを選んで保存するダイアログを開く（ファイル→文字コードを指定して保存、ステータスバーの文字コード）。
    pub(super) fn open_encoding_dialog(&mut self) {
        if self.mode != AppMode::Editor || self.filer_modal.is_some() {
            return;
        }
        self.filer_modal = Some(FilerModal::ChooseEncoding {
            selected: self.cur_buf().encoding,
            message: String::new(),
        });
    }

    /// 選んだ文字コードで保存する。無題なら文字コードを決めて保存先を尋ねる。失敗したら理由を返す。
    pub(super) fn save_with_encoding(&mut self, encoding: TextEncoding) -> Result<(), String> {
        let Some(path) = self.cur_buf().file_path.clone() else {
            self.cur_buf_mut().encoding = encoding;
            self.trigger_save();
            return Ok(());
        };
        let progress = self
            .save_buffer(self.active_buf, &path, encoding)
            .map_err(|e| e.to_string())?;
        if matches!(progress, SaveProgress::Saved) {
            self.status_message = format!("{encoding} で保存しました: {}", path.display());
        }
        Ok(())
    }

    pub(super) fn trigger_save(&mut self) {
        if let Some(path) = self.cur_buf().file_path.clone() {
            self.perform_save(path);
        } else if self.pending_dialog.is_none() {
            self.status_message = "保存先を選択中...".to_string();
            self.pending_dialog = Some(spawn_save_dialog(None));
        }
    }

    pub(super) fn trigger_save_as(&mut self) {
        if self.pending_dialog.is_none() {
            self.status_message = "名前を付けて保存先を選択中...".to_string();
            let current = self.cur_buf().file_path.clone();
            self.pending_dialog = Some(spawn_save_dialog(current.as_deref()));
        }
    }

    pub(super) fn trigger_open(&mut self) {
        if self.pending_dialog.is_none() {
            self.status_message = "開くファイルを選択中...".to_string();
            let dir = self
                .cur_buf()
                .file_path
                .as_ref()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf());
            self.pending_dialog = Some(spawn_open_dialog(dir.as_deref()));
        }
    }

    pub(super) fn close_current_buffer(&mut self) {
        let idx = self.active_buf;
        if self.buffers[idx].buffer.dirty {
            let name = self.buffers[idx].buffer.display_name();
            self.filer_modal = Some(FilerModal::ConfirmCloseBuffer { buf_idx: idx, name });
        } else {
            self.force_close_buffer(idx);
        }
    }

    pub(super) fn force_close_buffer(&mut self, idx: usize) {
        if idx >= self.buffers.len() {
            return;
        }
        let closed_name = self.buffers[idx].buffer.display_name();
        self.prefix = PrefixKey::None;
        self.ime_preedit.clear();
        if self.buffers.len() <= 1 {
            self.buffers[0] = BufferSlot::new(TextBuffer::new());
            self.active_buf = 0;
            self.need_scroll_to_cursor = true;
            self.status_message = format!("バッファを閉じました ({closed_name}) -> 新規 [無題]");
        } else {
            self.buffers.remove(idx);
            if self.active_buf >= self.buffers.len() {
                self.active_buf = self.buffers.len() - 1;
            }
            self.need_scroll_to_cursor = true;
            self.mark_md_scroll_dirty();
            let next_name = self.cur_buf().display_name();
            self.status_message = format!(
                "バッファを閉じました ({closed_name}) -> [{}/{}] {next_name}",
                self.active_buf + 1,
                self.buffers.len()
            );
        }
    }

    /// F10・メニュー「終了」・ウィンドウの×ボタン / Alt+F4 / Cmd+Q からの終了要求。
    /// 未保存（新規バッファへの入力を含む）があれば確認ダイアログを出す。
    pub(super) fn request_quit(&mut self, ctx: &egui::Context) {
        if self.has_any_dirty_buffer() {
            self.open_quit_confirm();
        } else {
            self.close_now(ctx);
        }
    }

    pub(super) fn open_quit_confirm(&mut self) {
        self.search.close();
        self.quit_after_save = false;
        self.open_menu = None;
        self.show_about = false;
        self.show_help = false;
        self.inline_prompt = None;
        self.filer_modal = Some(FilerModal::ConfirmQuit);
    }

    /// 確認済みとしてウィンドウを閉じる（close_requested を再度止めない）。
    pub(super) fn close_now(&mut self, ctx: &egui::Context) {
        self.allow_close = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// OS からの終了要求（×ボタン・Alt+F4・Cmd+Q）を横取りして確認する。
    pub(super) fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.allow_close {
            return;
        }
        // Alt+Shift+F4 は OS の閉じる要求として届くことがある（Windows）。未保存の確認より先に見る
        if restore::close_by_alt_shift_f4(ctx) {
            self.save_state_and_quit(ctx);
            return;
        }
        if !self.has_any_dirty_buffer() {
            self.allow_close = true;
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        if self.pending_dialog.is_some() {
            self.status_message = "保存ダイアログを閉じてから終了してください".to_string();
        } else if !matches!(self.filer_modal, Some(FilerModal::ConfirmQuit)) {
            self.open_quit_confirm();
        }
    }

    /// 「保存して終了」: パスのある未保存バッファを保存し、無題のバッファは順に保存先を尋ねる。
    pub(super) fn save_dirty_buffers_for_quit(&mut self) -> QuitSaveStep {
        if self.buffers.iter().any(|slot| slot.huge.is_saving()) {
            return QuitSaveStep::WaitingForHugeSave;
        }
        for idx in 0..self.buffers.len() {
            if !self.buffers[idx].buffer.dirty {
                continue;
            }
            let Some(path) = self.buffers[idx].buffer.file_path.clone() else {
                continue;
            };
            if self.buffers[idx].buffer.is_huge() {
                self.switch_buffer_to(idx);
            }
            let encoding = self.buffers[idx].buffer.encoding;
            match self.save_buffer(idx, &path, encoding) {
                Ok(SaveProgress::Saved) => {}
                Ok(SaveProgress::WaitingForHugeSave) => return QuitSaveStep::WaitingForHugeSave,
                Err(e) => {
                    self.switch_buffer_to(idx);
                    self.status_message = format!("保存エラーのため終了を中止しました: {e}");
                    return QuitSaveStep::Failed;
                }
            }
        }
        match self.buffers.iter().position(|s| s.buffer.dirty) {
            Some(idx) => QuitSaveStep::NeedSaveAs(idx),
            None => QuitSaveStep::Done,
        }
    }

    pub(super) fn continue_save_and_quit(&mut self, ctx: &egui::Context) {
        if !self.quit_after_save
            || self.pending_dialog.is_some()
            || self.inline_prompt.is_some()
            || self.filer_modal.is_some()
        {
            return;
        }
        match self.save_dirty_buffers_for_quit() {
            QuitSaveStep::Done => {
                self.quit_after_save = false;
                self.close_now(ctx);
            }
            QuitSaveStep::NeedSaveAs(idx) => {
                if self.mode != AppMode::Editor {
                    self.enter_editor();
                }
                self.switch_buffer_to(idx);
                self.trigger_save_as();
                self.status_message = format!(
                    "終了前に保存: {} の保存先を選択中...",
                    self.cur_buf().display_name()
                );
            }
            QuitSaveStep::Failed => {
                self.quit_after_save = false;
            }
            QuitSaveStep::WaitingForHugeSave => {}
        }
    }

    // --- 折り返し表示（Alt+W） ---
}
