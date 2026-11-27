//! 利用者向けの状態の記録・復元と、リリースでも使うセッション処理。

use super::*;
use std::time::{Duration, UNIX_EPOCH};

pub(super) fn stamp_to_record(stamp: Option<FileStamp>) -> Option<(u64, u64)> {
    let (time, len) = stamp?;
    let ns = time.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    Some((u64::try_from(ns).ok()?, len))
}

pub(super) fn stamp_from_record(stamp: Option<(u64, u64)>) -> Option<FileStamp> {
    stamp.map(|(ns, len)| (UNIX_EPOCH + Duration::from_nanos(ns), len))
}

impl VzEditorApp {
    /// Alt+Shift+F4: 状態を `session/saved/<pid>` に記録して終了する（ファイルは上書きしない）。
    /// 記録できなければ終了を中止する。
    pub(super) fn save_state_and_quit(&mut self, ctx: &egui::Context) {
        // 同じフレームで閉じる要求とキーの両方から呼ばれても、記録は 1 回だけ
        if self.allow_close {
            return;
        }
        let result = session::session_dir()
            .ok_or_else(|| std::io::Error::other("設定フォルダがありません"))
            .and_then(|d| self.save_state_to(&session::saved_dir(&d)));
        match result {
            Ok(_) => self.close_now(ctx),
            Err(e) => {
                if ctx.input(|i| i.viewport().close_requested()) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
                self.status_message = format!("状態を記録できないため終了を中止しました: {e}");
            }
        }
    }

    /// Alt+Shift+F4 のキー。モーダル表示中も受け付けるよう、ほかのキー処理より先に取り出す。
    pub(super) fn handle_save_state_key(&mut self, ctx: &egui::Context) {
        let mut hit = false;
        ctx.input_mut(|i| {
            i.events.retain(|ev| {
                let Event::Key {
                    key,
                    physical_key,
                    pressed,
                    modifiers,
                    ..
                } = ev
                else {
                    return true;
                };
                if window_style_key_action(*key, *physical_key, *modifiers)
                    != Some(MenuAction::SaveStateAndQuit)
                {
                    return true;
                }
                hit |= *pressed;
                false
            });
        });
        if hit {
            self.save_state_and_quit(ctx);
        }
    }

