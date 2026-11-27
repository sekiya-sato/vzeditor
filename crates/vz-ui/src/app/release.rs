//! リリース専用の PID 別の保存と復元完了処理。

use super::*;
use vz_config::session::release::State;

pub(super) enum Startup {
    Pending { state: Box<State>, terminal: bool },
    Failed,
}

impl VzEditorApp {
    pub(super) fn poll_release_request(&mut self, ctx: &egui::Context) {
        if self.allow_close || self.release_startup.is_some() {
            return;
        }
        let Some(request) = self
            .release_watcher
            .as_ref()
            .and_then(session::release::Watcher::next)
        else {
            return;
        };
        match request.publish(|dir| self.save_session(dir)) {
            Ok(()) => self.close_now(ctx),
            Err(e) => self.status_message = format!("リリース用の状態を保存できません: {e}"),
        }
    }

    pub(super) fn restore_release_state(&mut self, state: Box<State>) {
        let failed = self.restore_entries_with(&state.entries, true);
        self.apply_session_window(state.window.clone());
        self.release_startup = Some(if failed.is_empty() {
            Startup::Pending {
                state,
                terminal: self.pending_terminal_cwd.is_some(),
            }
        } else {
            Startup::Failed
        });
    }

    pub(super) fn finish_release_startup(&mut self, ctx: &egui::Context) {
        if self.release_startup.is_none() || self.allow_close {
            return;
        }
        // eframe は初回描画後にウィンドウを表示する。次の描画まで元の記録を保持する。
        if ctx.cumulative_pass_nr() < 2 {
            ctx.request_repaint();
            return;
        }
        let browser_failed = self.mode == AppMode::Browser
            && self
                .browsers
                .get(self.active_browser)
                .is_some_and(browser_view::BrowserTab::failed_to_open);
        match self.release_startup.take() {
            Some(Startup::Pending { state, terminal })
                if !browser_failed && (!terminal || self.terminal.is_some()) =>
            {
                if let Err(e) = state.finish() {
                    self.status_message = format!("リリース用の状態を削除できません: {e}");
                }
            }
            Some(_) => self.close_now(ctx),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::make_test_app;
    use super::*;
    use vz_config::session::release::{Batch, Watcher};

    #[test]
    fn release_restore_only_removes_record_after_all_buffers_and_startup_succeed() {
        let dir = std::env::temp_dir().join(format!("vz_release_app_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("document.txt");
        fs::write(&file, "original").unwrap();
        let pid = std::process::id();
        let id = "0123456789abcdef0123456789abcdef";
        let batch = Batch::new(&dir, id).unwrap();

        let mut first = TextBuffer::from_file(&file).unwrap();
        first.insert_text("local ");
        let mut second = TextBuffer::new();
        second.insert_text("unsaved");
        let mut former = make_test_app();
        former.buffers = vec![BufferSlot::new(first), BufferSlot::new(second)];
        former.release_watcher = Some(Watcher::start(&dir, || {}).unwrap());
        let capture = std::thread::spawn({
            let dir = dir.clone();
            move || Batch::new(&dir, id).unwrap().capture(&[pid]).unwrap()
        });
        let ctx = egui::Context::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !former.allow_close {
            assert!(std::time::Instant::now() < deadline);
            let _ = ctx.run(Default::default(), |ctx| former.poll_release_request(ctx));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        capture.join().unwrap();
        drop(former);
        assert_eq!(fs::read_to_string(&file).unwrap(), "original");

        let record = batch.load(pid).unwrap();
        let first_temp = record.entries[0].temp.clone().unwrap();
        let second_temp = record.entries[1].temp.clone().unwrap();
        let unavailable = dir.join("unavailable");
        fs::rename(&second_temp, &unavailable).unwrap();
        let ctx = egui::Context::default();
        let mut failed = make_test_app();
        failed.restore_release_state(Box::new(record));
        for _ in 0..3 {
            let _ = ctx.run(Default::default(), |ctx| failed.finish_release_startup(ctx));
        }
        assert!(failed.allow_close);
        assert!(first_temp.is_file());
        assert!(batch.has_state(pid));

        fs::rename(&unavailable, &second_temp).unwrap();
        let mut restored = make_test_app();
        restored.buffers = vec![BufferSlot::new(TextBuffer::new())];
        restored.restore_release_state(Box::new(batch.load(pid).unwrap()));
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            restored.finish_release_startup(ctx)
        });
        assert!(first_temp.is_file());
        assert!(second_temp.is_file());
        for _ in 0..3 {
            let _ = ctx.run(Default::default(), |ctx| {
                restored.finish_release_startup(ctx)
            });
        }
        assert_eq!(restored.buffers[0].buffer.line(0), "local original");
        assert!(restored.buffers[0].buffer.dirty);
        assert_eq!(restored.buffers[1].buffer.line(0), "unsaved");
        assert!(!batch.restore_pending(pid).unwrap());
        assert!(!first_temp.exists());
        assert!(!second_temp.exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
