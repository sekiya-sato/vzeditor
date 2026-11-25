//! 検索・置換・Grep・指定行ジャンプの操作とダイアログ。

use super::*;
use std::sync::mpsc::{self, TryRecvError};
use vz_core::grep::{self, GrepOptions, GrepOutput, GrepResults};
use vz_core::search::{SearchOptions, SearchPattern};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SearchDialog {
    Find,
    Replace,
    Grep,
    Jump,
}

impl SearchDialog {
    fn title(self) -> &'static str {
        match self {
            Self::Find => "検索",
            Self::Replace => "置換",
            Self::Grep => "Grep",
            Self::Jump => "指定行へジャンプ",
        }
    }
}

enum DialogAction {
    Find(bool),
    Mark,
    Replace,
    ReplaceAll,
    Grep,
    OpenHit(usize),
    Jump,
    Close,
}

struct GrepJob {
    receiver: Receiver<std::io::Result<GrepResults>>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for GrepJob {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

pub(super) struct SearchState {
    dialog: Option<SearchDialog>,
    focus: bool,
    message: String,
    query: String,
    options: SearchOptions,
    wrap: bool,
    show_miss: bool,
    auto_close: bool,
    replace: String,
    clipboard: bool,
    selection_only: bool,
    root: String,
    recursive: bool,
    include: String,
    exclude_files: String,
    exclude_dirs: String,
    output: GrepOutput,
    first_only: bool,
    grep_job: Option<GrepJob>,
    results: Option<GrepResults>,
    line: String,
    display_row: bool,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            dialog: None,
            focus: false,
            message: String::new(),
            query: String::new(),
            options: SearchOptions::default(),
            wrap: false,
            show_miss: true,
            auto_close: true,
            replace: String::new(),
            clipboard: false,
            selection_only: false,
            root: String::new(),
            recursive: true,
            include: "*".into(),
            exclude_files: "*.exe;*.dll;*.obj;*.pdb;*.lib;*.ico;*.png;*.jpg;*.zip".into(),
            exclude_dirs: ".git;.svn;.vs;target".into(),
            output: GrepOutput::default(),
            first_only: false,
            grep_job: None,
            results: None,
            line: String::new(),
            display_row: false,
        }
    }
}

impl SearchState {
    pub(super) fn is_open(&self) -> bool {
        self.dialog.is_some()
    }

    pub(super) fn close(&mut self) {
        self.dialog = None;
        self.grep_job = None;
    }

    fn pattern(&self) -> Result<SearchPattern, String> {
        SearchPattern::new(&self.query, &self.options)
    }

