use super::{MdBlock, MdInline};
use eframe::egui::{
    self, text::LayoutJob, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextFormat, Vec2,
};
use std::ops::Range;

// --- VZ-Harmonized Markdown View Palette ---
pub(super) const MD_BG_NAVY: Color32 = Color32::from_rgb(8, 24, 56); // #081838
pub(super) const MD_TEXT_BODY: Color32 = Color32::from_rgb(230, 242, 255);
pub(super) const MD_TEXT_STRONG: Color32 = Color32::from_rgb(255, 255, 255);
pub(super) const MD_TEXT_ITALIC: Color32 = Color32::from_rgb(194, 232, 255);
pub(super) const MD_TEXT_MUTED: Color32 = Color32::from_rgb(122, 168, 214);

pub(super) const MD_H1_COLOR: Color32 = Color32::from_rgb(128, 240, 255);
pub(super) const MD_H2_COLOR: Color32 = Color32::from_rgb(96, 224, 255);
pub(super) const MD_H3_COLOR: Color32 = Color32::from_rgb(255, 220, 96);
pub(super) const MD_H4_COLOR: Color32 = Color32::from_rgb(168, 220, 255);

pub(super) const MD_INLINE_CODE_BG: Color32 = Color32::from_rgb(20, 54, 110);
pub(super) const MD_INLINE_CODE_FG: Color32 = Color32::from_rgb(255, 228, 132);

pub(super) const MD_CODE_BLOCK_BG: Color32 = Color32::from_rgb(5, 16, 38);
pub(super) const MD_CODE_BLOCK_BORDER: Color32 = Color32::from_rgb(42, 98, 168);
pub(super) const MD_CODE_LANG_BG: Color32 = Color32::from_rgb(14, 38, 82);
pub(super) const MD_CODE_LANG_FG: Color32 = Color32::from_rgb(112, 212, 255);

pub(super) const MD_QUOTE_BG: Color32 = Color32::from_rgb(12, 34, 74);
pub(super) const MD_QUOTE_BAR: Color32 = Color32::from_rgb(90, 212, 255);
pub(super) const MD_QUOTE_TEXT: Color32 = Color32::from_rgb(196, 226, 250);

pub(super) const MD_LINK_COLOR: Color32 = Color32::from_rgb(96, 228, 255);
pub(super) const MD_BULLET_COLOR: Color32 = Color32::from_rgb(96, 224, 255);
pub(super) const MD_ORDERED_COLOR: Color32 = Color32::from_rgb(255, 220, 96);
pub(super) const MD_CHECK_DONE: Color32 = Color32::from_rgb(116, 240, 168);
pub(super) const MD_CHECK_TODO: Color32 = Color32::from_rgb(104, 158, 212);

pub(super) const MD_RULE_COLOR: Color32 = Color32::from_rgb(42, 98, 168);
pub(super) const MD_H1_RULE_COLOR: Color32 = Color32::from_rgb(76, 186, 232);
pub(super) const MD_H2_RULE_COLOR: Color32 = Color32::from_rgb(32, 76, 138);

pub(super) const MD_TABLE_HDR_BG: Color32 = Color32::from_rgb(16, 46, 98);
pub(super) const MD_TABLE_ROW_ALT_BG: Color32 = Color32::from_rgb(11, 31, 70);
pub(super) const MD_TABLE_BORDER: Color32 = Color32::from_rgb(36, 82, 144);

// --- egui Markdown Renderer ---

#[derive(Debug, Default)]
pub struct MdRenderOutput {
    pub scroll_y: f32,
    pub max_scroll_y: f32,
    pub viewport_h: f32,
    pub clicked_url: Option<String>,
}

#[derive(Clone, Copy)]
pub(super) struct InlineStyle {
    pub(super) size: f32,
    pub(super) color: Color32,
    pub(super) bold: bool,
    pub(super) italic: bool,
    pub(super) strike: bool,
}

/// Build an egui `LayoutJob` from inline spans along with clickable link char ranges.
pub(super) fn build_inline_job(
    inlines: &[MdInline],
    style: InlineStyle,
    max_width: f32,
) -> (LayoutJob, Vec<(Range<usize>, String)>) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = max_width.max(40.0);
    let mut links: Vec<(Range<usize>, String)> = Vec::new();
    let mut char_cursor = 0usize;

    append_inlines(&mut job, &mut links, &mut char_cursor, inlines, style);
    (job, links)
}