    /// 起動時の Shift を OS から読めない環境（Linux）: 起動直後のフレームで Shift が押されていれば記録を戻す。
    pub(super) fn poll_shift_restore(&mut self, ctx: &egui::Context) {
        let Some(until) = self.shift_restore_until else {
            return;
        };
        if std::time::Instant::now() >= until {
            self.shift_restore_until = None;
            return;
        }
        if ctx.input(|i| i.modifiers.shift) {
            self.shift_restore_until = None;
            if let Some(dir) = session::session_dir() {
                self.restore_saved_state_from(&session::saved_dir(&dir));
                self.sync_window_title(ctx);
                // 位置の合わせ直しは次のフレームで行う
                ctx.request_repaint();
            }
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    /// `saved` に自分の状態を書いて公開する。公開したフォルダを返す。
    pub(super) fn save_state_to(&mut self, saved: &Path) -> std::io::Result<PathBuf> {
        let writing = session::begin_saved_state(saved)?;
        let published = self
            .save_session(&writing)
            .and_then(|()| session::publish_saved_state(saved, &writing, std::process::id()));
        if published.is_err() {
            let _ = fs::remove_dir_all(&writing);
        }
        published
    }

    /// Shift を押して起動したとき: `saved` の記録を PID の小さい順に 1 つ戻す。無ければ何もしない。
    pub(super) fn restore_saved_state_from(&mut self, saved: &Path) {
        let Some(state) = session::take_saved_state(saved) else {
            return;
        };
        let failed = self.restore_entries(&state.entries);
        self.apply_session_window(state.window.clone());
        if let Err(e) = session::finish_saved_state(saved, &state, &failed) {
            self.status_message = format!("記録の後始末に失敗しました: {e}");
        }
    }

    /// 編集内容を一時保存し、バッファ・画面・ウィンドウの状態を記録する。
    pub(super) fn save_session(&self, dir: &Path) -> std::io::Result<()> {
        if self.buffers.iter().any(|slot| slot.huge.is_saving()) {
            return Err(std::io::Error::other("巨大ファイルを保存中です"));
        }
        if self.buffers.iter().any(|slot| slot.huge.is_replacing()) {
            return Err(std::io::Error::other(
                "巨大ファイルのすべて置換を実行中です",
            ));
        }
        fs::create_dir_all(dir)?;
        let mut entries = Vec::new();
        for idx in 0..self.buffers.len() {
            if self.buffers[idx].is_untouched_untitled() {
                continue;
            }
            let active = idx == self.active_buf;
            let slot = &self.buffers[idx];
            // 衝突の確認待ちの無題バッファは、次の復元でも確認する
            if let Some(origin) = &slot.conflict_of {
                let temp = session::temp_path(dir, idx);
                fs::write(&temp, slot.buffer.text_lines().join("\n"))?;
                entries.push(SessionEntry {
                    kind: EntryKind::Conflict,
                    line: slot.buffer.cursor_line,
                    col: slot.buffer.cursor_col,
                    markdown_view: false,
                    active,
                    crlf: slot.buffer.line_ending == "\r\n",
                    encoding: None,
                    disk_stamp: None,
                    temp: Some(temp),
                    path: Some(std::path::absolute(origin)?),
                });
                continue;
            }
            let buf = &slot.buffer;
            let mut entry = SessionEntry {
                kind: EntryKind::File,
                line: buf.cursor_line,
                col: buf.cursor_col,
                markdown_view: slot.markdown.visible,
                active,
                crlf: buf.line_ending == "\r\n",
                encoding: None,
                disk_stamp: None,
                temp: None,
                path: buf
                    .file_path
                    .as_deref()
                    .map(std::path::absolute)
                    .transpose()?,
            };
            if buf.is_huge() && buf.dirty {
                let temp = session::huge_edits_path(dir, idx);
                buf.write_huge_edits(&temp).map_err(|e| {
                    std::io::Error::other(format!(
                        "{}: 編集内容の一時保存: {e}",
                        buf.display_name()
                    ))
                })?;
                entry.kind = EntryKind::HugeEdits;
                entry.temp = Some(temp);
            } else if buf.dirty || buf.file_path.is_none() {
                let temp = session::temp_path(dir, idx);
                fs::write(&temp, buf.text_lines().join("\n"))?;
                entry.kind = EntryKind::Unsaved;
                entry.temp = Some(temp);
                // 元のファイルの文字コードと、読み込んだ時点の状態（外部変更の検知用）も戻す
                entry.encoding = Some(buf.encoding.label().to_string());
                entry.disk_stamp = stamp_to_record(buf.disk_stamp);
            }
            entries.push(entry);
        }
        // ターミナルは起動したフォルダ、ブラウザはタブごとに表示中の URL を残す（シェルの中身は戻せない）
        if let Some(term) = &self.terminal {
            entries.push(tool_entry(
                EntryKind::Terminal,
                self.mode == AppMode::Terminal,
                Some(term.cwd().to_path_buf()),
            ));
        }
        for (i, tab) in self.browsers.iter().enumerate() {
            let selected = i == self.active_browser;
            let mut entry = tool_entry(
                EntryKind::Browser,
                selected && self.mode == AppMode::Browser,
                tab.session_url().map(PathBuf::from),
            );
            // 選択中のタブ（ブラウザを表示していなくても残す）
            entry.line = usize::from(selected);
            entries.push(entry);
        }
        entries.push(tool_entry(
            EntryKind::FilerLeft,
            self.mode == AppMode::Filer,
            Some(self.filer.left.current_dir.clone()),
        ));
        entries.push(tool_entry(
            EntryKind::FilerRight,
            false,
            Some(self.filer.right.current_dir.clone()),
        ));
        session::write_session(dir, &entries, &self.capture_session_window())
    }

    /// 記録した項目を開き直す。戻せなかった項目（巨大ファイルの未保存の編集）を返す。
    pub(super) fn restore_entries(&mut self, entries: &[SessionEntry]) -> Vec<SessionEntry> {
        self.restore_entries_with(entries, false)
    }

    pub(super) fn restore_entries_with(
        &mut self,
        entries: &[SessionEntry],
        defer_cleanup: bool,
    ) -> Vec<SessionEntry> {
        let mut failed = Vec::new();
        if entries.is_empty() {
            return failed;
        }
        // 復元で開き直すファイルは、利用者が開いたものではないので履歴に記録しない
        self.history_paused = true;
        let mut active: Option<usize> = None;
        let mut restored = 0usize;
        let mut left_dir = None;
        let mut right_dir = None;
        // 表示していた画面（無ければエディター）
        let mut tool_mode: Option<AppMode> = None;
        // 戻せなかった巨大ファイルの未保存の編集（理由）
        let mut huge_errors: Vec<String> = Vec::new();
        for e in entries {
            let ok = match e.kind {
                EntryKind::Terminal => {
                    // シェルは最初のフレームで起動する（再描画の要求に egui の Context が要る）
                    if let Some(cwd) = e.path.clone().filter(|d| d.is_dir()) {
                        self.pending_terminal_cwd = Some(cwd);
                        if e.active {
                            tool_mode = Some(AppMode::Terminal);
                        }
                    } else if defer_cleanup {
                        failed.push(e.clone());
                    }
                    continue;
                }
                EntryKind::Browser => {
                    let url = e.path.as_ref().map(|u| u.to_string_lossy().into_owned());
                    self.browsers.push(browser_view::BrowserTab::new(url));
                    if e.line == 1 || e.active {
                        self.active_browser = self.browsers.len() - 1;
                    }
                    if e.active {
                        tool_mode = Some(AppMode::Browser);
                    }
                    continue;
                }
                EntryKind::FilerLeft => {
                    left_dir = Some(e);
                    if e.active {
                        tool_mode = Some(AppMode::Filer);
                    }
                    continue;
                }
                EntryKind::FilerRight => {
                    right_dir = Some(e);
                    continue;
                }
                EntryKind::File => {
                    let path = e.path.clone().unwrap_or_default();
                    let before = self.buffers.len();
                    self.open_paths(vec![path.clone()], true);
                    // 開けた（または既に開いていた）ときだけ active_buf がそのファイルを指す
                    self.cur_buf().file_path.as_deref() == Some(path.as_path())
                        || self.buffers.len() > before
                }
                EntryKind::Unsaved => {
                    let temp = e.temp.clone().unwrap_or_default();
                    match fs::read_to_string(&temp) {
                        Ok(text) => {
                            let mut buf = TextBuffer::from_lines(
                                text.split('\n').map(String::from).collect(),
                            );
                            buf.file_path = e.path.clone();
                            buf.line_ending = if e.crlf { "\r\n" } else { "\n" };
                            if let Some(enc) =
                                e.encoding.as_deref().and_then(TextEncoding::from_label)
                            {
                                buf.encoding = enc;
                            }
                            buf.disk_stamp = stamp_from_record(e.disk_stamp);
                            buf.dirty = true;
                            buf.revision = 1;
                            self.add_buffer(buf);
                            if !defer_cleanup {
                                let _ = fs::remove_file(&temp);
                            }
                            true
                        }
                        Err(_) => false,
                    }
                }
                EntryKind::HugeEdits => {
                    let temp = e.temp.clone().unwrap_or_default();
                    let path = e.path.clone().unwrap_or_default();
                    match TextBuffer::open_huge_with_edits(&path, &temp) {
                        Ok(buf) => {
                            self.add_buffer(buf);
                            if !defer_cleanup {
                                let _ = fs::remove_file(&temp);
                            }
                            true
                        }
                        Err(err) => {
                            // 戻せなければファイルだけを開き、一時保存はセッションのフォルダに残す
                            huge_errors.push(format!("{}: {err}", path.display()));
                            failed.push(e.clone());
                            let before = self.buffers.len();
                            self.open_paths(vec![path.clone()], true);
                            self.buffers.len() > before
                        }
                    }
                }
                EntryKind::Conflict => {
                    let temp = e.temp.clone().unwrap_or_default();
                    let origin = e.path.clone().unwrap_or_default();
                    match fs::read_to_string(&temp) {
                        Ok(text) => {
                            let mut buf = TextBuffer::from_lines(
                                text.split('\n').map(String::from).collect(),
                            );
                            buf.line_ending = if e.crlf { "\r\n" } else { "\n" };
                            buf.dirty = true;
                            buf.revision = 1;
                            self.add_buffer(buf);
                            self.cur_slot_mut().conflict_of = Some(origin.clone());
                            self.conflict_queue.push(origin);
                            if !defer_cleanup {
                                let _ = fs::remove_file(&temp);
                            }
                            true
                        }
                        Err(_) => false,
                    }
                }
            };
            if !ok {
                if defer_cleanup && e.kind != EntryKind::HugeEdits {
                    failed.push(e.clone());
                }
                continue;
            }
            restored += 1;
            let slot = self.cur_slot_mut();
            slot.buffer.set_cursor(e.line, e.col);
            slot.markdown.visible = e.markdown_view && slot.buffer.is_markdown();
            slot.markdown.scroll_dirty = true;
            if e.active {
                active = Some(self.active_buf);
            }
        }
        if let Some(idx) = active {
            self.switch_buffer_to(idx);
        }
        for (pane, entry) in [
            (&mut self.filer.left, left_dir),
            (&mut self.filer.right, right_dir),
        ] {
            if let Some(entry) = entry {
                let opened = entry
                    .path
                    .as_ref()
                    .is_some_and(|path| pane.enter_dir(path.clone()).is_ok());
                if !opened && defer_cleanup {
                    failed.push(entry.clone());
                }
            }
        }
        self.mode = tool_mode.unwrap_or(AppMode::Editor);
        self.return_mode = AppMode::Editor;
        self.need_scroll_to_cursor = true;
        let buffer_entries = entries
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    EntryKind::File
                        | EntryKind::Unsaved
                        | EntryKind::Conflict
                        | EntryKind::HugeEdits
                )
            })
            .count();
        self.status_message =
            format!("前回の状態を復元しました: {restored} / {buffer_entries} 個のバッファ");
        if !huge_errors.is_empty() {
            self.status_message = format!(
                "未保存の編集を戻せませんでした（一時保存は設定フォルダに残しました）: {}",
                huge_errors.join(" / ")
            );
        }
        self.history_paused = false;
        failed
    }
}

/// ファイラー・ターミナル・ブラウザの保存項目。
fn tool_entry(kind: EntryKind, active: bool, path: Option<PathBuf>) -> SessionEntry {
    SessionEntry {
        kind,
        line: 0,
        col: 0,
        markdown_view: false,
        active,
        crlf: false,
        encoding: None,
        disk_stamp: None,
        temp: None,
        path,
    }
}

/// OS の閉じる要求が Alt+Shift+F4 によるものか（Alt と Shift が押されている）。
/// WebView にフォーカスがあると egui にキーが届かないため、OS のキー状態も見る。
pub(super) fn close_by_alt_shift_f4(ctx: &egui::Context) -> bool {
    let m = ctx.input(|i| i.modifiers);
    (m.alt && m.shift) || vz_platform::keyboard::alt_shift_held() == Some(true)
}
