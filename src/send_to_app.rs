//! 編集中のテキストを Claude Code / Codex のデスクトップアプリへ送る（Alt+C → C / X）。
//!
//! どちらのアプリにも外部から入力欄へ文字を渡す API が無いため、次の手順で送る。
//! 1. 修飾キー（Alt など）が離されるのを待つ（押したままだと Ctrl+V が Ctrl+Alt+V になる）
//! 2. 対象アプリのウィンドウを探して前面にする
//! 3. クリップボードに本文を入れて貼り付け（Ctrl+V / Cmd+V）、送信キーを押す
//!    （Claude Code は Enter、Codex は Ctrl+Enter。macOS の Codex は Cmd+Enter）
//! 4. 元のクリップボードを戻し、vz を前面に戻す
//!
//! アプリ内のフォーカスが入力欄にあることが前提（Electron は前面化時に直前のフォーカスへ戻る）。
//! 処理は待ち時間を含むため別スレッドで行い、結果をチャネルで返す。

use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendTarget {
    /// Claude デスクトップアプリの Claude Code
    ClaudeCode,
    /// Codex デスクトップアプリ
    Codex,
}

impl SendTarget {
    pub fn label(self) -> &'static str {
        match self {
            SendTarget::ClaudeCode => "Claude Code",
            SendTarget::Codex => "Codex",
        }
    }

    /// 送信に修飾キーが要るか。Claude Code は Enter、Codex は Ctrl+Enter（macOS は Cmd+Enter）で送信する。
    pub fn submit_needs_modifier(self) -> bool {
        matches!(self, SendTarget::Codex)
    }

    /// Windows で対象ウィンドウを持つ実行ファイルか（ファイル名と配置先で判定）。
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn matches_exe_path(self, path: &str) -> bool {
        let lower = path.replace('/', "\\").to_lowercase();
        let file = lower.rsplit('\\').next().unwrap_or("");
        match self {
            SendTarget::ClaudeCode => file == "claude.exe",
            // Codex アプリの本体は OpenAI.Codex パッケージ内の ChatGPT.exe。
            // 同名の ChatGPT デスクトップアプリと区別するため配置先も見る。
            SendTarget::Codex => {
                file == "codex.exe" || (file == "chatgpt.exe" && lower.contains("openai.codex"))
            }
        }
    }
}

/// 送信結果（成功時は案内文、失敗時は理由）。
pub type SendResult = Result<String, String>;

/// 送る本文を作る。空白だけなら None。
pub fn prepare_text(lines: &[String]) -> Option<String> {
    let text = lines.join("\n");
    let trimmed = text.trim_end_matches(['\n', '\r', ' ', '\t']);
    if trimmed.trim().is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// 別スレッドで送信する。
pub fn spawn_send(target: SendTarget, text: String) -> Receiver<SendResult> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(platform::send(target, &text));
    });
    rx
}