    fn render(
        &mut self,
        ctx: &egui::Context,
        has_selection: bool,
        can_mark: bool,
        current_dir: &Path,
    ) -> Option<DialogAction> {
        let dialog = self.dialog?;
        let focus = std::mem::take(&mut self.focus);
        // TextEdit に渡す前に Enter をダイアログの既定操作にする。
        let mut action = if focus {
            None
        } else {
            // consume_key は余分な Shift を無視するので、Shift+Enter を先に判定する。
            let shift_enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, Key::Enter));
            let enter =
                !shift_enter && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
            match (dialog, enter, shift_enter) {
                (SearchDialog::Jump, true, _) => Some(DialogAction::Jump),
                (SearchDialog::Grep, true, _) if self.grep_job.is_none() => {
                    Some(DialogAction::Grep)
                }
                (SearchDialog::Find | SearchDialog::Replace, true, _) => {
                    Some(DialogAction::Find(false))
                }
                (SearchDialog::Find | SearchDialog::Replace, _, true) => {
                    Some(DialogAction::Find(true))
                }
                _ => None,
            }
        };
        let modal = egui::Modal::new(egui::Id::new(("search_dialog", dialog.title())))
            .frame(egui::Frame::popup(&ctx.style()).fill(STATUS_BG))
            .show(ctx, |ui| {
                let max_width = match dialog {
                    SearchDialog::Jump => 320.0,
                    SearchDialog::Find | SearchDialog::Replace => 540.0,
                    SearchDialog::Grep => 650.0,
                };
                ui.set_width((ctx.screen_rect().width() - 48.0).clamp(160.0, max_width));
                ui.heading(dialog.title());
                ui.separator();
                let max_h = (ctx.screen_rect().height() - 130.0).max(80.0);
                egui::ScrollArea::vertical()
                    .max_height(max_h)
                    .show(ui, |ui| {
                        if dialog == SearchDialog::Jump {
                            ui.horizontal(|ui| {
                                ui.label("行番号");
                                let field = ui.add(
                                    egui::TextEdit::singleline(&mut self.line).desired_width(100.0),
                                );
                                focus_and_select(ui, focus, field, self.line.chars().count());
                            });
                            ui.radio_value(&mut self.display_row, false, "改行単位");
                            ui.radio_value(&mut self.display_row, true, "折り返し単位");
                            if ui.button("ジャンプ").clicked() {
                                action = Some(DialogAction::Jump);
                            }
                        } else {
                            let running = self.grep_job.is_some();
                            ui.add_enabled_ui(!running, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(if dialog == SearchDialog::Replace {
                                        "置換前"
                                    } else {
                                        "検索条件"
                                    });
                                    let field = ui.add(
                                        egui::TextEdit::singleline(&mut self.query)
                                            .desired_width(f32::INFINITY),
                                    );
                                    focus_and_select(ui, focus, field, self.query.chars().count());
                                });
                                if dialog == SearchDialog::Replace {
                                    ui.horizontal(|ui| {
                                        ui.label("置換後");
                                        ui.add_enabled(
                                            !self.clipboard,
                                            egui::TextEdit::singleline(&mut self.replace)
                                                .desired_width(f32::INFINITY),
                                        );
                                    });
                                    ui.checkbox(&mut self.clipboard, "クリップボードの内容で置換");
                                    ui.add_enabled(
                                        has_selection || self.selection_only,
                                        egui::Checkbox::new(
                                            &mut self.selection_only,
                                            "すべて置換の対象を現在の選択範囲に限定",
                                        ),
                                    );
                                }
                                ui.horizontal_wrapped(|ui| {
                                    ui.checkbox(&mut self.options.whole_word, "単語単位");
                                    ui.checkbox(
                                        &mut self.options.case_sensitive,
                                        "大文字・小文字を区別",
                                    );
                                    ui.checkbox(&mut self.options.regex, "正規表現");
                                });
                                if dialog == SearchDialog::Grep {
                                    self.render_grep_options(ui, current_dir);
                                } else {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.checkbox(&mut self.wrap, "先頭／末尾から再検索");
                                        ui.checkbox(&mut self.show_miss, "見つからないときに表示");
                                        ui.checkbox(
                                            &mut self.auto_close,
                                            "検索で見つかったら閉じる",
                                        );
                                    });
                                    ui.horizontal_wrapped(|ui| {
                                        if ui.button("上検索").clicked() {
                                            action = Some(DialogAction::Find(true));
                                        }
                                        if ui.button("下検索").clicked() {
                                            action = Some(DialogAction::Find(false));
                                        }
                                        if ui
                                            .button(if can_mark {
                                                "行マーク解除"
                                            } else {
                                                "該当行をマーク"
                                            })
                                            .clicked()
                                        {
                                            action = Some(DialogAction::Mark);
                                        }
                                        if dialog == SearchDialog::Replace {
                                            if ui.button("置換").clicked() {
                                                action = Some(DialogAction::Replace);
                                            }
                                            if ui.button("すべて置換").clicked() {
                                                action = Some(DialogAction::ReplaceAll);
                                            }
                                        }
                                    });
                                }
                            });
                            if dialog == SearchDialog::Grep {
                                if running {
                                    ui.horizontal(|ui| {
                                        ui.spinner();
                                        ui.label("検索中");
                                    });
                                } else {
                                    self.render_grep_results(ui, &mut action);
                                }
                            }
                        }
                    });
                if !self.message.is_empty() {
                    ui.label(&self.message);
                }
                if ui
                    .button(if self.grep_job.is_some() {
                        "検索を中止して閉じる"
                    } else {
                        "閉じる"
                    })
                    .clicked()
                {
                    action = Some(DialogAction::Close);
                }
            });
        if !focus && modal.should_close() {
            Some(DialogAction::Close)
        } else {
            action
        }
    }

    fn render_grep_options(&mut self, ui: &mut egui::Ui, current_dir: &Path) {
        ui.horizontal(|ui| {
            ui.label("検索場所");
            ui.text_edit_singleline(&mut self.root);
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("現フォルダー").clicked() {
                self.root = current_dir.to_string_lossy().into_owned();
            }
            if ui.button("上階層").clicked() {
                if let Some(parent) = Path::new(&self.root)
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                {
                    self.root = parent.to_string_lossy().into_owned();
                }
            }
            ui.checkbox(&mut self.recursive, "サブフォルダーも検索");
        });
        egui::Grid::new("grep_filters")
            .num_columns(2)
            .show(ui, |ui| {
                for (label, field) in [
                    ("対象ファイル", &mut self.include),
                    ("除外ファイル", &mut self.exclude_files),
                    ("除外フォルダー", &mut self.exclude_dirs),
                ] {
                    ui.label(label);
                    ui.add(
                        egui::TextEdit::singleline(field)
                            .desired_width((ui.available_width() - 110.0).clamp(80.0, 460.0)),
                    );
                    ui.end_row();
                }
            });
        ui.horizontal_wrapped(|ui| {
            ui.radio_value(&mut self.output, GrepOutput::Line, "該当行");
            ui.radio_value(&mut self.output, GrepOutput::Match, "該当部分");
            ui.radio_value(&mut self.output, GrepOutput::NonMatch, "非該当行");
            ui.checkbox(&mut self.first_only, "各ファイルの最初の結果だけ");
        });
    }

    fn render_grep_results(&self, ui: &mut egui::Ui, action: &mut Option<DialogAction>) {
        if ui.button("検索").clicked() {
            *action = Some(DialogAction::Grep);
        }
        let Some(results) = &self.results else { return };
        ui.separator();
        let mut summary = format!("{}件", results.hits.len());
        if results.limited {
            summary.push_str("（表示上限）");
        }
        if results.skipped > 0 {
            summary.push_str(&format!("　読み飛ばし {}件", results.skipped));
        }
        ui.label(summary);
        let height = (ui.ctx().screen_rect().height() * 0.35).min(260.0);
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        egui::ScrollArea::vertical()
            .id_salt("grep_results")
            .max_height(height)
            .show_rows(ui, row_h, results.hits.len(), |ui, rows| {
                for i in rows {
                    let hit = &results.hits[i];
                    let label = format!(
                        "{}:{}:{}  {}",
                        hit.path.display(),
                        hit.line + 1,
                        hit.col + 1,
                        hit.text
                    );
                    if ui
                        .add(egui::Button::new(&label).truncate().frame(false))
                        .on_hover_text(&label)
                        .clicked()
                    {
                        *action = Some(DialogAction::OpenHit(i));
                    }
                }
            });
    }
}

