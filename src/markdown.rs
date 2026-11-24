use eframe::egui::{
    self, text::LayoutJob, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextFormat, Vec2,
};
use std::ops::Range;

// --- VZ-Harmonized Markdown View Palette ---
const MD_BG_NAVY: Color32 = Color32::from_rgb(8, 24, 56); // #081838
const MD_TEXT_BODY: Color32 = Color32::from_rgb(230, 242, 255);
const MD_TEXT_STRONG: Color32 = Color32::from_rgb(255, 255, 255);
const MD_TEXT_ITALIC: Color32 = Color32::from_rgb(194, 232, 255);
const MD_TEXT_MUTED: Color32 = Color32::from_rgb(122, 168, 214);

const MD_H1_COLOR: Color32 = Color32::from_rgb(128, 240, 255);
const MD_H2_COLOR: Color32 = Color32::from_rgb(96, 224, 255);
const MD_H3_COLOR: Color32 = Color32::from_rgb(255, 220, 96);
const MD_H4_COLOR: Color32 = Color32::from_rgb(168, 220, 255);

const MD_INLINE_CODE_BG: Color32 = Color32::from_rgb(20, 54, 110);
const MD_INLINE_CODE_FG: Color32 = Color32::from_rgb(255, 228, 132);

const MD_CODE_BLOCK_BG: Color32 = Color32::from_rgb(5, 16, 38);
const MD_CODE_BLOCK_BORDER: Color32 = Color32::from_rgb(42, 98, 168);
const MD_CODE_LANG_BG: Color32 = Color32::from_rgb(14, 38, 82);
const MD_CODE_LANG_FG: Color32 = Color32::from_rgb(112, 212, 255);

const MD_QUOTE_BG: Color32 = Color32::from_rgb(12, 34, 74);
const MD_QUOTE_BAR: Color32 = Color32::from_rgb(90, 212, 255);
const MD_QUOTE_TEXT: Color32 = Color32::from_rgb(196, 226, 250);

const MD_LINK_COLOR: Color32 = Color32::from_rgb(96, 228, 255);
const MD_BULLET_COLOR: Color32 = Color32::from_rgb(96, 224, 255);
const MD_ORDERED_COLOR: Color32 = Color32::from_rgb(255, 220, 96);
const MD_CHECK_DONE: Color32 = Color32::from_rgb(116, 240, 168);
const MD_CHECK_TODO: Color32 = Color32::from_rgb(104, 158, 212);

const MD_RULE_COLOR: Color32 = Color32::from_rgb(42, 98, 168);
const MD_H1_RULE_COLOR: Color32 = Color32::from_rgb(76, 186, 232);
const MD_H2_RULE_COLOR: Color32 = Color32::from_rgb(32, 76, 138);

