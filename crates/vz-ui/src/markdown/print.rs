//! Markdown ビューの印刷: 画面と同じ寸法比で egui に割り付け、グリフの位置をそのまま印刷項目にする。
//!
//! 色は白い紙向けの印刷用配色に置き換える。行番号は印字しない。
//! 改ページは行（段落の折り返し行・コードの行・表の行）の単位で行う。

use super::render::*;
use super::{MdBlock, MdInline};
use eframe::egui::{
    self,
    text::{Fonts, LayoutJob},
    Color32, FontId, Galley, TextFormat,
};
use std::sync::Arc;
use vz_core::print::{PageSetup, PrintItem, Rgb, PAGE_LIMIT};

/// egui の割付を細かくするための倍率（画素への丸めの影響を小さくする）
const LAYOUT_SCALE: f32 = 4.0;

const BLACK: Rgb = [0, 0, 0];

/// 画面の色 → 印刷の色（白い紙向け）。表に無い色は黒。
const PRINT_COLORS: &[(Color32, Rgb)] = &[
    (MD_TEXT_BODY, [34, 34, 34]),
    (MD_TEXT_STRONG, BLACK),
    (MD_TEXT_ITALIC, [52, 52, 72]),
    (MD_TEXT_MUTED, [128, 128, 128]),
    (MD_H1_COLOR, [0, 60, 130]),
    (MD_H2_COLOR, [0, 84, 150]),
    (MD_H3_COLOR, [150, 92, 0]),
    (MD_H4_COLOR, [40, 80, 120]),
    (MD_INLINE_CODE_BG, [236, 236, 236]),
    (MD_INLINE_CODE_FG, [160, 50, 0]),
    (MD_CODE_BLOCK_BG, [246, 246, 246]),
    (MD_CODE_BLOCK_BORDER, [190, 190, 190]),
    (MD_CODE_LANG_BG, [226, 234, 244]),
    (MD_CODE_LANG_FG, [30, 90, 150]),
    (MD_QUOTE_BG, [240, 244, 250]),
    (MD_QUOTE_BAR, [90, 140, 200]),
    (MD_QUOTE_TEXT, [60, 60, 84]),
    (MD_LINK_COLOR, [0, 90, 200]),
    (MD_BULLET_COLOR, [0, 110, 170]),
    (MD_ORDERED_COLOR, [160, 100, 0]),
    (MD_CHECK_DONE, [0, 140, 70]),
    (MD_CHECK_TODO, [100, 120, 150]),
    (MD_RULE_COLOR, [170, 170, 170]),
    (MD_H1_RULE_COLOR, [90, 140, 200]),
    (MD_H2_RULE_COLOR, [190, 200, 215]),
    (MD_TABLE_HDR_BG, [230, 238, 248]),
    (MD_TABLE_ROW_ALT_BG, [247, 249, 252]),
    (MD_TABLE_BORDER, [170, 180, 195]),
];

fn print_color(c: Color32) -> Rgb {
    PRINT_COLORS
        .iter()
        .find(|(screen, _)| *screen == c)
        .map_or(BLACK, |(_, print)| *print)
}

/// 背景・枠付きの箱（コードブロック・引用）
struct BoxStyle {
    x: f32,
    w: f32,
    pad_y: f32,
    fill: Color32,
    border: Option<Color32>,
    /// 左端の縦棒（幅・色）
    bar: Option<(f32, Color32)>,
}

/// ページに項目を積む。座標は用紙の左上からのポイント。
struct Pager<'a> {
    fonts: &'a Fonts,
    pages: Vec<Vec<PrintItem>>,
    items: Vec<PrintItem>,
    top: f32,
    bottom: f32,
    y: f32,
}

