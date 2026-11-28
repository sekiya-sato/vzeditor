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
            i += 1;
            if !trimmed.contains("-->") {
                while i < total && !lines[i].contains("-->") {
                    i += 1;
                }
                if i < total {
                    i += 1;
                }
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
                lang: lang.to_string(),
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
            let headers = header_cells.into_iter().map(parse_inlines).collect();
            i += 2;
            let mut rows = Vec::new();
            while i < total {
                let row_trimmed = lines[i].trim();
                if row_trimmed.is_empty() || !looks_like_table_row(row_trimmed) {
                    break;
                }
                let mut cells = split_table_cells(row_trimmed);
                cells.resize(col_count, "");
                rows.push(cells.into_iter().map(parse_inlines).collect());
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
            let inlines = parse_list_item(lines, &mut i, content);
            blocks.push(MdBlock::BulletItem {
                indent,
                task,
                inlines,
            });
            continue;
        }

        // 7. Ordered list item (1. / 2) ...)
        if let Some((indent, number, content)) = parse_ordered_item(raw) {
            let inlines = parse_list_item(lines, &mut i, content);
            blocks.push(MdBlock::OrderedItem {
                indent,
                number: format!("{number}."),
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

/// リストの項目 `lines[*i]`（本文 `content`）と、字下げした続きの行を読む。`*i` は次の行へ進む。
fn parse_list_item(lines: &[String], i: &mut usize, content: &str) -> Vec<MdInline> {
    let mut inlines = parse_inlines(content);
    let mut hard_break = lines[*i].ends_with("  ");
    *i += 1;
    // Attach indented continuation lines that are not new list items or block starters
    while let Some(cont_raw) = lines.get(*i) {
        let cont_trimmed = cont_raw.trim();
        if cont_trimmed.is_empty() || is_block_starter(cont_raw, cont_trimmed) {
            break;
        }
        if count_leading_spaces(cont_raw) < 2 {
            break;
        }
        if hard_break {
            inlines.push(MdInline::LineBreak);
        } else {
            push_soft_space(&mut inlines, cont_trimmed);
        }
        inlines.extend(parse_inlines(cont_trimmed));
        hard_break = cont_raw.ends_with("  ");
        *i += 1;
    }
    inlines
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

fn parse_fence_open(trimmed: &str) -> Option<(char, usize, &str)> {
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
    Some((first, count, rest))
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

/// 番号付きリストの項目なら (字下げ, 番号の数字, 本文)。
fn parse_ordered_item(raw: &str) -> Option<(usize, &str, &str)> {
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
    Some((spaces / 2, &trimmed[..digits_len], content.trim()))
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

fn split_table_cells(trimmed: &str) -> Vec<&str> {
    let s = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let s = s.strip_suffix('|').unwrap_or(s);
    s.split('|').map(str::trim).collect()
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
            if (rest.starts_with("http://") || rest.starts_with("https://")) && rest.contains('>') {
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
        if bytes[i] == b'_' && !input[i..].starts_with("__") && is_word_boundary_before(input, i) {
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

/// 閉じる記号を探すとき飛ばす部分（エスケープした 1 文字・インラインコード）の長さ。
fn skip_escape_or_code(rest: &str) -> Option<usize> {
    if let Some(c) = rest.strip_prefix('\\').and_then(|r| r.chars().next()) {
        return Some(1 + c.len_utf8());
    }
    if rest.starts_with('`') {
        let ticks = rest.bytes().take_while(|&b| b == b'`').count();
        let tick_str = &rest[..ticks];
        if let Some(end) = rest[ticks..].find(tick_str) {
            return Some(ticks + end + ticks);
        }
    }
    None
}

fn find_closing_delim(s: &str, delim: &str) -> Option<usize> {
    let mut pos = 0usize;
    while pos < s.len() {
        let rest = &s[pos..];
        if let Some(n) = skip_escape_or_code(rest) {
            pos += n;
            continue;
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
        if let Some(n) = skip_escape_or_code(rest) {
            pos += n;
            continue;
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
        if let Some(n) = skip_escape_or_code(rest) {
            pos += n;
            continue;
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
        assert!(matches!(blocks[6], MdBlock::BlockQuote { depth: 1, .. }));
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
        let readme = include_str!("../../../../tests/fixtures/markdown_sample.md");
        let lines: Vec<String> = readme.lines().map(String::from).collect();
        let blocks = parse_markdown(&lines);
        assert!(blocks
            .iter()
            .any(|b| matches!(b, MdBlock::Heading { level: 1, .. })));
        assert!(blocks.iter().any(|b| matches!(b, MdBlock::Table { .. })));
        assert!(blocks
            .iter()
            .any(|b| matches!(b, MdBlock::CodeBlock { .. })));
        assert!(blocks.iter().any(|b| matches!(
            b,
            MdBlock::BulletItem {
                task: Some(true),
                ..
            }
        )));
        assert!(blocks.iter().any(|b| matches!(b, MdBlock::HorizontalRule)));
    }
}