fn append_inlines(
    job: &mut LayoutJob,
    links: &mut Vec<(Range<usize>, String)>,
    char_cursor: &mut usize,
    inlines: &[MdInline],
    style: InlineStyle,
) {
    for node in inlines {
        match node {
            MdInline::Text(text) => {
                let color = if style.strike {
                    MD_TEXT_MUTED
                } else if style.bold {
                    if style.color == MD_TEXT_BODY {
                        MD_TEXT_STRONG
                    } else {
                        style.color
                    }
                } else if style.italic && style.color == MD_TEXT_BODY {
                    MD_TEXT_ITALIC
                } else {
                    style.color
                };
                let mut fmt = TextFormat {
                    font_id: FontId::proportional(style.size),
                    color,
                    italics: style.italic,
                    ..Default::default()
                };
                if style.strike {
                    fmt.strikethrough = Stroke::new(1.2_f32, MD_TEXT_MUTED);
                }
                // Subtle faux-bold visual accent via underline-free stronger contrast + slight weight
                if style.bold && style.color == MD_TEXT_BODY {
                    fmt.font_id = FontId::proportional(style.size * 1.02);
                }
                job.append(text, 0.0, fmt);
                *char_cursor += text.chars().count();
            }
            MdInline::LineBreak => {
                job.append(
                    "\n",
                    0.0,
                    TextFormat {
                        font_id: FontId::proportional(style.size),
                        color: style.color,
                        ..Default::default()
                    },
                );
                *char_cursor += 1;
            }
            MdInline::Bold(children) => {
                let mut child_style = style;
                child_style.bold = true;
                append_inlines(job, links, char_cursor, children, child_style);
            }
            MdInline::Italic(children) => {
                let mut child_style = style;
                child_style.italic = true;
                append_inlines(job, links, char_cursor, children, child_style);
            }
            MdInline::BoldItalic(children) => {
                let mut child_style = style;
                child_style.bold = true;
                child_style.italic = true;
                append_inlines(job, links, char_cursor, children, child_style);
            }
            MdInline::Strikethrough(children) => {
                let mut child_style = style;
                child_style.strike = true;
                append_inlines(job, links, char_cursor, children, child_style);
            }
            MdInline::Code(code) => {
                let padded = format!(" {code} ");
                let fmt = TextFormat {
                    font_id: FontId::monospace((style.size * 0.92).max(9.0)),
                    color: MD_INLINE_CODE_FG,
                    background: MD_INLINE_CODE_BG,
                    ..Default::default()
                };
                job.append(&padded, 0.0, fmt);
                *char_cursor += padded.chars().count();
            }
            MdInline::Link { label, url } => {
                let display = if label.is_empty() { url } else { label };
                let start = *char_cursor;
                let fmt = TextFormat {
                    font_id: FontId::proportional(style.size),
                    color: MD_LINK_COLOR,
                    underline: Stroke::new(1.0_f32, MD_LINK_COLOR),
                    ..Default::default()
                };
                job.append(display, 0.0, fmt);
                let len = display.chars().count();
                *char_cursor += len;
                links.push((start..*char_cursor, url.clone()));
            }
            MdInline::Image { alt, url } => {
                let display = if alt.is_empty() {
                    format!("[IMG: {url}]")
                } else {
                    format!("[IMG: {alt}]")
                };
                let start = *char_cursor;
                let fmt = TextFormat {
                    font_id: FontId::monospace((style.size * 0.88).max(9.0)),
                    color: MD_CODE_LANG_FG,
                    background: MD_CODE_LANG_BG,
                    ..Default::default()
                };
                job.append(&display, 0.0, fmt);
                let len = display.chars().count();
                *char_cursor += len;
                if url.starts_with("http://") || url.starts_with("https://") {
                    links.push((start..*char_cursor, url.clone()));
                }
            }
        }
    }
}