impl Pager<'_> {
    fn layout(&self, mut job: LayoutJob, max_width: f32) -> Arc<Galley> {
        job.wrap.max_width = (max_width * LAYOUT_SCALE).max(40.0);
        self.fonts.layout_job(job)
    }

    fn new_page(&mut self) {
        self.pages.push(std::mem::take(&mut self.items));
        self.y = self.top;
    }

    /// 高さ h が今のページに入らなければ改ページする（ページの先頭なら入れる）。
    fn ensure(&mut self, h: f32) {
        if self.y + h > self.bottom && self.y > self.top {
            self.new_page();
        }
    }

    fn space(&mut self, h: f32) {
        self.y += h;
    }

    fn finish(mut self) -> Vec<Vec<PrintItem>> {
        if !self.items.is_empty() {
            self.new_page();
        }
        self.pages
    }

    fn too_many_pages(&self) -> bool {
        self.pages.len() > PAGE_LIMIT
    }

    fn row_height(galley: &Galley, row: usize) -> f32 {
        galley.rows[row].rect.height() / LAYOUT_SCALE
    }

    /// galley の row 行目を、行の上端 top・左端 x に置く。
    fn emit_row(&mut self, galley: &Galley, row: usize, x: f32, top: f32) {
        let r = &galley.rows[row];
        let glyphs = &r.glyphs;
        let mut start = 0;
        while start < glyphs.len() {
            let section = glyphs[start].section_index;
            let end = glyphs[start..]
                .iter()
                .position(|g| g.section_index != section)
                .map_or(glyphs.len(), |n| start + n);
            let run = &glyphs[start..end];
            let format: &TextFormat = &galley.job.sections[section as usize].format;
            let run_x = x + run[0].pos.x / LAYOUT_SCALE;
            let run_end =
                x + (run[run.len() - 1].pos.x + run[run.len() - 1].advance_width) / LAYOUT_SCALE;
            let row_top = top;
            let row_bottom = top + r.rect.height() / LAYOUT_SCALE;
            if format.background != Color32::TRANSPARENT {
                self.items.push(PrintItem::Rect {
                    x: run_x,
                    y: row_top,
                    w: run_end - run_x,
                    h: row_bottom - row_top,
                    fill: Some(print_color(format.background)),
                    stroke: None,
                });
            }
            let baseline = top + (run[0].pos.y - r.rect.min.y) / LAYOUT_SCALE;
            let advances: Vec<f32> = run
                .iter()
                .enumerate()
                .map(|(i, g)| {
                    run.get(i + 1)
                        .map_or(g.advance_width, |next| next.pos.x - g.pos.x)
                        / LAYOUT_SCALE
                })
                .collect();
            let text: String = run.iter().map(|g| g.chr).collect();
            let size = format.font_id.size / LAYOUT_SCALE;
            if !text.trim().is_empty() {
                self.items.push(PrintItem::Text {
                    x: run_x,
                    y: baseline,
                    size,
                    color: print_color(format.color),
                    italic: format.italics,
                    text,
                    advances,
                });
            }
            for (stroke, dy) in [
                (format.underline, size * 0.15),
                (format.strikethrough, -size * 0.3),
            ] {
                if stroke.width > 0.0 {
                    self.items.push(PrintItem::Line {
                        from: (run_x, baseline + dy),
                        to: (run_end, baseline + dy),
                        width: (stroke.width * 0.5).max(0.3),
                        color: print_color(stroke.color),
                    });
                }
            }
            start = end;
        }
    }

    /// galley を行ごとに置く（行の途中で改ページする）。marker は 1 行目の左に置く。
    fn emit_galley(&mut self, galley: &Galley, x: f32, marker: Option<(&Galley, f32)>) {
        for row in 0..galley.rows.len() {
            let h = Self::row_height(galley, row);
            self.ensure(h);
            if row == 0 {
                if let Some((m, mx)) = marker {
                    if !m.rows.is_empty() {
                        self.emit_row(m, 0, mx, self.y);
                    }
                }
            }
            self.emit_row(galley, row, x, self.y);
            self.y += h;
        }
    }

    /// 背景・枠付きの箱に galley を置く（ページをまたぐと箱も分ける）。
    fn emit_boxed(&mut self, galley: &Galley, text_x: f32, style: &BoxStyle) {
        let last = galley.rows.len().saturating_sub(1);
        for row in 0..galley.rows.len() {
            let pad_top = if row == 0 { style.pad_y } else { 0.0 };
            let pad_bottom = if row == last { style.pad_y } else { 0.0 };
            let h = pad_top + Self::row_height(galley, row) + pad_bottom;
            self.ensure(h);
            let (x, w, y) = (style.x, style.w, self.y);
            self.items.push(PrintItem::Rect {
                x,
                y,
                w,
                h,
                fill: Some(print_color(style.fill)),
                stroke: None,
            });
            if let Some(border) = style.border {
                let color = print_color(border);
                let mut line = |from: (f32, f32), to: (f32, f32)| {
                    self.items.push(PrintItem::Line {
                        from,
                        to,
                        width: 0.6,
                        color,
                    })
                };
                line((x, y), (x, y + h));
                line((x + w, y), (x + w, y + h));
                if row == 0 {
                    line((x, y), (x + w, y));
                }
                if row == last {
                    line((x, y + h), (x + w, y + h));
                }
            }
            if let Some((bar_w, bar)) = style.bar {
                self.items.push(PrintItem::Rect {
                    x,
                    y,
                    w: bar_w,
                    h,
                    fill: Some(print_color(bar)),
                    stroke: None,
                });
            }
            self.emit_row(galley, row, text_x, y + pad_top);
            self.y += h;
        }
    }

    fn rule(&mut self, x: f32, w: f32, h: f32, width: f32, color: Color32) {
        self.ensure(h);
        let y = self.y + h * 0.5;
        self.items.push(PrintItem::Line {
            from: (x, y),
            to: (x + w, y),
            width,
            color: print_color(color),
        });
        self.y += h;
    }
}

