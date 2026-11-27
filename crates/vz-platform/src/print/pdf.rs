//! 割付済みの文書から PDF を作る（macOS / Linux の印刷用）。
//!
//! 文字は升目（半角 = 0.5em、全角 = 1em）に置き、フォントは使った文字だけを埋め込む（krilla）。

use krilla::geom::Point;
use krilla::page::PageSettings;
use krilla::surface::{Location, Surface};
use krilla::text::{Font, Glyph, GlyphId};
use krilla::Document;
use skrifa::MetadataProvider;
use std::ops::Range;
use vz_core::print::{char_cells, PrintDoc, HEADER_FONT_PT};

/// 埋め込む日本語フォント（ファイルの中身と TTC の番号）。
pub struct PdfFont {
    data: Vec<u8>,
    index: u32,
}

/// 画面と同じ優先順で、PDF に使える日本語フォントを読む。
#[cfg_attr(windows, allow(dead_code))]
pub fn load_font() -> Option<PdfFont> {
    crate::fonts::japanese_font_candidates()
        .into_iter()
        .find_map(|(path, index)| {
            let data = std::fs::read(path).ok()?;
            skrifa::FontRef::from_index(&data, index).ok()?;
            Some(PdfFont { data, index })
        })
}

/// 升目に置く 1 文字。
struct CellGlyph {
    id: GlyphId,
    range: Range<usize>,
    /// 送り幅（em）
    advance: f32,
}

impl Glyph for CellGlyph {
    fn glyph_id(&self) -> GlyphId {
        self.id
    }
    fn text_range(&self) -> Range<usize> {
        self.range.clone()
    }
    fn x_advance(&self, size: f32) -> f32 {
        self.advance * size
    }
    fn x_offset(&self, _size: f32) -> f32 {
        0.0
    }
    fn y_offset(&self, _size: f32) -> f32 {
        0.0
    }
    fn y_advance(&self, _size: f32) -> f32 {
        0.0
    }
    fn location(&self) -> Option<Location> {
        None
    }
}

pub fn render(doc: &PrintDoc, font: PdfFont) -> Result<Vec<u8>, String> {
    let face = skrifa::FontRef::from_index(&font.data, font.index)
        .map_err(|_| "フォントを読めません".to_string())?;
    let charmap = face.charmap();
    // 半角は 0.5em。ヘッダーと本文で文字サイズが違っても em 単位なので同じ升目の比率になる
    let glyphs_for = |text: &str| -> Vec<CellGlyph> {
        text.char_indices()
            .map(|(i, c)| CellGlyph {
                id: GlyphId::new(charmap.map(c).map_or(0, |g| g.to_u32())),
                range: i..i + c.len_utf8(),
                advance: char_cells(c) as f32 * 0.5,
            })
            .collect()
    };
    let krilla_font = Font::new(font.data.clone().into(), font.index)
        .ok_or_else(|| "フォントを PDF に埋め込めません".to_string())?;

    let (w, h) = doc.setup.paper_pt();
    let size = doc.setup.font_size_pt();
    let (num_x, text_x) = doc.body_x();
    let mut document = Document::new();
    for (idx, page) in doc.pages.iter().enumerate() {
        let settings = PageSettings::from_wh(w, h).ok_or("用紙の大きさが不正です")?;
        let mut pdf_page = document.start_page_with(settings);
        let mut surface = pdf_page.surface();
        let draw = |surface: &mut Surface, x: f32, y: f32, text: &str, size: f32| {
            if text.is_empty() {
                return;
            }
            let glyphs = glyphs_for(text);
            surface.draw_glyphs(
                Point::from_xy(x, y),
                &glyphs,
                krilla_font.clone(),
                text,
                size,
                false,
            );
        };
        let hy = doc.setup.header_baseline_pt();
        for (x, text) in doc.header_items(idx) {
            draw(&mut surface, x, hy, &text, HEADER_FONT_PT);
        }
        for (row, line) in page.iter().enumerate() {
            let y = doc.setup.row_baseline_pt(row);
            draw(&mut surface, num_x, y, &doc.number_label(line), size);
            draw(&mut surface, text_x, y, &line.text, size);
        }
        surface.finish();
        pdf_page.finish();
    }
    document
        .finish()
        .map_err(|e| format!("PDF を作れません: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vz_core::print::{layout, Orientation, PageSetup};

    #[test]
    fn test_render_pdf_with_japanese_font() {
        let Some(font) = load_font() else {
            eprintln!("日本語フォントが無いので省略");
            return;
        };
        let lines: Vec<String> = (0..120)
            .map(|i| format!("{i}\t日本語とASCII ｶﾅ {}", "x".repeat(i)))
            .collect();
        let setup = PageSetup {
            orientation: Orientation::Landscape,
            ..PageSetup::default()
        };
        let doc = layout(&lines, setup, "印刷テスト.txt", "2026-11-27 12:00").unwrap();
        let pdf = render(&doc, font).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert_eq!(doc.pages.len(), 4);
        if let Ok(dir) = std::env::var("VZ_PRINT_PDF_DIR") {
            // 目で確かめる用: VZ_PRINT_PDF_DIR=<フォルダ> cargo test -p vz-platform pdf
            std::fs::write(std::path::Path::new(&dir).join("vz-print-sample.pdf"), &pdf).unwrap();
        }
    }
}