fn focus_and_select(ui: &egui::Ui, focus: bool, field: egui::Response, len: usize) {
    if focus {
        field.request_focus();
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field.id) {
            use egui::text::{CCursor, CCursorRange};
            state
                .cursor
                .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(len))));
            state.store(ui.ctx(), field.id);
        }
    }
}

impl VzEditorApp {
    fn grep_current_dir(&self) -> &Path {
        self.cur_buf()
            .file_path
            .as_deref()
            .and_then(Path::parent)
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or(&self.filer.active_pane().current_dir)
    }

    pub(super) fn open_search_dialog(&mut self, dialog: SearchDialog) {
        if self.mode != AppMode::Editor || self.search.is_open() {
            return;
        }
        self.show_help = false;
        self.open_menu = None;
        self.prefix = PrefixKey::None;
        self.ime_preedit.clear();
        self.ime_active = false;
        self.search.dialog = Some(dialog);
        self.search.focus = true;
        self.search.message.clear();
        match dialog {
            SearchDialog::Find => {
                if let Some((start, end)) =
                    self.cur_buf().selection_range().filter(|(s, e)| s.0 == e.0)
                {
                    // 直前の検索結果が選択されたままなら、正規表現などの検索条件を残す。
                    let is_last_hit = self.search.pattern().is_ok_and(|pattern| {
                        pattern
                            .replacement_for(&self.cur_buf().lines[start.0], start.1, end.1, "")
                            .is_some()
                    });
                    if !is_last_hit {
                        self.search.query = self.cur_buf().text_in_range(start, end);
                    }
                }
            }
            SearchDialog::Replace => self.search.selection_only = false,
            SearchDialog::Grep if self.search.root.is_empty() => {
                self.search.root = self.grep_current_dir().to_string_lossy().into_owned();
            }
            SearchDialog::Jump => {
                if self.word_wrap && self.search.display_row {
                    self.ensure_wrap_layout(self.wrap_cols);
                }
                let (line, col) = self.cur_buf().cursor_pos();
                let row = self
                    .cur_slot()
                    .wrap
                    .as_ref()
                    .filter(|_| self.word_wrap && self.search.display_row)
                    .map_or(line, |wrap| wrap.row_of(line, col));
                self.search.line = (row + 1).to_string();
            }
            _ => {}
        }
    }

