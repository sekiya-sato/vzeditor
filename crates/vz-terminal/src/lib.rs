//! 組み込みターミナル。PTY（Windows は ConPTY）でシェルを起動し、出力を vt100 の画面モデルに流す。
//! 描画とキーの受け取りは vz-ui が行い、ここは egui に依存しない。

pub mod color;
pub mod keys;
pub mod shell;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

pub use shell::ShellCommand;
pub use vt100;

/// 保持するスクロールバックの行数
const SCROLLBACK_LINES: usize = 5000;

/// 画面モデルからの通知（タイトル変更と、端末への応答）。
#[derive(Default)]
struct Hooks {
    title: Option<String>,
    /// DSR（`ESC [ 6 n` など）への応答。シェルへ書き戻す
    replies: Vec<u8>,
}

impl vt100::Callbacks for Hooks {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = Some(String::from_utf8_lossy(title).into_owned());
    }

    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        if c != 'n' || i1.is_some() {
            return;
        }
        match params.first().and_then(|p| p.first()) {
            // 端末の状態: 正常
            Some(5) => self.replies.extend_from_slice(b"\x1b[0n"),
            // カーソル位置（1 始まり）
            Some(6) => {
                let (row, col) = screen.cursor_position();
                self.replies
                    .extend_from_slice(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
            }
            _ => {}
        }
    }
}

/// 起動中のシェル 1 つ分。
pub struct Terminal {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    parser: vt100::Parser<Hooks>,
    rx: Receiver<Vec<u8>>,
    exit_code: Option<u32>,
    program: String,
    /// シェルを起動したフォルダ（保存して終了・復元で使う）
    cwd: PathBuf,
}

impl Terminal {
    /// `shell` を `cwd` で起動する。`wake` は出力が届くたびに読み取りスレッドから呼ぶ（再描画の要求）。
    pub fn spawn(
        shell: &ShellCommand,
        cwd: &Path,
        rows: u16,
        cols: u16,
        wake: impl Fn() + Send + 'static,
    ) -> Result<Self, String> {
        let (rows, cols) = (rows.max(2), cols.max(10));
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("PTY を作れません: {e}"))?;

        // 正規化したパスの \\?\ は WSL・cmd が扱えないので外す
        let cwd_str = cwd.to_string_lossy();
        let cwd_str = cwd_str.strip_prefix(r"\\?\").unwrap_or(&cwd_str);
        let mut cmd = CommandBuilder::new(&shell.program);
        cmd.args(shell.args_for(cwd_str));
        cmd.cwd(cwd_str);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        // WSL へ TERM / COLORTERM を引き継ぐ
        let wslenv = std::env::var("WSLENV").unwrap_or_default();
        cmd.env(
            "WSLENV",
            if wslenv.is_empty() {
                "TERM:COLORTERM".to_string()
            } else {
                format!("{wslenv}:TERM:COLORTERM")
            },
        );
        // Git Bash の --login がホームへ移動しないようにする（起動したフォルダのまま）
        cmd.env("CHERE_INVOKING", "1");

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("{} を起動できません: {e}", shell.program.display()))?;
        // 子プロセスが端末を握ったので、こちら側の slave は閉じる（終了時に EOF が届く）
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("PTY を読めません: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("PTY に書けません: {e}"))?;

        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("vz-terminal-reader".into())
            .spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                            wake();
                        }
                    }
                }
                // 送信側を閉じて終了を知らせる
                drop(tx);
                wake();
            })
            .map_err(|e| format!("読み取りスレッドを作れません: {e}"))?;

        Ok(Self {
            master: pair.master,
            writer,
            child,
            parser: vt100::Parser::new_with_callbacks(
                rows,
                cols,
                SCROLLBACK_LINES,
                Hooks::default(),
            ),
            rx,
            exit_code: None,
            program: shell.program.display().to_string(),
            cwd: cwd.to_path_buf(),
        })
    }

    /// 届いた出力を画面モデルへ反映する。変化があれば true。
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        let mut closed = false;
        loop {
            match self.rx.try_recv() {
                Ok(bytes) => {
                    self.parser.process(&bytes);
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }
        let replies = std::mem::take(&mut self.parser.callbacks_mut().replies);
        if !replies.is_empty() {
            self.write(&replies);
        }
        // Windows の ConPTY は子が終わっても EOF にならないことがあるので、プロセスも見る
        if self.exit_code.is_none() {
            let exited = match self.child.try_wait() {
                Ok(Some(status)) => Some(status.exit_code()),
                Ok(None) => None,
                Err(_) => closed.then_some(0),
            };
            if exited.is_some() || closed {
                self.exit_code = exited.or(Some(0));
                changed = true;
            }
        }
        changed
    }

    /// シェルが終了していれば終了コード。
    pub fn exit_code(&self) -> Option<u32> {
        self.exit_code
    }

    pub fn is_running(&self) -> bool {
        self.exit_code.is_none()
    }

    /// 起動したシェルのパス（表示用）。
    pub fn program(&self) -> &str {
        &self.program
    }

    /// シェルを起動したフォルダ（シェルの中で cd した先ではない）。
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// OSC で設定されたタイトル。
    pub fn title(&self) -> Option<&str> {
        self.parser.callbacks().title.as_deref()
    }

    /// シェルへバイト列を送る。終了後は捨てる。
    pub fn write(&mut self, bytes: &[u8]) {
        if self.exit_code.is_some() || bytes.is_empty() {
            return;
        }
        // 書けないのはシェルが終了したとき。終了は poll で拾う
        let _ = self
            .writer
            .write_all(bytes)
            .and_then(|_| self.writer.flush());
    }

    /// 打鍵の送信。スクロールバックを見ていたら最下部へ戻す。
    pub fn send_input(&mut self, bytes: &[u8]) {
        self.parser.screen_mut().set_scrollback(0);
        self.write(bytes);
    }

    /// 貼り付け。アプリが bracketed paste を有効にしていれば囲む。改行は CR にする。
    pub fn paste(&mut self, text: &str) {
        let body = text.replace("\r\n", "\r").replace('\n', "\r");
        if self.parser.screen().bracketed_paste() {
            let mut out = Vec::with_capacity(body.len() + 12);
            out.extend_from_slice(b"\x1b[200~");
            out.extend_from_slice(body.as_bytes());
            out.extend_from_slice(b"\x1b[201~");
            self.send_input(&out);
        } else {
            self.send_input(body.as_bytes());
        }
    }

    /// 表示する行数・桁数を変える。
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(2), cols.max(10));
        if self.parser.screen().size() == (rows, cols) {
            return;
        }
        self.parser.screen_mut().set_size(rows, cols);
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// スクロールバックを `delta` 行動かす（正で過去へ）。
    pub fn scroll_by(&mut self, delta: i32) {
        let screen = self.parser.screen_mut();
        let cur = screen.scrollback() as i64;
        screen.set_scrollback((cur + i64::from(delta)).max(0) as usize);
    }

    /// 画面に見えている文字（コピー用）。行末の空白は除く。
    pub fn visible_text(&self) -> String {
        let text = self.parser.screen().contents();
        let mut out = String::with_capacity(text.len());
        for line in text.lines() {
            out.push_str(line.trim_end());
            out.push('\n');
        }
        let trimmed = out.trim_end_matches('\n').len();
        out.truncate(trimmed);
        out
    }
}

