//! 簡易ブラウザ（Alt+B）: 画面の切替、タブ、URL 欄、WebView の位置と表示の同期。
//! WebView 自体は vz-browser（Windows: WebView2、macOS: WKWebView。Linux は外部ブラウザ）。

use super::*;
use vz_browser::{file_url, is_web_file, normalize_url, Bounds, BrowserEvent, EmbeddedBrowser};

/// ブラウザのタブ 1 つ分。WebView は初めて表示するときに作る。
#[derive(Default)]
pub(super) struct BrowserTab {
    view: Option<EmbeddedBrowser>,
    pub(super) url_input: String,
    pub(super) title: String,
    /// WebView を作ったときに開く URL（作る前に指定されたもの）
    pub(super) pending_url: Option<String>,
    /// WebView を作れなかった理由（作り直さない）
    error: Option<String>,
    /// 次に表示したときにページへキー入力を渡す
    focus_page: bool,
}

/// 案内ページ・空白ページ（復元・ファイルの照合では URL として扱わない）。
fn is_blank_url(url: &str) -> bool {
    url.starts_with("data:") || url.starts_with("about:")
}

impl BrowserTab {
    pub(super) fn failed_to_open(&self) -> bool {
        self.error.is_some()
    }

    pub(super) fn new(url: Option<String>) -> Self {
        Self {
            url_input: url.clone().unwrap_or_default(),
            pending_url: url,
            focus_page: true,
            ..Self::default()
        }
    }

    /// 表示中（または作るときに開く）の URL。案内ページ・空白ページは None。
    pub(super) fn current_url(&self) -> Option<String> {
        self.view
            .as_ref()
            .and_then(EmbeddedBrowser::url)
            .or_else(|| self.pending_url.clone())
            .filter(|u| !is_blank_url(u))
    }

    /// 復元用の URL（URL 欄の入力も見る）。
    pub(super) fn session_url(&self) -> Option<String> {
        self.current_url()
            .or_else(|| normalize_url(&self.url_input).filter(|u| !is_blank_url(u)))
    }

    /// タブ帯・タイトルバーに出す名前。
    pub(super) fn label(&self) -> &str {
        if self.title.is_empty() {
            &self.url_input
        } else {
            &self.title
        }
    }

    /// URL を開く（WebView がまだ無ければ作るときに開く）。
    fn open(&mut self, url: String) {
        match &self.view {
            Some(b) => b.load(&url),
            None => self.pending_url = Some(url.clone()),
        }
        self.url_input = url;
        self.focus_page = true;
    }
}

impl VzEditorApp {
    /// 編集中のファイルがブラウザで確認できるもの（HTML・SVG）なら、そのパス。
    fn current_web_file(&self) -> Option<PathBuf> {
        if self.mode != AppMode::Editor {
            return None;
        }
        self.cur_buf()
            .file_path
            .clone()
            .filter(|p| is_web_file(p) && p.is_file())
    }

    pub(super) fn active_browser_tab(&self) -> Option<&BrowserTab> {
        self.browsers.get(self.active_browser)
    }

    fn active_browser_tab_mut(&mut self) -> Option<&mut BrowserTab> {
        self.browsers.get_mut(self.active_browser)
    }

    /// 選択中のタブを、次に表示したときにページへキー入力を渡すようにする。
    fn focus_active_browser_page(&mut self) {
        if let Some(tab) = self.active_browser_tab_mut() {
            tab.focus_page = true;
        }
    }

    /// タブが無ければ案内ページのタブを作り、選択中のタブの位置を範囲内に収める。
    fn ensure_browser_tab(&mut self) {
        if self.browsers.is_empty() {
            self.browsers.push(BrowserTab::new(None));
        }
        self.active_browser = self.active_browser.min(self.browsers.len() - 1);
    }

