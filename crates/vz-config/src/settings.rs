//! 設定（背景・文字の不透明度、最前面固定、フォントサイズ、キー割り当て、折り返し、ウィンドウの位置とサイズ）の保存と読み込み。
//!
//! 保存先:
//! - Windows: `%APPDATA%\vzeditor\settings.ini`
//! - macOS:   `~/Library/Application Support/vzeditor/settings.ini`
//! - Linux:   `$XDG_CONFIG_HOME/vzeditor/settings.ini`（未設定時 `~/.config`）

use std::path::PathBuf;

/// キー割り当ての種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyScheme {
    /// サクラエディタ互換（既定）
    Sakura,
    /// VZ Editor 互換（ダイヤモンドカーソル、^Q / ^K の2打鍵）
    Vz,
}

impl KeyScheme {
    pub fn as_str(self) -> &'static str {
        match self {
            KeyScheme::Sakura => "sakura",
            KeyScheme::Vz => "vz",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            KeyScheme::Sakura => "サクラ互換",
            KeyScheme::Vz => "VZ互換",
        }
    }
}

/// 前回のウィンドウの位置とサイズ（前回のモニターで起動するため）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowGeometry {
    /// 外枠の左上（物理ピクセル。複数モニターの仮想画面座標）
    pub x: i32,
    pub y: i32,
    /// 内側のサイズ（論理ピクセル = egui のポイント）
    pub width: f32,
    pub height: f32,
    /// 保存時のモニターの拡大率（1.0 = 100%）
    pub scale: f32,
    /// 最大化していた（位置とサイズは元に戻したときのもの）
    pub maximized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub opacity: u8,
    pub text_opacity: u8,
    pub always_on_top: bool,
    pub font_size: f32,
    pub key_scheme: KeyScheme,
    /// 長い行を折り返して表示する
    pub word_wrap: bool,
    /// 前回のウィンドウの位置とサイズ（未保存なら None = OS の既定位置）
    pub window: Option<WindowGeometry>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            opacity: 100,
            text_opacity: 100,
            always_on_top: false,
            font_size: 16.0,
            key_scheme: KeyScheme::Sakura,
            word_wrap: false,
            window: None,
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        let (mut wx, mut wy, mut ww, mut wh) = (None, None, None, None);
        let mut wscale = 1.0_f32;
        let mut wmax = false;
        let finite = |v: &str| v.parse::<f32>().ok().filter(|v| v.is_finite());
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "opacity" => {
                    if let Ok(v) = value.parse::<u8>() {
                        s.opacity = v;
                    }
                }
                "text_opacity" => {
                    if let Ok(v) = value.parse::<u8>() {
                        s.text_opacity = v;
                    }
                }
                "always_on_top" => s.always_on_top = value == "true",
                "word_wrap" => s.word_wrap = value == "true",
                "key_scheme" => {
                    s.key_scheme = if value == "vz" {
                        KeyScheme::Vz
                    } else {
                        KeyScheme::Sakura
                    }
                }
                "font_size" => {
                    if let Some(v) = finite(value) {
                        s.font_size = v;
                    }
                }
                "window_x" => wx = value.parse::<i32>().ok(),
                "window_y" => wy = value.parse::<i32>().ok(),
                "window_width" => ww = finite(value).filter(|v| *v > 0.0),
                "window_height" => wh = finite(value).filter(|v| *v > 0.0),
                "window_scale" => {
                    if let Some(v) = finite(value).filter(|v| *v > 0.0) {
                        wscale = v;
                    }
                }
                "window_maximized" => wmax = value == "true",
                _ => {}
            }
        }
        if let (Some(x), Some(y), Some(width), Some(height)) = (wx, wy, ww, wh) {
            s.window = Some(WindowGeometry {
                x,
                y,
                width,
                height,
                scale: wscale,
                maximized: wmax,
            });
        }
        s
    }

    pub fn serialize(&self) -> String {
        let mut text = format!(
            "opacity={}\ntext_opacity={}\nalways_on_top={}\nfont_size={}\nkey_scheme={}\nword_wrap={}\n",
            self.opacity,
            self.text_opacity,
            self.always_on_top,
            self.font_size,
            self.key_scheme.as_str(),
            self.word_wrap
        );
        if let Some(w) = &self.window {
            text.push_str(&format!(
                "window_x={}
window_y={}
window_width={}
window_height={}
window_scale={}
window_maximized={}
",
                w.x, w.y, w.width, w.height, w.scale, w.maximized
            ));
        }
        text
    }

    pub fn load() -> Self {
        config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = config_path() else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.serialize())
    }
}

fn config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library").join("Application Support"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }
}

/// vz の設定フォルダ（`settings.ini` を置く場所）。
pub fn app_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join("vzeditor"))
}

pub fn config_path() -> Option<PathBuf> {
    app_dir().map(|d| d.join("settings.ini"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_roundtrip() {
        let s = Settings {
            opacity: 70,
            text_opacity: 60,
            always_on_top: true,
            font_size: 20.0,
            key_scheme: KeyScheme::Vz,
            word_wrap: true,
            window: Some(WindowGeometry {
                x: -1920,
                y: 120,
                width: 900.5,
                height: 600.0,
                scale: 1.5,
                maximized: true,
            }),
        };
        assert_eq!(Settings::parse(&s.serialize()), s);
        let no_window = Settings { window: None, ..s };
        assert_eq!(Settings::parse(&no_window.serialize()), no_window);
    }

    #[test]
    fn test_settings_window_needs_all_values() {
        // 位置かサイズが欠けていれば復元しない
        let s = Settings::parse("window_x=10
window_y=20
window_width=800
");
        assert_eq!(s.window, None);
        let s = Settings::parse("window_x=10
window_y=20
window_width=800
window_height=0
");
        assert_eq!(s.window, None);
        // 拡大率が無ければ 1.0
        let s = Settings::parse("window_x=10
window_y=-5
window_width=800
window_height=500
");
        let w = s.window.unwrap();
        assert_eq!((w.x, w.y, w.scale, w.maximized), (10, -5, 1.0, false));
    }

    #[test]
    fn test_settings_parse_ignores_garbage() {
        let s = Settings::parse("opacity=abc\nfoo=bar\nalways_on_top=true\n");
        assert_eq!(s.opacity, 100);
        assert_eq!(s.text_opacity, 100);
        assert!(s.always_on_top);
        assert_eq!(s.font_size, 16.0);
        // 既定はサクラ互換
        assert_eq!(s.key_scheme, KeyScheme::Sakura);
        assert!(!s.word_wrap);
        assert_eq!(s.window, None);
    }
}
