use eframe::egui::IconData;

const ICON_SIZE: usize = 64;

/// Signed distance to a centered rounded rectangle (negative = inside).
fn sd_rounded_box(px: f32, py: f32, cx: f32, cy: f32, half_w: f32, half_h: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (half_w - r);
    let qy = (py - cy).abs() - (half_h - r);
    let outside = (qx.max(0.0).hypot(qy.max(0.0))).max(0.0);
    let inside = qx.max(qy).min(0.0);
    outside + inside - r
}

fn in_rounded_rect(x: f32, y: f32, rx: f32, ry: f32, rw: f32, rh: f32, r: f32) -> bool {
    let cx = rx + rw * 0.5;
    let cy = ry + rh * 0.5;
    sd_rounded_box(x, y, cx, cy, rw * 0.5, rh * 0.5, r) <= 0.0
}

fn point_in_polygon(x: f32, y: f32, verts: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let n = verts.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = verts[i];
        let (xj, yj) = verts[j];
        let intersects =
            ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / ((yj - yi) + f32::EPSILON) + xi);
        if intersects {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    let t = t.clamp(0.0, 1.0);
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

fn blend_rgba(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let sa = src[3] as f32 / 255.0;
    let inv = 1.0 - sa;
    [
        (src[0] as f32 * sa + dst[0] as f32 * inv).round() as u8,
        (src[1] as f32 * sa + dst[1] as f32 * inv).round() as u8,
        (src[2] as f32 * sa + dst[2] as f32 * inv).round() as u8,
        255,
    ]
}

/// Sample the 128x128 SVG design (`assets/vzeditor.svg`) at floating-point coordinate `(x, y)`.
fn sample_svg(x: f32, y: f32) -> [u8; 4] {
    // Window frame: rect x=6 y=6 w=116 h=116 rx=16 stroke=#40D2FF stroke-width=4
    let frame_sd = sd_rounded_box(x, y, 64.0, 64.0, 58.0, 58.0, 16.0);
    if frame_sd > 2.0 {
        return [0, 0, 0, 0];
    }

    // Cyan border ring (#40D2FF)
    if frame_sd >= -2.0 {
        return [0x40, 0xD2, 0xFF, 255];
    }

    // Interior deep navy gradient: #0E2A5C (top) -> #081838 (bottom)
    let bg_t = ((y - 6.0) / 116.0).clamp(0.0, 1.0);
    let mut color = [
        lerp_u8(0x0E, 0x08, bg_t),
        lerp_u8(0x2A, 0x18, bg_t),
        lerp_u8(0x5C, 0x38, bg_t),
        255,
    ];

    // Top VZ status bar (y <= 29)
    if y <= 29.0 {
        color = [0x16, 0x3E, 0x82, 255];
    }
    if (28.0..=30.0).contains(&y) {
        color = [0x40, 0xD2, 0xFF, 255];
    }
    // Status bar badges
    if in_rounded_rect(x, y, 18.0, 15.0, 22.0, 9.0, 2.0) {
        color = [0x40, 0xD2, 0xFF, 255];
    }
    if in_rounded_rect(x, y, 82.0, 17.0, 28.0, 5.0, 2.0) {
        color = blend_rgba(color, [0x70, 0xE8, 0xFF, 217]);
    }

    // Wide diagonal arms keep the V visible in the 16 px Windows icon.
    const V_VERTS: [(f32, f32); 7] = [
        (16.0, 40.0),
        (25.0, 40.0),
        (36.0, 72.0),
        (47.0, 40.0),
        (56.0, 40.0),
        (41.0, 88.0),
        (31.0, 88.0),
    ];
    if point_in_polygon(x, y, &V_VERTS) {
        color = [0xEA, 0xF4, 0xFF, 255];
    }

    // Vector letter Z, spaced apart from the V and the cursor.
    const Z_VERTS: [(f32, f32); 10] = [
        (60.0, 40.0),
        (92.0, 40.0),
        (92.0, 49.0),
        (73.0, 79.0),
        (92.0, 79.0),
        (92.0, 88.0),
        (60.0, 88.0),
        (60.0, 79.0),
        (79.0, 49.0),
        (60.0, 49.0),
    ];
    if point_in_polygon(x, y, &Z_VERTS) {
        color = [0xEA, 0xF4, 0xFF, 255];
    }

    // Glowing PC-98 cursor block (#70E8FF -> #2E8CD0).
    if in_rounded_rect(x, y, 100.0, 40.0, 12.0, 48.0, 2.0) {
        let tx = (x - 100.0) / 12.0;
        let ty = (y - 40.0) / 48.0;
        let t = ((tx + ty) * 0.5).clamp(0.0, 1.0);
        color = [
            lerp_u8(0x70, 0x2E, t),
            lerp_u8(0xE8, 0x8C, t),
            lerp_u8(0xFF, 0xD0, t),
            255,
        ];
    }

    // Bottom F-key guide strip
    if (97.0..=99.0).contains(&y) {
        color = [0x2A, 0x62, 0xA8, 255];
    }
    for &kx in &[16.0_f32, 36.0, 56.0, 76.0, 96.0] {
        if in_rounded_rect(x, y, kx, 104.0, 16.0, 10.0, 2.0) {
            color = [0x2E, 0x8C, 0xD0, 255];
        }
    }

    color
}

/// Build the 64x64 RGBA window icon matching `assets/vzeditor.svg` with 2x2 supersampling.
pub fn build_app_icon() -> IconData {
    IconData {
        rgba: render_icon_rgba(ICON_SIZE, 2),
        width: ICON_SIZE as u32,
        height: ICON_SIZE as u32,
    }
}

/// Render the icon at `size`x`size` pixels with `ss`x`ss` supersampling
/// (mapped onto the 128x128 SVG coordinate grid).
fn render_icon_rgba(size: usize, ss: usize) -> Vec<u8> {
    let mut rgba = vec![0u8; size * size * 4];
    let scale = 128.0 / (size * ss) as f32;
    let samples = (ss * ss) as f32;

    for py in 0..size {
        for px in 0..size {
            let mut acc_r = 0.0_f32;
            let mut acc_g = 0.0_f32;
            let mut acc_b = 0.0_f32;
            let mut acc_a = 0.0_f32;

            for sy in 0..ss {
                for sx in 0..ss {
                    let x = ((px * ss + sx) as f32 + 0.5) * scale;
                    let y = ((py * ss + sy) as f32 + 0.5) * scale;
                    let s = sample_svg(x, y);
                    let a = s[3] as f32 / 255.0;
                    acc_r += s[0] as f32 * a;
                    acc_g += s[1] as f32 * a;
                    acc_b += s[2] as f32 * a;
                    acc_a += a;
                }
            }

            let idx = (py * size + px) * 4;
            if acc_a > 0.001 {
                rgba[idx] = (acc_r / acc_a).round().clamp(0.0, 255.0) as u8;
                rgba[idx + 1] = (acc_g / acc_a).round().clamp(0.0, 255.0) as u8;
                rgba[idx + 2] = (acc_b / acc_a).round().clamp(0.0, 255.0) as u8;
                rgba[idx + 3] = ((acc_a / samples) * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    rgba
}

/// Windows の exe に埋め込む ICO（`assets/vzeditor.ico`）を作る。
/// 16〜256px の各サイズを 32bit BMP で格納する。
#[cfg(test)]
fn build_ico() -> Vec<u8> {
    const SIZES: [usize; 7] = [16, 24, 32, 48, 64, 128, 256];
    let images: Vec<Vec<u8>> = SIZES
        .iter()
        .map(|&size| {
            let rgba = render_icon_rgba(size, 8);
            let mask_row = (size.div_ceil(32)) * 4;
            let mut dib = Vec::new();
            // BITMAPINFOHEADER（高さは XOR + AND マスクで 2 倍）
            dib.extend_from_slice(&40u32.to_le_bytes());
            dib.extend_from_slice(&(size as i32).to_le_bytes());
            dib.extend_from_slice(&((size * 2) as i32).to_le_bytes());
            dib.extend_from_slice(&1u16.to_le_bytes());
            dib.extend_from_slice(&32u16.to_le_bytes());
            dib.extend_from_slice(&0u32.to_le_bytes());
            dib.extend_from_slice(&((size * size * 4 + mask_row * size) as u32).to_le_bytes());
            dib.extend_from_slice(&[0u8; 16]);
            // 画素は下から上へ BGRA
            for y in (0..size).rev() {
                for x in 0..size {
                    let i = (y * size + x) * 4;
                    dib.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
                }
            }
            // AND マスク（透明画素を 1）
            for y in (0..size).rev() {
                let mut row = vec![0u8; mask_row];
                for x in 0..size {
                    if rgba[(y * size + x) * 4 + 3] == 0 {
                        row[x / 8] |= 0x80 >> (x % 8);
                    }
                }
                dib.extend_from_slice(&row);
            }
            dib
        })
        .collect();

    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len();
    for (&size, dib) in SIZES.iter().zip(&images) {
        let dim = if size >= 256 { 0 } else { size as u8 };
        ico.extend_from_slice(&[dim, dim, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(dib.len() as u32).to_le_bytes());
        ico.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += dib.len();
    }
    for dib in &images {
        ico.extend_from_slice(dib);
    }
    ico
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_icon_geometry_and_colors() {
        let icon = build_app_icon();
        assert_eq!(icon.width, 64);
        assert_eq!(icon.height, 64);
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);

        let pixel = |x: usize, y: usize| -> [u8; 4] {
            let i = (y * 64 + x) * 4;
            [
                icon.rgba[i],
                icon.rgba[i + 1],
                icon.rgba[i + 2],
                icon.rgba[i + 3],
            ]
        };

        // Outside rounded corner must be transparent
        assert_eq!(pixel(0, 0)[3], 0);
        assert_eq!(pixel(63, 63)[3], 0);

        // Top border center (svg x=64, y=6 -> pixel 32, 3) must be cyan (#40D2FF)
        let border = pixel(32, 3);
        assert_eq!(border[3], 255);
        assert!(border[2] > 230 && border[1] > 180);

        // White 'V' left arm (svg x=26, y=60 -> pixel 13, 30)
        let letter_v = pixel(13, 30);
        assert_eq!(letter_v, [0xEA, 0xF4, 0xFF, 255]);

        // 'V' notch between the arms stays background (svg x=35, y=50 -> pixel 17, 25)
        assert_ne!(pixel(17, 25), [0xEA, 0xF4, 0xFF, 255]);

        // White 'Z' top bar (svg x=70, y=44 -> pixel 35, 22)
        let letter_z = pixel(35, 22);
        assert_eq!(letter_z, [0xEA, 0xF4, 0xFF, 255]);

        // Glowing cyan cursor block (svg x=106, y=64 -> pixel 53, 32)
        let cursor = pixel(53, 32);
        assert_eq!(cursor[3], 255);
        assert!(cursor[2] > 210 && cursor[1] > 160);
    }

    #[test]
    fn test_small_icon_keeps_both_v_arms_and_the_centered_tip() {
        let rgba = render_icon_rgba(16, 8);
        let red = |x: usize, y: usize| rgba[(16 * y + x) * 4];

        assert!(red(2, 5) > 150 && red(6, 5) > 150);
        assert!(red(4, 5) < 50);
        assert!(red(4, 10) > 150);
        assert!(red(2, 10) < 50 && red(6, 10) < 50);
    }

    /// `assets/vzeditor.ico` が現在の描画と一致すること。
    /// 図柄を変えたら `VZ_WRITE_ICO=1 cargo test test_ico_asset_is_up_to_date` で作り直す。
    #[test]
    fn test_ico_asset_is_up_to_date() {
        let ico = build_ico();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/vzeditor.ico");
        if std::env::var_os("VZ_WRITE_ICO").is_some() {
            std::fs::write(&path, &ico).unwrap();
        }
        let current = std::fs::read(&path).expect("assets/vzeditor.ico がありません");
        assert!(
            current == ico,
            "assets/vzeditor.ico が古い（VZ_WRITE_ICO=1 で再生成）"
        );
    }
}
