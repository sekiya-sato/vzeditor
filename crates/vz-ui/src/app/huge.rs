//! 巨大ファイルモード（128MB 以上）の画面側の処理: 行索引の取り込み、裏のスレッドでの保存、
//! ステータスの表示。文書そのものは vz-core の `HugeDoc`（docs/巨大ファイル詳細設計.md）。

use super::*;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{self, TryRecvError};
use vz_core::huge::search::{self as huge_search, Matcher};
use vz_core::search::SearchPattern;

/// 裏のスレッドで実行中の保存。
pub(super) struct HugeSaveJob {
    path: PathBuf,
    encoding: TextEncoding,
    /// 保存を始めたときの内容の番号
    rev: u64,
    /// すべて置換の作業ファイルを rename して保存したなら、その作業ファイル
    renamed_work: Option<PathBuf>,
    total: u64,
    progress: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<std::io::Result<()>>,
}

impl Drop for HugeSaveJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl VzEditorApp {
    /// 毎フレーム: 索引を取り込み、保存の終わりを受け取る。作成中・保存中は再描画を頼む。
    pub(super) fn poll_huge_buffers(&mut self, ctx: &egui::Context) {
        let mut busy = false;
        let mut messages = Vec::new();
        for slot in &mut self.buffers {
            if !slot.buffer.is_huge() {
                continue;
            }
            slot.buffer.poll();
            busy |= slot.buffer.huge().is_some_and(HugeDoc::is_indexing);
            let Some(job) = &slot.huge_save else {
                continue;
            };
            busy = true;
            let result = match job.receiver.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => continue,
                Err(TryRecvError::Disconnected) => {
                    Err(std::io::Error::other("保存が中断されました"))
                }
            };
            let Some(job) = slot.huge_save.take() else {
                continue;
            };
            if let Some(h) = slot.buffer.huge_mut() {
                h.set_busy(false);
            }
            match result {
                Ok(()) => {
                    if let (Some(work), Some(h)) = (&job.renamed_work, slot.buffer.huge_mut()) {
                        h.promote_work_file(work, &job.path);
                    }
                    slot.buffer
                        .finish_huge_save(&job.path, job.encoding, job.rev);
                    messages.push(format!(
                        "保存しました（{}）: {}",
                        job.encoding,
                        job.path.display()
                    ));
                }
                Err(e) => {
                    messages.push(format!("保存エラー: {e}"));
                    self.quit_after_save = false;
                }
            }
        }
        if let Some(m) = messages.pop() {
            self.status_message = m;
        }
        self.poll_huge_tasks();
        busy |= self.huge_task_running();
        // 編集できなかった理由（索引作成中・保存中・表せない文字）
        if let Some(e) = self.cur_buf_mut().take_error() {
            self.status_message = e;
        }
        if busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// 巨大ファイルを保存する。ハードリンクがあるとリンクが切れるので、先に確認する。
    pub(super) fn request_huge_save(&mut self, path: PathBuf, encoding: TextEncoding) {
        let links = vz_platform::file_info::hard_link_count(&path).unwrap_or(1);
        if links > 1 && self.filer_modal.is_none() {
            self.filer_modal = Some(FilerModal::ConfirmHardLinkSave { path, encoding });
            return;
        }
        self.start_huge_save(path, encoding);
    }

