//! 日本語フォントの候補（ファイルのパスと TTC の番号）。画面（vz-ui の `fonts`）と印刷で共用する。

use std::path::{Path, PathBuf};

/// 見つかった日本語フォントの候補を優先順に返す（等幅を先に。重複なし）。
pub fn japanese_font_candidates() -> Vec<(PathBuf, u32)> {
    let mut found: Vec<(PathBuf, u32)> = Vec::new();
    #[cfg(not(any(windows, target_os = "macos")))]
    for pattern in [
        "Noto Sans Mono CJK JP",
        "IPAGothic",
        "VL Gothic",
        "TakaoGothic",
        "Noto Sans CJK JP",
        "monospace:lang=ja",
        "sans-serif:lang=ja",
    ] {
        if let Some(cand) = query_fc_match(pattern) {
            if looks_japanese(&cand.0) && !found.contains(&cand) {
                found.push(cand);
            }
        }
    }
    for cand in static_font_fallbacks() {
        if cand.0.exists() && !found.contains(&cand) {
            found.push(cand);
        }
    }
    found
}

/// fontconfig（`fc-match`）でパターンに合うフォントのファイルと TTC の番号を得る。
#[cfg(not(any(windows, target_os = "macos")))]
fn query_fc_match(pattern: &str) -> Option<(PathBuf, u32)> {
    let output = std::process::Command::new("fc-match")
        .args(["-f", "%{file}\n%{index}\n", pattern])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let path = PathBuf::from(lines.next()?.trim());
    let index = lines
        .next()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0);
    if path.as_os_str().is_empty() || !path.exists() {
        return None;
    }
    Some((path, index))
}

/// パターンに合わないとき fc-match が返す日本語以外のフォントを除く。
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn looks_japanese(path: &Path) -> bool {
    let lower = path.to_string_lossy().to_lowercase();
    [
        "cjk", "ipa", "vl-", "takao", "japanese", "noto", "droid", "wqy", "unifont", "hiragino",
        "mplus", "biz", "hackgen", "udev", "plemol",
    ]
    .iter()
    .any(|k| lower.contains(k))
}

/// OS ごとの日本語フォント候補（優先順）。等幅フォントを先に並べる。
fn static_font_fallbacks() -> Vec<(PathBuf, u32)> {
    #[cfg(windows)]
    {
        let windir = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:/Windows"));
        let mut dirs = vec![windir.join("Fonts")];
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(
                PathBuf::from(local)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
        let mut v = Vec::new();
        for dir in &dirs {
            for (file, index) in [
                ("BIZ-UDGothicR.ttc", 0), // BIZ UDゴシック（等幅）
                ("HackGen-Regular.ttf", 0),
                ("PlemolJP-Regular.ttf", 0),
                ("msgothic.ttc", 0), // MS ゴシック（等幅）
                ("YuGothM.ttc", 0),
                ("meiryo.ttc", 0),
            ] {
                v.push((dir.join(file), index));
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
        .map(|&(p, i)| (PathBuf::from(p), i))
        .collect()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        [
            ("/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf", 0),
            (
                "/usr/share/fonts/truetype/vlgothic/VL-Gothic-Regular.ttf",
                0,
            ),
            ("/usr/share/fonts/truetype/takao-gothic/TakaoGothic.ttf", 0),
            ("/usr/share/fonts/truetype/fonts-japanese-gothic.ttf", 0),
            ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 5),
            ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
            ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 5),
            (
                "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
                0,
            ),
        ]
        .iter()
        .map(|&(p, i)| (PathBuf::from(p), i))
        .collect()
    }
}