const MD_TABLE_HDR_BG: Color32 = Color32::from_rgb(16, 46, 98);
const MD_TABLE_ROW_ALT_BG: Color32 = Color32::from_rgb(11, 31, 70);
const MD_TABLE_BORDER: Color32 = Color32::from_rgb(36, 82, 144);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MdInline {
    Text(String),
    LineBreak,
    Bold(Vec<MdInline>),
    Italic(Vec<MdInline>),
    BoldItalic(Vec<MdInline>),
    Strikethrough(Vec<MdInline>),
    Code(String),
    Link { label: String, url: String },
    Image { alt: String, url: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MdBlock {
    Heading {
        level: u8,
        inlines: Vec<MdInline>,
    },
    Paragraph(Vec<MdInline>),
    CodeBlock {
        lang: String,
        code: String,
    },
    BulletItem {
        indent: usize,
        task: Option<bool>,
        inlines: Vec<MdInline>,
    },
    OrderedItem {
        indent: usize,
        number: String,
        inlines: Vec<MdInline>,
    },
    BlockQuote {
        depth: usize,
        inlines: Vec<MdInline>,
    },
    HorizontalRule,
    Table {
        headers: Vec<Vec<MdInline>>,
        rows: Vec<Vec<Vec<MdInline>>>,
    },
}

/// Parse lines of a Markdown buffer into structured blocks.
pub fn parse_markdown(lines: &[String]) -> Vec<MdBlock> {
    let mut blocks = Vec::new();
    let mut i = 0usize;
    let total = lines.len();

    while i < total {
        let raw = &lines[i];
        let trimmed = raw.trim();

        // Skip empty lines
        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // Skip HTML comments <!-- ... -->
        if trimmed.starts_with("<!--") {
            if trimmed.contains("-->") {
                i += 1;
                continue;
            }
            i += 1;
            while i < total && !lines[i].contains("-->") {
                i += 1;
            }
            if i < total {
                i += 1;
            }
            continue;
        }

        // 1. Fenced code block (``` or ~~~)
        if let Some((fence_char, fence_len, lang)) = parse_fence_open(trimmed) {
            let mut code_lines = Vec::new();
            i += 1;
            while i < total {
                let candidate = lines[i].trim();
                if is_fence_close(candidate, fence_char, fence_len) {
                    i += 1;
                    break;
                }
                code_lines.push(lines[i].as_str());
                i += 1;
            }
            blocks.push(MdBlock::CodeBlock {
                lang,
                code: code_lines.join("\n"),
            });
            continue;
        }

        // 2. ATX Heading (# .. ######)
        if let Some((level, content)) = parse_heading(trimmed) {
            blocks.push(MdBlock::Heading {
                level,
                inlines: parse_inlines(content),
            });
            i += 1;
            continue;
        }

        // 3. Horizontal rule (---, ***, ___)
        if is_horizontal_rule(trimmed) {
            blocks.push(MdBlock::HorizontalRule);
            i += 1;
            continue;
        }

        // 4. Pipe table (header row + delimiter row)
        if i + 1 < total && looks_like_table_row(trimmed) && is_table_delimiter(lines[i + 1].trim())
        {
            let header_cells = split_table_cells(trimmed);
            let col_count = header_cells.len();
            let headers = header_cells
                .into_iter()
                .map(|c| parse_inlines(&c))
                .collect();
            i += 2;
            let mut rows = Vec::new();
            while i < total {
                let row_trimmed = lines[i].trim();
                if row_trimmed.is_empty() || !looks_like_table_row(row_trimmed) {
                    break;
                }
                let mut cells = split_table_cells(row_trimmed);
                cells.resize(col_count, String::new());
                rows.push(cells.into_iter().map(|c| parse_inlines(&c)).collect());
                i += 1;
            }
            blocks.push(MdBlock::Table { headers, rows });
            continue;
        }

        // 5. Blockquote (> ...)
        if let Some((depth, first_text)) = parse_blockquote_line(trimmed) {
            let mut quote_inlines = parse_inlines(first_text);
            i += 1;
            while i < total {
                let next_trimmed = lines[i].trim();
                if let Some((next_depth, next_text)) = parse_blockquote_line(next_trimmed) {
                    if next_depth == depth {
                        if !next_text.is_empty() {
                            if !quote_inlines.is_empty() {
                                quote_inlines.push(MdInline::LineBreak);
                            }
                            quote_inlines.extend(parse_inlines(next_text));
                        }
                        i += 1;
                        continue;
                    }
                }
                break;
            }
            blocks.push(MdBlock::BlockQuote {
                depth,
                inlines: quote_inlines,
            });
            continue;
        }

        // 6. Unordered list item (- / * / +) with optional task checkbox
        if let Some((indent, task, content)) = parse_bullet_item(raw) {
            let mut inlines = parse_inlines(content);
            let mut hard_break = raw.ends_with("  ");
            i += 1;
            // Attach indented continuation lines that are not new list items or block starters
            while i < total {
                let cont_raw = &lines[i];
                let cont_trimmed = cont_raw.trim();
                if cont_trimmed.is_empty() || is_block_starter(cont_raw, cont_trimmed) {
                    break;
                }
                let leading_spaces = count_leading_spaces(cont_raw);
                if leading_spaces < 2 {
                    break;
                }
                if hard_break {
                    inlines.push(MdInline::LineBreak);
                } else {
                    push_soft_space(&mut inlines, cont_trimmed);
                }
                inlines.extend(parse_inlines(cont_trimmed));
                hard_break = cont_raw.ends_with("  ");
                i += 1;
            }
            blocks.push(MdBlock::BulletItem {
                indent,
                task,
                inlines,
            });
            continue;
        }

        // 7. Ordered list item (1. / 2) ...)
        if let Some((indent, number, content)) = parse_ordered_item(raw) {
            let mut inlines = parse_inlines(content);
            let mut hard_break = raw.ends_with("  ");
            i += 1;
            while i < total {
                let cont_raw = &lines[i];
                let cont_trimmed = cont_raw.trim();
                if cont_trimmed.is_empty() || is_block_starter(cont_raw, cont_trimmed) {
                    break;
                }
                let leading_spaces = count_leading_spaces(cont_raw);
                if leading_spaces < 2 {
                    break;
                }
                if hard_break {
                    inlines.push(MdInline::LineBreak);
                } else {
                    push_soft_space(&mut inlines, cont_trimmed);
                }
                inlines.extend(parse_inlines(cont_trimmed));
                hard_break = cont_raw.ends_with("  ");
                i += 1;
            }
            blocks.push(MdBlock::OrderedItem {
                indent,
                number,
                inlines,
            });
            continue;
        }

        // 8. Paragraph: collect consecutive plain lines
        let mut para_inlines = Vec::new();
        let mut first = true;
        let mut prev_hard_break = false;
        while i < total {
            let line_raw = &lines[i];
            let line_trimmed = line_raw.trim();
            if line_trimmed.is_empty() {
                break;
            }
            if !first && is_block_starter(line_raw, line_trimmed) {
                break;
            }
            // Also break if a table starts on this line
            if !first
                && i + 1 < total
                && looks_like_table_row(line_trimmed)
                && is_table_delimiter(lines[i + 1].trim())
            {
                break;
            }

            if !first {
                if prev_hard_break {
                    para_inlines.push(MdInline::LineBreak);
                } else {
                    push_soft_space(&mut para_inlines, line_trimmed);
                }
            }
            para_inlines.extend(parse_inlines(line_trimmed));
            prev_hard_break = line_raw.ends_with("  ");
            first = false;
            i += 1;
        }
        if !para_inlines.is_empty() {
            blocks.push(MdBlock::Paragraph(para_inlines));
        }
    }

    blocks
}

fn push_soft_space(inlines: &mut Vec<MdInline>, next_text: &str) {
    let needs_space = next_text.chars().next().is_some_and(|c| c.is_ascii());
    if needs_space {
        inlines.push(MdInline::Text(" ".to_string()));
    }
}

fn is_block_starter(raw: &str, trimmed: &str) -> bool {
    trimmed.starts_with("<!--")
        || parse_fence_open(trimmed).is_some()
        || parse_heading(trimmed).is_some()
        || is_horizontal_rule(trimmed)
        || parse_blockquote_line(trimmed).is_some()
        || parse_bullet_item(raw).is_some()
        || parse_ordered_item(raw).is_some()
}

fn count_leading_spaces(s: &str) -> usize {
    let mut n = 0usize;
    for c in s.chars() {
        match c {
            ' ' => n += 1,
            '\t' => n += 4,
            _ => break,
        }
    }
    n
}

fn parse_fence_open(trimmed: &str) -> Option<(char, usize, String)> {
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|&c| c == first).count();
    if count < 3 {
        return None;
    }
    let rest = trimmed[count..].trim();
    // Backtick info string cannot contain backticks
    if first == '`' && rest.contains('`') {
        return None;
    }
    Some((first, count, rest.to_string()))
}

fn is_fence_close(trimmed: &str, fence_char: char, min_len: usize) -> bool {
    let count = trimmed.chars().take_while(|&c| c == fence_char).count();
    count >= min_len && trimmed[count..].trim().is_empty()
}

fn parse_heading(trimmed: &str) -> Option<(u8, &str)> {
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &trimmed[hashes..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let content = rest.trim().trim_end_matches('#').trim();
    Some((hashes as u8, content))
}

fn is_horizontal_rule(trimmed: &str) -> bool {
    let mut marker: Option<char> = None;
    let mut count = 0usize;
    for c in trimmed.chars() {
        if c == ' ' || c == '\t' {
            continue;
        }
        if c == '-' || c == '*' || c == '_' {
            match marker {
                None => {
                    marker = Some(c);
                    count = 1;
                }
                Some(m) if m == c => count += 1,
                Some(_) => return false,
            }
        } else {
            return false;
        }
    }
    count >= 3
}

fn parse_blockquote_line(trimmed: &str) -> Option<(usize, &str)> {
    if !trimmed.starts_with('>') {
        return None;
    }
    let mut rest = trimmed;
    let mut depth = 0usize;
    while let Some(after) = rest.strip_prefix('>') {
        depth += 1;
        rest = after.strip_prefix(' ').unwrap_or(after);
    }
    Some((depth, rest.trim_end()))
}

fn parse_bullet_item(raw: &str) -> Option<(usize, Option<bool>, &str)> {
    let spaces = count_leading_spaces(raw);
    let trimmed = raw.trim_start();
    let mut chars = trimmed.chars();
    let marker = chars.next()?;
    if marker != '-' && marker != '*' && marker != '+' {
        return None;
    }
    let after_marker = chars.as_str();
    let content = after_marker
        .strip_prefix(' ')
        .or_else(|| after_marker.strip_prefix('\t'))?;
    let indent = spaces / 2;

    if let Some(rest) = content.strip_prefix("[ ] ") {
        return Some((indent, Some(false), rest.trim()));
    }
    if let Some(rest) = content
        .strip_prefix("[x] ")
        .or_else(|| content.strip_prefix("[X] "))
    {
        return Some((indent, Some(true), rest.trim()));
    }

    Some((indent, None, content.trim()))
}

fn parse_ordered_item(raw: &str) -> Option<(usize, String, &str)> {
    let spaces = count_leading_spaces(raw);
    let trimmed = raw.trim_start();
    let digits_len = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits_len == 0 || digits_len > 9 {
        return None;
    }
    let after_digits = &trimmed[digits_len..];
    let sep = after_digits.as_bytes().first().copied()?;
    if sep != b'.' && sep != b')' {
        return None;
    }
    let after_sep = &after_digits[1..];
    let content = after_sep
        .strip_prefix(' ')
        .or_else(|| after_sep.strip_prefix('\t'))?;
    let number = format!("{}.", &trimmed[..digits_len]);
    Some((spaces / 2, number, content.trim()))
}

fn looks_like_table_row(trimmed: &str) -> bool {
    trimmed.contains('|') && trimmed.len() >= 3
}

fn is_table_delimiter(trimmed: &str) -> bool {
    if !trimmed.contains('|') || !trimmed.contains('-') {
        return false;
    }
    let cells = split_table_cells(trimmed);
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let c = cell.trim();
            !c.is_empty()
                && c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
                && c.contains('-')
        })
}

