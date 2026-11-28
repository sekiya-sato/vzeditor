//! キー入力を端末へ送るバイト列に変える（xterm 互換）。

/// 文字以外のキー。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermKey {
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// F1〜F12
    F(u8),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Mods {
    /// xterm の修飾キー番号（1 = なし）
    fn param(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }
}

/// 文字以外のキーのバイト列。`app_cursor` は DECCKM（矢印キーを `ESC O x` で送る）。
pub fn encode_key(key: TermKey, mods: Mods, app_cursor: bool) -> Vec<u8> {
    let m = mods.param();
    let alt_prefix = |bytes: &[u8]| {
        let mut v = Vec::with_capacity(bytes.len() + 1);
        if mods.alt {
            v.push(0x1b);
        }
        v.extend_from_slice(bytes);
        v
    };
    // 矢印・Home・End: 修飾なしは CSI x（DECCKM 中は SS3 x）、修飾ありは CSI 1;m x
    let cursor = |c: u8| -> Vec<u8> {
        if m > 1 {
            format!("\x1b[1;{m}{}", c as char).into_bytes()
        } else if app_cursor {
            vec![0x1b, b'O', c]
        } else {
            vec![0x1b, b'[', c]
        }
    };
    // Ins・Del・PgUp・PgDn・F5 以降: CSI n ~（修飾ありは CSI n;m ~）
    let tilde = |n: u8| -> Vec<u8> {
        if m > 1 {
            format!("\x1b[{n};{m}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    match key {
        TermKey::Enter => alt_prefix(b"\r"),
        TermKey::Backspace => alt_prefix(if mods.ctrl { b"\x08" } else { b"\x7f" }),
        TermKey::Tab => {
            if mods.shift {
                b"\x1b[Z".to_vec()
            } else {
                alt_prefix(b"\t")
            }
        }
        TermKey::Escape => alt_prefix(b"\x1b"),
        TermKey::Up => cursor(b'A'),
        TermKey::Down => cursor(b'B'),
        TermKey::Right => cursor(b'C'),
        TermKey::Left => cursor(b'D'),
        TermKey::Home => cursor(b'H'),
        TermKey::End => cursor(b'F'),
        TermKey::Insert => tilde(2),
        TermKey::Delete => tilde(3),
        TermKey::PageUp => tilde(5),
        TermKey::PageDown => tilde(6),
        TermKey::F(n @ 1..=4) => {
            let c = b'P' + (n - 1);
            if m > 1 {
                format!("\x1b[1;{m}{}", c as char).into_bytes()
            } else {
                vec![0x1b, b'O', c]
            }
        }
        TermKey::F(n) => {
            let code = match n {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                _ => 24,
            };
            tilde(code)
        }
    }
}

/// Ctrl+文字 の制御コード（Ctrl+A → 0x01、Ctrl+[ → ESC、Ctrl+Space → NUL）。対象外は None。
pub fn ctrl_char(c: char) -> Option<u8> {
    match c.to_ascii_lowercase() {
        c @ 'a'..='z' => Some(c as u8 - b'a' + 1),
        ' ' | '@' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '7' | '/' => Some(0x1f),
        '8' | '?' => Some(0x7f),
        _ => None,
    }
}

/// Ctrl / Alt 付きの文字キー。Alt は ESC を前置する。
pub fn encode_char(c: char, mods: Mods) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(5);
    if mods.alt {
        out.push(0x1b);
    }
    if mods.ctrl {
        out.push(ctrl_char(c)?);
    } else {
        let c = if mods.shift {
            c.to_ascii_uppercase()
        } else {
            c
        };
        let mut buf = [0u8; 4];
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Mods = Mods {
        ctrl: false,
        alt: false,
        shift: false,
    };

    #[test]
    fn test_cursor_keys_follow_decckm_and_modifiers() {
        assert_eq!(encode_key(TermKey::Up, NONE, false), b"\x1b[A");
        assert_eq!(encode_key(TermKey::Up, NONE, true), b"\x1bOA");
        let ctrl = Mods { ctrl: true, ..NONE };
        assert_eq!(encode_key(TermKey::Right, ctrl, true), b"\x1b[1;5C");
    }

    #[test]
    fn test_editing_and_function_keys() {
        assert_eq!(encode_key(TermKey::Enter, NONE, false), b"\r");
        assert_eq!(encode_key(TermKey::Backspace, NONE, false), b"\x7f");
        assert_eq!(encode_key(TermKey::Delete, NONE, false), b"\x1b[3~");
        assert_eq!(encode_key(TermKey::F(1), NONE, false), b"\x1bOP");
        assert_eq!(encode_key(TermKey::F(12), NONE, false), b"\x1b[24~");
        let shift = Mods {
            shift: true,
            ..NONE
        };
        assert_eq!(encode_key(TermKey::Tab, shift, false), b"\x1b[Z");
    }

    #[test]
    fn test_ctrl_and_alt_chars() {
        let ctrl = Mods { ctrl: true, ..NONE };
        assert_eq!(encode_char('c', ctrl), Some(vec![0x03]));
        assert_eq!(encode_char('[', ctrl), Some(vec![0x1b]));
        assert_eq!(encode_char(' ', ctrl), Some(vec![0x00]));
        assert_eq!(encode_char('!', ctrl), None);
        let alt = Mods { alt: true, ..NONE };
        assert_eq!(encode_char('f', alt), Some(b"\x1bf".to_vec()));
    }
}
