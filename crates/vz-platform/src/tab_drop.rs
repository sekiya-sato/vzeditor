//! マウスを離した位置にあるウィンドウのプロセスを調べる。
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub enum DropTarget {
    OwnWindow,
    Other(Option<u32>),
    Unsupported,
}

pub fn target_at_pointer(window: &impl HasWindowHandle) -> DropTarget {
    let Ok(handle) = window.window_handle() else {
        return DropTarget::Unsupported;
    };
    match handle.as_raw() {
        #[cfg(windows)]
        RawWindowHandle::Win32(window) => windows_target(window.hwnd.get()),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xlib(window) => x11_target(window.window as u32),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xcb(window) => x11_target(window.window.get()),
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(_) => macos_target(),
        _ => DropTarget::Unsupported,
    }
}

/// 一部の環境ではウィンドウを出ると egui にボタン解放が届かない。
pub fn primary_down(window: &impl HasWindowHandle) -> Option<bool> {
    match window.window_handle().ok()?.as_raw() {
        #[cfg(windows)]
        RawWindowHandle::Win32(_) => {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON,
            };
            use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};
            // SAFETY: システムの左右ボタン設定と、対応する物理ボタンの状態を問い合わせるだけ。
            unsafe {
                let button = if GetSystemMetrics(SM_SWAPBUTTON) == 0 {
                    VK_LBUTTON
                } else {
                    VK_RBUTTON
                };
                Some(GetAsyncKeyState(i32::from(button)) < 0)
            }
        }
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xlib(window) => x11_primary_down(window.window as u32),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xcb(window) => x11_primary_down(window.window.get()),
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(_) => {
            use core_graphics::event_source::CGEventSourceStateID;
            #[link(name = "CoreGraphics", kind = "framework")]
            extern "C" {
                fn CGEventSourceButtonState(state_id: CGEventSourceStateID, button: u32) -> bool;
            }
            // SAFETY: 有効なセッション種別と左ボタンを指定する。
            Some(unsafe { CGEventSourceButtonState(CGEventSourceStateID::CombinedSessionState, 0) })
        }
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn x11_primary_down(window: u32) -> Option<bool> {
    use x11rb::protocol::xproto::{ConnectionExt, KeyButMask};
    let (conn, _) = x11rb::connect(None).ok()?;
    let pointer = conn.query_pointer(window).ok()?.reply().ok()?;
    Some(pointer.mask.contains(KeyButMask::BUTTON1))
}

#[cfg(windows)]
fn windows_target(source: isize) -> DropTarget {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetCursorPos, GetWindowThreadProcessId, WindowFromPoint, GA_ROOT,
    };

    let mut point = POINT { x: 0, y: 0 };
    // SAFETY: 出力引数は有効な変数。取得した HWND は照会にだけ使用する。
    unsafe {
        if GetCursorPos(&mut point) == 0 {
            return DropTarget::Unsupported;
        }
        let hit = WindowFromPoint(point);
        if hit.is_null() {
            return DropTarget::Other(None);
        }
        let root = GetAncestor(hit, GA_ROOT);
        if root as isize == source {
            return DropTarget::OwnWindow;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(if root.is_null() { hit } else { root }, &mut pid);
        if pid == std::process::id() {
            DropTarget::OwnWindow
        } else {
            DropTarget::Other((pid != 0).then_some(pid))
        }
    }
}

#[cfg(target_os = "linux")]
fn x11_target(source: u32) -> DropTarget {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

    fn hit(source: u32) -> Option<DropTarget> {
        let (conn, _) = x11rb::connect(None).ok()?;
        let root = conn.query_tree(source).ok()?.reply().ok()?.root;
        let pid_atom = conn
            .intern_atom(true, b"_NET_WM_PID")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let frame = conn.query_pointer(root).ok()?.reply().ok()?.child;
        let mut window = frame;
        let owner = |window| {
            let reply = conn
                .get_property(false, window, pid_atom, AtomEnum::CARDINAL, 0, 1)
                .ok()?
                .reply()
                .ok()?;
            let mut values = reply.value32()?;
            values.next()
        };
        // WM の枠や WebView の子を通って、カーソル直下のクライアントを探す。
        while window != 0 {
            if window == source {
                return Some(DropTarget::OwnWindow);
            }
            if let Some(pid) = owner(window) {
                return Some(DropTarget::Other(Some(pid)));
            }
            window = conn.query_pointer(window).ok()?.reply().ok()?.child;
        }
        // 枠のタイトルバー上では、クライアントが直下の子として見つからないことがある。
        if frame != 0 {
            for child in conn.query_tree(frame).ok()?.reply().ok()?.children {
                if child == source {
                    return Some(DropTarget::OwnWindow);
                }
                if let Some(pid) = owner(child) {
                    return Some(DropTarget::Other(Some(pid)));
                }
            }
        }
        Some(DropTarget::Other(None))
    }

    hit(source).unwrap_or(DropTarget::Unsupported)
}

#[cfg(target_os = "macos")]
fn macos_target() -> DropTarget {
    use core_foundation::{dictionary::CFDictionary, number::CFNumber, string::CFString};
    use core_graphics::{
        event::CGEvent,
        event_source::{CGEventSource, CGEventSourceStateID},
        geometry::CGRect,
        window::{
            create_description_from_array, create_window_list, kCGNullWindowID,
            kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly,
        },
    };

    let Some(pointer) = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .and_then(CGEvent::new)
        .map(|event| event.location())
        .ok()
    else {
        return DropTarget::Unsupported;
    };
    let Some(windows) = create_window_list(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    )
    .and_then(create_description_from_array) else {
        return DropTarget::Unsupported;
    };
    let bounds_key = CFString::new("kCGWindowBounds");
    let pid_key = CFString::new("kCGWindowOwnerPID");
    let alpha_key = CFString::new("kCGWindowAlpha");
    for window in &windows {
        let bounds = window
            .find(&bounds_key)
            .and_then(|value| value.downcast::<CFDictionary>())
            .and_then(|dict| CGRect::from_dict_representation(&dict));
        if !bounds.is_some_and(|rect| rect.contains(&pointer)) {
            continue;
        }
        if window
            .find(&alpha_key)
            .and_then(|value| value.downcast::<CFNumber>())
            .and_then(|alpha| alpha.to_f64())
            == Some(0.0)
        {
            continue;
        }
        let pid = window
            .find(&pid_key)
            .and_then(|value| value.downcast::<CFNumber>())
            .and_then(|pid| pid.to_i32())
            .and_then(|pid| u32::try_from(pid).ok());
        return if pid == Some(std::process::id()) {
            DropTarget::OwnWindow
        } else {
            DropTarget::Other(pid)
        };
    }
    DropTarget::Other(None)
}