/// Paint a wrapped rich-text galley with interactive hover/click detection for embedded links.
fn paint_rich_inlines(
    ui: &mut egui::Ui,
    inlines: &[MdInline],
    style: InlineStyle,
    max_width: f32,
    clicked_url: &mut Option<String>,
) {
    let (job, links) = build_inline_job(inlines, style, max_width);
    let galley = ui.fonts(|f| f.layout_job(job));
    let size = galley.size();
    let sense = if links.is_empty() {
        Sense::hover()
    } else {
        Sense::click()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);

    let mut hovered_url: Option<&str> = None;
    if !links.is_empty() {
        if let Some(pointer_pos) = resp.hover_pos() {
            let rel = pointer_pos - rect.min;
            let cursor = galley.cursor_from_pos(rel);
            let cidx = cursor.ccursor.index;
            for (range, url) in &links {
                if range.contains(&cidx) {
                    hovered_url = Some(url.as_str());
                    break;
                }
            }
        }
    }

    if let Some(url) = hovered_url {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        if resp.clicked() {
            *clicked_url = Some(url.to_string());
        }
        resp.on_hover_text(url);
    }

    if ui.is_rect_visible(rect) {
        ui.painter().galley(rect.min, galley, style.color);
    }
}

/// Render parsed Markdown blocks into the current central panel with VZ-harmonized navy styling.
pub fn render_markdown_view(
    ui: &mut egui::Ui,
    id_salt: usize,
    blocks: &[MdBlock],
    font_size: f32,
    scroll_y: f32,
    apply_scroll_offset: bool,
) -> MdRenderOutput {
    let avail = ui.available_rect_before_wrap();
    crate::window_style::bg_painter(ui.painter()).rect_filled(
        avail,
        CornerRadius::ZERO,
        crate::window_style::bg(MD_BG_NAVY),
    );

    let mut clicked_url: Option<String> = None;

    let mut scroll_area = egui::ScrollArea::vertical()
        .id_salt(("lz_md_view", id_salt))
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible);

    if apply_scroll_offset {
        scroll_area = scroll_area.vertical_scroll_offset(scroll_y.max(0.0));
    }

    let scroll_out = scroll_area.show(ui, |ui| {
        let pad_x = (font_size * 1.5).clamp(16.0, 40.0);
        let pad_y = (font_size * 1.0).clamp(12.0, 28.0);
        let avail_w = ui.available_width();
        let content_max_w = (avail_w - pad_x * 2.0).clamp(240.0, 960.0);
        let side_margin = ((avail_w - content_max_w) * 0.5).max(pad_x);

        ui.add_space(pad_y);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(side_margin);
            ui.vertical(|ui| {
                ui.set_width(content_max_w);
                ui.set_max_width(content_max_w);

                if blocks.is_empty() {
                    ui.add_space(font_size * 2.0);
                    ui.label(
                        egui::RichText::new(
                            "(空のドキュメント — Ctrl+P / Alt+P / Ctrl+K+V で編集モードへ戻ります)",
                        )
                        .font(FontId::monospace(font_size * 0.9))
                        .color(MD_TEXT_MUTED),
                    );
                    return;
                }

                let block_gap = (font_size * 0.65).max(8.0);

                for (idx, block) in blocks.iter().enumerate() {
                    match block {
                        MdBlock::Heading { level, inlines } => {
                            if idx > 0 {
                                ui.add_space(match level {
                                    1 => font_size * 0.9,
                                    2 => font_size * 0.75,
                                    _ => font_size * 0.5,
                                });
                            }
                            let (h_size, h_color) = match level {
                                1 => (font_size * 1.65, MD_H1_COLOR),
                                2 => (font_size * 1.38, MD_H2_COLOR),
                                3 => (font_size * 1.18, MD_H3_COLOR),
                                _ => (font_size * 1.06, MD_H4_COLOR),
                            };
                            paint_rich_inlines(
                                ui,
                                inlines,
                                InlineStyle {
                                    size: h_size,
                                    color: h_color,
                                    bold: true,
                                    italic: false,
                                    strike: false,
                                },
                                content_max_w,
                                &mut clicked_url,
                            );
                            if *level <= 2 {
                                let (rule_rect, _) = ui.allocate_exact_size(
                                    Vec2::new(content_max_w, 6.0),
                                    Sense::hover(),
                                );
                                let y = rule_rect.center().y;
                                let (stroke_w, stroke_col) = if *level == 1 {
                                    (2.0_f32, MD_H1_RULE_COLOR)
                                } else {
                                    (1.0_f32, MD_H2_RULE_COLOR)
                                };
                                ui.painter().line_segment(
                                    [Pos2::new(rule_rect.min.x, y), Pos2::new(rule_rect.max.x, y)],
                                    Stroke::new(stroke_w, stroke_col),
                                );
                            }
                            ui.add_space(block_gap * 0.7);
                        }

                        MdBlock::Paragraph(inlines) => {
                            paint_rich_inlines(
                                ui,
                                inlines,
                                InlineStyle {
                                    size: font_size,
                                    color: MD_TEXT_BODY,
                                    bold: false,
                                    italic: false,
                                    strike: false,
                                },
                                content_max_w,
                                &mut clicked_url,
                            );
                            ui.add_space(block_gap);
                        }

                        MdBlock::CodeBlock { lang, code } => {
                            let code_font_size = (font_size * 0.90).max(9.0);
                            egui::Frame::NONE
                                .fill(MD_CODE_BLOCK_BG)
                                .stroke(Stroke::new(1.0_f32, MD_CODE_BLOCK_BORDER))
                                .corner_radius(CornerRadius::same(3))
                                .inner_margin(egui::Margin::symmetric(
                                    (font_size * 0.75).round() as i8,
                                    (font_size * 0.55).round() as i8,
                                ))
                                .show(ui, |ui| {
                                    ui.set_min_width((content_max_w - font_size * 1.5).max(120.0));
                                    if !lang.is_empty() {
                                        ui.horizontal(|ui| {
                                            egui::Frame::NONE
                                                .fill(MD_CODE_LANG_BG)
                                                .corner_radius(CornerRadius::same(2))
                                                .inner_margin(egui::Margin::symmetric(6, 1))
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        egui::RichText::new(lang)
                                                            .font(FontId::monospace(
                                                                (code_font_size * 0.82).max(8.5),
                                                            ))
                                                            .color(MD_CODE_LANG_FG),
                                                    );
                                                });
                                        });
                                        ui.add_space(4.0);
                                    }
                                    egui::ScrollArea::horizontal()
                                        .id_salt(("md_code", id_salt, idx))
                                        .show(ui, |ui| {
                                            ui.label(
                                                egui::RichText::new(code)
                                                    .font(FontId::monospace(code_font_size))
                                                    .color(MD_TEXT_BODY),
                                            );
                                        });
                                });
                            ui.add_space(block_gap);
                        }

                        MdBlock::BulletItem {
                            indent,
                            task,
                            inlines,
                        } => {
                            let indent_px = (*indent as f32) * (font_size * 1.35);
                            let marker_w = font_size * 1.35;
                            let text_w = (content_max_w - indent_px - marker_w).max(80.0);

                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                if indent_px > 0.0 {
                                    ui.add_space(indent_px);
                                }
                                let (marker, color) = match task {
                                    Some(true) => ("☑", MD_CHECK_DONE),
                                    Some(false) => ("☐", MD_CHECK_TODO),
                                    None => match indent % 3 {
                                        0 => ("•", MD_BULLET_COLOR),
                                        1 => ("◦", MD_BULLET_COLOR),
                                        _ => ("▪", MD_BULLET_COLOR),
                                    },
                                };
                                ui.allocate_ui_with_layout(
                                    Vec2::new(marker_w, font_size * 1.3),
                                    egui::Layout::left_to_right(egui::Align::TOP),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new(marker)
                                                .font(FontId::monospace(font_size))
                                                .color(color),
                                        );
                                    },
                                );
                                paint_rich_inlines(
                                    ui,
                                    inlines,
                                    InlineStyle {
                                        size: font_size,
                                        color: MD_TEXT_BODY,
                                        bold: false,
                                        italic: false,
                                        strike: false,
                                    },
                                    text_w,
                                    &mut clicked_url,
                                );
                            });
                            ui.add_space(font_size * 0.28);
                        }

                        MdBlock::OrderedItem {
                            indent,
                            number,
                            inlines,
                        } => {
                            let indent_px = (*indent as f32) * (font_size * 1.35);
                            let marker_w = ((number.len() as f32 + 0.8) * font_size * 0.65)
                                .max(font_size * 1.8);
                            let text_w = (content_max_w - indent_px - marker_w).max(80.0);

                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                if indent_px > 0.0 {
                                    ui.add_space(indent_px);
                                }
                                ui.allocate_ui_with_layout(
                                    Vec2::new(marker_w, font_size * 1.3),
                                    egui::Layout::left_to_right(egui::Align::TOP),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new(number)
                                                .font(FontId::monospace(font_size * 0.95))
                                                .strong()
                                                .color(MD_ORDERED_COLOR),
                                        );
                                    },
                                );
                                paint_rich_inlines(
                                    ui,
                                    inlines,
                                    InlineStyle {
                                        size: font_size,
                                        color: MD_TEXT_BODY,
                                        bold: false,
                                        italic: false,
                                        strike: false,
                                    },
                                    text_w,
                                    &mut clicked_url,
                                );
                            });
                            ui.add_space(font_size * 0.28);
                        }

                        MdBlock::BlockQuote { depth, inlines } => {
                            let extra_indent = (depth.saturating_sub(1) as f32) * font_size;
                            let bar_w = 3.5_f32;
                            let inner_pad_x = (font_size * 0.75).max(10.0);
                            let inner_pad_y = (font_size * 0.45).max(6.0);
                            let quote_w =
                                (content_max_w - extra_indent - bar_w - inner_pad_x * 2.0)
                                    .max(100.0);

                            let (job, links) = build_inline_job(
                                inlines,
                                InlineStyle {
                                    size: font_size,
                                    color: MD_QUOTE_TEXT,
                                    bold: false,
                                    italic: true,
                                    strike: false,
                                },
                                quote_w,
                            );
                            let galley = ui.fonts(|f| f.layout_job(job));
                            let box_h = galley.size().y + inner_pad_y * 2.0;
                            let total_w = content_max_w;

                            let (outer_rect, resp) = ui.allocate_exact_size(
                                Vec2::new(total_w, box_h),
                                if links.is_empty() {
                                    Sense::hover()
                                } else {
                                    Sense::click()
                                },
                            );

                            let box_rect = Rect::from_min_max(
                                Pos2::new(outer_rect.min.x + extra_indent, outer_rect.min.y),
                                outer_rect.max,
                            );
                            let text_origin = Pos2::new(
                                box_rect.min.x + bar_w + inner_pad_x,
                                box_rect.min.y + inner_pad_y,
                            );

                            if !links.is_empty() {
                                if let Some(ptr) = resp.hover_pos() {
                                    let rel = ptr - text_origin;
                                    let cidx = galley.cursor_from_pos(rel).ccursor.index;
                                    for (range, url) in &links {
                                        if range.contains(&cidx) {
                                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                            if resp.clicked() {
                                                clicked_url = Some(url.clone());
                                            }
                                            resp.on_hover_text(url);
                                            break;
                                        }
                                    }
                                }
                            }

                            if ui.is_rect_visible(box_rect) {
                                let p = ui.painter();
                                p.rect_filled(box_rect, CornerRadius::same(2), MD_QUOTE_BG);
                                let bar_rect = Rect::from_min_size(
                                    box_rect.min,
                                    Vec2::new(bar_w, box_rect.height()),
                                );
                                p.rect_filled(bar_rect, CornerRadius::same(1), MD_QUOTE_BAR);
                                p.galley(text_origin, galley, MD_QUOTE_TEXT);
                            }
                            ui.add_space(block_gap);
                        }

                        MdBlock::HorizontalRule => {
                            let (rect, _) = ui.allocate_exact_size(
                                Vec2::new(content_max_w, font_size * 0.9),
                                Sense::hover(),
                            );
                            let y = rect.center().y;
                            ui.painter().line_segment(
                                [Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
                                Stroke::new(1.5_f32, MD_RULE_COLOR),
                            );
                            ui.add_space(block_gap * 0.5);
                        }

                        MdBlock::Table { headers, rows } => {
                            render_markdown_table(
                                ui,
                                headers,
                                rows,
                                font_size,
                                content_max_w,
                                &mut clicked_url,
                            );
                            ui.add_space(block_gap);
                        }
                    }
                }
                ui.add_space(pad_y * 2.0);
            });
            ui.add_space(side_margin);
        });
    });

    let viewport_h = scroll_out.inner_rect.height().max(1.0);
    let max_scroll_y = (scroll_out.content_size.y - viewport_h).max(0.0);

    MdRenderOutput {
        scroll_y: scroll_out.state.offset.y,
        max_scroll_y,
        viewport_h,
        clicked_url,
    }
}