    /// 巨大ファイルを裏のスレッドで保存する。保存中は編集できない。
    pub(super) fn start_huge_save(&mut self, path: PathBuf, encoding: TextEncoding) {
        let idx = self.active_buf;
        if self.buffers[idx].huge_save.is_some() {
            self.status_message = "保存中です".into();
            return;
        }
        let Some(h) = self.buffers[idx].buffer.huge_mut() else {
            return;
        };
        let rev = h.current_rev();
        // すべて置換の結果をそのまま保存するなら、作業ファイルを rename するだけで済む
        // （BOM を付けたり文字コードを変えたりするときは全体を書き出す）
        let renamed_work = h
            .whole_work_file()
            .filter(|_| encoding == h.encoding && encoding.bom().is_empty());
        let snapshot = h.doc_snapshot();
        h.set_busy(true);
        let total = snapshot.len();
        let progress = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let (p, c, target) = (Arc::clone(&progress), Arc::clone(&cancel), path.clone());
        let work = renamed_work.clone();
        let spawned = std::thread::Builder::new()
            .name("vz-huge-save".into())
            .spawn(move || {
                let result = match work {
                    Some(work) => vz_core::huge::save_by_rename(&work, &target),
                    None => snapshot.save(&target, encoding, &p, &c),
                };
                let _ = sender.send(result);
            });
        if let Err(e) = spawned {
            if let Some(h) = self.buffers[idx].buffer.huge_mut() {
                h.set_busy(false);
            }
            self.status_message = format!("保存エラー: {e}");
            self.quit_after_save = false;
            return;
        }
        self.buffers[idx].huge_save = Some(HugeSaveJob {
            path,
            encoding,
            rev,
            renamed_work,
            total,
            progress,
            cancel,
            receiver,
        });
        self.status_message = "保存中…".into();
    }

    /// ステータスバーに出す巨大ファイルの状態（索引の作成中・保存中）。
    pub(super) fn huge_status(&self) -> Option<String> {
        let slot = self.cur_slot();
        let h = slot.buffer.huge()?;
        // mmap できずにメモリへ読み込んだときは、そのことも示す
        let mut text = String::from(if h.is_heap_backed() {
            "[巨大・メモリ読込]"
        } else {
            "[巨大]"
        });
        if let Some(job) = &slot.huge_save {
            let done = job.progress.load(Ordering::Relaxed);
            text.push_str(&format!(" 保存中 {}%", percent(done, job.total)));
        } else if let Some(job) = &slot.huge_replace {
            let done = job.progress.load(Ordering::Relaxed);
            text.push_str(&format!(" 置換中 {}%", percent(done, job.total)));
        } else if slot.huge_find.is_some() {
            text.push_str(" 検索中");
        } else if h.is_indexing() {
            let (done, total) = h.index_progress();
            text.push_str(&format!(" 索引 {}%", percent(done, total)));
        }
        Some(text)
    }
}

fn percent(done: u64, total: u64) -> u64 {
    done.saturating_mul(100)
        .checked_div(total)
        .map_or(100, |p| p.min(100))
}

/// 裏のスレッドで実行中の検索（次・前）。
pub(super) struct HugeFindJob {
    up: bool,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<Option<(u64, u64)>>,
    /// 見つかったが、索引がまだその位置に届いていない
    found: Option<(u64, u64)>,
}

impl Drop for HugeFindJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// 裏のスレッドで実行中のすべて置換（作業ファイルへの書き出し）。
pub(super) struct HugeReplaceJob {
    path: PathBuf,
    /// Esc で中止した（スレッドの終わりを待って作業ファイルを消す）
    cancelled: bool,
    total: u64,
    progress: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<std::io::Result<u64>>,
}

impl Drop for HugeReplaceJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl VzEditorApp {
    /// 巨大ファイルで次（前）を検索する（裏のスレッド）。結果は `poll_huge_tasks` で選択する。
    pub(super) fn start_huge_find(&mut self, pattern: &SearchPattern, up: bool, wrap: bool) {
        let buf = self.cur_buf();
        let Some(h) = buf.huge() else {
            return;
        };
        let matcher = match Matcher::new(pattern, h.encoding) {
            Ok(m) => m,
            Err(e) => {
                self.status_message = e;
                return;
            }
        };
        let cursor = buf.cursor_pos();
        let (start, end) = buf.selection_range().unwrap_or((cursor, cursor));
        let origin = if up { start } else { end };
        let Some(origin) = h.pos_to_offset(origin.0, origin.1) else {
            return;
        };
        let snapshot = h.doc_snapshot();
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::clone(&cancel);
        let spawned = std::thread::Builder::new()
            .name("vz-huge-search".into())
            .spawn(move || {
                let _ = sender.send(huge_search::find(
                    &snapshot, &matcher, origin, up, wrap, &stop,
                ));
            });
        match spawned {
            Ok(_) => {
                self.cur_slot_mut().huge_find = Some(HugeFindJob {
                    up,
                    cancel,
                    receiver,
                    found: None,
                });
                self.status_message = "検索中…".into();
            }
            Err(e) => self.status_message = format!("検索を開始できません: {e}"),
        }
    }