    /// Alt+B: ブラウザを表示する。表示中なら元の画面へ戻る。
    /// エディターで HTML・SVG を編集中なら、そのファイルを表示する（表示中のタブがあれば再読み込み、
    /// 無ければ新しいタブ。選択中のタブが案内ページならそこで開く）。
    pub(super) fn toggle_browser(&mut self) {
        if self.mode == AppMode::Browser {
            self.leave_tool_mode();
            return;
        }
        let target = self.current_web_file();
        let dirty = self.cur_buf().dirty;
        self.remember_return_mode();
        self.show_help = false;
        self.prefix = PrefixKey::None;
        self.ime_preedit.clear();
        self.mode = AppMode::Browser;
        self.ensure_browser_tab();

        self.status_message = if !vz_browser::is_supported() {
            "この OS では埋め込みブラウザを使えません。「外部ブラウザ」で開いてください".to_string()
        } else {
            "ブラウザ: Alt+B 戻る / Ctrl+T 新しいタブ / Ctrl+W 閉じる / Ctrl+Tab 切替 / F5 再読み込み"
                .to_string()
        };
        if let Some(path) = target {
            let url = file_url(&path);
            if dirty {
                self.status_message =
                    "未保存の変更があります（保存済みの内容を表示しています）".to_string();
            }
            if let Some(i) = self
                .browsers
                .iter()
                .position(|t| t.current_url().as_deref() == Some(url.as_str()))
            {
                self.active_browser = i;
                if let Some(b) = &self.browsers[i].view {
                    b.reload();
                }
            } else if self
                .active_browser_tab()
                .is_some_and(|t| t.current_url().is_none())
            {
                self.open_in_browser(url);
            } else {
                self.new_browser_tab(Some(url));
            }
        }
        self.focus_active_browser_page();
    }

    /// 選択中のタブで URL を開く。
    fn open_in_browser(&mut self, url: String) {
        if let Some(tab) = self.active_browser_tab_mut() {
            tab.open(url);
        }
    }

    /// 新しいタブを開いて選択する（`url` が None なら案内ページ）。
    pub(super) fn new_browser_tab(&mut self, url: Option<String>) {
        self.browsers.push(BrowserTab::new(url));
        self.active_browser = self.browsers.len() - 1;
    }

    /// タブを閉じる（WebView も破棄する）。最後のタブを閉じたらブラウザを閉じて元の画面へ戻る。
    pub(super) fn close_browser_tab(&mut self, idx: usize) {
        if idx >= self.browsers.len() {
            return;
        }
        self.browsers.remove(idx);
        if self.browsers.is_empty() {
            self.active_browser = 0;
            if self.mode == AppMode::Browser {
                self.leave_tool_mode();
            }
            self.status_message = "ブラウザを閉じました".to_string();
            return;
        }
        if idx < self.active_browser || self.active_browser >= self.browsers.len() {
            self.active_browser = self.active_browser.saturating_sub(1);
        }
        self.focus_active_browser_page();
    }

    /// 選択中のタブを `step` 個先（負なら前）のタブへ切り替える（端から反対の端へ回る）。
    pub(super) fn switch_browser_tab(&mut self, step: isize) {
        let n = self.browsers.len();
        if n == 0 {
            return;
        }
        self.active_browser = (self.active_browser as isize + step).rem_euclid(n as isize) as usize;
        self.focus_active_browser_page();
    }

    /// タブを選んでブラウザを表示する（バッファ帯のクリック）。
    pub(super) fn show_browser_tab(&mut self, idx: usize) {
        if idx >= self.browsers.len() {
            return;
        }
        self.active_browser = idx;
        if self.mode != AppMode::Browser {
            self.remember_return_mode();
            self.ime_preedit.clear();
            self.mode = AppMode::Browser;
        }
        self.browsers[idx].focus_page = true;
    }

    /// URL 欄の入力を開く。
    pub(super) fn navigate_browser_input(&mut self) {
        let Some(tab) = self.active_browser_tab() else {
            return;
        };
        match normalize_url(&tab.url_input) {
            Some(url) => self.open_in_browser(url),
            None => {
                self.status_message = format!("URL を解釈できません: {}", tab.url_input.trim());
            }
        }
    }

    /// タブ操作のキー（`T` 新しいタブ / `W` 閉じる / `N` 次 / `P` 前）。ページからは IPC で届く。
    pub(super) fn browser_tab_shortcut(&mut self, key: char) {
        match key {
            'T' => self.new_browser_tab(None),
            'W' => self.close_browser_tab(self.active_browser),
            'N' => self.switch_browser_tab(1),
            'P' => self.switch_browser_tab(-1),
            _ => {}
        }
    }

