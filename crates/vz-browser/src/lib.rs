//! 簡易ブラウザ。メインウィンドウに WebView（Windows: WebView2、macOS: WKWebView）を子として埋め込む。
//! egui には依存せず、位置・表示の切替・移動だけを受け持つ。Linux は埋め込めないので `is_supported()` が false。

use raw_window_handle::HasWindowHandle;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

/// WebView を置く位置（親ウィンドウのクライアント領域の物理ピクセル）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// ページ側から届く知らせ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserEvent {
    /// ページ上で押されたショートカット（`B` = Alt+B、`K` = Alt+K、`T` = Ctrl+T、`W` = Ctrl+W、
    /// `N` = Ctrl+Tab、`P` = Ctrl+Shift+Tab）
    Shortcut(char),
    /// 新しいウィンドウの要求（`target="_blank"` など）。新しいタブで開く
    NewWindow(String),
    /// 読み込みを始めた / 終えた URL
    Loading(String),
    Loaded(String),
    TitleChanged(String),
}

/// ページ上の Alt+B / Alt+K とタブ操作（Ctrl+T / Ctrl+W / Ctrl+Tab / Ctrl+Shift+Tab）を親（vz）へ知らせる。
/// WebView にフォーカスがあると egui にキーが届かないため。
#[cfg(any(windows, target_os = "macos"))]
const SHORTCUT_SCRIPT: &str = r#"
window.addEventListener('keydown', function (e) {
  if (e.metaKey) return;
  // code は合成入力などで空になることがあるので keyCode も見る（macOS の Option+B は key が別の文字）
  var is = function (code, kc) { return e.code === code || e.keyCode === kc; };
  var k = '';
  if (e.altKey && !e.ctrlKey && !e.shiftKey) {
    k = is('KeyB', 66) ? 'B' : is('KeyK', 75) ? 'K' : '';
  } else if (e.ctrlKey && !e.altKey) {
    if (is('Tab', 9)) k = e.shiftKey ? 'P' : 'N';
    else if (!e.shiftKey) k = is('KeyT', 84) ? 'T' : is('KeyW', 87) ? 'W' : '';
  }
  if (!k) return;
  e.preventDefault();
  e.stopPropagation();
  window.ipc.postMessage('shortcut:' + k);
}, true);
"#;

/// 何も開いていないときのページ
pub const START_PAGE: &str = r#"<!doctype html><meta charset="utf-8"><title>VZ ブラウザ</title>
<body style="font-family:sans-serif;background:#10203a;color:#dde;padding:2em;line-height:1.7">
<h2>VZ 簡易ブラウザ</h2>
<ul>
<li>上の欄に URL・ファイルのパス・<code>localhost:3000</code> などを入れて Enter</li>
<li>エディターで HTML を開いた状態で Alt+B を押すと、そのファイルを表示（表示中なら再読み込み）</li>
<li>Alt+B でエディターへ戻る / Alt+K でターミナルへ</li>
<li>Ctrl+T 新しいタブ / Ctrl+W タブを閉じる / Ctrl+Tab・Ctrl+Shift+Tab でタブの切替</li>
</ul></body>"#;

/// このOSで WebView を埋め込めるか。
pub fn is_supported() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

pub struct EmbeddedBrowser {
    #[cfg(any(windows, target_os = "macos"))]
    view: wry::WebView,
    /// WebView のデータ（Cookie・キャッシュ）の保存先。WebView より後に破棄する
    #[cfg(any(windows, target_os = "macos"))]
    _context: wry::WebContext,
    rx: Receiver<BrowserEvent>,
    bounds: Bounds,
    visible: bool,
}

