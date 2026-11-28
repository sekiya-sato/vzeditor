//! 外部でのファイル変更の検知と、復元時に保留した上書きの確認。

use super::*;

impl VzEditorApp {
    /// 表示中のファイルで、まだ尋ねていない外部変更。
    pub(super) fn active_external_change(&self) -> Option<FileStamp> {
        let slot = self.cur_slot();
        // 巨大ファイルの保存中・置換中は、自分の書き込みを外部の変更と取り違えないよう調べない
        if slot.huge.is_saving() || slot.huge.is_replacing() {
            return None;
        }
        let stamp = slot.buffer.changed_on_disk()?;
        (slot.ignored_stamp != Some(stamp)).then_some(stamp)
    }

    /// エディターで表示中のファイルを 1 秒ごとに調べ、外部で変更されていれば再読み込みを尋ねる。
    /// 復元直後は、外部の変更と衝突したファイルの確認を先に出す。
    pub(super) fn check_external_changes(&mut self, ctx: &egui::Context) {
        let interval = std::time::Duration::from_secs(1);
        let check_active = self.mode == AppMode::Editor && self.cur_buf().disk_stamp.is_some();
        if !self.conflict_queue.is_empty() || check_active {
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
        if !check_active || self.last_disk_check.elapsed() < interval {
            return;
        }
        self.last_disk_check = std::time::Instant::now();
        if let Some(stamp) = self.active_external_change() {
            self.filer_modal = Some(FilerModal::ConfirmReload {
                buf_idx: self.active_buf,
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
                slot.markdown.scroll_dirty = true;
                self.need_scroll_to_cursor = true;
                self.status_message = format!(
                    "外部での変更を読み込みました: {name}（元に戻すで読み込み前に戻せます）"
                );
            }
            Err(e) => self.status_message = format!("再読み込みできません: {name}: {e}"),
        }
    }

    /// 衝突したファイルの確認ダイアログを開く（vz の編集内容を持つ無題バッファとファイルの差分を見せる）。
    pub(super) fn open_conflict_dialog(&mut self, path: PathBuf) {
        let Some(u) = self
            .buffers
            .iter()
            .position(|s| s.conflict_of.as_deref() == Some(path.as_path()))
        else {
            return;
        };
        let disk = vz_core::buffer::read_lines(&path)
            .map(|(l, _)| l)
            .unwrap_or_default();
        let diff = diff::unified(&disk, self.buffers[u].buffer.text_lines(), 3);
        if diff.is_empty() {
            // 内容が同じなら確認は要らない
            self.force_close_buffer(u);
            self.status_message = format!(
                "{} は vz の編集内容と同じため、そのまま使います",
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
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
        let name = file_name_or_path(path);
        let Some(u) = self
            .buffers
            .iter()
            .position(|s| s.conflict_of.as_deref() == Some(path))
        else {
            return;
        };
        match adopt_vz {
            None => {
                self.status_message =
                    format!("{name}: vz の編集内容は [無題] バッファに残しています（次回の復元でも確認します）");
            }
            Some(true) => {
                let lines = self.buffers[u].buffer.text_lines().to_vec();
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
                    self.status_message = format!(
                        "{name} に保存できません: {e}（vz の内容は [無題] に残しています）"
                    );
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
                    if self.buffers[f].buffer.changed_on_disk().is_some()
                        && !self.buffers[f].buffer.dirty
                    {
                        self.reload_buffer(f);
                    }
                    self.switch_buffer_to(f);
                }
                self.status_message =
                    format!("{name} はファイル（外部の変更）の内容を採用しました");
            }
        }
    }
}