    pub(super) fn handle_browser_events(&mut self, ctx: &egui::Context) {
        use egui::Modifiers;
        // ページにフォーカスがある間のキーは WebView が受け取る。ここに来るのは URL 欄などのとき
        let (reload, back, forward, tab_key) = ctx.input_mut(|i| {
            let alt_only = i.modifiers.alt && !i.modifiers.ctrl && !i.modifiers.shift;
            let tab_key = if i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab) {
                Some('P')
            } else if i.consume_key(Modifiers::CTRL, Key::Tab) {
                Some('N')
            } else if i.consume_key(Modifiers::CTRL, Key::T) {
                Some('T')
            } else if i.consume_key(Modifiers::CTRL, Key::W) {
                Some('W')
            } else {
                None
            };
            (
                i.key_pressed(Key::F5),
                alt_only && i.key_pressed(Key::ArrowLeft),
                alt_only && i.key_pressed(Key::ArrowRight),
                tab_key,
            )
        });
        if let Some(b) = self.active_browser_tab().and_then(|t| t.view.as_ref()) {
            if reload {
                b.reload();
            }
            if back {
                b.back();
            }
            if forward {
                b.forward();
            }
        }
        if let Some(key) = tab_key {
            self.browser_tab_shortcut(key);
        }
    }

    pub(super) fn render_browser_canvas(&mut self, ctx: &egui::Context) {
        ctx.output_mut(|o| o.ime = None);
        self.ensure_browser_tab();
        let idx = self.active_browser;
        let mut go = false;
        let mut open_external = false;
        let mut open_current = false;
        let mut nav: Option<i8> = None;
        let mut tab_key: Option<char> = None;
        let current_file = self.cur_buf().file_path.clone().filter(|p| p.is_file());

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(bg(BG_NAVY)))
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                egui::Frame::NONE
                    .fill(bg(BG_TAB_BAR))
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let font = FontId::monospace(13.0);
                            let btn = |ui: &mut egui::Ui, label: &str, tip: &str| {
                                ui.add(egui::Button::new(egui::RichText::new(label).font(font.clone())))
                                    .on_hover_text(tip)
                                    .clicked()
                            };
                            if btn(ui, "←", "戻る (Alt+←)") {
                                nav = Some(-1);
                            }
                            if btn(ui, "→", "進む (Alt+→)") {
                                nav = Some(1);
                            }
                            if btn(ui, "再読込", "再読み込み (F5)") {
                                nav = Some(0);
                            }
                            if btn(ui, "+", "新しいタブ (Ctrl+T)") {
                                tab_key = Some('T');
                            }
                            if btn(ui, "×", "このタブを閉じる (Ctrl+W)") {
                                tab_key = Some('W');
                            }
                            // 右端のボタンを先に置き、URL 欄は残りの幅いっぱいにする（ボタンがはみ出さない）
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if btn(ui, "外部ブラウザ", "既定のブラウザで開く") {
                                    open_external = true;
                                }
                                if current_file.is_some()
                                    && btn(ui, "編集中のファイル", "エディターで開いているファイルを表示")
                                {
                                    open_current = true;
                                }
                                if btn(ui, "開く", "URL 欄の内容を開く (Enter)") {
                                    go = true;
                                }
                                let edit = ui.add(
                                    egui::TextEdit::singleline(&mut self.browsers[idx].url_input)
                                        .font(font.clone())
                                        .hint_text("URL・ファイルのパス・localhost:3000")
                                        .desired_width(ui.available_width().max(80.0)),
                                );
                                self.browser_url_editing = edit.has_focus();
                                if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                                    go = true;
                                }
                            });
                        });
                    });

                let rest = ui.available_rect_before_wrap();
                self.browser_view_rect = Some(rest);
                // WebView を置けないときは理由を出す
                let message = if !vz_browser::is_supported() {
                    Some("この OS では埋め込みブラウザを使えません。「外部ブラウザ」で開いてください。")
                } else {
                    self.browsers[idx].error.as_deref()
                };
                if let Some(msg) = message {
                    ui.painter().text(
                        rest.center(),
                        Align2::CENTER_CENTER,
                        msg,
                        FontId::proportional(15.0),
                        TEXT_AMBER,
                    );
                }
            });

        if go {
            self.navigate_browser_input();
        }
        if open_current {
            if let Some(path) = current_file {
                self.open_in_browser(file_url(&path));
            }
        }
        if open_external {
            let url = self.active_browser_tab().and_then(BrowserTab::session_url);
            if let Some(url) = url {
                ctx.open_url(egui::OpenUrl::new_tab(&url));
                self.status_message = format!("外部ブラウザで開きました: {url}");
            }
        }
        if let (Some(dir), Some(b)) = (nav, self.active_browser_tab().and_then(|t| t.view.as_ref()))
        {
            match dir {
                -1 => b.back(),
                1 => b.forward(),
                _ => b.reload(),
            }
        }
        if let Some(key) = tab_key {
            self.browser_tab_shortcut(key);
        }
    }

    /// WebView の作成・位置・表示を画面に合わせ、ページからの知らせを処理する（毎フレーム、描画の後）。
    /// 表示するのは選択中のタブだけ。メニュー・ダイアログは WebView の下に隠れるので、その間は WebView を隠す。
    pub(super) fn sync_browser(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let overlay = self.open_menu.is_some()
            || self.show_help
            || self.show_about
            || self.filer_modal.is_some()
            || self.inline_prompt.is_some()
            || ctx.input(|i| !i.raw.hovered_files.is_empty());
        let rect = self
            .browser_view_rect
            .filter(|_| self.mode == AppMode::Browser);
        let want = rect.is_some() && !overlay;
        let bounds = rect
            .map(|r| to_bounds(r, ctx.pixels_per_point()))
            .unwrap_or_default();
        let active = self.active_browser;

        // 隠すとフォーカスが親へ戻るので、選択中のタブを表示する前にほかのタブを隠す
        for (i, tab) in self.browsers.iter_mut().enumerate() {
            if i != active {
                if let Some(view) = tab.view.as_mut() {
                    view.set_visible(false);
                }
            }
        }

        if let Some(tab) = self.browsers.get_mut(active) {
            if want && tab.view.is_none() && tab.error.is_none() && vz_browser::is_supported() {
                let data_dir = vz_config::settings::app_dir().map(|d| d.join("webview"));
                let url = tab.pending_url.take();
                let waker = ctx.clone();
                match EmbeddedBrowser::new(frame, bounds, url.as_deref(), data_dir, move || {
                    waker.request_repaint()
                }) {
                    Ok(b) => tab.view = Some(b),
                    Err(e) => {
                        self.status_message = format!("ブラウザの作成エラー: {e}");
                        tab.error = Some(e);
                    }
                }
            }
            if let Some(view) = tab.view.as_mut() {
                if want {
                    view.set_bounds(bounds);
                    if let Some(url) = tab.pending_url.take() {
                        view.load(&url);
                    }
                }
                view.set_visible(want);
                if want && tab.focus_page {
                    view.focus();
                    tab.focus_page = false;
                }
                // ページの外（URL 欄・タブ・メニューなど）をクリックしたら、キー入力を vz へ戻す。
                // WebView は別ウィンドウなので、そのままではクリック後もキーがページへ行ってしまう
                if want {
                    let clicked_outside = ctx.input(|i| {
                        i.pointer.any_pressed()
                            && i.pointer
                                .interact_pos()
                                .is_some_and(|p| rect.is_some_and(|r| !r.contains(p)))
                    });
                    if clicked_outside {
                        view.focus_parent();
                    }
                }
            }
        }

        // 隠れているタブのタイトルなども更新する
        let mut shortcut: Option<char> = None;
        let mut new_windows: Vec<String> = Vec::new();
        for (i, tab) in self.browsers.iter_mut().enumerate() {
            let Some(view) = tab.view.as_ref() else {
                continue;
            };
            let is_active = i == active;
            for ev in view.poll_events() {
                match ev {
                    BrowserEvent::Shortcut(c) => shortcut = Some(c),
                    BrowserEvent::NewWindow(url) => new_windows.push(url),
                    BrowserEvent::Loading(url) => {
                        if is_active {
                            self.status_message = format!("読み込み中: {url}");
                        }
                    }
                    BrowserEvent::Loaded(url) => {
                        // 入力中の URL 欄は書き換えない
                        if !(is_active && self.browser_url_editing) && !is_blank_url(&url) {
                            tab.url_input = url;
                        }
                        if is_active {
                            self.status_message = "読み込み完了".to_string();
                        }
                    }
                    BrowserEvent::TitleChanged(title) => tab.title = title,
                }
            }
        }
        // target="_blank" などは新しいタブで開く
        let opened = !new_windows.is_empty();
        for url in new_windows {
            self.new_browser_tab(Some(url));
        }
        match shortcut {
            Some('B') => self.toggle_browser(),
            Some('K') => self.toggle_terminal(ctx),
            Some(c) => self.browser_tab_shortcut(c),
            None => {}
        }
        if shortcut.is_some() || opened {
            ctx.request_repaint();
        }
    }
}

/// egui の論理座標の矩形 → WebView の物理ピクセル。
fn to_bounds(rect: Rect, ppp: f32) -> Bounds {
    Bounds {
        x: (rect.min.x * ppp).round() as i32,
        y: (rect.min.y * ppp).round() as i32,
        width: (rect.width() * ppp).round().max(1.0) as u32,
        height: (rect.height() * ppp).round().max(1.0) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_bounds_scales_to_physical_pixels() {
        let r = Rect::from_min_size(Pos2::new(10.0, 20.5), Vec2::new(100.0, 50.0));
        assert_eq!(
            to_bounds(r, 1.5),
            Bounds {
                x: 15,
                y: 31,
                width: 150,
                height: 75
            }
        );
    }
}