impl EmbeddedBrowser {
    /// `parent` の子として作る。`url` が None なら案内ページ。`data_dir` は Cookie などの保存先。
    /// `wake` はページから知らせが届いたときに呼ぶ（再描画の要求）。
    #[cfg(any(windows, target_os = "macos"))]
    pub fn new(
        parent: &impl HasWindowHandle,
        bounds: Bounds,
        url: Option<&str>,
        data_dir: Option<PathBuf>,
        wake: impl Fn() + 'static,
    ) -> Result<Self, String> {
        use std::rc::Rc;
        use std::sync::mpsc;

        let (tx, rx) = mpsc::channel();
        // 知らせを送って再描画を求める（各ハンドラーで共有する）
        let emit = Rc::new(move |event: BrowserEvent| {
            let _ = tx.send(event);
            wake();
        });
        let mut context = wry::WebContext::new(data_dir);

        let (ipc_emit, load_emit, title_emit, new_emit) =
            (emit.clone(), emit.clone(), emit.clone(), emit);

        let builder = wry::WebViewBuilder::new_with_web_context(&mut context)
            .with_bounds(to_rect(bounds))
            .with_initialization_script(SHORTCUT_SCRIPT)
            .with_ipc_handler(move |req| {
                if let Some(key) = req
                    .body()
                    .strip_prefix("shortcut:")
                    .and_then(|k| k.chars().next())
                {
                    ipc_emit(BrowserEvent::Shortcut(key));
                }
            })
            .with_on_page_load_handler(move |event, url| {
                let ev = match event {
                    wry::PageLoadEvent::Started => BrowserEvent::Loading(url),
                    wry::PageLoadEvent::Finished => BrowserEvent::Loaded(url),
                };
                load_emit(ev);
            })
            .with_document_title_changed_handler(move |title| {
                title_emit(BrowserEvent::TitleChanged(title));
            })
            // 新しいウィンドウ（target="_blank" など）は vz の新しいタブで開く
            .with_new_window_req_handler(move |url, _| {
                new_emit(BrowserEvent::NewWindow(url));
                wry::NewWindowResponse::Deny
            });
        let builder = match url {
            Some(url) => builder.with_url(url),
            None => builder.with_html(START_PAGE),
        };
        let view = builder
            .build_as_child(parent)
            .map_err(|e| format!("WebView を作れません: {e}"))?;
        Ok(Self {
            view,
            _context: context,
            rx,
            bounds,
            visible: true,
        })
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn new(
        _parent: &impl HasWindowHandle,
        _bounds: Bounds,
        _url: Option<&str>,
        _data_dir: Option<PathBuf>,
        _wake: impl Fn() + 'static,
    ) -> Result<Self, String> {
        Err("この OS では埋め込みブラウザを使えません（外部ブラウザで開いてください）".to_string())
    }

    /// 届いた知らせ。
    pub fn poll_events(&self) -> Vec<BrowserEvent> {
        self.rx.try_iter().collect()
    }

    pub fn set_bounds(&mut self, bounds: Bounds) {
        if bounds == self.bounds {
            return;
        }
        self.bounds = bounds;
        #[cfg(any(windows, target_os = "macos"))]
        let _ = self.view.set_bounds(to_rect(bounds));
    }

    /// 表示・非表示。隠すときはキー入力を親ウィンドウへ戻す。
    pub fn set_visible(&mut self, visible: bool) {
        if visible == self.visible {
            return;
        }
        self.visible = visible;
        #[cfg(any(windows, target_os = "macos"))]
        {
            let _ = self.view.set_visible(visible);
            if !visible {
                let _ = self.view.focus_parent();
            }
        }
    }

    /// キー入力を親ウィンドウ（vz）へ戻す。
    pub fn focus_parent(&self) {
        #[cfg(any(windows, target_os = "macos"))]
        let _ = self.view.focus_parent();
    }

    /// キー入力をページへ渡す。
    pub fn focus(&self) {
        #[cfg(any(windows, target_os = "macos"))]
        let _ = self.view.focus();
    }

    pub fn load(&self, url: &str) {
        #[cfg(any(windows, target_os = "macos"))]
        let _ = self.view.load_url(url);
        #[cfg(not(any(windows, target_os = "macos")))]
        let _ = url;
    }

    pub fn reload(&self) {
        #[cfg(any(windows, target_os = "macos"))]
        let _ = self.view.reload();
    }

    pub fn back(&self) {
        self.eval("history.back()");
    }

    pub fn forward(&self) {
        self.eval("history.forward()");
    }

    fn eval(&self, js: &str) {
        #[cfg(any(windows, target_os = "macos"))]
        let _ = self.view.evaluate_script(js);
        #[cfg(not(any(windows, target_os = "macos")))]
        let _ = js;
    }

    /// 表示中の URL。
    pub fn url(&self) -> Option<String> {
        #[cfg(any(windows, target_os = "macos"))]
        {
            self.view.url().ok()
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            None
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn to_rect(b: Bounds) -> wry::Rect {
    use wry::dpi::{PhysicalPosition, PhysicalSize};
    wry::Rect {
        position: PhysicalPosition::new(b.x, b.y).into(),
        size: PhysicalSize::new(b.width.max(1), b.height.max(1)).into(),
    }
}

mod url;

pub use url::{file_url, is_web_file, normalize_url};
