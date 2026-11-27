//! 印刷（Ctrl+P）: 確認ダイアログ、割付（`vz_core::print`）、裏のスレッドでの印刷（`vz_platform::print`）。

use super::*;
use vz_core::print::{PrintDoc, TooLarge, MAX_FONT_PT, MIN_FONT_PT};

/// 印刷できないときの文言（巨大ファイル・ページ数の目安を超えたとき）
const TOO_LARGE_MESSAGE: &str = "ファイルが大きすぎます";

impl VzEditorApp {
    /// 確認ダイアログを開く。印刷できない場合はステータスバーに理由を出す。
    pub(super) fn open_print_dialog(&mut self) {
        if self.mode != AppMode::Editor || self.filer_modal.is_some() {
            return;
        }
        if self.pending_print.is_some() {
            self.status_message = "印刷中です。完了までお待ちください".to_string();
            return;
        }
        if self.cur_buf().is_huge() {
            self.status_message = TOO_LARGE_MESSAGE.to_string();
            return;
        }
        let printers = vz_platform::print::printers();
        if printers.is_empty() {
            self.status_message = "プリンターがありません".to_string();
            return;
        }
        let printer = initial_printer(
            &printers,
            self.print_printer.as_deref(),
            vz_platform::print::default_printer().as_deref(),
        );
        let Ok(doc) = self.layout_for_print() else {
            self.status_message = TOO_LARGE_MESSAGE.to_string();
            return;
        };
        self.filer_modal = Some(FilerModal::ConfirmPrint {
            printers,
            printer,
            pages: Some(doc.pages.len()),
            shown: std::time::Instant::now(),
        });
    }