    /// 巨大ファイルのすべて置換を始める（裏のスレッドで作業ファイルに書き出す）。
    pub(super) fn start_huge_replace_all(
        &mut self,
        pattern: &SearchPattern,
        replacement: &str,
        scope: Option<(TextPos, TextPos)>,
    ) -> Result<(), String> {
        let idx = self.active_buf;
        if self.buffers[idx].huge_replace.is_some() || self.buffers[idx].huge_save.is_some() {
            return Err("保存中・置換中です".into());
        }
        let buf = &self.buffers[idx].buffer;
        let h = buf.huge().ok_or("巨大ファイルではありません")?;
        let matcher = Matcher::new(pattern, h.encoding)?;
        let scope = match scope {
            Some((a, b)) => Some((
                h.pos_to_offset(a.0, a.1).ok_or("索引作成中です")?,
                h.pos_to_offset(b.0, b.1).ok_or("索引作成中です")?,
            )),
            None => None,
        };
        let file = buf.file_path.clone().ok_or("ファイル名がありません")?;
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("vz");
        let work = file.with_file_name(format!(
            ".{name}.vzwork-{}-{}.tmp",
            std::process::id(),
            buf.revision
        ));
        let expand = pattern.expands_captures();
        let replacement = replacement.to_string();
        let Some(h) = self.buffers[idx].buffer.huge_mut() else {
            return Ok(());
        };
        let snapshot = h.doc_snapshot();
        h.set_busy(true);
        let total = snapshot.len();
        let progress = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let (p, c, out) = (Arc::clone(&progress), Arc::clone(&cancel), work.clone());
        let spawned = std::thread::Builder::new()
            .name("vz-huge-replace".into())
            .spawn(move || {
                let result = huge_search::replace_all_to_file(
                    &snapshot,
                    &matcher,
                    &replacement,
                    expand,
                    scope,
                    &out,
                    &p,
                    &c,
                );
                if result.is_err() {
                    let _ = std::fs::remove_file(&out);
                }
                let _ = sender.send(result);
            });
        if let Err(e) = spawned {
            if let Some(h) = self.buffers[idx].buffer.huge_mut() {
                h.set_busy(false);
            }
            return Err(format!("置換を開始できません: {e}"));
        }
        self.buffers[idx].huge_replace = Some(HugeReplaceJob {
            path: work,
            cancelled: false,
            total,
            progress,
            cancel,
            receiver,
        });
        self.status_message = "すべて置換: 書き出し中…".into();
        Ok(())
    }

    /// 巨大ファイルの行ジャンプ。索引がまだ届いていなければ、届いたときに移動する（待つなら true）。
    pub(super) fn huge_jump_or_wait(&mut self, row: usize) -> Result<bool, String> {
        let buf = self.cur_buf();
        let Some(h) = buf.huge() else {
            return Ok(false);
        };
        if row < buf.line_count() {
            return Ok(false);
        }
        if h.is_indexing() {
            self.cur_slot_mut().pending_jump = Some(row);
            self.status_message = format!("索引作成中: {}行に届いたら移動します", row + 1);
            return Ok(true);
        }
        Err(format!("行番号は1〜{}です", buf.line_count()))
    }