fn plain_job(text: &str, font_id: FontId, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(
        text,
        0.0,
        TextFormat {
            font_id,
            color,
            ..Default::default()
        },
    );
    job
}

/// Markdown のブロックを印刷のページに割り付ける。基準の文字サイズは印刷設定の文字サイズ。
/// 目安（`PAGE_LIMIT` ページ）を超えたら途中でやめる（ページ数で `TooLarge` と判断する）。
pub fn layout_markdown_print(
    ctx: &egui::Context,
    blocks: &[MdBlock],
    setup: &PageSetup,
) -> Vec<Vec<PrintItem>> {
    ctx.fonts(|fonts| layout_with_fonts(fonts, blocks, setup))
}

fn layout_with_fonts(fonts: &Fonts, blocks: &[MdBlock], setup: &PageSetup) -> Vec<Vec<PrintItem>> {
    let fs = setup.font_size_pt();
    let s = LAYOUT_SCALE;
    let left = setup.margin_pt();
    let width = setup.body_width_pt();
    let mut p = Pager {
        fonts,
        pages: Vec::new(),
        items: Vec::new(),
        top: setup.body_top_pt(),
        bottom: setup.body_bottom_pt(),
        y: setup.body_top_pt(),
    };
    let block_gap = fs * 0.65;
    let body = InlineStyle::plain(fs, MD_TEXT_BODY);
    let inline = |inlines: &[MdInline], st: InlineStyle, w: f32| {
        let st = InlineStyle {
            size: st.size * s,
            ..st
        };
        build_inline_job(inlines, st, w * s).0
    };

    for (idx, block) in blocks.iter().enumerate() {
        if p.too_many_pages() {
            break;
        }
        match block {
            MdBlock::Heading { level, inlines } => {
                if idx > 0 {
                    p.space(heading_gap(*level, fs));
                }
                let (size, color) = heading_style(*level, fs);
                let st = InlineStyle {
                    bold: true,
                    ..InlineStyle::plain(size, color)
                };
                let galley = p.layout(inline(inlines, st, width), width);
                // 見出しだけがページ末に残らないよう、続く 2 行分も確保する
                p.ensure(galley.size().y / s + fs * 2.8);
                p.emit_galley(&galley, left, None);
                if *level <= 2 {
                    let (w, c) = if *level == 1 {
                        (1.2, MD_H1_RULE_COLOR)
                    } else {
                        (0.6, MD_H2_RULE_COLOR)
                    };
                    p.rule(left, width, fs * 0.4, w, c);
                }
                p.space(block_gap * 0.7);
            }
            MdBlock::Paragraph(inlines) => {
                let galley = p.layout(inline(inlines, body, width), width);
                p.emit_galley(&galley, left, None);
                p.space(block_gap);
            }
            MdBlock::CodeBlock { lang, code } => {
                let code_size = fs * 0.9;
                let pad_x = fs * 0.75;
                let inner_w = width - pad_x * 2.0;
                let mut job = LayoutJob::default();
                if !lang.is_empty() {
                    job.append(
                        &format!(" {lang} "),
                        0.0,
                        TextFormat {
                            font_id: FontId::monospace(code_size * 0.82 * s),
                            color: MD_CODE_LANG_FG,
                            background: MD_CODE_LANG_BG,
                            ..Default::default()
                        },
                    );
                    job.append(
                        "\n",
                        0.0,
                        TextFormat {
                            font_id: FontId::monospace(code_size * s),
                            ..Default::default()
                        },
                    );
                }
                job.append(
                    code.trim_end_matches('\n'),
                    0.0,
                    TextFormat {
                        font_id: FontId::monospace(code_size * s),
                        color: MD_TEXT_BODY,
                        ..Default::default()
                    },
                );
                let galley = p.layout(job, inner_w);
                let boxed = BoxStyle {
                    x: left,
                    w: width,
                    pad_y: fs * 0.55,
                    fill: MD_CODE_BLOCK_BG,
                    border: Some(MD_CODE_BLOCK_BORDER),
                    bar: None,
                };
                p.emit_boxed(&galley, left + pad_x, &boxed);
                p.space(block_gap);
            }
            MdBlock::BulletItem {
                indent,
                task,
                inlines,
            } => {
                let indent_x = *indent as f32 * fs * 1.35;
                let marker_w = fs * 1.35;
                let text_w = (width - indent_x - marker_w).max(fs * 5.0);
                let (marker, color) = bullet_marker(*task, *indent);
                let marker = p.layout(plain_job(marker, FontId::monospace(fs * s), color), width);
                let galley = p.layout(inline(inlines, body, text_w), text_w);
                let x = left + indent_x;
                p.emit_galley(&galley, x + marker_w, Some((&marker, x)));
                p.space(fs * 0.28);
            }
            MdBlock::OrderedItem {
                indent,
                number,
                inlines,
            } => {
                let indent_x = *indent as f32 * fs * 1.35;
                let marker_w = ordered_marker_width(number, fs);
                let text_w = (width - indent_x - marker_w).max(fs * 5.0);
                let marker = p.layout(
                    plain_job(number, FontId::monospace(fs * 0.95 * s), MD_ORDERED_COLOR),
                    width,
                );
                let galley = p.layout(inline(inlines, body, text_w), text_w);
                let x = left + indent_x;
                p.emit_galley(&galley, x + marker_w, Some((&marker, x)));
                p.space(fs * 0.28);
            }
            MdBlock::BlockQuote { depth, inlines } => {
                let extra = depth.saturating_sub(1) as f32 * fs;
                let bar_w = fs * 0.3;
                let pad_x = fs * 0.75;
                let quote_w = (width - extra - bar_w - pad_x * 2.0).max(fs * 5.0);
                let st = InlineStyle {
                    italic: true,
                    ..InlineStyle::plain(fs, MD_QUOTE_TEXT)
                };
                let galley = p.layout(inline(inlines, st, quote_w), quote_w);
                let boxed = BoxStyle {
                    x: left + extra,
                    w: width - extra,
                    pad_y: fs * 0.45,
                    fill: MD_QUOTE_BG,
                    border: None,
                    bar: Some((bar_w, MD_QUOTE_BAR)),
                };
                p.emit_boxed(&galley, left + extra + bar_w + pad_x, &boxed);
                p.space(block_gap);
            }
            MdBlock::HorizontalRule => {
                p.rule(left, width, fs * 0.9, 1.0, MD_RULE_COLOR);
                p.space(block_gap * 0.5);
            }
            MdBlock::Table { headers, rows } => {
                layout_table(&mut p, headers, rows, fs, left, width);
                p.space(block_gap);
            }
        }
    }
    p.finish()
}