/// 画面上の 2 つのセル（行, 桁。どちらも含む）の間の文字（マウス選択のコピー用）。
/// 順序は問わない。各行の行末の空白は除き、折り返した行はつなげる。
pub fn selection_text(screen: &vt100::Screen, a: (u16, u16), b: (u16, u16)) -> String {
    let (start, end) = if a <= b { (a, b) } else { (b, a) };
    let (_, cols) = screen.size();
    // 終わりのセルを含める（全角の左半分なら右半分まで）
    let end_wide = screen.cell(end.0, end.1).is_some_and(vt100::Cell::is_wide);
    let end_col = (end.1 + if end_wide { 2 } else { 1 }).min(cols);
    let text = screen.contents_between(start.0, start.1, end.0, end_col);
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end());
    }
    out
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if self.exit_code.is_none() {
            let _ = self.child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn echo_command(text: &str) -> ShellCommand {
        if cfg!(windows) {
            ShellCommand {
                program: "cmd.exe".into(),
                args: vec!["/c".into(), format!("echo {text}")],
            }
        } else {
            ShellCommand {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), format!("echo {text}")],
            }
        }
    }

    #[test]
    fn test_selection_text_spans_rows_and_trims() {
        let mut parser = vt100::Parser::new(5, 20, 0);
        parser.process("hello world   \r\nsecond line\r\n日本語abc".as_bytes());
        let screen = parser.screen();
        // 1 行の中（終わりのセルを含む）
        assert_eq!(selection_text(screen, (0, 6), (0, 10)), "world");
        // 逆向きにドラッグしても同じ
        assert_eq!(selection_text(screen, (0, 10), (0, 6)), "world");
        // 複数行（行末の空白を除く）
        assert_eq!(selection_text(screen, (0, 6), (1, 5)), "world\nsecond");
        // 全角の左半分で終わると 1 文字まるごと入る
        assert_eq!(selection_text(screen, (2, 0), (2, 2)), "日本");
    }

    #[test]
    fn test_spawn_shows_output_and_exits() {
        let cwd = std::env::temp_dir();
        let mut term =
            Terminal::spawn(&echo_command("vz-terminal-ok"), &cwd, 24, 80, || {}).expect("spawn");
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            term.poll();
            if term.visible_text().contains("vz-terminal-ok") && !term.is_running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            term.visible_text().contains("vz-terminal-ok"),
            "{}",
            term.visible_text()
        );
        assert!(!term.is_running());
    }

    #[test]
    fn test_dsr_is_answered_with_cursor_position() {
        let mut hooks = Hooks::default();
        let mut parser = vt100::Parser::new_with_callbacks(5, 20, 0, std::mem::take(&mut hooks));
        parser.process(b"ab\x1b[6n");
        assert_eq!(parser.callbacks().replies, b"\x1b[1;3R");
    }
}
