use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

/// Load a system Japanese monospace font into egui's font definitions to prevent tofu (missing glyphs).
/// 候補の探索（fontconfig・OS ごとの既定パス）は `vz_platform::fonts` と共用する。
pub fn configure_japanese_fonts(ctx: &egui::Context) -> Option<String> {
    let mut fonts = FontDefinitions::default();
    let mut loaded: Option<String> = None;

    // Loading the single best Japanese monospace font keeps startup instant and RAM minimal.
    for (path, index) in vz_platform::fonts::japanese_font_candidates() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let name = "ja_font".to_string();
        let mut font_data = FontData::from_owned(bytes);
        font_data.index = index;
        fonts.font_data.insert(name.clone(), Arc::new(font_data));
        // Insert the Japanese font at the front of Monospace and Proportional font families.
        for family in [FontFamily::Monospace, FontFamily::Proportional] {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, name.clone());
        }
        loaded = Some(format!("{} (index {index})", path.display()));
        break;
    }

    ctx.set_fonts(fonts);
    loaded
}