fn render_markdown_table(
    ui: &mut egui::Ui,
    headers: &[Vec<MdInline>],
    rows: &[Vec<Vec<MdInline>>],
    font_size: f32,
    max_width: f32,
    clicked_url: &mut Option<String>,
) {
    let col_count = headers.len().max(1);
    let cell_font = (font_size * 0.90).max(9.0);
    let pad_x = (font_size * 0.55).max(6.0);
    let pad_y = (font_size * 0.35).max(4.0);
    let col_w = (max_width / col_count as f32).max(80.0);
    let inner_w = (col_w - pad_x * 2.0).max(40.0);

    let paint_row = |ui: &mut egui::Ui,
                     cells: &[Vec<MdInline>],
                     bg: Color32,
                     is_header: bool,
                     clicked_url: &mut Option<String>| {
        let mut galleys = Vec::with_capacity(col_count);
        let mut row_links = Vec::with_capacity(col_count);
        let mut max_h = cell_font * 1.25;

        for c in 0..col_count {
            let cell_inlines = cells.get(c).map(|v| v.as_slice()).unwrap_or(&[]);
            let (job, links) = build_inline_job(
                cell_inlines,
                InlineStyle {
                    size: cell_font,
                    color: if is_header { MD_H2_COLOR } else { MD_TEXT_BODY },
                    bold: is_header,
                    italic: false,
                    strike: false,
                },
                inner_w,
            );
            let galley = ui.fonts(|f| f.layout_job(job));
            max_h = max_h.max(galley.size().y);
            galleys.push(galley);
            row_links.push(links);
        }

        let row_h = max_h + pad_y * 2.0;
        let has_any_link = row_links.iter().any(|l| !l.is_empty());
        let (row_rect, resp) = ui.allocate_exact_size(
            Vec2::new(col_w * col_count as f32, row_h),
            if has_any_link {
                Sense::click()
            } else {
                Sense::hover()
            },
        );

        if ui.is_rect_visible(row_rect) {
            let p = ui.painter();
            p.rect(
                row_rect,
                CornerRadius::ZERO,
                bg,
                Stroke::new(1.0_f32, MD_TABLE_BORDER),
                StrokeKind::Inside,
            );

            for (c, galley) in galleys.into_iter().enumerate() {
                let cell_x = row_rect.min.x + c as f32 * col_w;
                if c > 0 {
                    p.line_segment(
                        [
                            Pos2::new(cell_x, row_rect.min.y),
                            Pos2::new(cell_x, row_rect.max.y),
                        ],
                        Stroke::new(1.0_f32, MD_TABLE_BORDER),
                    );
                }
                let text_pos = Pos2::new(cell_x + pad_x, row_rect.min.y + pad_y);

                if !row_links[c].is_empty() {
                    if let Some(ptr) = resp.hover_pos() {
                        if ptr.x >= cell_x && ptr.x <= cell_x + col_w {
                            let cidx = galley.cursor_from_pos(ptr - text_pos).ccursor.index;
                            for (range, url) in &row_links[c] {
                                if range.contains(&cidx) {
                                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                    if resp.clicked() {
                                        *clicked_url = Some(url.clone());
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }

                p.galley(
                    text_pos,
                    galley,
                    if is_header { MD_H2_COLOR } else { MD_TEXT_BODY },
                );
            }
        }
    };

    ui.spacing_mut().item_spacing.y = 0.0;
    paint_row(ui, headers, MD_TABLE_HDR_BG, true, clicked_url);
    for (r_idx, row) in rows.iter().enumerate() {
        let bg = if r_idx % 2 == 1 {
            MD_TABLE_ROW_ALT_BG
        } else {
            Color32::TRANSPARENT
        };
        paint_row(ui, row, bg, false, clicked_url);
    }
}