fn split_table_cells(trimmed: &str) -> Vec<String> {
    let s = trimmed
        .strip_prefix('|')
        .unwrap_or(trimmed);
    let s = s.strip_suffix('|').unwrap_or(s);
    s.split('|').map(|c| c.trim().to_string()).collect()
}

/// Parse inline Markdown spans (bold, italic, inline code, links, images, strikethrough).
pub fn parse_inlines(input: &str) -> Vec<MdInline> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;

    let flush_buf = |buf: &mut String, out: &mut Vec<MdInline>| {
        if !buf.is_empty() {
            out.push(MdInline::Text(std::mem::take(buf)));
        }
    };

    while i < len {
        // 1. Backslash escape
        if bytes[i] == b'\\' && i + 1 < len {
            let next_ch = input[i + 1..].chars().next().unwrap();
            if next_ch.is_ascii_punctuation() {
                buf.push(next_ch);
                i += 1 + next_ch.len_utf8();
                continue;
            }
        }

        // 2. Inline code (`...` or ``...``)
        if bytes[i] == b'`' {
            let tick_count = bytes[i..].iter().take_while(|&&b| b == b'`').count();
            let delim = &input[i..i + tick_count];
            if let Some(rel_end) = input[i + tick_count..].find(delim) {
                let code_slice = &input[i + tick_count..i + tick_count + rel_end];
                flush_buf(&mut buf, &mut out);
                out.push(MdInline::Code(code_slice.trim().to_string()));
                i = i + tick_count + rel_end + tick_count;
                continue;
            }
        }

        // 3. Image ![alt](url)
        if bytes[i] == b'!' && i + 1 < len && bytes[i + 1] == b'[' {
            if let Some((alt, url, next_i)) = parse_bracket_link(input, i + 1) {
                flush_buf(&mut buf, &mut out);
                out.push(MdInline::Image { alt, url });
                i = next_i;
                continue;
            }
        }

        // 4. Link [label](url)
        if bytes[i] == b'[' {
            if let Some((label, url, next_i)) = parse_bracket_link(input, i) {
                flush_buf(&mut buf, &mut out);
                out.push(MdInline::Link { label, url });
                i = next_i;
                continue;
            }
        }

        // 5. Autolink <https://...> or <http://...>
        if bytes[i] == b'<' {
            let rest = &input[i + 1..];
            if (rest.starts_with("http://") || rest.starts_with("https://"))
                && rest.contains('>')
            {
                if let Some(end) = rest.find('>') {
                    let url = &rest[..end];
                    if !url.contains(char::is_whitespace) {
                        flush_buf(&mut buf, &mut out);
                        out.push(MdInline::Link {
                            label: url.to_string(),
                            url: url.to_string(),
                        });
                        i = i + 1 + end + 1;
                        continue;
                    }
                }
            }
        }

        // 6. Bold+Italic ***...***
        if input[i..].starts_with("***") {
            if let Some(rel_end) = find_closing_delim(&input[i + 3..], "***") {
                if rel_end > 0 {
                    flush_buf(&mut buf, &mut out);
                    let inner = &input[i + 3..i + 3 + rel_end];
                    out.push(MdInline::BoldItalic(parse_inlines(inner)));
                    i = i + 3 + rel_end + 3;
                    continue;
                }
            }
        }

        // 7. Bold **...** or __...__
        if input[i..].starts_with("**") {
            if let Some(rel_end) = find_closing_delim(&input[i + 2..], "**") {
                if rel_end > 0 {
                    flush_buf(&mut buf, &mut out);
                    let inner = &input[i + 2..i + 2 + rel_end];
                    out.push(MdInline::Bold(parse_inlines(inner)));
                    i = i + 2 + rel_end + 2;
                    continue;
                }
            }
        }
        if input[i..].starts_with("__") && is_word_boundary_before(input, i) {
            if let Some(rel_end) = find_closing_underscore_delim(&input[i + 2..], "__") {
                if rel_end > 0 {
                    flush_buf(&mut buf, &mut out);
                    let inner = &input[i + 2..i + 2 + rel_end];
                    out.push(MdInline::Bold(parse_inlines(inner)));
                    i = i + 2 + rel_end + 2;
                    continue;
                }
            }
        }

        // 8. Strikethrough ~~...~~
        if input[i..].starts_with("~~") {
            if let Some(rel_end) = find_closing_delim(&input[i + 2..], "~~") {
                if rel_end > 0 {
                    flush_buf(&mut buf, &mut out);
                    let inner = &input[i + 2..i + 2 + rel_end];
                    out.push(MdInline::Strikethrough(parse_inlines(inner)));
                    i = i + 2 + rel_end + 2;
                    continue;
                }
            }
        }

        // 9. Italic *...* or _..._
        if bytes[i] == b'*' && !input[i..].starts_with("**") {
            if let Some(rel_end) = find_closing_single_star(&input[i + 1..]) {
                if rel_end > 0 {
                    let inner = &input[i + 1..i + 1 + rel_end];
                    if !inner.starts_with(' ') && !inner.ends_with(' ') {
                        flush_buf(&mut buf, &mut out);
                        out.push(MdInline::Italic(parse_inlines(inner)));
                        i = i + 1 + rel_end + 1;
                        continue;
                    }
                }
            }
        }
        if bytes[i] == b'_'
            && !input[i..].starts_with("__")
            && is_word_boundary_before(input, i)
        {
            if let Some(rel_end) = find_closing_underscore_delim(&input[i + 1..], "_") {
                if rel_end > 0 {
                    let inner = &input[i + 1..i + 1 + rel_end];
                    if !inner.starts_with(' ') && !inner.ends_with(' ') {
                        flush_buf(&mut buf, &mut out);
                        out.push(MdInline::Italic(parse_inlines(inner)));
                        i = i + 1 + rel_end + 1;
                        continue;
                    }
                }
            }
        }

        // Fallback: consume one UTF-8 character
        let ch = input[i..].chars().next().unwrap();
        buf.push(ch);
        i += ch.len_utf8();
    }

    flush_buf(&mut buf, &mut out);
    out
}

