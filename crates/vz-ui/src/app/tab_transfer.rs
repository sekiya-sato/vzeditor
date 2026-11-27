//! 編集タブを別プロセスへ移動し、受理されたときだけ元のタブを閉じる。
use super::*;
use std::io::{self, BufWriter, Write};
use std::process::Command;
use std::time::{Duration, Instant};
use vz_config::tab_transfer::{Inbox, Offer, Outcome, Request};
use vz_platform::tab_drop::DropTarget;

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30);
const TAB_BODY_FILE: &str = "body.txt";
const HUGE_EDITS_FILE: &str = "huge.vzedit";
const HUGE_WORK_FILE: &str = "huge-work";

#[derive(Default)]
pub(super) struct TabTransfer {
    root: Option<PathBuf>,
    inbox: Option<Inbox>,
    pending: Option<Pending>,
    had_workspace: bool,
}

struct Pending {
    offer: Offer,
    tab_id: usize,
    version: BufferVersion,
    deadline: Instant,
}

// 待機中に編集・保存・ファイル名変更があったタブは元にも残す。
#[derive(PartialEq, Eq)]
struct BufferVersion {
    revision: u64,
    disk_stamp: Option<FileStamp>,
    dirty: bool,
    encoding: TextEncoding,
    path: Option<PathBuf>,
}

impl BufferVersion {
    fn of(buf: &TextBuffer) -> Self {
        Self {
            revision: buf.revision,
            disk_stamp: buf.disk_stamp,
            dirty: buf.dirty,
            encoding: buf.encoding,
            path: buf.file_path.clone(),
        }
    }
}

impl VzEditorApp {
    pub(super) fn start_tab_inbox(&mut self, session_dir: &Path, ctx: &egui::Context) {
        let root = session_dir.join("tab-transfers");
        let ctx = ctx.clone();
        self.tab_transfer.inbox =
            Inbox::start(&root, std::process::id(), move || ctx.request_repaint()).ok();
        self.tab_transfer.root = Some(root);
    }

    pub(super) fn restore_transferred_tab(&mut self, ctx: &egui::Context, id: &str) {
        let request = self
            .tab_transfer
            .root
            .as_deref()
            .and_then(|root| Request::open(root, id));
        if !request.is_some_and(|request| self.receive_tab(request)) {
            self.close_now(ctx);
        }
    }

    pub(super) fn send_tab(&mut self, tab_id: usize, target: DropTarget) {
        let DropTarget::Other(pid) = target else {
            return;
        };
        if self.tab_transfer.pending.is_some() {
            self.status_message = "別のタブを移動中です".into();
            return;
        }
        match self.prepare_tab_transfer(tab_id, pid) {
            Ok(pending) => {
                self.tab_transfer.pending = Some(pending);
                self.status_message = "タブを引き継いでいます".into();
            }
            Err(error) => self.status_message = format!("タブの移動エラー: {error}"),
        }
    }