    fn find_match(&mut self, pattern: &SearchPattern, up: bool) -> bool {
        if let Some((start, end)) = pattern.find(self.cur_buf(), up, self.search.wrap) {
            self.cur_slot_mut().markdown_view = false;
            self.cur_buf_mut().select_range(start, end);
            self.select_mode = false;
            self.need_scroll_to_cursor = true;
            self.status_message = format!("検索結果　{}行 {}桁", start.0 + 1, start.1 + 1);
            true
        } else {
            if self.search.show_miss {
                self.search.message = "一致する文字列が見つかりません".into();
                self.status_message = self.search.message.clone();
            }
            false
        }
    }

    pub(super) fn repeat_search(&mut self, up: bool) {
        self.search.message.clear();
        match self.search.pattern() {
            Ok(pattern) => {
                self.find_match(&pattern, up);
            }
            Err(message) => self.status_message = message,
        }
    }

    pub(super) fn toggle_search_marks(&mut self) -> Result<(), String> {
        if self
            .cur_slot_mut()
            .search_marks
            .take()
            .is_some_and(|(revision, _)| revision == self.cur_buf().revision)
        {
            self.status_message = "検索の行マークを解除しました".into();
        } else {
            let pattern = self.search.pattern()?;
            let buf = self.cur_buf();
            let rows: Vec<_> = buf
                .lines
                .iter()
                .enumerate()
                .filter_map(|(i, line)| pattern.is_match(line).then_some(i))
                .collect();
            self.status_message = format!("{}行をマークしました", rows.len());
            let revision = self.cur_buf().revision;
            self.cur_slot_mut().search_marks = Some((revision, rows));
            self.cur_slot_mut().markdown_view = false;
        }
        Ok(())
    }

    fn replace_match(&mut self, pattern: &SearchPattern, all: bool) -> Result<(), String> {
        let replacement = if self.search.clipboard {
            self.clipboard_text()
        } else {
            self.search.replace.clone()
        };
        let selection = self.cur_buf().selection_range();
        if all {
            let scope = if self.search.selection_only {
                Some(selection.ok_or("置換する範囲を選択してください")?)
            } else {
                None
            };
            let count = pattern.replace_all(self.cur_buf_mut(), scope, &replacement);
            self.status_message = format!("{count}件を置換しました");
        } else {
            let cursor = self.cur_buf().cursor_pos();
            let (start, end) = selection.unwrap_or((cursor, cursor));
            let output = if start.0 == end.0
                && (selection.is_some() || self.cur_buf().anchor == Some(cursor))
            {
                pattern.replacement_for(
                    &self.cur_buf().lines[start.0],
                    start.1,
                    end.1,
                    &replacement,
                )
            } else {
                None
            };
            let Some(output) = output else {
                self.find_match(pattern, false);
                return Ok(());
            };
            if output.is_empty() && start != end {
                self.cur_buf_mut().delete_selection();
            } else if start != end || !output.is_empty() {
                self.cur_buf_mut().insert_text(&output);
            }
            // サクラと同じく次の一致を選択する。置換後の位置の空一致は飛ばす。
            let wrap = self.search.wrap;
            let buf = self.cur_buf_mut();
            let cursor = buf.cursor_pos();
            buf.select_range(cursor, cursor);
            match pattern.find(buf, false, wrap) {
                Some((next_start, next_end)) => {
                    buf.select_range(next_start, next_end);
                    self.status_message = "1件を置換しました".into();
                }
                None => {
                    buf.clear_selection();
                    self.status_message = "1件を置換しました（次の一致はありません）".into();
                }
            }
        }
        self.search.message = self.status_message.clone();
        self.cur_slot_mut().markdown_view = false;
        self.select_mode = false;
        self.need_scroll_to_cursor = true;
        Ok(())
    }