    /// 現在のバッファを今の印刷設定で割り付ける。
    /// Markdown ビュー表示中はビューの見た目のまま割り付ける（行番号は印字しない）。
    pub(super) fn layout_for_print(&mut self) -> Result<PrintDoc, TooLarge> {
        let buf = self.cur_buf();
        if buf.is_huge() {
            return Err(TooLarge);
        }
        let title = buf
            .file_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| buf.display_name());
        let printed_at = vz_core::print::printed_at_now();
        if self.is_markdown_view() {
            self.cur_slot_mut().ensure_md_cache();
            let setup = self.print_setup.clamped();
            let pages = crate::markdown::layout_markdown_print(
                &self.egui_ctx,
                &self.cur_slot().markdown.blocks,
                &setup,
            );
            return PrintDoc::from_items(setup, &title, &printed_at, pages);
        }
        vz_core::print::layout(buf.text_lines(), self.print_setup, &title, &printed_at)
    }

    /// 印刷の確認ダイアログ。閉じたら false。
    /// `pages` は今の設定でのページ数（目安を超えたら None で、印刷できない）。
    pub(super) fn render_print_dialog(
        &mut self,
        ctx: &egui::Context,
        printers: &[String],
        printer: &mut usize,
        pages: &mut Option<usize>,
        shown: std::time::Instant,
    ) -> bool {
        // 表示直後の打鍵（Ctrl+P の押し続けなど）では答えない
        let keys_ok = shown.elapsed() >= std::time::Duration::from_millis(300);
        // プリンターの一覧を開いている間のキーは一覧の操作（Esc で一覧だけ閉じる）
        let list_open = ctx.memory(|m| m.any_popup_open());
        let mut setup = self.print_setup;
        let mut answer: Option<bool> = None;
        modal_window(ctx, "印刷", TEXT_CYAN).show(ctx, |ui| {
            ui.set_min_width(420.0);
            ui.label(
                egui::RichText::new("印刷しますか？")
                    .font(FontId::monospace(14.0))
                    .color(TEXT_WHITE),
            );
            ui.add_space(6.0);
            let mono = |s: &str| egui::RichText::new(s).font(FontId::monospace(13.5));
            ui.horizontal(|ui| {
                ui.label(mono("プリンター"));
                egui::ComboBox::from_id_salt("print_printer")
                    .width(260.0)
                    .selected_text(mono(&printers[*printer]))
                    .show_index(ui, printer, printers.len(), |i| mono(&printers[i]));
                ui.label(mono("(PgUp PgDn)").color(TEXT_DIM_CYAN));
            });
            ui.horizontal(|ui| {
                ui.label(mono("用紙　　　"));
                for o in [Orientation::Portrait, Orientation::Landscape] {
                    ui.radio_value(&mut setup.orientation, o, mono(o.label()));
                }
                ui.label(mono("(← →)").color(TEXT_DIM_CYAN));
            });
            ui.horizontal(|ui| {
                ui.label(mono("文字サイズ"));
                if ui
                    .add_enabled(setup.font_pt > MIN_FONT_PT, egui::Button::new(mono("－")))
                    .clicked()
                {
                    setup.font_pt -= 1;
                }
                ui.label(mono(&format!("{:>2} pt", setup.font_pt)));
                if ui
                    .add_enabled(setup.font_pt < MAX_FONT_PT, egui::Button::new(mono("＋")))
                    .clicked()
                {
                    setup.font_pt += 1;
                }
                ui.label(mono("(↑ ↓)").color(TEXT_DIM_CYAN));
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut setup.page_numbers, mono("ページ番号を印字する"));
                ui.label(mono("(Space)").color(TEXT_DIM_CYAN));
            });
            ui.add_space(6.0);
            match pages {
                Some(n) => ui.label(mono(&format!("{n} ページ")).color(TEXT_WHITE)),
                None => ui.label(mono(TOO_LARGE_MESSAGE).color(TEXT_AMBER)),
            };
            ui.add_space(8.0);
            let can_print = pages.is_some();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(can_print, egui::Button::new("印刷 (Enter / Y)"))
                    .clicked()
                    || (keys_ok
                        && !list_open
                        && can_print
                        && ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Y)))
                {
                    answer = Some(true);
                }
                if ui.button("キャンセル (ESC / N)").clicked()
                    || (!list_open
                        && ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::N)))
                {
                    answer = Some(false);
                }
            });
            if list_open {
                return;
            }
            ui.input(|i| {
                if i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowRight) {
                    setup.orientation = setup.orientation.toggled();
                }
                if i.key_pressed(Key::ArrowUp) && setup.font_pt < MAX_FONT_PT {
                    setup.font_pt += 1;
                }
                if i.key_pressed(Key::ArrowDown) && setup.font_pt > MIN_FONT_PT {
                    setup.font_pt -= 1;
                }
                if i.key_pressed(Key::Space) {
                    setup.page_numbers = !setup.page_numbers;
                }
                if i.key_pressed(Key::PageUp) {
                    *printer = (*printer + printers.len() - 1) % printers.len();
                }
                if i.key_pressed(Key::PageDown) {
                    *printer = (*printer + 1) % printers.len();
                }
            });
        });

        if setup != self.print_setup {
            self.print_setup = setup;
            self.save_settings();
            *pages = self.layout_for_print().ok().map(|doc| doc.pages.len());
        }
        match answer {
            Some(true) => {
                let name = printers[*printer].clone();
                self.print_printer = Some(name.clone());
                self.start_print(name);
                false
            }
            Some(false) => {
                self.status_message = "印刷をキャンセルしました".to_string();
                false
            }
            None => true,
        }
    }

    /// 割り付けて、裏のスレッドで printer へ印刷する。
    fn start_print(&mut self, printer: String) {
        match self.layout_for_print() {
            Ok(doc) => {
                self.status_message = format!("印刷中... ({} ページ)", doc.pages.len());
                self.pending_print = Some(vz_platform::print::spawn_print(doc, printer));
            }
            Err(TooLarge) => self.status_message = TOO_LARGE_MESSAGE.to_string(),
        }
    }

    pub(super) fn poll_print(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.pending_print else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.pending_print = None;
                self.status_message = match result {
                    Ok(msg) => msg,
                    Err(msg) => format!("印刷エラー: {msg}"),
                };
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_print = None;
                self.status_message = "印刷エラー: 印刷処理が中断されました".to_string();
            }
        }
    }
}

/// 印刷ダイアログで最初に選ぶプリンター: 前回印刷したもの → 既定 → 先頭。
pub(super) fn initial_printer(
    printers: &[String],
    last: Option<&str>,
    default: Option<&str>,
) -> usize {
    [last, default]
        .into_iter()
        .flatten()
        .find_map(|name| printers.iter().position(|p| p == name))
        .unwrap_or(0)
}