fn sleep_ms(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
mod platform {
    use super::{sleep_ms, SendResult, SendTarget};
    use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{
        AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN, VK_SHIFT, VK_V,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindow, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow,
        GW_OWNER, SW_RESTORE,
    };

    struct FindCtx {
        target: SendTarget,
        found: HWND,
    }

    fn exe_path_of_window(hwnd: HWND) -> Option<String> {
        let mut pid: u32 = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid == 0 {
            return None;
        }
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
            CloseHandle(h);
            if ok == 0 {
                return None;
            }
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam as *mut FindCtx);
        // 表示中・所有者なし・タイトルありのトップレベルウィンドウだけを見る
        if IsWindowVisible(hwnd) == 0
            || !GetWindow(hwnd, GW_OWNER).is_null()
            || GetWindowTextLengthW(hwnd) == 0
        {
            return 1;
        }
        if let Some(path) = exe_path_of_window(hwnd) {
            if ctx.target.matches_exe_path(&path) {
                ctx.found = hwnd;
                return 0; // 列挙を止める
            }
        }
        1
    }

    fn find_window(target: SendTarget) -> Option<HWND> {
        let mut ctx = FindCtx {
            target,
            found: std::ptr::null_mut(),
        };
        unsafe { EnumWindows(Some(enum_proc), &mut ctx as *mut FindCtx as LPARAM) };
        (!ctx.found.is_null()).then_some(ctx.found)
    }

    fn key_down(vk: VIRTUAL_KEY) -> bool {
        unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
    }

    /// Alt / Ctrl / Shift / Win がすべて離されるまで待つ（最大 3 秒）。
    fn wait_modifiers_released() -> bool {
        for _ in 0..150 {
            if ![VK_MENU, VK_CONTROL, VK_SHIFT, VK_LWIN, VK_RWIN]
                .iter()
                .any(|&vk| key_down(vk))
            {
                return true;
            }
            sleep_ms(20);
        }
        false
    }

    fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn send_keys(inputs: &[INPUT]) -> bool {
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        sent as usize == inputs.len()
    }

    /// ウィンドウを前面にする。通常の SetForegroundWindow が拒否された場合は
    /// 現在の前面ウィンドウの入力スレッドに一時的に接続して再試行する。
    fn activate(hwnd: HWND) -> bool {
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            if SetForegroundWindow(hwnd) == 0 {
                let fg = GetForegroundWindow();
                let fg_thread = GetWindowThreadProcessId(fg, std::ptr::null_mut());
                let me = GetCurrentThreadId();
                AttachThreadInput(me, fg_thread, 1);
                BringWindowToTop(hwnd);
                SetForegroundWindow(hwnd);
                AttachThreadInput(me, fg_thread, 0);
            }
        }
        for _ in 0..25 {
            if unsafe { GetForegroundWindow() } == hwnd {
                return true;
            }
            sleep_ms(20);
        }
        false
    }

    pub fn send(target: SendTarget, text: &str) -> SendResult {
        let label = target.label();
        let vz_hwnd = unsafe { GetForegroundWindow() };

        if !wait_modifiers_released() {
            return Err("Alt などのキーが押されたままのため送信を中止しました".to_string());
        }
        let Some(hwnd) = find_window(target) else {
            return Err(format!("{label} のアプリが見つかりません（起動しているか確認してください）"));
        };

        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| format!("クリップボードを開けません: {e}"))?;
        let saved = clipboard.get_text().ok();
        clipboard
            .set_text(text.to_string())
            .map_err(|e| format!("クリップボードに書き込めません: {e}"))?;

        let restore = |clipboard: &mut arboard::Clipboard| {
            if let Some(old) = &saved {
                let _ = clipboard.set_text(old.clone());
            }
        };

        if !activate(hwnd) {
            restore(&mut clipboard);
            return Err(format!("{label} のウィンドウを前面にできませんでした"));
        }
        // Electron が入力欄へフォーカスを戻すのを待つ
        sleep_ms(250);

        let paste = [
            key_input(VK_CONTROL, false),
            key_input(VK_V, false),
            key_input(VK_V, true),
            key_input(VK_CONTROL, true),
        ];
        let ok_paste = send_keys(&paste);
        // 貼り付けの反映を待ってから送信
        sleep_ms(400);
        let ok_enter = ok_paste
            && unsafe { GetForegroundWindow() } == hwnd
            && if target.submit_needs_modifier() {
                // Codex: Ctrl+Enter
                send_keys(&[
                    key_input(VK_CONTROL, false),
                    key_input(VK_RETURN, false),
                    key_input(VK_RETURN, true),
                    key_input(VK_CONTROL, true),
                ])
            } else {
                send_keys(&[key_input(VK_RETURN, false), key_input(VK_RETURN, true)])
            };
        sleep_ms(300);

        restore(&mut clipboard);
        if !vz_hwnd.is_null() {
            activate(vz_hwnd);
        }

        if !ok_paste {
            return Err(format!("{label} への貼り付けに失敗しました"));
        }
        if !ok_enter {
            return Err(format!(
                "{label} へ貼り付けましたが、途中でウィンドウが切り替わったため送信していません"
            ));
        }
        Ok(format!("{label} へ送信しました（{} 文字）", text.chars().count()))
    }
}

// ---------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
mod platform {
    use super::{sleep_ms, SendResult, SendTarget};
    use std::process::Command;

