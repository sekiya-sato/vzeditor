//! 簡易ブラウザ。メインウィンドウに WebView（Windows: WebView2、macOS: WKWebView）を子として埋め込む。
//! egui には依存せず、位置・表示の切替・移動だけを受け持つ。Linux は埋め込めないので `is_supported()` が false。

use raw_window_handle::HasWindowHandle;
use std::path::{Path, PathBuf};
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
        let wake: Rc<dyn Fn()> = Rc::new(wake);
        let mut context = wry::WebContext::new(data_dir);

        let (ipc_tx, ipc_wake) = (tx.clone(), wake.clone());
        let (load_tx, load_wake) = (tx.clone(), wake.clone());
        let (title_tx, title_wake) = (tx.clone(), wake.clone());
        let (new_tx, new_wake) = (tx, wake);

        let builder = wry::WebViewBuilder::new_with_web_context(&mut context)
            .with_bounds(to_rect(bounds))
            .with_initialization_script(SHORTCUT_SCRIPT)
            .with_ipc_handler(move |req| {
                if let Some(key) = req
                    .body()
                    .strip_prefix("shortcut:")
                    .and_then(|k| k.chars().next())
                {
                    let _ = ipc_tx.send(BrowserEvent::Shortcut(key));
                    ipc_wake();
                }
            })
            .with_on_page_load_handler(move |event, url| {
                let ev = match event {
                    wry::PageLoadEvent::Started => BrowserEvent::Loading(url),
                    wry::PageLoadEvent::Finished => BrowserEvent::Loaded(url),
                };
                let _ = load_tx.send(ev);
                load_wake();
            })
            .with_document_title_changed_handler(move |title| {
                let _ = title_tx.send(BrowserEvent::TitleChanged(title));
                title_wake();
            })
            // 新しいウィンドウ（target="_blank" など）は vz の新しいタブで開く
            .with_new_window_req_handler(move |url, _| {
                let _ = new_tx.send(BrowserEvent::NewWindow(url));
                new_wake();
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

    pub fn is_visible(&self) -> bool {
        self.visible
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

/// ブラウザで確認する対象になるファイル（拡張子で判定）。
pub fn is_web_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["html", "htm", "xhtml", "svg"]
            .iter()
            .any(|x| e.eq_ignore_ascii_case(x))
    })
}

/// ファイルのパス → `file:///` URL（空白や日本語は % で符号化）。
pub fn file_url(path: &Path) -> String {
    let s = path.to_string_lossy();
    // 正規化した Windows のパス（\\?\C:\...）の接頭辞を外す
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// URL 欄の入力を URL にする。解釈できなければ None。
/// スキーム付きはそのまま、存在するパスは file URL、localhost・IP は http、ドメインらしいものは https。
pub fn normalize_url(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if s.contains("://") || s.starts_with("about:") || s.starts_with("data:") {
        return Some(s.to_string());
    }
    let path = Path::new(s);
    if path.is_absolute() && path.exists() {
        return Some(file_url(path));
    }
    if s.contains(char::is_whitespace) {
        return None;
    }
    let host = s.split(['/', '?', '#']).next().unwrap_or(s);
    let host_name = host.rsplit_once(':').map_or(host, |(h, _)| h);
    let is_local = host_name == "localhost"
        || host_name.ends_with(".localhost")
        || host_name.parse::<std::net::IpAddr>().is_ok();
    if is_local {
        return Some(format!("http://{s}"));
    }
    if host_name.contains('.') {
        return Some(format!("https://{s}"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_url() {
        assert_eq!(
            normalize_url("https://example.com"),
            Some("https://example.com".into())
        );
        assert_eq!(
            normalize_url("example.com/a"),
            Some("https://example.com/a".into())
        );
        assert_eq!(
            normalize_url("localhost:3000"),
            Some("http://localhost:3000".into())
        );
        assert_eq!(
            normalize_url("127.0.0.1:8080/x"),
            Some("http://127.0.0.1:8080/x".into())
        );
        assert_eq!(normalize_url("about:blank"), Some("about:blank".into()));
        assert_eq!(normalize_url("  "), None);
        assert_eq!(normalize_url("hello world"), None);
        assert_eq!(normalize_url("hello"), None);
    }

    #[test]
    fn test_normalize_existing_path() {
        let dir = std::env::temp_dir();
        let url = normalize_url(&dir.to_string_lossy()).expect("path");
        assert!(url.starts_with("file:///"), "{url}");
    }

    #[test]
    fn test_file_url_encodes() {
        assert_eq!(
            file_url(Path::new(r"\\?\C:\a b\テ.html")),
            "file:///C:/a%20b/%E3%83%86.html"
        );
        assert_eq!(
            file_url(Path::new("/home/u/x.html")),
            "file:///home/u/x.html"
        );
    }

    #[test]
    fn test_is_web_file() {
        assert!(is_web_file(Path::new("a/Index.HTML")));
        assert!(is_web_file(Path::new("x.svg")));
        assert!(!is_web_file(Path::new("x.md")));
        assert!(!is_web_file(Path::new("html")));
    }
}
