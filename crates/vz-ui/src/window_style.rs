//! 背面透過（背景）・文字透過・最前面固定のウィンドウスタイル管理。
//!
//! 背景色は `bg()` でアルファを掛ける。文字など前景は各パネルの内容に
//! `Ui::multiply_opacity(text_alpha())` を掛ける。ダイアログとメニューのポップアップは常に不透明。
//! 描画コードの各所から参照できるよう、不透明度はプロセス全体で共有する。

use eframe::egui::{Color32, Painter};
use std::sync::atomic::{AtomicU8, Ordering};

pub const MIN_OPACITY: u8 = 20;
pub const MAX_OPACITY: u8 = 100;
pub const OPACITY_STEP: u8 = 10;

static BG_OPACITY: AtomicU8 = AtomicU8::new(MAX_OPACITY);
static TEXT_OPACITY: AtomicU8 = AtomicU8::new(MAX_OPACITY);

/// 背景不透明度を 20〜100% に丸めて設定し、設定後の値を返す。
pub fn set_opacity_percent(percent: u8) -> u8 {
    let p = clamp_opacity(percent);
    BG_OPACITY.store(p, Ordering::Relaxed);
    p
}

pub fn opacity_percent() -> u8 {
    BG_OPACITY.load(Ordering::Relaxed)
}

/// 文字（前景）の不透明度を 20〜100% に丸めて設定し、設定後の値を返す。
pub fn set_text_opacity_percent(percent: u8) -> u8 {
    let p = clamp_opacity(percent);
    TEXT_OPACITY.store(p, Ordering::Relaxed);
    p
}

pub fn text_opacity_percent() -> u8 {
    TEXT_OPACITY.load(Ordering::Relaxed)
}

/// `Ui::multiply_opacity` に渡す文字の不透明度（0.2〜1.0）。
pub fn text_alpha() -> f32 {
    text_opacity_percent() as f32 / 100.0
}

/// 文字の不透明度の影響を受けない背景用ペインター。
/// 文字透過を掛けた Ui の中で背景を塗るときに使う。
pub fn bg_painter(painter: &Painter) -> Painter {
    let mut p = painter.clone();
    p.set_opacity(1.0);
    p
}

pub fn clamp_opacity(percent: u8) -> u8 {
    percent.clamp(MIN_OPACITY, MAX_OPACITY)
}

/// 背景色に現在の不透明度を適用する。
pub fn bg(c: Color32) -> Color32 {
    with_opacity(c, opacity_percent())
}

pub fn with_opacity(c: Color32, percent: u8) -> Color32 {
    let percent = clamp_opacity(percent);
    if percent >= MAX_OPACITY {
        return c;
    }
    let a = ((c.a() as u32 * percent as u32 + 50) / 100) as u8;
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_with_opacity() {
        let navy = Color32::from_rgb(8, 24, 56);
        assert_eq!(with_opacity(navy, 100), navy);
        assert_eq!(with_opacity(navy, 50).a(), 128);
        assert_eq!(with_opacity(navy, 0).a(), with_opacity(navy, MIN_OPACITY).a());
        assert_eq!(clamp_opacity(150), MAX_OPACITY);
    }
}
