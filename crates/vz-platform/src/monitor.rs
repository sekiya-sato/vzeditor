//! モニターの作業領域の判定。モニターを外した・配置を変えた後に、
//! 保存していたウィンドウ位置で画面外へ出さないために使う。

/// タイトルバーの矩形 `bar`（物理ピクセルの left, top, right, bottom）が作業領域 `work` に十分見えるか。
#[cfg_attr(not(windows), allow(dead_code))]
fn title_bar_fits(bar: [i32; 4], work: [i32; 4]) -> bool {
    let w = bar[2].min(work[2]) - bar[0].max(work[0]);
    let h = bar[3].min(work[3]) - bar[1].max(work[1]);
    let need_w = (bar[2] - bar[0]).min(100);
    w >= need_w && h >= (bar[3] - bar[1]).min(10)
}

/// Windows: `rect`（物理ピクセルの left, top, right, bottom）が一番重なるモニター。無ければ None。
#[cfg(windows)]
fn monitor_from_rect(rect: [i32; 4]) -> Option<windows_sys::Win32::Graphics::Gdi::HMONITOR> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{MonitorFromRect, MONITOR_DEFAULTTONULL};
    let rect = RECT {
        left: rect[0],
        top: rect[1],
        right: rect[2],
        bottom: rect[3],
    };
    // SAFETY: RECT は呼び出しの間だけ有効なローカル変数を渡す
    let monitor = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONULL) };
    (!monitor.is_null()).then_some(monitor)
}

/// Windows: `bar` が重なるモニターの作業領域に十分見えるか。
#[cfg(windows)]
pub fn title_bar_visible(bar: [i32; 4]) -> bool {
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
    let Some(monitor) = monitor_from_rect(bar) else {
        return false;
    };
    // SAFETY: MONITORINFO は呼び出しの間だけ有効なローカル変数を渡す
    unsafe {
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return false;
        }
        let w = info.rcWork;
        title_bar_fits(bar, [w.left, w.top, w.right, w.bottom])
    }
}

/// macOS は OS が画面内へ収め、Wayland は位置を指定できないため判定しない。
#[cfg(not(windows))]
pub fn title_bar_visible(_bar: [i32; 4]) -> bool {
    true
}

/// Windows: `rect`（物理ピクセル）が一番重なるモニターの名前（`\\.\DISPLAY1` など）。
/// 状態の記録に残すだけで、位置の補正には使わない（番号は抜き差しで変わる）。
#[cfg(windows)]
pub fn monitor_name(rect: [i32; 4]) -> Option<String> {
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO, MONITORINFOEXW};
    let monitor = monitor_from_rect(rect)?;
    // SAFETY: MONITORINFOEXW は呼び出しの間だけ有効なローカル変数を渡す。
    // MONITORINFOEXW は先頭が MONITORINFO で、cbSize で拡張版と伝える
    unsafe {
        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(
            monitor,
            &mut info as *mut MONITORINFOEXW as *mut MONITORINFO,
        ) == 0
        {
            return None;
        }
        let len = info.szDevice.iter().position(|c| *c == 0).unwrap_or(32);
        let name = String::from_utf16_lossy(&info.szDevice[..len]);
        (!name.is_empty()).then_some(name)
    }
}

/// 他 OS はモニター名を記録しない。
#[cfg(not(windows))]
pub fn monitor_name(_rect: [i32; 4]) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_title_bar_fits() {
        let work = [0, 0, 1920, 1040];
        assert!(title_bar_fits([100, 100, 900, 132], work));
        // 左にはみ出しても 100px 以上見えていればよい
        assert!(title_bar_fits([-700, 100, 100, 132], work));
        assert!(!title_bar_fits([-750, 100, 50, 132], work));
        // 外したモニターの位置（作業領域の外）
        assert!(!title_bar_fits([2000, 100, 2800, 132], work));
        assert!(!title_bar_fits([100, 1035, 900, 1067], work));
    }
}
