//! Claude Code / Codex / GitHub Desktop への送信（Alt+C → C / X / N / M / P）。

use super::*;

impl VzEditorApp {
    /// 現在のバッファ全体を送信先アプリへ送る（別スレッド）。送信後も内容は残す。
    pub(super) fn start_send(&mut self, target: SendTarget) {
        self.send_prefix = false;
        if self.pending_send.is_some() {
            self.status_message = "送信中です。完了までお待ちください".to_string();
            return;
        }
        if !target.sends_text() {
            // 本文は送らず、GitHub Desktop は Push（Ctrl+P）、Claude Code / Codex は新規セッション（Ctrl+N）を押す
            let (_, name) = target.key_command();
            self.status_message = format!(
                "{} へ {name} を指示中... (Alt などのキーを離してください)",
                target.label()
            );
            self.pending_send = Some(spawn_send(target, String::new()));
            return;
        }
        let Some(text) = prepare_text(self.cur_buf().text_lines()) else {
            self.status_message = format!("{} へ送る文字がありません", target.label());
            return;
        };
        self.status_message = format!(
            "{} へ送信中... (Alt などのキーを離してください)",
            target.label()
        );
        self.pending_send = Some(spawn_send(target, text));
    }

    pub(super) fn poll_send(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.pending_send else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.pending_send = None;
                self.status_message = match result {
                    Ok(msg) => msg,
                    Err(msg) => format!("送信エラー: {msg}"),
                };
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_send = None;
                self.status_message = "送信エラー: 送信処理が中断されました".to_string();
            }
        }
    }

    /// Alt+C → C（Claude Code）/ X（Codex）/ P（GitHub Desktop の Push）/ N・M（Claude Code・Codex の新規セッション）
    /// の2打鍵を処理し、使ったキーは入力から取り除く。2打目は Alt を押したままでも離してもよい。それ以外のキーなら取り消す。
    pub(super) fn handle_send_keys(&mut self, ctx: &egui::Context) {
        let mut action: Option<Option<SendTarget>> = None; // Some(None) = 取り消し
        let mut consumed = false;
        let mut prefix = self.send_prefix;
        ctx.input_mut(|i| {
            i.events.retain(|ev| {
                let Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } = ev
                else {
                    return true;
                };
                let ctrl = modifiers.ctrl || modifiers.command;
                if prefix {
                    if !*pressed {
                        // 2打目の離し
                        if matches!(key, Key::C | Key::X | Key::P | Key::N | Key::M) {
                            consumed = true;
                            return false;
                        }
                        return true;
                    }
                    if action.is_none() {
                        action = Some(send_target_for_key(*key, ctrl));
                        prefix = false;
                        consumed = true;
                        return false;
                    }
                    return true;
                }
                if *key == Key::C && modifiers.alt && !ctrl && !modifiers.shift {
                    if *pressed {
                        prefix = true;
                    }
                    consumed = true;
                    return false;
                }
                true
            });
            if consumed {
                i.events.retain(|ev| !matches!(ev, Event::Text(_)));
            }
        });
        let started_prefix = prefix && !self.send_prefix;
        self.send_prefix = prefix;
        if started_prefix {
            self.status_message =
                "Alt+C: 続けて C で Claude Code へ / X で Codex へ送信、N / M で Claude Code / Codex の新規セッション、P で GitHub Desktop の Push（他のキーで取り消し）"
                    .to_string();
        }
        match action {
            Some(Some(target)) => self.start_send(target),
            Some(None) => self.status_message = "送信を取り消しました".to_string(),
            None => {}
        }
    }

    // --- キー割り当て（サクラ互換 / VZ 互換） ---
}
