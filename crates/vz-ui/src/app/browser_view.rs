//! 簡易ブラウザ（Alt+B）: 画面の切替、URL 欄、WebView の位置と表示の同期。
//! WebView 自体は vz-browser（Windows: WebView2、macOS: WKWebView。Linux は外部ブラウザ）。

use super::*;
use vz_browser::{file_url, is_web_file, normalize_url, Bounds, BrowserEvent, EmbeddedBrowser};

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

    /// Alt+B: ブラウザを表示する。表示中なら元の画面へ戻る。
    /// エディターで HTML・SVG を編集中なら、そのファイルを開く（表示中のページなら再読み込み）。
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
        self.browser_focus_page = true;

        self.status_message = if !vz_browser::is_supported() {
            "この OS では埋め込みブラウザを使えません。「外部ブラウザ」で開いてください".to_string()
        } else {
            "ブラウザ: Alt+B で戻る / Alt+K ターミナル / F5 再読み込み / Alt+← → 戻る・進む"
                .to_string()
        };
        if let Some(path) = target {
            let url = file_url(&path);
            if dirty {
                self.status_message =
                    "未保存の変更があります（保存済みの内容を表示しています）".to_string();
            }
            let showing = self.browser.as_ref().and_then(EmbeddedBrowser::url);
            if showing.as_deref() == Some(url.as_str()) {
                if let Some(b) = &self.browser {
                    b.reload();
                }
            } else {
                self.open_in_browser(url);
            }
        }
    }

    /// URL を開く（WebView がまだ無ければ作るときに開く）。
    fn open_in_browser(&mut self, url: String) {
        match &self.browser {
            Some(b) => b.load(&url),
            None => self.browser_pending_url = Some(url.clone()),
        }
        self.browser_url_input = url;
        self.browser_focus_page = true;
    }

    /// URL 欄の入力を開く。
    pub(super) fn navigate_browser_input(&mut self) {
        match normalize_url(&self.browser_url_input) {
            Some(url) => self.open_in_browser(url),
            None => {
                self.status_message =
                    format!("URL を解釈できません: {}", self.browser_url_input.trim());
            }
        }
    }

    pub(super) fn handle_browser_events(&mut self, ctx: &egui::Context) {
        // ページにフォーカスがある間のキーは WebView が受け取る。ここに来るのは URL 欄などのとき
        let (reload, back, forward) = ctx.input(|i| {
            let alt_only = i.modifiers.alt && !i.modifiers.ctrl && !i.modifiers.shift;
            (
                i.key_pressed(Key::F5),
                alt_only && i.key_pressed(Key::ArrowLeft),
                alt_only && i.key_pressed(Key::ArrowRight),
            )
        });
        if let Some(b) = &self.browser {
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
    }

    pub(super) fn render_browser_canvas(&mut self, ctx: &egui::Context) {
        ctx.output_mut(|o| o.ime = None);
        let mut go = false;
        let mut open_external = false;
        let mut open_current = false;
        let mut nav: Option<i8> = None;
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
                            let right_w = 230.0;
                            let edit = ui.add(
                                egui::TextEdit::singleline(&mut self.browser_url_input)
                                    .font(font.clone())
                                    .hint_text("URL・ファイルのパス・localhost:3000")
                                    .desired_width((ui.available_width() - right_w).max(120.0)),
                            );
                            if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                                go = true;
                            }
                            if btn(ui, "開く", "URL 欄の内容を開く (Enter)") {
                                go = true;
                            }
                            if current_file.is_some()
                                && btn(ui, "編集中のファイル", "エディターで開いているファイルを表示")
                            {
                                open_current = true;
                            }
                            if btn(ui, "外部ブラウザ", "既定のブラウザで開く") {
                                open_external = true;
                            }
                        });
                    });

                let rest = ui.available_rect_before_wrap();
                self.browser_view_rect = Some(rest);
                // WebView を置けないときは理由を出す
                let message = if !vz_browser::is_supported() {
                    Some("この OS では埋め込みブラウザを使えません。「外部ブラウザ」で開いてください。")
                } else {
                    self.browser_error.as_deref()
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
            let url = self
                .browser
                .as_ref()
                .and_then(EmbeddedBrowser::url)
                .or_else(|| normalize_url(&self.browser_url_input));
            if let Some(url) = url {
                ctx.open_url(egui::OpenUrl::new_tab(&url));
                self.status_message = format!("外部ブラウザで開きました: {url}");
            }
        }
        if let (Some(dir), Some(b)) = (nav, &self.browser) {
            match dir {
                -1 => b.back(),
                1 => b.forward(),
                _ => b.reload(),
            }
        }
    }

    /// WebView の作成・位置・表示を画面に合わせ、ページからの知らせを処理する（毎フレーム、描画の後）。
    /// メニュー・ダイアログは WebView の下に隠れるので、その間は WebView を隠す。
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

        if want
            && self.browser.is_none()
            && self.browser_error.is_none()
            && vz_browser::is_supported()
        {
            let data_dir = vz_config::settings::app_dir().map(|d| d.join("webview"));
            let url = self.browser_pending_url.take();
            let waker = ctx.clone();
            match EmbeddedBrowser::new(frame, bounds, url.as_deref(), data_dir, move || {
                waker.request_repaint()
            }) {
                Ok(b) => self.browser = Some(b),
                Err(e) => {
                    self.status_message = format!("ブラウザの作成エラー: {e}");
                    self.browser_error = Some(e);
                }
            }
        }
        let Some(browser) = self.browser.as_mut() else {
            return;
        };
        if want {
            browser.set_bounds(bounds);
            if let Some(url) = self.browser_pending_url.take() {
                browser.load(&url);
            }
        }
        browser.set_visible(want);
        if want && self.browser_focus_page {
            browser.focus();
            self.browser_focus_page = false;
        }

        let mut shortcut: Option<char> = None;
        for ev in browser.poll_events() {
            match ev {
                BrowserEvent::Shortcut(c) => shortcut = Some(c),
                BrowserEvent::Loading(url) => {
                    self.status_message = format!("読み込み中: {url}");
                }
                BrowserEvent::Loaded(url) => {
                    if !url.starts_with("data:") && !url.starts_with("about:") {
                        self.browser_url_input = url;
                    }
                    self.status_message = "読み込み完了".to_string();
                }
                BrowserEvent::TitleChanged(title) => self.browser_title = title,
            }
        }
        match shortcut {
            Some('B') => self.toggle_browser(),
            Some('K') => self.toggle_terminal(ctx),
            _ => {}
        }
        if shortcut.is_some() {
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