fn layout_table(
    p: &mut Pager,
    headers: &[Vec<MdInline>],
    rows: &[Vec<Vec<MdInline>>],
    fs: f32,
    left: f32,
    width: f32,
) {
    let cols = headers.len().max(1);
    let cell_size = fs * 0.9;
    let pad_x = fs * 0.55;
    let pad_y = fs * 0.35;
    let col_w = width / cols as f32;
    let inner_w = (col_w - pad_x * 2.0).max(fs * 2.0);
    let all = std::iter::once((headers, true)).chain(rows.iter().map(|r| (r.as_slice(), false)));
    for (r_idx, (cells, is_header)) in all.enumerate() {
        let color = if is_header { MD_H2_COLOR } else { MD_TEXT_BODY };
        let galleys: Vec<Arc<Galley>> = (0..cols)
            .map(|c| {
                let inlines = cells.get(c).map_or(&[][..], |v| v.as_slice());
                let st = InlineStyle {
                    size: cell_size * LAYOUT_SCALE,
                    bold: is_header,
                    ..InlineStyle::plain(0.0, color)
                };
                p.layout(
                    build_inline_job(inlines, st, inner_w * LAYOUT_SCALE).0,
                    inner_w,
                )
            })
            .collect();
        let content_h = galleys
            .iter()
            .map(|g| g.size().y / LAYOUT_SCALE)
            .fold(cell_size * 1.25, f32::max);
        let h = content_h + pad_y * 2.0;
        p.ensure(h);
        let y = p.y;
        let bg = if is_header {
            Some(MD_TABLE_HDR_BG)
        } else if r_idx % 2 == 0 {
            // 本文の 2 行目・4 行目…（画面と同じ縞）
            Some(MD_TABLE_ROW_ALT_BG)
        } else {
            None
        };
        p.items.push(PrintItem::Rect {
            x: left,
            y,
            w: col_w * cols as f32,
            h,
            fill: bg.map(print_color),
            stroke: Some((0.6, print_color(MD_TABLE_BORDER))),
        });
        for (c, galley) in galleys.iter().enumerate() {
            let cell_x = left + c as f32 * col_w;
            if c > 0 {
                p.items.push(PrintItem::Line {
                    from: (cell_x, y),
                    to: (cell_x, y + h),
                    width: 0.6,
                    color: print_color(MD_TABLE_BORDER),
                });
            }
            let mut top = y + pad_y;
            for row in 0..galley.rows.len() {
                p.emit_row(galley, row, cell_x + pad_x, top);
                top += Pager::row_height(galley, row);
            }
        }
        p.y += h;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parse_markdown;
    use vz_core::print::Orientation;

    fn ctx_with_fonts() -> egui::Context {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        ctx
    }

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    fn texts(page: &[PrintItem]) -> Vec<&str> {
        page.iter()
            .filter_map(|i| match i {
                PrintItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn test_markdown_print_draws_view_elements() {
        let ctx = ctx_with_fonts();
        let md = "# Title\n\nHello **bold** and `code` [link](https://x)\n\n- item\n1. one\n\n> quote\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---\n";
        let blocks = parse_markdown(&lines(md));
        let setup = PageSetup::default();
        let pages = layout_markdown_print(&ctx, &blocks, &setup);
        assert_eq!(pages.len(), 1);
        let page = &pages[0];
        let all = texts(page).concat();
        for want in [
            "Title", "Hello", "bold", "code", "link", "item", "1.", "quote", "rust", "fn", "a", "2",
        ] {
            assert!(all.contains(want), "{want} が無い: {all}");
        }
        // 行番号は印字しない（先頭の項目が "1" などの行番号ではない）
        assert!(!texts(page).contains(&"1  "));
        // 見出しの罫線・コードの枠・表の枠がある
        assert!(page.iter().any(|i| matches!(i, PrintItem::Line { .. })));
        assert!(page.iter().any(|i| matches!(
            i,
            PrintItem::Rect {
                stroke: Some(_),
                ..
            }
        )));
        // すべて本文の欄の中、色は白い紙向け（本文は暗い色）
        for item in page {
            if let PrintItem::Text {
                x,
                y,
                color,
                text,
                advances,
                ..
            } = item
            {
                assert!(*x >= setup.margin_pt() - 0.01, "{text}");
                assert!(*y > setup.body_top_pt() && *y < setup.body_bottom_pt());
                assert!(color.iter().map(|&c| u32::from(c)).sum::<u32>() < 3 * 200);
                assert_eq!(advances.len(), text.chars().count());
            }
        }
    }

    #[test]
    fn test_markdown_print_paginates_and_scales_with_font_size() {
        let ctx = ctx_with_fonts();
        let md: String = (0..300).map(|i| format!("段落 {i}\n\n")).collect();
        let blocks = parse_markdown(&lines(&md));
        let small = PageSetup {
            font_pt: 8,
            ..PageSetup::default()
        };
        let large = PageSetup {
            font_pt: 14,
            orientation: Orientation::Landscape,
            ..PageSetup::default()
        };
        let a = layout_markdown_print(&ctx, &blocks, &small);
        let b = layout_markdown_print(&ctx, &blocks, &large);
        assert!(a.len() > 1 && b.len() > a.len(), "{} {}", a.len(), b.len());
        // 各ページの文字は本文の欄に収まる
        for (pages, setup) in [(&a, &small), (&b, &large)] {
            for page in pages.iter() {
                for item in page {
                    if let PrintItem::Text { y, .. } = item {
                        assert!(*y <= setup.body_bottom_pt());
                    }
                }
            }
        }
        // 最後の段落まで入っている
        assert!(texts(b.last().unwrap()).concat().contains("299"));
    }

    #[test]
    fn test_markdown_print_stops_over_page_limit() {
        let ctx = ctx_with_fonts();
        let md: String = (0..20000).map(|i| format!("p{i}\n\n")).collect();
        let blocks = parse_markdown(&lines(&md));
        let pages = layout_markdown_print(&ctx, &blocks, &PageSetup::default());
        assert!(pages.len() > PAGE_LIMIT && pages.len() <= PAGE_LIMIT + 2);
    }

    #[test]
    fn test_print_colors_are_for_white_paper() {
        assert_eq!(print_color(MD_TEXT_STRONG), BLACK);
        assert_eq!(print_color(Color32::from_rgb(1, 2, 3)), BLACK);
        for (_, c) in PRINT_COLORS {
            assert_ne!(*c, [255, 255, 255]);
        }
    }
}
