//! エディターと Grep で共有する行単位の検索・置換。

use crate::buffer::{char_to_byte_idx, TextBuffer, TextPos};
use regex::{Regex, RegexBuilder};
use std::borrow::Cow;

pub type TextRange = (TextPos, TextPos);

#[derive(Default)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

#[derive(Clone)]
pub struct SearchPattern {
    regex: Regex,
    expand_captures: bool,
    case_insensitive: bool,
}

impl SearchPattern {
    pub fn new(query: &str, options: &SearchOptions) -> Result<Self, String> {
        if query.is_empty() {
            return Err("検索条件を入力してください".into());
        }
        let mut pattern = if options.regex {
            query.to_owned()
        } else {
            regex::escape(query)
        };
        if options.whole_word {
            pattern = format!(r"\b{{start-half}}(?:{pattern})\b{{end-half}}");
        }
        let regex = RegexBuilder::new(&pattern)
            .case_insensitive(!options.case_sensitive)
            .build()
            .map_err(|err| format!("正規表現エラー: {err}"))?;
        Ok(Self {
            regex,
            expand_captures: options.regex,
            case_insensitive: !options.case_sensitive,
        })
    }

    /// 組み立てた正規表現（巨大ファイルの検索でバイト列用の正規表現を作るのに使う）。
    pub fn regex_source(&self) -> &str {
        self.regex.as_str()
    }

    pub fn case_insensitive(&self) -> bool {
        self.case_insensitive
    }

    /// 置換文字列の $1 などを展開するか（正規表現のとき）。
    pub fn expands_captures(&self) -> bool {
        self.expand_captures
    }

