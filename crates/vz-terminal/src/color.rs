//! 端末の色番号（16 色・256 色）を RGB にする。

/// 基本 16 色（xterm に近い配色）
const ANSI16: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00),
    (0xcd, 0x31, 0x31),
    (0x0d, 0xbc, 0x79),
    (0xe5, 0xe5, 0x10),
    (0x24, 0x72, 0xc8),
    (0xbc, 0x3f, 0xbc),
    (0x11, 0xa8, 0xcd),
    (0xe5, 0xe5, 0xe5),
    (0x66, 0x66, 0x66),
    (0xf1, 0x4c, 0x4c),
    (0x23, 0xd1, 0x8b),
    (0xf5, 0xf5, 0x43),
    (0x3b, 0x8e, 0xea),
    (0xd6, 0x70, 0xd6),
    (0x29, 0xb8, 0xdb),
    (0xff, 0xff, 0xff),
];

/// 256 色の番号 → RGB。
pub fn indexed_rgb(idx: u8) -> (u8, u8, u8) {
    match idx {
        0..=15 => ANSI16[idx as usize],
        16..=231 => {
            let i = idx - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        _ => {
            let v = 8 + (idx - 232) * 10;
            (v, v, v)
        }
    }
}

/// vt100 の色 → RGB。既定色は None（呼び出し側の前景・背景色を使う）。
/// `bold` の前景は基本 8 色を明るい色にする。
pub fn rgb(color: vt100::Color, bold: bool) -> Option<(u8, u8, u8)> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) if bold && i < 8 => Some(indexed_rgb(i + 8)),
        vt100::Color::Idx(i) => Some(indexed_rgb(i)),
        vt100::Color::Rgb(r, g, b) => Some((r, g, b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indexed_rgb_ranges() {
        assert_eq!(indexed_rgb(1), (0xcd, 0x31, 0x31));
        assert_eq!(indexed_rgb(16), (0, 0, 0));
        assert_eq!(indexed_rgb(231), (255, 255, 255));
        assert_eq!(indexed_rgb(232), (8, 8, 8));
        assert_eq!(indexed_rgb(255), (238, 238, 238));
    }

    #[test]
    fn test_bold_brightens_basic_colors() {
        assert_eq!(rgb(vt100::Color::Idx(1), true), Some(indexed_rgb(9)));
        assert_eq!(rgb(vt100::Color::Idx(100), true), Some(indexed_rgb(100)));
        assert_eq!(rgb(vt100::Color::Default, false), None);
    }
}