    /// 検索・すべて置換・行ジャンプの待ちを処理する（毎フレーム）。
    pub(super) fn poll_huge_tasks(&mut self) {
        let idx = self.active_buf;
        // 行ジャンプ
        if let Some(row) = self.buffers[idx].pending_jump {
            if row < self.buffers[idx].buffer.line_count() {
                self.buffers[idx].pending_jump = None;
                self.move_to_search_position(row, 0);
                self.status_message = format!("{}行へ移動しました", row + 1);
            }
        }
        // 検索
        if let Some(job) = &mut self.buffers[idx].huge_find {
            if job.found.is_none() {
                match job.receiver.try_recv() {
                    Ok(Some(hit)) => job.found = Some(hit),
                    Ok(None) | Err(TryRecvError::Disconnected) => {
                        self.buffers[idx].huge_find = None;
                        self.status_message = "一致する文字列が見つかりません".into();
                    }
                    Err(TryRecvError::Empty) => {}
                }
            }
        }
        if let Some((a, b)) = self.buffers[idx].huge_find.as_ref().and_then(|j| j.found) {
            let positions = self.buffers[idx]
                .buffer
                .huge()
                .and_then(|h| Some((h.offset_to_pos(a)?, h.offset_to_pos(b)?)));
            if let Some((start, end)) = positions {
                let up = self.buffers[idx].huge_find.take().is_some_and(|j| j.up);
                let buf = &mut self.buffers[idx].buffer;
                // 上検索はカーソルを一致の先頭に置く
                if up {
                    buf.select_range(end, start);
                } else {
                    buf.select_range(start, end);
                }
                self.need_scroll_to_cursor = true;
                self.select_mode = false;
                self.status_message = format!("検索結果　{}行 {}桁", start.0 + 1, start.1 + 1);
            } else {
                self.status_message = "検索結果: 索引作成中（届いたら移動します）".into();
            }
        }
        // すべて置換
        let result = match &self.buffers[idx].huge_replace {
            Some(job) => match job.receiver.try_recv() {
                Ok(r) => Some(r),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err(std::io::Error::other("置換が中断されました")))
                }
            },
            None => None,
        };
        if let Some(result) = result {
            let Some(job) = self.buffers[idx].huge_replace.take() else {
                return;
            };
            if job.cancelled {
                let _ = std::fs::remove_file(&job.path);
                return;
            }
            let buf = &mut self.buffers[idx].buffer;
            if let Some(h) = buf.huge_mut() {
                h.set_busy(false);
            }
            self.status_message = match result {
                Ok(0) => {
                    let _ = std::fs::remove_file(&job.path);
                    "一致する文字列が見つかりません".into()
                }
                Ok(n) => match buf.adopt_huge_work_file(&job.path) {
                    Ok(()) => format!("{n}件を置換しました（Ctrl+Z で戻せます）"),
                    Err(e) => {
                        let _ = std::fs::remove_file(&job.path);
                        format!("置換エラー: {e}")
                    }
                },
                Err(e) => format!("置換エラー: {e}"),
            };
            self.need_scroll_to_cursor = true;
        }
    }

    /// Esc: 巨大ファイルの検索・すべて置換・行ジャンプの待ちを中止する。中止したら true。
    pub(super) fn cancel_huge_task(&mut self) -> bool {
        let slot = self.cur_slot_mut();
        let replacing = slot.huge_replace.as_mut().filter(|job| !job.cancelled);
        let message = if let Some(job) = replacing {
            // スレッドに中止を伝え、終わったら作業ファイルを消す（書き終えていても取り込まない）
            job.cancel.store(true, Ordering::Relaxed);
            job.cancelled = true;
            if let Some(h) = slot.buffer.huge_mut() {
                h.set_busy(false);
            }
            "すべて置換を中止しました"
        } else if slot.huge_find.take().is_some() {
            "検索を中止しました"
        } else if slot.pending_jump.take().is_some() {
            "行ジャンプを中止しました"
        } else {
            return false;
        };
        self.status_message = message.into();
        true
    }

    /// 巨大ファイルで、検索・置換が実行中か（実行中は再描画を続ける）。
    fn huge_task_running(&self) -> bool {
        let slot = self.cur_slot();
        slot.huge_find.is_some() || slot.huge_replace.is_some() || slot.pending_jump.is_some()
    }
}
