//! 修飾キーの現在の状態（ウィンドウのフォーカスに関係なく OS から読む）。
//!
//! 起動時の Shift（前回の状態の復元）と、OS の閉じる要求が Alt+Shift+F4 によるものかの判定に使う。
//! 読めない OS では `None` を返し、呼び出し側が egui の修飾キーで判定する。

/// Shift が押されているか。
#[cfg(windows)]
pub fn shift_held() -> Option<bool> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_SHIFT;
    Some(key_down(VK_SHIFT))
}

/// Alt と Shift が両方押されているか。
#[cfg(windows)]
pub fn alt_shift_held() -> Option<bool> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_MENU, VK_SHIFT};
    Some(key_down(VK_MENU) && key_down(VK_SHIFT))
}

#[cfg(windows)]
fn key_down(vk: u16) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    // SAFETY: 引数は仮想キーコードだけで、ポインタを渡さない
    let state = unsafe { GetAsyncKeyState(i32::from(vk)) };
    // 最上位ビットが立っていれば押されている
    state < 0
}

#[cfg(target_os = "macos")]
mod mac {
    /// kCGEventSourceStateCombinedSessionState
    pub const COMBINED_SESSION_STATE: i32 = 0;
    /// kCGEventFlagMaskShift
    pub const FLAG_SHIFT: u64 = 0x0002_0000;
    /// kCGEventFlagMaskAlternate（Option）
    pub const FLAG_ALTERNATE: u64 = 0x0008_0000;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        pub fn CGEventSourceFlagsState(state_id: i32) -> u64;
    }

    pub fn flags() -> u64 {
        // SAFETY: 引数は列挙値だけで、戻り値はビットフラグ
        unsafe { CGEventSourceFlagsState(COMBINED_SESSION_STATE) }
    }
}

/// Shift が押されているか。
#[cfg(target_os = "macos")]
pub fn shift_held() -> Option<bool> {
    Some(mac::flags() & mac::FLAG_SHIFT != 0)
}

/// Option（Alt）と Shift が両方押されているか。
#[cfg(target_os = "macos")]
pub fn alt_shift_held() -> Option<bool> {
    let f = mac::flags();
    Some(f & mac::FLAG_SHIFT != 0 && f & mac::FLAG_ALTERNATE != 0)
}

/// Linux など: フォーカスの無い状態ではキーを読めない（Wayland）ため判定しない。
#[cfg(not(any(windows, target_os = "macos")))]
pub fn shift_held() -> Option<bool> {
    None
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn alt_shift_held() -> Option<bool> {
    None
}
