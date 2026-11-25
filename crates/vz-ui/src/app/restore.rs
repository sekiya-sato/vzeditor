//! 保存して終了・状態の復元（`--save-and-quit` / `--restore`）と、外部でのファイル変更の検知。

use super::*;

impl VzEditorApp {
    /// 外部からの終了要求を受け取っていれば、状態を保存して終了する。
    pub(super) fn poll_quit_request(&mut self, ctx: &egui::Context) {
        let requested = self
            .quit_request
            .as_ref()
            .is_some_and(|f| f.swap(false, Ordering::SeqCst));
        if !requested {
            return;
        }
        let Some(dir) = session::session_dir() else {
            return;
        };
        if let Err(e) = self.save_session_to(&dir) {
            // 保存できなければ終了しない（要求側には完了として返す）
            session::finish_request(&dir);
            self.status_message = format!("終了要求: 状態を保存できないため終了を中止しました: {e}");
            return;
        }
        session::finish_request(&dir);
        self.close_now(ctx);
    }

    /// 未保存のファイルを保存し、無題・保存できなかったバッファは `dir` に一時保存して、
    /// 開いているバッファとツールの状態を `dir/session.ini` に追記する。
    pub(super) fn save_session_to(&mut self, dir: &Path) -> std::io::Result<()> {
        fs::create_dir_all(dir)?;
        let mut entries = Vec::new();
        for idx in 0..self.buffers.len() {
            if self.buffers[idx].is_untouched_untitled() {
                continue;
            }
            let active = idx == self.active_buf;
            let slot = &mut self.buffers[idx];
            // 衝突の確認待ちの無題バッファは、次の復元でも確認する
            if let Some(origin) = slot.conflict_of.clone() {
                let temp = session::temp_path(dir, idx);
                fs::write(&temp, slot.buffer.lines.join("\n"))?;
                entries.push(SessionEntry {
                    kind: EntryKind::Conflict,
                    line: slot.buffer.cursor_line,
                    col: slot.buffer.cursor_col,
                    markdown_view: false,
                    active,
                    crlf: slot.buffer.line_ending == "\r\n",
                    temp: Some(temp),
                    path: Some(origin),
                });
                continue;
            }
            let buf = &mut slot.buffer;
            // 読み込み後に外部でファイルが変更されていたら上書きしない。
            // ファイルは外部の内容のまま開き直し、vz の編集内容は無題バッファに残して復元後に確認する
            if buf.dirty && buf.changed_on_disk().is_some() && !buf.same_as_disk() {
                let temp = session::temp_path(dir, idx);
                fs::write(&temp, buf.lines.join("\n"))?;
                entries.push(SessionEntry {
                    kind: EntryKind::File,
                    line: buf.cursor_line,
                    col: buf.cursor_col,
                    markdown_view: slot.markdown_view,
                    active,
                    crlf: buf.line_ending == "\r\n",
                    temp: None,
                    path: buf.file_path.clone(),
                });
                entries.push(SessionEntry {
                    kind: EntryKind::Conflict,
                    line: buf.cursor_line,
                    col: buf.cursor_col,
                    markdown_view: false,
                    active: false,
                    crlf: buf.line_ending == "\r\n",
                    temp: Some(temp),
                    path: buf.file_path.clone(),
                });
                continue;
            }
            let mut saved = !buf.dirty;
            if buf.dirty {
                if let Some(path) = buf.file_path.clone() {
                    saved = buf.save_to_path(&path).is_ok();
                }
            }
            let mut entry = SessionEntry {
                kind: EntryKind::File,
                line: buf.cursor_line,
                col: buf.cursor_col,
                markdown_view: slot.markdown_view,
                active,
                crlf: buf.line_ending == "\r\n",
                temp: None,
                path: buf.file_path.clone(),
            };
            if !saved || buf.file_path.is_none() {
                let temp = session::temp_path(dir, idx);
                fs::write(&temp, buf.lines.join("\n"))?;
                entry.kind = EntryKind::Unsaved;
                entry.temp = Some(temp);
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
        session::append_session(dir, &entries)
    }

    /// `dir/session.ini` のバッファを開き直す。読み終えた一時保存ファイルは削除する。
    pub(super) fn restore_session_from(&mut self, dir: &Path) {
        let entries = session::take_session(dir);
        if entries.is_empty() {
            return;
        }
        let mut active: Option<usize> = None;
        let mut restored = 0usize;
        let mut left_dir = None;
        let mut right_dir = None;
        // 表示していた画面（無ければエディター）
        let mut tool_mode: Option<AppMode> = None;
        for e in &entries {
            let ok = match e.kind {
                EntryKind::Terminal => {
                    // シェルは最初のフレームで起動する（再描画の要求に egui の Context が要る）
                    if let Some(cwd) = e.path.clone().filter(|d| d.is_dir()) {
                        self.pending_terminal_cwd = Some(cwd);
                        if e.active {
                            tool_mode = Some(AppMode::Terminal);
                        }
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
                    left_dir = e.path.as_ref();
                    if e.active {
                        tool_mode = Some(AppMode::Filer);
                    }
                    continue;
                }
                EntryKind::FilerRight => {
                    right_dir = e.path.as_ref();
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
                            let mut buf = TextBuffer::new();
                            buf.lines = text.split('\n').map(String::from).collect();
                            buf.file_path = e.path.clone();
                            buf.line_ending = if e.crlf { "\r\n" } else { "\n" };
                            buf.dirty = true;
                            buf.revision = 1;
                            self.add_buffer(buf);
                            let _ = fs::remove_file(&temp);
                            true
                        }
                        Err(_) => false,
                    }
                }
                EntryKind::Conflict => {
                    let temp = e.temp.clone().unwrap_or_default();
                    let origin = e.path.clone().unwrap_or_default();
                    match fs::read_to_string(&temp) {
                        Ok(text) => {
                            let mut buf = TextBuffer::new();
                            buf.lines = text.split('\n').map(String::from).collect();
                            buf.line_ending = if e.crlf { "\r\n" } else { "\n" };
                            buf.dirty = true;
                            buf.revision = 1;
                            self.add_buffer(buf);
                            self.cur_slot_mut().conflict_of = Some(origin.clone());
                            self.conflict_queue.push(origin);
                            let _ = fs::remove_file(&temp);
                            true
                        }
                        Err(_) => false,
                    }
                }
            };
            if !ok {
                continue;
            }
            restored += 1;
            let slot = self.cur_slot_mut();
            slot.buffer.set_cursor(e.line, e.col);
            slot.markdown_view = e.markdown_view && slot.buffer.is_markdown();
            slot.md_scroll_dirty = true;
            if e.active {
                active = Some(self.active_buf);
            }
        }
        if let Some(idx) = active {
            self.switch_buffer_to(idx);
        }
        if let Some(path) = left_dir {
            let _ = self.filer.left.enter_dir(path.clone());
        }
        if let Some(path) = right_dir {
            let _ = self.filer.right.enter_dir(path.clone());
        }
        self.mode = tool_mode.unwrap_or(AppMode::Editor);
        self.return_mode = AppMode::Editor;
        self.need_scroll_to_cursor = true;
        let buffer_entries = entries
            .iter()
            .filter(|e| matches!(e.kind, EntryKind::File | EntryKind::Unsaved | EntryKind::Conflict))
            .count();
        self.status_message =
            format!("前回の状態を復元しました: {restored} / {buffer_entries} 個のバッファ");
    }

    // --- 外部でのファイル変更の検知（再読み込みの確認）と、保存して終了時の衝突の確認 ---

    /// `path` を開いているバッファ（無題を除く）。
    pub(super) fn find_buffer_by_path(&self, path: &Path) -> Option<usize> {
        let target = canonical_or_self(path);
        self.buffers
            .iter()
            .position(|slot| slot.buffer.file_path.as_deref().is_some_and(|p| same_file(p, path, &target)))
    }

    /// 外部で変更された（まだ尋ねていない）ファイルのバッファ。
    pub(super) fn find_externally_changed(&self) -> Option<(usize, FileStamp)> {
        self.buffers.iter().enumerate().find_map(|(idx, slot)| {
            let stamp = slot.buffer.changed_on_disk()?;
            (slot.ignored_stamp != Some(stamp)).then_some((idx, stamp))
        })
    }

    /// 1 秒ごとに開いているファイルの外部での変更を調べ、変わっていれば再読み込みを尋ねる。
    /// 復元直後は、外部の変更と衝突したファイルの確認を先に出す。
    pub(super) fn check_external_changes(&mut self, ctx: &egui::Context) {
        let interval = std::time::Duration::from_secs(1);
        if !self.conflict_queue.is_empty() || self.buffers.iter().any(|s| s.buffer.disk_stamp.is_some()) {
            ctx.request_repaint_after(interval);
        }
        if self.filer_modal.is_some()
            || self.search.is_open()
            || self.inline_prompt.is_some()
            || self.pending_dialog.is_some()
            || self.quit_after_save
            || self.open_menu.is_some()
            || self.show_about
        {
            return;
        }
        if !self.conflict_queue.is_empty() {
            let path = self.conflict_queue.remove(0);
            self.open_conflict_dialog(path);
            return;
        }
        if self.last_disk_check.elapsed() < interval {
            return;
        }
        self.last_disk_check = std::time::Instant::now();
        if let Some((idx, stamp)) = self.find_externally_changed() {
            if self.mode == AppMode::Editor {
                self.switch_buffer_to(idx);
            }
            self.filer_modal = Some(FilerModal::ConfirmReload {
                buf_idx: idx,
                stamp,
                shown: std::time::Instant::now(),
            });
        }
    }

    /// バッファをファイルから読み直す。
    pub(super) fn reload_buffer(&mut self, idx: usize) {
        let Some(slot) = self.buffers.get_mut(idx) else {
            return;
        };
        let name = slot.buffer.display_name();
        match slot.buffer.reload_from_disk() {
            Ok(()) => {
                slot.ignored_stamp = None;
                slot.wrap = None;
                slot.md_scroll_dirty = true;
                self.need_scroll_to_cursor = true;
                self.status_message = format!("外部での変更を読み込みました: {name}（元に戻すで読み込み前に戻せます）");
            }
            Err(e) => self.status_message = format!("再読み込みできません: {name}: {e}"),
        }
    }

    /// 衝突したファイルの確認ダイアログを開く（vz の編集内容を持つ無題バッファとファイルの差分を見せる）。
    pub(super) fn open_conflict_dialog(&mut self, path: PathBuf) {
        let Some(u) = self.buffers.iter().position(|s| s.conflict_of.as_deref() == Some(path.as_path())) else {
            return;
        };
        let disk = vz_core::buffer::read_lines(&path).map(|(l, _)| l).unwrap_or_default();
        let diff = diff::unified(&disk, &self.buffers[u].buffer.lines, 3);
        if diff.is_empty() {
            // 内容が同じなら確認は要らない
            self.force_close_buffer(u);
            self.status_message = format!(
                "{} は vz の編集内容と同じため、そのまま使います",
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            );
            return;
        }
        self.mode = AppMode::Editor;
        self.switch_buffer_to(u);
        self.filer_modal = Some(FilerModal::ResolveConflict {
            path,
            diff,
            shown: std::time::Instant::now(),
        });
    }

    /// 衝突の確認の結果。`Some(true)`: vz の内容でファイルを上書き、`Some(false)`: ファイルの内容を採用、
    /// `None`: あとで決める（無題バッファを残し、次の保存して終了でも引き継ぐ）。
    pub(super) fn resolve_conflict(&mut self, path: &Path, adopt_vz: Option<bool>) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let Some(u) = self.buffers.iter().position(|s| s.conflict_of.as_deref() == Some(path)) else {
            return;
        };
        match adopt_vz {
            None => {
                self.status_message =
                    format!("{name}: vz の編集内容は [無題] バッファに残しています（次回の復元でも確認します）");
            }
            Some(true) => {
                let lines = self.buffers[u].buffer.lines.clone();
                let f = match self.find_buffer_by_path(path) {
                    Some(f) => f,
                    None => {
                        self.open_paths(vec![path.to_path_buf()], true);
                        match self.find_buffer_by_path(path) {
                            Some(f) => f,
                            None => return,
                        }
                    }
                };
                let buf = &mut self.buffers[f].buffer;
                buf.replace_all(lines);
                if let Err(e) = buf.save_to_path(path) {
                    self.switch_buffer_to(f);
                    self.status_message = format!("{name} に保存できません: {e}（vz の内容は [無題] に残しています）");
                    return;
                }
                self.force_close_buffer(u);
                if let Some(f) = self.find_buffer_by_path(path) {
                    self.switch_buffer_to(f);
                }
                self.status_message = format!("{name} を vz の編集内容で上書き保存しました");
            }
            Some(false) => {
                self.force_close_buffer(u);
                if let Some(f) = self.find_buffer_by_path(path) {
                    if self.buffers[f].buffer.changed_on_disk().is_some() && !self.buffers[f].buffer.dirty {
                        self.reload_buffer(f);
                    }
                    self.switch_buffer_to(f);
                }
                self.status_message = format!("{name} はファイル（外部の変更）の内容を採用しました");
            }
        }
    }

    // --- Clipboard & Edit Menu Helpers ---
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
        temp: None,
        path,
    }
}
