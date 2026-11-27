//! 割付済みの文書から PDF を作る（macOS / Linux の印刷用）。
//!
//! 文字は升目（半角 = 0.5em、全角 = 1em）に置き、フォントは使った文字だけを埋め込む（krilla）。

use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect, Transform};
use krilla::page::PageSettings;
use krilla::paint::{Fill, Stroke};
use krilla::surface::{Location, Surface};
use krilla::text::{Font, Glyph, GlyphId};
use krilla::Document;
use skrifa::MetadataProvider;
use std::ops::Range;
use vz_core::print::{char_cells, PrintDoc, PrintItem, PrintPage, Rgb, HEADER_FONT_PT};

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
    for (idx, page) in doc.printed_pages() {
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
        match page {
            PrintPage::Lines(lines) => {
                for (row, line) in lines.iter().enumerate() {
                    let y = doc.setup.row_baseline_pt(row);
                    draw(&mut surface, num_x, y, &doc.number_label(line), size);
                    draw(&mut surface, text_x, y, &line.text, size);
                }
            }
            PrintPage::Items(items) => {
                draw_items(&mut surface, items, &charmap, &krilla_font);
                surface.set_fill(None);
                surface.set_stroke(None);
            }
        }
        surface.finish();
        pdf_page.finish();
    }
    document
        .finish()
        .map_err(|e| format!("PDF を作れません: {e:?}"))
}

fn fill(color: Rgb) -> Fill {
    Fill {
        paint: rgb::Color::new(color[0], color[1], color[2]).into(),
        ..Fill::default()
    }
}

/// 斜体の傾き（文字の高さに対する横のずれ）
const ITALIC_SKEW: f32 = 0.2;

/// 図のページの項目を描く（Markdown ビューの印刷）。
fn draw_items(
    surface: &mut Surface,
    items: &[PrintItem],
    charmap: &skrifa::charmap::Charmap,
    font: &Font,
) {
    for item in items {
        match item {
            PrintItem::Text {
                x,
                y,
                size,
                color,
                italic,
                text,
                advances,
            } => {
                if text.is_empty() {
                    continue;
                }
                let glyphs: Vec<CellGlyph> = text
                    .char_indices()
                    .zip(advances)
                    .map(|((i, c), adv)| CellGlyph {
                        id: GlyphId::new(charmap.map(c).map_or(0, |g| g.to_u32())),
                        range: i..i + c.len_utf8(),
                        advance: adv / size,
                    })
                    .collect();
                surface.set_stroke(None);
                surface.set_fill(Some(fill(*color)));
                if *italic {
                    // 基準線を軸に右へ傾ける
                    let skew =
                        Transform::from_row(1.0, 0.0, -ITALIC_SKEW, 1.0, ITALIC_SKEW * y, 0.0);
                    surface.push_transform(&skew);
                }
                surface.draw_glyphs(
                    Point::from_xy(*x, *y),
                    &glyphs,
                    font.clone(),
                    text,
                    *size,
                    false,
                );
                if *italic {
                    surface.pop();
                }
            }
            PrintItem::Rect {
                x,
                y,
                w,
                h,
                fill: rect_fill,
                stroke,
            } => {
                let Some(rect) = Rect::from_xywh(*x, *y, w.max(0.01), h.max(0.01)) else {
                    continue;
                };
                let mut pb = PathBuilder::new();
                pb.push_rect(rect);
                let Some(path) = pb.finish() else {
                    continue;
                };
                surface.set_fill(rect_fill.map(fill));
                surface.set_stroke(stroke.map(|(width, color)| Stroke {
                    paint: rgb::Color::new(color[0], color[1], color[2]).into(),
                    width,
                    ..Stroke::default()
                }));
                if rect_fill.is_some() || stroke.is_some() {
                    surface.draw_path(&path);
                }
            }
            PrintItem::Line {
                from,
                to,
                width,
                color,
            } => {
                let mut pb = PathBuilder::new();
                pb.move_to(from.0, from.1);
                pb.line_to(to.0, to.1);
                let Some(path) = pb.finish() else {
                    continue;
                };
                surface.set_fill(None);
                surface.set_stroke(Some(Stroke {
                    paint: rgb::Color::new(color[0], color[1], color[2]).into(),
                    width: *width,
                    ..Stroke::default()
                }));
                surface.draw_path(&path);
            }
        }
    }
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

    #[test]
    fn test_render_pdf_with_items() {
        let Some(font) = load_font() else {
            eprintln!("日本語フォントが無いので省略");
            return;
        };
        let text = "見出し abc".to_string();
        let advances = text.chars().map(|c| char_cells(c) as f32 * 6.0).collect();
        let items = vec![
            PrintItem::Rect {
                x: 30.0,
                y: 40.0,
                w: 200.0,
                h: 20.0,
                fill: Some([240, 240, 240]),
                stroke: Some((0.6, [120, 120, 120])),
            },
            PrintItem::Text {
                x: 32.0,
                y: 55.0,
                size: 12.0,
                color: [0, 60, 130],
                italic: true,
                text,
                advances,
            },
            PrintItem::Line {
                from: (30.0, 70.0),
                to: (230.0, 70.0),
                width: 1.0,
                color: [90, 140, 200],
            },
        ];
        let doc = PrintDoc::from_items(PageSetup::default(), "t.md", "d", vec![items, Vec::new()])
            .unwrap();
        let pdf = render(&doc, font).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
    }
}