fn is_word_boundary_before(input: &str, byte_idx: usize) -> bool {
    if byte_idx == 0 {
        return true;
    }
    input[..byte_idx]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric())
}

fn find_closing_delim(s: &str, delim: &str) -> Option<usize> {
    let mut pos = 0usize;
    while pos < s.len() {
        let rest = &s[pos..];
        if rest.starts_with('\\') && rest.len() > 1 {
            let next_ch = rest[1..].chars().next().unwrap();
            pos += 1 + next_ch.len_utf8();
            continue;
        }
        if rest.starts_with('`') {
            let ticks = rest.bytes().take_while(|&b| b == b'`').count();
            let tick_str = &rest[..ticks];
            if let Some(end) = rest[ticks..].find(tick_str) {
                pos += ticks + end + ticks;
                continue;
            }
        }
        if rest.starts_with(delim) {
            return Some(pos);
        }
        let ch = rest.chars().next().unwrap();
        pos += ch.len_utf8();
    }
    None
}

fn find_closing_single_star(s: &str) -> Option<usize> {
    let mut pos = 0usize;
    while pos < s.len() {
        let rest = &s[pos..];
        if rest.starts_with('\\') && rest.len() > 1 {
            let next_ch = rest[1..].chars().next().unwrap();
            pos += 1 + next_ch.len_utf8();
            continue;
        }
        if rest.starts_with('`') {
            let ticks = rest.bytes().take_while(|&b| b == b'`').count();
            let tick_str = &rest[..ticks];
            if let Some(end) = rest[ticks..].find(tick_str) {
                pos += ticks + end + ticks;
                continue;
            }
        }
        if rest.starts_with("**") {
            pos += 2;
            continue;
        }
        if rest.starts_with('*') {
            return Some(pos);
        }
        let ch = rest.chars().next().unwrap();
        pos += ch.len_utf8();
    }
    None
}