    fn app_name(target: SendTarget) -> &'static str {
        match target {
            SendTarget::ClaudeCode => "Claude",
            SendTarget::Codex => "Codex",
        }
    }

    fn osascript(script: &str) -> Result<(), String> {
        let out = Command::new("osascript")
            .args(["-e", script])
            .output()
            .map_err(|e| format!("osascript を実行できません: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }

    pub fn send(target: SendTarget, text: &str) -> SendResult {
        let label = target.label();
        let app = app_name(target);
        // Option キーなどが離されるのを少し待つ
        sleep_ms(300);

        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| format!("クリップボードを開けません: {e}"))?;
        let saved = clipboard.get_text().ok();
        clipboard
            .set_text(text.to_string())
            .map_err(|e| format!("クリップボードに書き込めません: {e}"))?;

        // 貼り付けと Enter には「アクセシビリティ」の許可が必要。Codex は Cmd+Enter で送信する
        let submit = if target.submit_needs_modifier() {
            "key code 36 using command down"
        } else {
            "key code 36"
        };
        let script = format!(
            "tell application \"{app}\" to activate\n\
             delay 0.3\n\
             tell application \"System Events\"\n\
             keystroke \"v\" using command down\n\
             delay 0.4\n\
             {submit}\n\
             end tell"
        );
        let result = osascript(&script);
        sleep_ms(300);
        if let Some(old) = saved {
            let _ = clipboard.set_text(old);
        }
        let pid = std::process::id();
        let _ = osascript(&format!(
            "tell application \"System Events\" to set frontmost of (first process whose unix id is {pid}) to true"
        ));
        match result {
            Ok(()) => Ok(format!("{label} へ送信しました（{} 文字）", text.chars().count())),
            Err(e) => Err(format!(
                "{label} へ送信できませんでした（アプリの起動と、システム設定のアクセシビリティ許可を確認してください）: {e}"
            )),
        }
    }
}

// ---------------------------------------------------------------- Linux (X11, xdotool + xclip)

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use super::{sleep_ms, SendResult, SendTarget};
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn window_class(target: SendTarget) -> &'static str {
        match target {
            SendTarget::ClaudeCode => "Claude",
            SendTarget::Codex => "Codex",
        }
    }

    fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
        let out = Command::new(cmd)
            .args(args)
            .output()
            .map_err(|e| format!("{cmd} を実行できません: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(format!("{cmd} が失敗しました"))
        }
    }

    fn set_clipboard(text: &str) -> Result<(), String> {
        let mut child = Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("xclip を実行できません: {e}"))?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| format!("xclip へ書き込めません: {e}"))?;
        }
        drop(child.stdin.take());
        // xclip は選択を保持するため常駐する。終了は待たない
        Ok(())
    }

    pub fn send(target: SendTarget, text: &str) -> SendResult {
        let label = target.label();
        if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_none() {
            return Err("Wayland では送信に対応していません（X11 と xdotool が必要です）".to_string());
        }
        sleep_ms(300);
        let me = run("xdotool", &["getactivewindow"]).ok();
        let win = run(
            "xdotool",
            &["search", "--onlyvisible", "--class", window_class(target)],
        )
        .ok()
        .and_then(|s| s.lines().next().map(str::to_string))
        .ok_or_else(|| format!("{label} のウィンドウが見つかりません（xdotool が必要です）"))?;

        let saved = run("xclip", &["-o", "-selection", "clipboard"]).ok();
        set_clipboard(text)?;
        run("xdotool", &["windowactivate", "--sync", &win])?;
        sleep_ms(250);
        run("xdotool", &["key", "--clearmodifiers", "ctrl+v"])?;
        sleep_ms(400);
        let submit = if target.submit_needs_modifier() {
            "ctrl+Return" // Codex
        } else {
            "Return"
        };
        run("xdotool", &["key", "--clearmodifiers", submit])?;
        sleep_ms(300);
        if let Some(old) = saved {
            let _ = set_clipboard(&old);
        }
        if let Some(me) = me {
            let _ = run("xdotool", &["windowactivate", &me]);
        }
        Ok(format!("{label} へ送信しました（{} 文字）", text.chars().count()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_exe_path() {
        let claude = r"C:\Program Files\WindowsApps\Claude_6.28899.6.0_x64__pzs8sxrjxfjjc\app\Claude.exe";
        let codex = r"C:\Program Files\WindowsApps\OpenAI.Codex_26.1117.4957.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe";
        let chatgpt = r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.0.0.0_x64__x\app\ChatGPT.exe";
        assert!(SendTarget::ClaudeCode.matches_exe_path(claude));
        assert!(!SendTarget::ClaudeCode.matches_exe_path(codex));
        assert!(SendTarget::Codex.matches_exe_path(codex));
        assert!(!SendTarget::Codex.matches_exe_path(claude));
        // 同名の ChatGPT デスクトップアプリには送らない
        assert!(!SendTarget::Codex.matches_exe_path(chatgpt));
        // 送信キー: Claude Code は Enter、Codex は Ctrl+Enter
        assert!(!SendTarget::ClaudeCode.submit_needs_modifier());
        assert!(SendTarget::Codex.submit_needs_modifier());
    }

    #[test]
    fn test_prepare_text() {
        let lines = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(prepare_text(&lines(&["", "  "])), None);
        assert_eq!(
            prepare_text(&lines(&["一行目", "二行目", "", ""])),
            Some("一行目\n二行目".to_string())
        );
    }
}
