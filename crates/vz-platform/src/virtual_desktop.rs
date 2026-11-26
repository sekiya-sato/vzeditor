//! 仮想デスクトップ（Windows の仮想デスクトップ、X11 のワークスペース）の取得と、記録したデスクトップへの移動。
//!
//! VZ 本体は `VirtualDesktopManager` だけを使い、OS の違いはこのモジュールの中に閉じ込める。
//! 取得・移動できない環境（macOS Spaces・Wayland・ウィンドウマネージャーが対応していない X11 など）では
//! `desktop_of` が `None`、`restore` が false を返すだけで、呼び出し側は通常の保存・復元を続ける。
//! 待ちや再試行はしない。

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// OS のウィンドウ（Windows: HWND、X11: Window ID）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeWindow(NativeId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeId {
    // OS ごとに片方だけを作る（他方は型を揃えるために残す）
    #[cfg_attr(not(windows), allow(dead_code))]
    Win32(isize),
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    X11(u32),
}

impl NativeWindow {
    /// 扱えないハンドル（Wayland・macOS など）は None。
    pub fn from_handle(h: &impl HasWindowHandle) -> Option<Self> {
        let raw = h.window_handle().ok()?.as_raw();
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawWindowHandle) -> Option<Self> {
        let id = match raw {
            #[cfg(windows)]
            RawWindowHandle::Win32(h) => Some(NativeId::Win32(h.hwnd.get())),
            #[cfg(target_os = "linux")]
            RawWindowHandle::Xlib(h) => u32::try_from(h.window).ok().map(NativeId::X11),
            #[cfg(target_os = "linux")]
            RawWindowHandle::Xcb(h) => Some(NativeId::X11(h.window.get())),
            _ => None,
        };
        id.map(Self)
    }
}

/// 仮想デスクトップ（記録・復元に使う値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualDesktop {
    /// 取得した仕組み（`"windows"` / `"x11"`）。違う仕組みの記録は戻さない
    pub backend: String,
    /// Windows: GUID（`{XXXXXXXX-...}`）、X11: 0 始まりのデスクトップ番号
    pub id: String,
    /// 名前（分かれば。X11 のみ）
    pub name: Option<String>,
}

/// 仮想デスクトップの取得と移動。使うときに作り、使い終えたら捨てる。
pub struct VirtualDesktopManager {
    backend: Option<imp::Backend>,
}

impl Default for VirtualDesktopManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtualDesktopManager {
    /// 失敗しない（使えない環境では内部で無効になる）。
    pub fn new() -> Self {
        Self {
            backend: imp::Backend::connect(),
        }
    }

    /// ウィンドウのある仮想デスクトップ。使えない・取得できなければ None。
    pub fn desktop_of(&self, w: &NativeWindow) -> Option<VirtualDesktop> {
        self.backend.as_ref()?.desktop_of(w.0)
    }

    /// 記録した仮想デスクトップへウィンドウを移す。移動したときだけ true
    /// （仕組みが違う・デスクトップが無い・既にそこにある・失敗は false）。
    pub fn restore(&self, w: &NativeWindow, saved: &VirtualDesktop) -> bool {
        let Some(backend) = &self.backend else {
            return false;
        };
        if saved.backend != imp::BACKEND {
            return false;
        }
        if backend
            .desktop_of(w.0)
            .is_some_and(|now| now.id.eq_ignore_ascii_case(&saved.id))
        {
            return false;
        }
        backend.move_to(w.0, &saved.id)
    }
}

/// 記録の GUID 文字列（`{...}`、波括弧なしも可）から波括弧を外す。
#[cfg_attr(not(windows), allow(dead_code))]
fn strip_braces(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('{')
        .and_then(|t| t.strip_suffix('}'))
        .unwrap_or(s)
}

/// `_NET_DESKTOP_NAMES`（NUL 区切りの UTF-8）の `index` 番目。空なら None。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn desktop_name_at(names: &[u8], index: usize) -> Option<String> {
    let raw = names.split(|b| *b == 0).nth(index)?;
    let name = String::from_utf8_lossy(raw).trim().to_string();
    (!name.is_empty()).then_some(name)
}

