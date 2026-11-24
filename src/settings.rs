//! 設定（背景・文字の不透明度、最前面固定、フォントサイズ、キー割り当て）の保存と読み込み。
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub opacity: u8,
    pub text_opacity: u8,
    pub always_on_top: bool,
    pub font_size: f32,
    pub key_scheme: KeyScheme,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            opacity: 100,
            text_opacity: 100,
            always_on_top: false,
            font_size: 16.0,
            key_scheme: KeyScheme::Sakura,
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
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
                "key_scheme" => {
                    s.key_scheme = if value == "vz" {
                        KeyScheme::Vz
                    } else {
                        KeyScheme::Sakura
                    }
                }
                "font_size" => {
                    if let Ok(v) = value.parse::<f32>() {
                        if v.is_finite() {
                            s.font_size = v;
                        }
                    }
                }
                _ => {}
            }
        }
        s
    }

    pub fn serialize(&self) -> String {
        format!(
            "opacity={}\ntext_opacity={}\nalways_on_top={}\nfont_size={}\nkey_scheme={}\n",
            self.opacity,
            self.text_opacity,
            self.always_on_top,
            self.font_size,
            self.key_scheme.as_str()
        )
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
        };
        assert_eq!(Settings::parse(&s.serialize()), s);
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
    }
}