fn find_closing_underscore_delim(s: &str, delim: &str) -> Option<usize> {
    let dlen = delim.len();
    let mut pos = 0usize;
    while pos < s.len() {
        let rest = &s[pos..];
        if rest.starts_with('\\') && rest.len() > 1 {
            let next_ch = rest[1..].chars().next().unwrap();
            pos += 1 + next_ch.len_utf8();
            continue;
        }
        if rest.starts_with('`') {
            let ticks = rest.bytes().take_while(|&b| b == b'`').count();
            let tick_str = &rest[..ticks];
            if let Some(end) = rest[ticks..].find(tick_str) {
                pos += ticks + end + ticks;
                continue;
            }
        }
        if rest.starts_with(delim) {
            let after = &rest[dlen..];
            let boundary_after = after.chars().next().is_none_or(|c| !c.is_alphanumeric());
            if boundary_after {
                return Some(pos);
            }
        }
        let ch = rest.chars().next().unwrap();
        pos += ch.len_utf8();
    }
    None
}

fn parse_bracket_link(input: &str, open_bracket: usize) -> Option<(String, String, usize)> {
    let rest = &input[open_bracket + 1..];
    let close_bracket_rel = rest.find(']')?;
    let label = &rest[..close_bracket_rel];
    let after_bracket = &rest[close_bracket_rel + 1..];
    if !after_bracket.starts_with('(') {
        return None;
    }
    let url_part = &after_bracket[1..];
    // Support nested parentheses in URLs (e.g. Wikipedia links)
    let mut depth = 1usize;
    let mut url_end = None;
    for (idx, ch) in url_part.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    url_end = Some(idx);
                    break;
                }
            }
            _ => {}
        }
    }
    let url_end = url_end?;
    let url = url_part[..url_end].trim();
    let next_i = open_bracket + 1 + close_bracket_rel + 1 + 1 + url_end + 1;
    Some((label.to_string(), url.to_string(), next_i))
}