    fn prepare_tab_transfer(&self, tab_id: usize, pid: Option<u32>) -> io::Result<Pending> {
        let root = self
            .tab_transfer
            .root
            .as_deref()
            .ok_or_else(|| io::Error::other("引き継ぎフォルダがありません"))?;
        let slot = self
            .buffers
            .iter()
            .find(|slot| slot.tab_id == tab_id)
            .ok_or_else(|| io::Error::other("タブが見つかりません"))?;
        if slot.huge.is_saving() || slot.huge.is_replacing() {
            return Err(io::Error::other(
                "巨大ファイルの処理が終わるまでお待ちください",
            ));
        }
        let offer = Offer::new(root)?;
        slot.write_transfer(offer.path())?;
        offer.publish()?;
        let delivered = match pid.filter(|&pid| pid != std::process::id()) {
            Some(pid) => offer.send_to(root, pid)?,
            None => false,
        };
        if !delivered {
            let mut child = Command::new(std::env::current_exe()?)
                .arg("--tab-transfer")
                .arg(offer.id())
                .spawn()?;
            // Unix でも子プロセスの終了状態を回収する。
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Ok(Pending {
            offer,
            tab_id,
            version: BufferVersion::of(&slot.buffer),
            deadline: Instant::now() + TRANSFER_TIMEOUT,
        })
    }

    pub(super) fn poll_tab_transfers(&mut self, ctx: &egui::Context) {
        if self.allow_close
            || self.tab_transfer.root.is_none()
            || (self.tab_transfer.inbox.is_none() && self.tab_transfer.pending.is_none())
        {
            return;
        }
        if self.tab_transfer.pending.is_some() {
            ctx.request_repaint_after(POLL_INTERVAL);
        }
        // ダイアログが参照するバッファの位置や、保存先の選択を変えない。
        if self.pending_dialog.is_some()
            || self.filer_modal.is_some()
            || self.inline_prompt.is_some()
            || self.quit_after_save
        {
            return;
        }
        while let Some(request) = self.tab_transfer.inbox.as_ref().and_then(Inbox::next) {
            if self.receive_tab(request) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
        let Some(pending) = &self.tab_transfer.pending else {
            return;
        };
        let outcome = if Instant::now() >= pending.deadline {
            pending.offer.cancel()
        } else {
            pending.offer.outcome()
        };
        if outcome == Outcome::Pending {
            return;
        }
        let Some(pending) = self.tab_transfer.pending.take() else {
            return;
        };
        if outcome == Outcome::Accepted {
            self.finish_sent_tab(pending.tab_id, pending.version);
        } else {
            self.status_message = "タブの移動エラー: 移動先がタブを受理できませんでした".into();
        }
    }

    fn finish_sent_tab(&mut self, tab_id: usize, version: BufferVersion) {
        let Some(idx) = self.buffers.iter().position(|slot| slot.tab_id == tab_id) else {
            return;
        };
        let slot = &self.buffers[idx];
        if BufferVersion::of(&slot.buffer) != version
            || slot.huge.is_saving()
            || slot.huge.is_replacing()
        {
            self.status_message = "移動中に変更があったため元のタブも残しました".into();
            return;
        }
        // 非表示のタブを移動しても、表示中だったタブを維持する。
        if idx < self.active_buf {
            self.active_buf -= 1;
        }
        self.force_close_buffer(idx);
        self.status_message = "タブを移動しました".into();
    }

    fn receive_tab(&mut self, request: Request) -> bool {
        let result = BufferSlot::read_transfer(request.path()).and_then(|slot| {
            if slot.buffer.file_path.as_deref().is_some_and(|path| {
                self.find_buffer_by_path(path).is_some()
                    || self.buffers.iter().any(|slot| {
                        slot.buffer
                            .file_path
                            .as_deref()
                            .and_then(|open| std::path::absolute(open).ok())
                            .is_some_and(|open| open == path)
                    })
            }) {
                return Err(io::Error::other("同じファイルを既に開いています"));
            }
            Ok(slot)
        });
        match result {
            Ok(slot) if request.finish(true).is_ok() => {
                if let Some(origin) = &slot.conflict_of {
                    self.conflict_queue.push(origin.clone());
                }
                self.add_buffer_slot(slot);
                self.status_message = "タブを引き継ぎました".into();
                true
            }
            Ok(_) => false,
            Err(error) => {
                let _ = request.finish(false);
                self.status_message = format!("タブの受け取りエラー: {error}");
                false
            }
        }
    }

    pub(super) fn close_if_workspace_emptied(&mut self, ctx: &egui::Context) {
        let has_workspace = self.terminal.is_some()
            || self.pending_terminal_cwd.is_some()
            || !self.browsers.is_empty()
            || self
                .buffers
                .iter()
                .any(|slot| !slot.is_untouched_untitled());
        if has_workspace {
            self.tab_transfer.had_workspace = true;
        } else if self.tab_transfer.had_workspace
            && !self.allow_close
            && self.pending_dialog.is_none()
            && self.filer_modal.is_none()
            && self.tab_transfer.pending.is_none()
        {
            self.close_now(ctx);
        }
    }
}

impl BufferSlot {
    fn write_transfer(&self, dir: &Path) -> io::Result<()> {
        let buf = &self.buffer;
        let kind = if buf.is_huge() {
            if buf.dirty {
                EntryKind::HugeEdits
            } else {
                EntryKind::File
            }
        } else if self.conflict_of.is_some() {
            EntryKind::Conflict
        } else {
            EntryKind::Unsaved
        };
        let path = self.conflict_of.as_ref().or(buf.file_path.as_ref());
        let temp = match kind {
            EntryKind::HugeEdits => {
                let temp = dir.join(HUGE_EDITS_FILE);
                match buf.write_huge_edits(&temp) {
                    Ok(()) => Some(temp),
                    Err(error) => {
                        let original = path.ok_or(error)?;
                        if buf.disk_stamp.is_none()
                            || vz_core::buffer::file_stamp(original) != buf.disk_stamp
                        {
                            return Err(io::Error::other("移動前に元ファイルが変更されました"));
                        }
                        // すべて置換の作業ファイルを含む場合は、現在の本文を順に書き出す。
                        let work = dir.join(HUGE_WORK_FILE);
                        let snapshot = buf
                            .huge()
                            .ok_or_else(|| io::Error::other("巨大ファイルではありません"))?
                            .doc_snapshot();
                        let mut writer = BufWriter::new(fs::File::create(&work)?);
                        for span in snapshot.spans() {
                            writer.write_all(span)?;
                        }
                        writer.flush()?;
                        Some(work)
                    }
                }
            }
            EntryKind::Unsaved | EntryKind::Conflict => {
                let temp = dir.join(TAB_BODY_FILE);
                let mut writer = BufWriter::new(fs::File::create(&temp)?);
                for (i, line) in buf.text_lines().iter().enumerate() {
                    if i > 0 {
                        writer.write_all(b"\n")?;
                    }
                    writer.write_all(line.as_bytes())?;
                }
                writer.flush()?;
                Some(temp)
            }
            _ => None,
        };
        let entry = SessionEntry {
            kind,
            line: buf.cursor_line,
            col: buf.cursor_col,
            markdown_view: self.markdown.visible,
            active: true,
            crlf: buf.line_ending == "\r\n",
            encoding: Some(buf.encoding.label().to_owned()),
            disk_stamp: super::restore::stamp_to_record(buf.disk_stamp),
            temp: temp.as_deref().and_then(Path::file_name).map(PathBuf::from),
            path: path.map(std::path::absolute).transpose()?,
        };
        let serialized = session::serialize(std::slice::from_ref(&entry));
        if session::parse(&serialized).as_slice() != [entry] {
            return Err(io::Error::other("ファイル名を引き継ぎ形式に記録できません"));
        }
        fs::write(
            dir.join("tab.ini"),
            format!("{}\n{serialized}", u8::from(buf.dirty)),
        )
    }

    fn read_transfer(dir: &Path) -> io::Result<Self> {
        let invalid = || io::Error::other("引き継ぎデータの形式が違います");
        let metadata = fs::read_to_string(dir.join("tab.ini"))?;
        let (dirty, serialized) = metadata.split_once('\n').ok_or_else(invalid)?;
        let dirty = match dirty {
            "0" => false,
            "1" => true,
            _ => return Err(invalid()),
        };
        let entries = session::parse(serialized);
        let [entry] = entries.as_slice() else {
            return Err(invalid());
        };
        // v0.3.2 の送信元が付ける PID 付きの名前も、記録どおりに解決する。
        let payload_path = || {
            let name = entry.temp.as_deref().ok_or_else(invalid)?;
            if name.file_name() != Some(name.as_os_str()) {
                return Err(invalid());
            }
            Ok(dir.join(name))
        };
        let stamp = super::restore::stamp_from_record(entry.disk_stamp);
        let mut buffer = match entry.kind {
            EntryKind::File | EntryKind::HugeEdits
                if entry.kind == EntryKind::File
                    || entry.temp.as_deref() == Some(Path::new(HUGE_WORK_FILE)) =>
            {
                let path = entry.path.as_deref().ok_or_else(invalid)?;
                let mut buf = TextBuffer::open_huge(path)?;
                if buf.disk_stamp.is_none() || buf.disk_stamp != stamp {
                    return Err(io::Error::other("移動中に元ファイルが変更されました"));
                }
                if entry.kind == EntryKind::HugeEdits {
                    let work = vz_config::tab_transfer::take_work_file(&payload_path()?)?;
                    let path = work.keep().map_err(|error| error.error)?;
                    if let Err(error) = buf.adopt_huge_work_file(&path) {
                        let _ = fs::remove_file(path);
                        return Err(error);
                    }
                }
                buf
            }
            EntryKind::HugeEdits => {
                let path = entry.path.as_deref().ok_or_else(invalid)?;
                TextBuffer::open_huge_with_edits(path, &payload_path()?)?
            }
            EntryKind::Unsaved | EntryKind::Conflict => {
                let text = fs::read_to_string(payload_path()?)?;
                let mut buf = TextBuffer::from_lines(text.split('\n').map(String::from).collect());
                if entry.kind == EntryKind::Unsaved {
                    buf.file_path = entry.path.clone();
                }
                buf.line_ending = if entry.crlf { "\r\n" } else { "\n" };
                buf.encoding = entry
                    .encoding
                    .as_deref()
                    .and_then(TextEncoding::from_label)
                    .ok_or_else(invalid)?;
                buf.disk_stamp = stamp;
                buf
            }
            _ => return Err(invalid()),
        };
        buffer.dirty = dirty;
        buffer.set_cursor(entry.line, entry.col);
        let mut slot = Self::new(buffer);
        slot.markdown.visible = entry.markdown_view && slot.buffer.is_markdown();
        slot.markdown.scroll_dirty = true;
        if entry.kind == EntryKind::Conflict {
            slot.conflict_of = entry.path.clone();
        }
        Ok(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::make_test_app;
    use super::*;

    #[test]
    fn tab_transfer_keeps_unsaved_content_and_closes_source_after_ack() {
        let root = std::env::temp_dir().join(format!("vz-tab-move-{}", std::process::id()));
        let mut source = make_test_app();
        let mut destination = make_test_app();
        let input = source.cur_slot_mut();
        input.buffer.line_ending = "\r\n";
        input.buffer.encoding = TextEncoding::ShiftJis;
        input.buffer.set_cursor(2, 3);
        input.markdown.visible = true;
        destination.buffers[0] = BufferSlot::new(TextBuffer::new());
        let id = input.tab_id;
        let version = BufferVersion::of(&input.buffer);
        let offer = Offer::new(&root).unwrap();
        input.write_transfer(offer.path()).unwrap();
        offer.publish().unwrap();
        assert_eq!(offer.outcome(), Outcome::Pending);

        assert!(destination.receive_tab(Request::open(&root, offer.id()).unwrap()));
        let transferred = destination.cur_slot();
        assert_eq!(
            transferred.buffer.text_lines(),
            source.cur_buf().text_lines()
        );
        assert_eq!(
            transferred.buffer.file_path,
            std::path::absolute("notes.md").ok()
        );
        assert_eq!(transferred.buffer.encoding, TextEncoding::ShiftJis);
        assert_eq!(transferred.buffer.line_ending, "\r\n");
        assert_eq!(transferred.buffer.cursor_pos(), (2, 3));
        assert!(transferred.buffer.dirty && transferred.markdown.visible);
        assert_eq!(offer.outcome(), Outcome::Accepted);

        let ctx = egui::Context::default();
        source.close_if_workspace_emptied(&ctx);
        source.finish_sent_tab(id, version);
        assert!(source.cur_slot().is_untouched_untitled());
        source.close_if_workspace_emptied(&ctx);
        assert!(source.allow_close);
        drop(offer);
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn tab_transfer_does_not_overwrite_duplicate_or_close_a_changed_source() {
        let root = std::env::temp_dir().join(format!("vz-tab-duplicate-{}", std::process::id()));
        let mut source = make_test_app();
        let mut destination = make_test_app();
        destination.cur_buf_mut().file_path = std::path::absolute("notes.md").ok();
        destination.cur_buf_mut().insert_text("Existing edit");
        let offer = Offer::new(&root).unwrap();
        source.cur_slot().write_transfer(offer.path()).unwrap();
        offer.publish().unwrap();
        assert!(!destination.receive_tab(Request::open(&root, offer.id()).unwrap()));
        assert_eq!(offer.outcome(), Outcome::Rejected);
        assert!(destination
            .cur_buf()
            .text_lines()
            .iter()
            .any(|line| line.contains("Existing edit")));

        let id = source.cur_slot().tab_id;
        let version = BufferVersion::of(source.cur_buf());
        source.cur_buf_mut().insert_text("New edit");
        source.finish_sent_tab(id, version);
        assert_eq!(source.cur_slot().tab_id, id);
        assert!(source
            .cur_buf()
            .text_lines()
            .iter()
            .any(|line| line.contains("New edit")));
        drop(offer);
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn transfer_reads_payload_names_from_a_different_process() {
        let root = std::env::temp_dir().join(format!("vz-tab-other-pid-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let original = root.join("original.txt");
        fs::write(&original, "original\n").unwrap();
        let sender_pid = std::process::id().wrapping_add(1);
        for huge in [false, true] {
            let offer = Offer::new(&root).unwrap();
            let mut buffer = if huge {
                TextBuffer::open_huge(&original).unwrap()
            } else {
                TextBuffer::from_file(&original).unwrap()
            };
            buffer.insert_text("edited ");
            assert!(buffer.dirty);
            let tab = BufferSlot::new(buffer);
            tab.write_transfer(offer.path()).unwrap();
            let (local_payload, sender_name) = if huge {
                (
                    offer.path().join(HUGE_EDITS_FILE),
                    format!("huge-{sender_pid}-0.vzedit"),
                )
            } else {
                (
                    offer.path().join(TAB_BODY_FILE),
                    format!("unsaved-{sender_pid}-0.txt"),
                )
            };
            fs::rename(&local_payload, offer.path().join(&sender_name)).unwrap();
            let manifest = offer.path().join("tab.ini");
            let metadata = fs::read_to_string(&manifest).unwrap();
            let local_name = local_payload.file_name().unwrap().to_str().unwrap();
            assert!(metadata.contains(local_name));
            fs::write(&manifest, metadata.replacen(local_name, &sender_name, 1)).unwrap();

            let restored = BufferSlot::read_transfer(offer.path()).unwrap();
            assert!(restored.buffer.dirty);
            assert_eq!(restored.buffer.is_huge(), huge);
            if let Some(huge) = restored.buffer.huge() {
                let snapshot = huge.doc_snapshot();
                assert_eq!(
                    snapshot.read(0, snapshot.len()).as_ref(),
                    b"edited original\n"
                );
            } else {
                assert_eq!(restored.buffer.line(0), "edited original");
            }
        }
        fs::remove_file(original).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn huge_tab_transfers_an_unsaved_replace_work_file() {
        let root = std::env::temp_dir().join(format!("vz-tab-huge-{}", std::process::id()));
        let offer = Offer::new(&root).unwrap();
        let original = root.join("original.txt");
        let work = root.join("source-work");
        fs::write(&original, "original\n").unwrap();
        fs::write(&work, "replacement\n").unwrap();
        let mut buffer = TextBuffer::open_huge(&original).unwrap();
        buffer.adopt_huge_work_file(&work).unwrap();
        let source = BufferSlot::new(buffer);

        source.write_transfer(offer.path()).unwrap();
        assert!(offer.path().join(HUGE_WORK_FILE).is_file());
        offer.publish().unwrap();
        let request = Request::open(&root, offer.id()).unwrap();
        let restored = BufferSlot::read_transfer(request.path()).unwrap();
        request.finish(true).unwrap();
        drop(offer);
        assert!(restored.buffer.is_huge() && restored.buffer.dirty);
        assert_eq!(
            restored.buffer.file_path.as_deref(),
            Some(original.as_path())
        );
        let snapshot = restored.buffer.huge().unwrap().doc_snapshot();
        assert_eq!(snapshot.read(0, snapshot.len()).as_ref(), b"replacement\n");
        assert_eq!(fs::read_to_string(&original).unwrap(), "original\n");

        drop(snapshot);
        drop(restored);
        drop(source);
        fs::remove_file(original).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn empty_startup_waits_and_last_browser_keeps_app_alive() {
        let mut app = make_test_app();
        app.buffers[0] = BufferSlot::new(TextBuffer::new());
        let ctx = egui::Context::default();
        app.close_if_workspace_emptied(&ctx);
        assert!(!app.allow_close);

        app.new_browser_tab(None);
        app.close_if_workspace_emptied(&ctx);
        assert!(!app.allow_close);
        app.close_browser_tab(0);
        app.close_if_workspace_emptied(&ctx);
        assert!(app.allow_close);
    }
}