#[cfg(windows)]
mod imp {
    use super::{strip_braces, NativeId, VirtualDesktop};
    use windows::core::GUID;
    use windows::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        IVirtualDesktopManager, VirtualDesktopManager as CLSID_VIRTUAL_DESKTOP_MANAGER,
    };

    pub const BACKEND: &str = "windows";

    pub struct Backend {
        // CoUninitialize より先に解放するため Option にして Drop で取り出す
        manager: Option<IVirtualDesktopManager>,
        uninit: bool,
    }

    impl Backend {
        pub fn connect() -> Option<Self> {
            // SAFETY: 予約引数は None。UI スレッドで呼ぶ。winit が OLE を初期化済みなら S_FALSE
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            // S_OK / S_FALSE なら対の CoUninitialize が要る。別の方式で初期化済み（RPC_E_CHANGED_MODE）でも使える
            let uninit = hr.is_ok();
            if !uninit && hr != RPC_E_CHANGED_MODE {
                return None;
            }
            // SAFETY: CLSID と IID は windows クレートの定数。失敗は Err で返る
            let created: windows::core::Result<IVirtualDesktopManager> =
                unsafe { CoCreateInstance(&CLSID_VIRTUAL_DESKTOP_MANAGER, None, CLSCTX_ALL) };
            match created {
                Ok(manager) => Some(Self {
                    manager: Some(manager),
                    uninit,
                }),
                Err(_) => {
                    if uninit {
                        // SAFETY: 上の CoInitializeEx の成功と対にする
                        unsafe { CoUninitialize() };
                    }
                    None
                }
            }
        }

        fn hwnd(id: NativeId) -> Option<HWND> {
            match id {
                NativeId::Win32(h) => Some(HWND(h as *mut core::ffi::c_void)),
                NativeId::X11(_) => None,
            }
        }

        pub fn desktop_of(&self, id: NativeId) -> Option<VirtualDesktop> {
            let manager = self.manager.as_ref()?;
            let hwnd = Self::hwnd(id)?;
            // SAFETY: hwnd は自プロセスのウィンドウ。失敗は Err で返る
            let guid = unsafe { manager.GetWindowDesktopId(hwnd) }.ok()?;
            if guid == GUID::zeroed() {
                return None;
            }
            Some(VirtualDesktop {
                backend: BACKEND.to_string(),
                id: format!("{{{guid:?}}}"),
                name: None,
            })
        }

        pub fn move_to(&self, id: NativeId, desktop_id: &str) -> bool {
            let (Some(manager), Some(hwnd)) = (self.manager.as_ref(), Self::hwnd(id)) else {
                return false;
            };
            let Ok(guid) = GUID::try_from(strip_braces(desktop_id)) else {
                return false;
            };
            // SAFETY: hwnd は自プロセスのウィンドウ、guid はこの関数の間だけ有効なローカル変数。
            // 削除されたデスクトップなどは Err で返る
            unsafe { manager.MoveWindowToDesktop(hwnd, &guid) }.is_ok()
        }
    }

    impl Drop for Backend {
        fn drop(&mut self) {
            self.manager = None;
            if self.uninit {
                // SAFETY: connect の CoInitializeEx の成功と対にする（インターフェースは解放済み）
                unsafe { CoUninitialize() };
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{desktop_name_at, NativeId, VirtualDesktop};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        Atom, AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, Window,
    };
    use x11rb::rust_connection::RustConnection;

    pub const BACKEND: &str = "x11";

    /// 全デスクトップに表示（`_NET_WM_DESKTOP` の特別な値）
    const ALL_DESKTOPS: u32 = 0xFFFF_FFFF;

    pub struct Backend {
        conn: RustConnection,
        root: Window,
        wm_desktop: Atom,
        number_of_desktops: Atom,
        desktop_names: Atom,
    }

    impl Backend {
        /// `DISPLAY` に接続し、ウィンドウマネージャーが `_NET_WM_DESKTOP` に対応していれば使う。
        pub fn connect() -> Option<Self> {
            std::env::var_os("DISPLAY")?;
            let (conn, screen) = x11rb::connect(None).ok()?;
            let root = conn.setup().roots.get(screen)?.root;
            let atom = |name: &[u8]| -> Option<Atom> {
                Some(conn.intern_atom(false, name).ok()?.reply().ok()?.atom)
            };
            let supported = atom(b"_NET_SUPPORTED")?;
            let wm_desktop = atom(b"_NET_WM_DESKTOP")?;
            let number_of_desktops = atom(b"_NET_NUMBER_OF_DESKTOPS")?;
            let desktop_names = atom(b"_NET_DESKTOP_NAMES")?;
            let list = conn
                .get_property(false, root, supported, AtomEnum::ATOM, 0, 4096)
                .ok()?
                .reply()
                .ok()?;
            if !list.value32()?.any(|a| a == wm_desktop) {
                return None;
            }
            Some(Self {
                conn,
                root,
                wm_desktop,
                number_of_desktops,
                desktop_names,
            })
        }

        fn cardinal(&self, window: Window, property: Atom) -> Option<u32> {
            let reply = self
                .conn
                .get_property(false, window, property, AtomEnum::CARDINAL, 0, 1)
                .ok()?
                .reply()
                .ok()?;
            let value = reply.value32()?.next();
            value
        }

        pub fn desktop_of(&self, id: NativeId) -> Option<VirtualDesktop> {
            let NativeId::X11(window) = id else {
                return None;
            };
            let index = self.cardinal(window, self.wm_desktop)?;
            if index == ALL_DESKTOPS {
                return None;
            }
            let name = self
                .conn
                .get_property(false, self.root, self.desktop_names, AtomEnum::ANY, 0, 4096)
                .ok()
                .and_then(|c| c.reply().ok())
                .and_then(|r| desktop_name_at(&r.value, index as usize));
            Some(VirtualDesktop {
                backend: BACKEND.to_string(),
                id: index.to_string(),
                name,
            })
        }

        pub fn move_to(&self, id: NativeId, desktop_id: &str) -> bool {
            let NativeId::X11(window) = id else {
                return false;
            };
            let Ok(index) = desktop_id.parse::<u32>() else {
                return false;
            };
            let count = self.cardinal(self.root, self.number_of_desktops);
            if count.is_none_or(|n| index >= n) {
                return false;
            }
            // EWMH: ルートへ ClientMessage を送り、ウィンドウマネージャーに移してもらう（source = 1: アプリ）
            let event = ClientMessageEvent::new(32, window, self.wm_desktop, [index, 1, 0, 0, 0]);
            let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
            self.conn.send_event(false, self.root, mask, event).is_ok() && self.conn.flush().is_ok()
        }
    }
}

/// macOS Spaces・その他: 公開 API が無いため扱わない。
#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use super::{NativeId, VirtualDesktop};

    pub const BACKEND: &str = "";

    pub struct Backend;

    impl Backend {
        pub fn connect() -> Option<Self> {
            None
        }

        pub fn desktop_of(&self, _id: NativeId) -> Option<VirtualDesktop> {
            None
        }

        pub fn move_to(&self, _id: NativeId, _desktop_id: &str) -> bool {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_braces() {
        assert_eq!(
            strip_braces("{AA509086-5CA9-4C25-8F95-589D3C07B48A}"),
            "AA509086-5CA9-4C25-8F95-589D3C07B48A"
        );
        assert_eq!(strip_braces("1234"), "1234");
    }

    #[test]
    fn test_desktop_name_at() {
        let names = "仕事\0\0Web\0".as_bytes();
        assert_eq!(desktop_name_at(names, 0).as_deref(), Some("仕事"));
        assert_eq!(desktop_name_at(names, 1), None);
        assert_eq!(desktop_name_at(names, 2).as_deref(), Some("Web"));
        assert_eq!(desktop_name_at(names, 9), None);
    }

    #[test]
    fn test_manager_without_window_does_not_panic() {
        let manager = VirtualDesktopManager::new();
        // 存在しないウィンドウ（0）は取得できず、移動もしない
        let none = NativeWindow(NativeId::X11(0));
        assert_eq!(manager.desktop_of(&none), None);
        let saved = VirtualDesktop {
            backend: "other".into(),
            id: "1".into(),
            name: None,
        };
        assert!(!manager.restore(&none, &saved));
    }

    #[cfg(windows)]
    #[test]
    fn test_guid_record_roundtrip() {
        let guid = windows::core::GUID::from_u128(0xaa509086_5ca9_4c25_8f95_589d3c07b48a);
        let text = format!("{{{guid:?}}}");
        assert_eq!(text, "{AA509086-5CA9-4C25-8F95-589D3C07B48A}");
        assert_eq!(
            windows::core::GUID::try_from(strip_braces(&text)).unwrap(),
            guid
        );
    }
}