    fn jump_line(&mut self) -> Result<(), String> {
        let row = self
            .search
            .line
            .trim()
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1))
            .ok_or("1以上の行番号を入力してください")?;
        let (line, col) = if self.search.display_row && self.word_wrap {
            self.ensure_wrap_layout(self.wrap_cols);
            let layout = self
                .cur_slot()
                .wrap
                .as_ref()
                .ok_or("折り返し行を取得できません")?;
            if row >= layout.total_rows() {
                return Err(format!("行番号は1〜{}です", layout.total_rows()));
            }
            let (line, seg) = layout.line_of_row(row);
            (
                line,
                layout
                    .seg_range(line, seg, self.cur_buf().line_char_len(line))
                    .0,
            )
        } else {
            if row >= self.cur_buf().lines.len() {
                return Err(format!("行番号は1〜{}です", self.cur_buf().lines.len()));
            }
            (row, 0)
        };
        self.move_to_search_position(line, col);
        self.status_message = format!("{}行へ移動しました", row + 1);
        Ok(())
    }

    fn move_to_search_position(&mut self, line: usize, col: usize) {
        let slot = self.cur_slot_mut();
        slot.markdown_view = false;
        slot.buffer.clear_selection();
        slot.buffer.set_cursor(line, col);
        self.need_scroll_to_cursor = true;
        self.select_mode = false;
    }

    fn start_grep(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let pattern = self.search.pattern()?;
        let root = Path::new(self.search.root.trim());
        if root.as_os_str().is_empty() {
            return Err("検索場所を指定してください".into());
        }
        let root = if root.is_absolute() {
            root.to_path_buf()
        } else {
            self.grep_current_dir().join(root)
        };
        let options = GrepOptions {
            root,
            recursive: self.search.recursive,
            include: grep::file_patterns(&self.search.include)?,
            exclude_files: grep::file_patterns(&self.search.exclude_files)?,
            exclude_dirs: grep::file_patterns(&self.search.exclude_dirs)?,
            output: self.search.output,
            first_only: self.search.first_only,
        };
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancelled);
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("vz-grep".into())
            .spawn(move || {
                let outcome = std::fs::canonicalize(&options.root).and_then(|root| {
                    if !root.is_dir() {
                        return Err(std::io::Error::from(std::io::ErrorKind::NotADirectory));
                    }
                    Ok(grep::grep(
                        &pattern,
                        &GrepOptions { root, ..options },
                        &stop,
                    ))
                });
                let _ = sender.send(outcome);
                ctx.request_repaint();
            })
            .map_err(|err| format!("Grep を開始できません: {err}"))?;
        self.search.results = None;
        self.search.grep_job = Some(GrepJob {
            receiver,
            cancelled,
        });
        Ok(())
    }

    fn poll_grep(&mut self) {
        let Some(job) = &self.search.grep_job else {
            return;
        };
        match job.receiver.try_recv() {
            Ok(Ok(results)) => self.search.results = Some(results),
            Ok(Err(err)) => self.search.message = format!("検索場所を開けません: {err}"),
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => self.search.message = "Grep が中断されました".into(),
        }
        self.search.grep_job = None;
    }

    fn perform_search_action(
        &mut self,
        ctx: &egui::Context,
        action: DialogAction,
    ) -> Result<(), String> {
        match action {
            DialogAction::Find(up) => {
                let pattern = self.search.pattern()?;
                if self.find_match(&pattern, up) && self.search.auto_close {
                    self.search.close();
                }
            }
            DialogAction::Mark => {
                self.toggle_search_marks()?;
                self.search.message = self.status_message.clone();
            }
            DialogAction::Replace | DialogAction::ReplaceAll => {
                let pattern = self.search.pattern()?;
                self.replace_match(&pattern, matches!(action, DialogAction::ReplaceAll))?;
            }
            DialogAction::Grep => self.start_grep(ctx)?,
            DialogAction::OpenHit(i) => {
                if let Some(hit) = self
                    .search
                    .results
                    .as_ref()
                    .and_then(|results| results.hits.get(i))
                {
                    let (path, line, col) = (hit.path.clone(), hit.line, hit.col);
                    if self.open_file_in_buffer(path) {
                        self.move_to_search_position(line, col);
                        self.search.close();
                    } else {
                        self.search.message = self.status_message.clone();
                    }
                }
            }
            DialogAction::Jump => {
                self.jump_line()?;
                self.search.close();
            }
            DialogAction::Close => self.search.close(),
        }
        Ok(())
    }

    pub(super) fn render_search_dialog(&mut self, ctx: &egui::Context) {
        if !self.search.is_open() {
            return;
        }
        self.poll_grep();
        let has_selection = self.cur_buf().has_selection();
        let can_mark = self
            .cur_slot()
            .search_marks
            .as_ref()
            .is_some_and(|(rev, _)| *rev == self.cur_buf().revision);
        let current_dir = self.grep_current_dir().to_path_buf();
        if let Some(action) = self
            .search
            .render(ctx, has_selection, can_mark, &current_dir)
        {
            self.search.message.clear();
            if let Err(message) = self.perform_search_action(ctx, action) {
                self.search.message = message;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::make_test_app;
    use super::*;

    fn app_with_text(text: &str) -> VzEditorApp {
        let mut app = make_test_app();
        let mut buf = TextBuffer::new();
        buf.insert_text(text);
        buf.move_doc_start();
        app.buffers = vec![BufferSlot::new(buf)];
        app
    }

    fn input_frame(ctx: &egui::Context, app: &mut VzEditorApp, events: Vec<Event>) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            app.handle_events(ctx);
            app.render_search_dialog(ctx);
        });
    }

    fn key_event(key: Key, modifiers: egui::Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn search_dialog_enter_selects_match_instead_of_editing() {
        let ctx = egui::Context::default();
        let mut app = app_with_text("一 猫\n猫");
        app.key_scheme = KeyScheme::Sakura;
        input_frame(
            &ctx,
            &mut app,
            vec![key_event(Key::F, egui::Modifiers::CTRL)],
        );
        assert_eq!(app.search.dialog, Some(SearchDialog::Find));
        input_frame(&ctx, &mut app, vec![Event::Text("猫".into())]);
        assert_eq!(app.search.query, "猫");
        input_frame(
            &ctx,
            &mut app,
            vec![key_event(Key::Enter, egui::Modifiers::NONE)],
        );
        assert!(!app.search.is_open());
        assert_eq!(app.cur_buf().selection_range(), Some(((0, 2), (0, 3))));
        input_frame(
            &ctx,
            &mut app,
            vec![key_event(Key::N, egui::Modifiers::ALT)],
        );
        assert_eq!(app.cur_buf().selection_range(), Some(((1, 0), (1, 1))));
        assert_eq!(app.cur_buf().lines, ["一 猫", "猫"]);
    }

    #[test]
    fn shift_enter_searches_up() {
        let ctx = egui::Context::default();
        let mut app = app_with_text("猫\n猫\n猫");
        app.cur_buf_mut().set_cursor(1, 1);
        app.search.query = "猫".into();
        app.search.auto_close = false;
        app.open_search_dialog(SearchDialog::Find);
        input_frame(&ctx, &mut app, vec![]);
        input_frame(
            &ctx,
            &mut app,
            vec![key_event(Key::Enter, egui::Modifiers::SHIFT)],
        );
        assert_eq!(app.cur_buf().selection_range(), Some(((1, 0), (1, 1))));
    }

    #[test]
    fn reopening_find_keeps_regex_query_for_its_own_hit() {
        let mut app = app_with_text("a12 b3");
        app.search.query = r"\d+".into();
        app.search.options.regex = true;
        app.repeat_search(false);
        assert_eq!(app.cur_buf().selection_range(), Some(((0, 1), (0, 3))));
        app.open_search_dialog(SearchDialog::Find);
        assert_eq!(app.search.query, r"\d+");
        app.search.close();
        app.cur_buf_mut().select_range((0, 4), (0, 5));
        app.open_search_dialog(SearchDialog::Find);
        assert_eq!(app.search.query, "b");
    }

    #[test]
    fn single_replace_selects_next_match() {
        let ctx = egui::Context::default();
        let mut app = app_with_text("a a\na");
        app.open_search_dialog(SearchDialog::Replace);
        app.search.query = "a".into();
        app.search.replace = "bb".into();
        app.perform_search_action(&ctx, DialogAction::Replace)
            .unwrap();
        assert_eq!(app.cur_buf().selection_range(), Some(((0, 0), (0, 1))));
        app.perform_search_action(&ctx, DialogAction::Replace)
            .unwrap();
        assert_eq!(app.cur_buf().lines, ["bb a", "a"]);
        assert_eq!(app.cur_buf().selection_range(), Some(((0, 3), (0, 4))));
        app.perform_search_action(&ctx, DialogAction::Replace)
            .unwrap();
        app.perform_search_action(&ctx, DialogAction::Replace)
            .unwrap();
        assert_eq!(app.cur_buf().lines, ["bb bb", "bb"]);
        assert!(!app.cur_buf().has_selection());
        // 空一致は置換後の位置で繰り返さない
        let mut app = app_with_text("x\ny");
        app.open_search_dialog(SearchDialog::Replace);
        app.search.query = "$".into();
        app.search.options.regex = true;
        app.search.replace = ";".into();
        for _ in 0..4 {
            app.perform_search_action(&ctx, DialogAction::Replace)
                .unwrap();
        }
        assert_eq!(app.cur_buf().lines, ["x;", "y;"]);
    }

    #[test]
    fn scoped_replace_undo_and_display_row_jump() {
        let ctx = egui::Context::default();
        let mut app = app_with_text("abcdefgh\nabc");
        app.cur_buf_mut().select_range((1, 0), (1, 3));
        app.open_search_dialog(SearchDialog::Replace);
        app.search.query = "abc".into();
        app.search.replace = "猫".into();
        app.search.selection_only = true;
        app.perform_search_action(&ctx, DialogAction::ReplaceAll)
            .unwrap();
        assert_eq!(app.cur_buf().lines, ["abcdefgh", "猫"]);
        assert!(app.cur_buf_mut().undo());
        app.search.close();
        app.word_wrap = true;
        app.wrap_cols = 4;
        app.open_search_dialog(SearchDialog::Jump);
        app.search.display_row = true;
        app.search.line = "0".into();
        assert!(app.perform_search_action(&ctx, DialogAction::Jump).is_err());
        app.search.line = "2".into();
        app.perform_search_action(&ctx, DialogAction::Jump).unwrap();
        assert_eq!(app.cur_buf().cursor_pos(), (0, 4));
        assert!(!app.search.is_open());
    }

    #[test]
    fn grep_result_opens_exact_file_and_position() {
        let ctx = egui::Context::default();
        let mut app = app_with_text("");
        let root = std::env::temp_dir().join(format!("vz-ui-grep-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("sample.txt");
        fs::write(&path, "first\r\n字 猫").unwrap();
        app.open_search_dialog(SearchDialog::Grep);
        app.search.root = root.to_string_lossy().into_owned();
        app.search.query = "猫".into();
        app.start_grep(&ctx).unwrap();
        let job = app.search.grep_job.take().unwrap();
        app.search.results = Some(
            job.receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
                .unwrap(),
        );
        app.perform_search_action(&ctx, DialogAction::OpenHit(0))
            .unwrap();
        assert_eq!(
            app.cur_buf().file_path.as_deref(),
            Some(canonical_or_self(&path).as_path())
        );
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&root).unwrap();
        assert_eq!(app.cur_buf().cursor_pos(), (1, 2));
        assert!(!app.search.is_open());
    }
}
