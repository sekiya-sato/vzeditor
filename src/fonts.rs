use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::path::Path;
use std::sync::Arc;

struct FontCandidate {
    path: String,
    index: u32,
}

/// Query fontconfig via `fc-match` to get the exact file path and TTC face index.
#[cfg(not(any(windows, target_os = "macos")))]
fn query_fc_match(pattern: &str) -> Option<FontCandidate> {
    let output = std::process::Command::new("fc-match")
        .args(["-f", "%{file}\n%{index}\n", pattern])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let path = lines.next()?.trim().to_string();
    let index = lines.next().and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
    if path.is_empty() || !Path::new(&path).exists() {
        return None;
    }
    Some(FontCandidate { path, index })
}

/// Load a system Japanese monospace font into egui's font definitions to prevent tofu (missing glyphs).
pub fn configure_japanese_fonts(ctx: &egui::Context) -> Option<String> {
    let mut fonts = FontDefinitions::default();
    let mut loaded_names: Vec<String> = Vec::new();
    let mut seen_keys: Vec<(String, u32)> = Vec::new();

    // Priority 1: Query fontconfig for Japanese monospace fonts (works on Ubuntu, Debian, Raspberry Pi OS).
    // Prefer TrueType Japanese monospace fonts and Noto Sans Mono CJK JP.
    #[cfg(not(any(windows, target_os = "macos")))]
    let fc_patterns = [
        "Noto Sans Mono CJK JP",
        "IPAGothic",
        "VL Gothic",
        "TakaoGothic",
        "Noto Sans CJK JP",
        "monospace:lang=ja",
        "sans-serif:lang=ja",
    ];

    let mut candidates: Vec<FontCandidate> = Vec::new();
    #[cfg(not(any(windows, target_os = "macos")))]
    for pattern in fc_patterns {
        if let Some(cand) = query_fc_match(pattern) {
            // Skip non-Japanese fallback fonts returned when pattern doesn't match a CJK font.
            let lower = cand.path.to_lowercase();
            let looks_ja = lower.contains("cjk")
                || lower.contains("ipa")
                || lower.contains("vl-")
                || lower.contains("takao")
                || lower.contains("japanese")
                || lower.contains("noto")
                || lower.contains("droid")
                || lower.contains("wqy")
                || lower.contains("unifont")
                || lower.contains("hiragino")
                || lower.contains("mplus")
                || lower.contains("biz")
                || lower.contains("hackgen")
                || lower.contains("udev")
                || lower.contains("plemol");
            if looks_ja {
                candidates.push(cand);
            }
        }
    }

    // Priority 2: Direct filesystem fallbacks per OS.
    for (path, index) in static_font_fallbacks() {
        if Path::new(&path).exists() {
            candidates.push(FontCandidate { path, index });
        }
    }

    let mut primary_loaded_path: Option<String> = None;

    for (i, cand) in candidates.into_iter().enumerate() {
        let key = (cand.path.clone(), cand.index);
        if seen_keys.contains(&key) {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&cand.path) {
            seen_keys.push(key);
            let font_name = format!("ja_font_{i}");
            let mut font_data = FontData::from_owned(bytes);
            font_data.index = cand.index;
            fonts.font_data.insert(font_name.clone(), Arc::new(font_data));
            loaded_names.push(font_name);
            if primary_loaded_path.is_none() {
                primary_loaded_path = Some(format!("{} (index {})", cand.path, cand.index));
            }
            // Loading the single best Japanese monospace font keeps startup instant and RAM minimal.
            if !loaded_names.is_empty() {
                break;
            }
        }
    }

    // Insert loaded Japanese fonts at the front of Monospace and Proportional font families.
    for (pos, name) in loaded_names.iter().enumerate() {
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(pos, name.clone());
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(pos, name.clone());
    }

    ctx.set_fonts(fonts);
    primary_loaded_path
}

/// OS ごとの日本語フォント候補（優先順）。等幅フォントを先に並べる。
fn static_font_fallbacks() -> Vec<(String, u32)> {
    #[cfg(windows)]
    {
        use std::path::PathBuf;
        let windir = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:/Windows"));
        let mut v: Vec<(String, u32)> = Vec::new();
        let mut dirs = vec![windir.join("Fonts")];
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
        }
        for dir in &dirs {
            for (file, index) in [
                ("BIZ-UDGothicR.ttc", 0),  // BIZ UDゴシック（等幅）
                ("HackGen-Regular.ttf", 0),
                ("PlemolJP-Regular.ttf", 0),
                ("msgothic.ttc", 0),       // MS ゴシック（等幅）
                ("YuGothM.ttc", 0),
                ("meiryo.ttc", 0),
            ] {
                v.push((dir.join(file).to_string_lossy().into_owned(), index));
            }
        }
        v
    }
    #[cfg(target_os = "macos")]
    {
        [
            ("/Library/Fonts/Osaka.ttf", 0),
            ("/System/Library/Fonts/Supplemental/Osaka.ttf", 0),
            ("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", 0),
            ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
            ("/System/Library/Fonts/Supplemental/Arial Unicode.ttf", 0),
        ]
        .iter()
        .map(|&(p, i)| (p.to_string(), i))
        .collect()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        [
            ("/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf", 0),
            ("/usr/share/fonts/truetype/vlgothic/VL-Gothic-Regular.ttf", 0),
            ("/usr/share/fonts/truetype/takao-gothic/TakaoGothic.ttf", 0),
            ("/usr/share/fonts/truetype/fonts-japanese-gothic.ttf", 0),
            ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 5),
            ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
            ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 5),
            ("/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf", 0),
        ]
        .iter()
        .map(|&(p, i)| (p.to_string(), i))
        .collect()
    }
}