    pub fn matches<'a>(&self, line: &'a str) -> regex::Matches<'_, 'a> {
        self.regex.find_iter(line)
    }

    pub fn is_match(&self, line: &str) -> bool {
        self.regex.is_match(line)
    }

    pub fn find(&self, buf: &TextBuffer, up: bool, wrap: bool) -> Option<TextRange> {
        let cursor = buf.cursor_pos();
        let (start, end) = buf.selection_range().unwrap_or((cursor, cursor));
        let origin = if up { start } else { end };
        let skip_origin = start == end && buf.anchor == Some(origin);
        let boundary = char_to_byte_idx(&buf.line(origin.0), origin.1);
        let find_line = |line: usize, wrapped: bool| {
            let text = buf.line(line);
            let text = text.as_ref();
            let candidates = self.regex.find_iter(text).filter(|hit| {
                if line != origin.0 || wrapped {
                    return true;
                }
                if skip_origin && hit.is_empty() && hit.start() == boundary {
                    return false;
                }
                if up {
                    hit.end() <= boundary
                } else {
                    hit.start() >= boundary
                }
            });
            let hit = if up {
                candidates.last()
            } else {
                candidates.into_iter().next()
            }?;
            Some((
                (line, text[..hit.start()].chars().count()),
                (line, text[..hit.end()].chars().count()),
            ))
        };
        let len = buf.line_count();
        if up {
            (0..=origin.0)
                .rev()
                .find_map(|line| find_line(line, false))
                .or_else(|| {
                    wrap.then(|| (origin.0..len).rev().find_map(|line| find_line(line, true)))
                        .flatten()
                })
        } else {
            (origin.0..len)
                .find_map(|line| find_line(line, false))
                .or_else(|| {
                    wrap.then(|| (0..=origin.0).find_map(|line| find_line(line, true)))
                        .flatten()
                })
        }
    }

    pub fn replacement_for(
        &self,
        line: &str,
        from: usize,
        to: usize,
        replacement: &str,
    ) -> Option<String> {
        let from = char_to_byte_idx(line, from);
        let to = char_to_byte_idx(line, to);
        let captures = self.regex.captures_at(line, from)?;
        let hit = captures.get(0)?;
        if (hit.start(), hit.end()) != (from, to) {
            return None;
        }
        let mut output = String::new();
        if self.expand_captures {
            captures.expand(replacement, &mut output);
        } else {
            output.push_str(replacement);
        }
        Some(output)
    }

    /// 範囲だけを対象とし、アンカーや単語境界は元の行で判定する。
    pub fn replace_all(
        &self,
        buf: &mut TextBuffer,
        scope: Option<TextRange>,
        replacement: &str,
    ) -> usize {
        let replacement = if replacement.contains('\r') {
            Cow::Owned(replacement.replace("\r\n", "\n").replace('\r', "\n"))
        } else {
            Cow::Borrowed(replacement)
        };
        if buf.is_huge() {
            // 巨大ファイルは別の経路（作業ファイルへの書き出し）で置換する
            return 0;
        }
        let last = buf.lines.len() - 1;
        let (start, end) = scope.unwrap_or(((0, 0), (last, buf.line_char_len(last))));
        let mut count = 0;
        let mut output = Vec::with_capacity(buf.lines.len());
        for (i, line) in buf.lines.iter().enumerate() {
            if i < start.0 || i > end.0 || (i == end.0 && end.1 == 0 && start.0 != end.0) {
                output.push(line.clone());
                continue;
            }
            let from = if i == start.0 {
                char_to_byte_idx(line, start.1)
            } else {
                0
            };
            let to = if i == end.0 {
                char_to_byte_idx(line, end.1)
            } else {
                line.len()
            };
            let mut replaced = String::new();
            let mut copied = 0;
            for captures in self.regex.captures_iter(line) {
                let Some(hit) = captures.get(0) else { continue };
                if hit.start() < from {
                    continue;
                }
                if hit.end() > to {
                    break;
                }
                replaced.push_str(&line[copied..hit.start()]);
                if self.expand_captures {
                    captures.expand(&replacement, &mut replaced);
                } else {
                    replaced.push_str(&replacement);
                }
                copied = hit.end();
                count += 1;
            }
            replaced.push_str(&line[copied..]);
            if replaced.contains('\n') {
                output.extend(replaced.split('\n').map(str::to_owned));
            } else {
                output.push(replaced);
            }
        }
        if output != buf.lines {
            buf.replace_all(output);
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_direction_wrap_and_unicode_columns() {
        let mut buf = TextBuffer::new();
        buf.insert_text("猫 Cat scatter\ncat");
        buf.move_doc_start();
        let pattern = SearchPattern::new(
            "cat",
            &SearchOptions {
                whole_word: true,
                ..Default::default()
            },
        )
        .unwrap();
        let first = ((0, 2), (0, 5));
        let last = ((1, 0), (1, 3));
        assert_eq!(pattern.find(&buf, false, false), Some(first));
        assert_eq!(pattern.find(&buf, true, true), Some(last));
        buf.select_range(last.0, last.1);
        assert_eq!(pattern.find(&buf, false, true), Some(first));
        assert_eq!(pattern.find(&buf, true, false), Some(first));
        assert_eq!(pattern.find(&buf, false, false), None);
    }

    #[test]
    fn scoped_regex_replacement_retains_anchors_and_undo() {
        let mut buf = TextBuffer::new();
        buf.insert_text("abc\nabc\nabc");
        let pattern = SearchPattern::new(
            "^(a)(bc)",
            &SearchOptions {
                regex: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            pattern.replace_all(&mut buf, Some(((0, 1), (2, 0))), "$2\r\n$1"),
            1
        );
        assert_eq!(buf.lines, ["abc", "bc", "a", "abc"]);
        assert!(buf.undo());
        assert_eq!(buf.lines, ["abc", "abc", "abc"]);
    }

    #[test]
    fn zero_width_search_advances_and_literal_dollars_stay_literal() {
        let mut buf = TextBuffer::new();
        buf.insert_text("a\na");
        buf.move_doc_start();
        let anchor = SearchPattern::new(
            "^",
            &SearchOptions {
                regex: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(anchor.find(&buf, false, false), Some(((0, 0), (0, 0))));
        buf.select_range((0, 0), (0, 0));
        assert_eq!(anchor.find(&buf, false, false), Some(((1, 0), (1, 0))));
        let literal = SearchPattern::new("a", &SearchOptions::default()).unwrap();
        assert_eq!(literal.replace_all(&mut buf, None, "$1"), 2);
        assert_eq!(buf.lines, ["$1", "$1"]);
    }
}