// --- egui Markdown Renderer ---

#[derive(Debug, Default)]
pub struct MdRenderOutput {
    pub scroll_y: f32,
    pub max_scroll_y: f32,
    pub viewport_h: f32,
    pub clicked_url: Option<String>,
}

#[derive(Clone, Copy)]
struct InlineStyle {
    size: f32,
    color: Color32,
    bold: bool,
    italic: bool,
    strike: bool,
}

/// Build an egui `LayoutJob` from inline spans along with clickable link char ranges.
fn build_inline_job(
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
    ui.painter().rect_filled(avail, CornerRadius::ZERO, crate::window_style::bg(MD_BG_NAVY));

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
                                    [
                                        Pos2::new(rule_rect.min.x, y),
                                        Pos2::new(rule_rect.max.x, y),
                                    ],
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
                                    ui.set_min_width(
                                        (content_max_w - font_size * 1.5).max(120.0),
                                    );
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
                            let marker_w =
                                ((number.len() as f32 + 0.8) * font_size * 0.65).max(font_size * 1.8);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn to_lines(s: &str) -> Vec<String> {
        s.lines().map(String::from).collect()
    }

    #[test]
    fn test_headings_and_horizontal_rule() {
        let src = "# 見出し1\n## Heading 2 ##\n### Subheading\n\n---\n";
        let blocks = parse_markdown(&to_lines(src));
        assert_eq!(blocks.len(), 4);
        assert_eq!(
            blocks[0],
            MdBlock::Heading {
                level: 1,
                inlines: vec![MdInline::Text("見出し1".into())],
            }
        );
        assert_eq!(
            blocks[1],
            MdBlock::Heading {
                level: 2,
                inlines: vec![MdInline::Text("Heading 2".into())],
            }
        );
        assert_eq!(
            blocks[2],
            MdBlock::Heading {
                level: 3,
                inlines: vec![MdInline::Text("Subheading".into())],
            }
        );
        assert_eq!(blocks[3], MdBlock::HorizontalRule);
    }

    #[test]
    fn test_inline_formatting_and_snake_case_safety() {
        let inlines = parse_inlines(
            "通常 **太字** *斜体* `inline_code` foo_bar_baz ~~打消~~ [LZ](https://example.com)",
        );
        assert_eq!(
            inlines,
            vec![
                MdInline::Text("通常 ".into()),
                MdInline::Bold(vec![MdInline::Text("太字".into())]),
                MdInline::Text(" ".into()),
                MdInline::Italic(vec![MdInline::Text("斜体".into())]),
                MdInline::Text(" ".into()),
                MdInline::Code("inline_code".into()),
                MdInline::Text(" foo_bar_baz ".into()),
                MdInline::Strikethrough(vec![MdInline::Text("打消".into())]),
                MdInline::Text(" ".into()),
                MdInline::Link {
                    label: "LZ".into(),
                    url: "https://example.com".into(),
                },
            ]
        );
    }

    #[test]
    fn test_code_block_lists_quotes_and_table() {
        let src = "\
```rust
fn main() {
    println!(\"hello\");
}
```

- 項目A
  - [x] 完了タスク
  - [ ] 未完了タスク
1. 第一項目
2. 第二項目

> 引用テスト
> 2行目

| Key | Action |
| --- | --- |
| `Ctrl+P` | Preview |
";
        let blocks = parse_markdown(&to_lines(src));
        assert_eq!(
            blocks[0],
            MdBlock::CodeBlock {
                lang: "rust".into(),
                code: "fn main() {\n    println!(\"hello\");\n}".into(),
            }
        );
        assert!(matches!(
            blocks[1],
            MdBlock::BulletItem {
                indent: 0,
                task: None,
                ..
            }
        ));
        assert!(matches!(
            blocks[2],
            MdBlock::BulletItem {
                indent: 1,
                task: Some(true),
                ..
            }
        ));
        assert!(matches!(
            blocks[3],
            MdBlock::BulletItem {
                indent: 1,
                task: Some(false),
                ..
            }
        ));
        assert!(matches!(
            blocks[4],
            MdBlock::OrderedItem {
                indent: 0,
                ref number,
                ..
            } if number == "1."
        ));
        assert!(matches!(
            blocks[6],
            MdBlock::BlockQuote { depth: 1, .. }
        ));
        assert!(matches!(
            blocks[7],
            MdBlock::Table { ref headers, ref rows } if headers.len() == 2 && rows.len() == 1
        ));
    }

    #[test]
    fn test_hard_line_break_and_autolink() {
        let src = "行1  \n行2 <https://github.com/TouryuTamane/lz-editor>";
        let blocks = parse_markdown(&to_lines(src));
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::Paragraph(inlines) => {
                assert!(inlines.contains(&MdInline::LineBreak));
                assert!(inlines.iter().any(|n| matches!(
                    n,
                    MdInline::Link { url, .. } if url == "https://github.com/TouryuTamane/lz-editor"
                )));
            }
            other => panic!("Expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_readme_document() {
        let readme = include_str!("../tests/fixtures/markdown_sample.md");
        let lines: Vec<String> = readme.lines().map(String::from).collect();
        let blocks = parse_markdown(&lines);
        assert!(blocks
            .iter()
            .any(|b| matches!(b, MdBlock::Heading { level: 1, .. })));
        assert!(blocks.iter().any(|b| matches!(b, MdBlock::Table { .. })));
        assert!(blocks
            .iter()
            .any(|b| matches!(b, MdBlock::CodeBlock { .. })));
        assert!(blocks
            .iter()
            .any(|b| matches!(b, MdBlock::BulletItem { task: Some(true), .. })));
        assert!(blocks
            .iter()
            .any(|b| matches!(b, MdBlock::HorizontalRule)));
    }
}
