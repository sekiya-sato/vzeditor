//! 巨大ファイルの検索・すべて置換・Grep。文書を行の境界にそろえた約 4MB のチャンクに分け、
//! スレッドの数ずつ並列に調べる（docs/巨大ファイル詳細設計.md の 5.8・5.9）。

use super::DocSnapshot;
use crate::encoding::TextEncoding;
use crate::search::SearchPattern;
use memchr::memchr_iter;
use rayon::prelude::*;
use std::borrow::Cow;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// 検索の分割単位
const SEARCH_CHUNK: u64 = 4 << 20;
/// 行頭を探すとき、前へ戻って見る最大のバイト数
const LINE_BACK_LIMIT: u64 = 1 << 20;

/// 文書の文字コードに合わせた正規表現。UTF-8 はバイト列のまま、Shift_JIS はデコードして照合する。
pub enum Matcher {
    Bytes(regex::bytes::Regex),
    Text(regex::Regex, TextEncoding),
}

impl Matcher {
    pub fn new(pattern: &SearchPattern, encoding: TextEncoding) -> Result<Self, String> {
        let source = pattern.regex_source();
        let err = |e: regex::Error| format!("正規表現エラー: {e}");
        Ok(match encoding {
            TextEncoding::ShiftJis => Self::Text(
                regex::RegexBuilder::new(source)
                    .case_insensitive(pattern.case_insensitive())
                    .multi_line(true)
                    .crlf(true)
                    .build()
                    .map_err(err)?,
                encoding,
            ),
            _ => Self::Bytes(
                regex::bytes::RegexBuilder::new(source)
                    .case_insensitive(pattern.case_insensitive())
                    .multi_line(true)
                    .crlf(true)
                    .build()
                    .map_err(err)?,
            ),
        })
    }

    /// チャンク内の一致（バイト位置）を順に f へ渡す。f が false を返したら止める。改行をまたぐ一致は飛ばす。
    fn for_each(&self, chunk: &[u8], mut f: impl FnMut(usize, usize) -> bool) {
        match self {
            Self::Bytes(re) => {
                for m in re.find_iter(chunk) {
                    if memchr::memchr(b'\n', m.as_bytes()).is_none() && !f(m.start(), m.end()) {
                        return;
                    }
                }
            }
            Self::Text(re, enc) => {
                let text = enc.decode_body(chunk);
                // デコードした文字列の位置 → 元のバイト位置（一致の手前を元の文字コードに戻した長さ）
                let mut last_text = 0usize;
                let mut last_byte = 0usize;
                for m in re.find_iter(&text) {
                    if m.as_str().contains('\n') {
                        continue;
                    }
                    last_byte += encoded_len(*enc, &text[last_text..m.start()]);
                    last_text = m.start();
                    let end = last_byte + encoded_len(*enc, m.as_str());
                    if !f(last_byte, end) {
                        return;
                    }
                }
            }
        }
    }
}

fn encoded_len(enc: TextEncoding, s: &str) -> usize {
    enc.encode(s).map_or(s.len(), |b| b.len())
}

/// [start, end) を、行の境界にそろえた約 SEARCH_CHUNK のチャンクに分ける。
fn chunk_bounds(snap: &DocSnapshot, start: u64, end: u64) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut s = start;
    while s < end {
        let mut e = (s + SEARCH_CHUNK).min(end);
        // 次の改行の直後まで伸ばす（長い行は 64KB ずつ探す）
        while e < end {
            let window = snap.read(e, (64 << 10).min(end - e));
            match memchr::memchr(b'\n', &window) {
                Some(i) => {
                    e += i as u64 + 1;
                    break;
                }
                None => e += window.len() as u64,
            }
        }
        out.push((s, e));
        s = e;
    }
    out
}

/// pos を含む行の行頭（見つからなければ LINE_BACK_LIMIT 手前まで）。
fn line_start_before(snap: &DocSnapshot, pos: u64) -> u64 {
    let from = pos.saturating_sub(LINE_BACK_LIMIT);
    let window = snap.read(from, pos - from);
    match memchr::memrchr(b'\n', &window) {
        Some(i) => from + i as u64 + 1,
        None => from,
    }
}

/// 次（up なら前）の一致の範囲。wrap なら端から続けて探す。cancel が立ったら None。
pub fn find(
    snap: &DocSnapshot,
    m: &Matcher,
    origin: u64,
    up: bool,
    wrap: bool,
    cancel: &AtomicBool,
) -> Option<(u64, u64)> {
    let len = snap.len();
    let origin = origin.min(len);
    let first = if up {
        find_backward(snap, m, 0, origin, origin, cancel)
    } else {
        find_forward(snap, m, origin, len, origin, cancel)
    };
    if first.is_some() || !wrap || cancel.load(Ordering::Relaxed) {
        return first;
    }
    if up {
        find_backward(snap, m, origin, len, len, cancel)
    } else {
        find_forward(snap, m, 0, origin, 0, cancel)
    }
}

/// [from, to) で、位置 min 以降に始まる最初の一致（min の空一致は飛ばす）。
fn find_forward(
    snap: &DocSnapshot,
    m: &Matcher,
    from: u64,
    to: u64,
    min: u64,
    cancel: &AtomicBool,
) -> Option<(u64, u64)> {
    let start = line_start_before(snap, from);
    let chunks = chunk_bounds(snap, start, to);
    let batch = rayon::current_num_threads().max(1);
    for group in chunks.chunks(batch) {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let hits: Vec<Option<(u64, u64)>> = group
            .par_iter()
            .map(|&(s, e)| {
                let bytes = snap.read(s, e - s);
                let mut hit = None;
                m.for_each(&bytes, |a, b| {
                    let (a, b) = (s + a as u64, s + b as u64);
                    if a < min || (a == min && a == b && min == from && from > 0) || b > to {
                        return true;
                    }
                    hit = Some((a, b));
                    false
                });
                hit
            })
            .collect();
        if let Some(hit) = hits.into_iter().flatten().next() {
            return Some(hit);
        }
    }
    None
}

/// [from, to) で、位置 max 以前に終わる最後の一致（max の空一致は飛ばす）。
fn find_backward(
    snap: &DocSnapshot,
    m: &Matcher,
    from: u64,
    to: u64,
    max: u64,
    cancel: &AtomicBool,
) -> Option<(u64, u64)> {
    let chunks = chunk_bounds(snap, line_start_before(snap, from), to);
    let batch = rayon::current_num_threads().max(1);
    for group in chunks.rchunks(batch) {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let hits: Vec<Option<(u64, u64)>> = group
            .par_iter()
            .map(|&(s, e)| {
                let bytes = snap.read(s, e - s);
                let mut hit = None;
                m.for_each(&bytes, |a, b| {
                    let (a, b) = (s + a as u64, s + b as u64);
                    if a >= from && b <= max && !(a == b && b == max) {
                        hit = Some((a, b));
                    }
                    true
                });
                hit
            })
            .collect();
        if let Some(hit) = hits.into_iter().rev().flatten().next() {
            return Some(hit);
        }
    }
    None
}

/// すべて置換: 置換した結果を out に書き出し、置換した件数を返す。scope があればその範囲だけ置換する。
/// 書き出すのは本文だけ（BOM は付けない）。
#[allow(clippy::too_many_arguments)]
pub fn replace_all_to_file(
    snap: &DocSnapshot,
    m: &Matcher,
    replacement: &str,
    expand: bool,
    scope: Option<(u64, u64)>,
    out: &Path,
    progress: &AtomicU64,
    cancel: &AtomicBool,
) -> io::Result<u64> {
    let len = snap.len();
    let (a, b) = scope.unwrap_or((0, len));
    // 置換後の文字列の改行は文書の改行コードにそろえる
    let replacement = if snap.line_ending == "\r\n" {
        replacement.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        replacement.replace("\r\n", "\n")
    };
    let encoded = snap
        .encoding
        .encode(&replacement)
        .map_err(|e| {
            io::Error::other(format!(
                "置換後の文字列の「{}」は {} で表せません",
                e.ch, snap.encoding
            ))
        })?
        .into_owned();
    let mut w = BufWriter::with_capacity(8 << 20, File::create(out)?);
    let copy = |w: &mut BufWriter<File>, from: u64, to: u64| -> io::Result<()> {
        let mut pos = from;
        while pos < to {
            let n = (to - pos).min(SEARCH_CHUNK);
            w.write_all(&snap.read(pos, n))?;
            pos += n;
        }
        Ok(())
    };
    copy(&mut w, 0, a)?;
    let chunks = chunk_bounds(snap, a, b);
    let batch = rayon::current_num_threads().max(1) * 2;
    let mut count = 0u64;
    for group in chunks.chunks(batch) {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::other("置換を中止しました"));
        }
        let outputs: Vec<io::Result<(Vec<u8>, u64)>> = group
            .par_iter()
            .map(|&(s, e)| replace_chunk(m, &snap.read(s, e - s), &replacement, &encoded, expand))
            .collect();
        for out in outputs {
            let (bytes, n) = out?;
            w.write_all(&bytes)?;
            count += n;
        }
        if let Some(&(_, e)) = group.last() {
            progress.store(e, Ordering::Relaxed);
        }
    }
    copy(&mut w, b, len)?;
    w.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    Ok(count)
}

fn replace_chunk(
    m: &Matcher,
    chunk: &[u8],
    replacement: &str,
    encoded: &[u8],
    expand: bool,
) -> io::Result<(Vec<u8>, u64)> {
    let mut n = 0u64;
    match m {
        Matcher::Bytes(re) => {
            let out = re.replace_all(chunk, |caps: &regex::bytes::Captures<'_>| {
                n += 1;
                let mut dst = Vec::new();
                if expand {
                    caps.expand(encoded, &mut dst);
                } else {
                    dst.extend_from_slice(encoded);
                }
                dst
            });
            Ok((out.into_owned(), n))
        }
        Matcher::Text(re, enc) => {
            let text = enc.decode_body(chunk);
            let out = re.replace_all(&text, |caps: &regex::Captures<'_>| {
                n += 1;
                let mut dst = String::new();
                if expand {
                    caps.expand(replacement, &mut dst);
                } else {
                    dst.push_str(replacement);
                }
                dst
            });
            if n == 0 {
                return Ok((chunk.to_vec(), 0));
            }
            let bytes = enc
                .encode(&out)
                .map_err(|e| io::Error::other(format!("「{}」は {enc} で表せません", e.ch)))?
                .into_owned();
            Ok((bytes, n))
        }
    }
}

/// Grep の 1 件（行は 0 始まり、桁は文字位置）。
pub struct LineHit {
    pub line: usize,
    pub col: usize,
    pub text: String,
}

/// 文書を先頭から調べ、一致した行を順に f へ渡す（f が false を返したら止める）。
/// `each_match` なら同じ行の一致を全部、`non_match` なら一致しない行を渡す。
pub fn scan_lines(
    snap: &DocSnapshot,
    m: &Matcher,
    each_match: bool,
    non_match: bool,
    cancel: &AtomicBool,
    mut f: impl FnMut(LineHit) -> bool,
) {
    let enc = snap.encoding;
    let mut line_base = 0usize;
    for (s, e) in chunk_bounds(snap, 0, snap.len()) {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let bytes = snap.read(s, e - s);
        let chunk: &[u8] = &bytes;
        let line_of = |pos: usize, from: usize, base: usize| -> usize {
            base + memchr_iter(b'\n', &chunk[from..pos]).count()
        };
        if non_match {
            let mut line_start = 0usize;
            let ends = memchr_iter(b'\n', chunk).chain(std::iter::once(chunk.len()));
            for (line, nl) in (line_base..).zip(ends) {
                // チャンクは改行で終わるので、最後のチャンク以外の末尾の空の「行」は数えない
                if line_start == chunk.len() && e != snap.len() {
                    break;
                }
                let mut text = &chunk[line_start..nl];
                if text.last() == Some(&b'\r') {
                    text = &text[..text.len() - 1];
                }
                let mut found = false;
                m.for_each(text, |_, _| {
                    found = true;
                    false
                });
                if !found
                    && !f(LineHit {
                        line,
                        col: 0,
                        text: snippet(enc, text),
                    })
                {
                    return;
                }
                line_start = nl + 1;
            }
        } else {
            let mut stop = false;
            let (mut scan_pos, mut scan_line) = (0usize, line_base);
            let mut last_line = usize::MAX;
            m.for_each(chunk, |a, b| {
                scan_line = line_of(a, scan_pos, scan_line);
                scan_pos = a;
                if !each_match && scan_line == last_line {
                    return true;
                }
                last_line = scan_line;
                let ls = memchr::memrchr(b'\n', &chunk[..a]).map_or(0, |i| i + 1);
                let le = memchr::memchr(b'\n', &chunk[a..]).map_or(chunk.len(), |i| a + i);
                let mut text = &chunk[ls..le];
                if text.last() == Some(&b'\r') {
                    text = &text[..text.len() - 1];
                }
                let col = super::count_chars(enc, &chunk[ls..a]);
                // 一致ごとの出力では一致した文字列、行ごとの出力では行を返す（通常の Grep と同じ）
                if each_match {
                    text = &chunk[a..b];
                }
                if !f(LineHit {
                    line: scan_line,
                    col,
                    text: snippet(enc, text),
                }) {
                    stop = true;
                    return false;
                }
                true
            });
            if stop {
                return;
            }
        }
        line_base += memchr_iter(b'\n', chunk).count();
    }
}

/// 表示用の行の抜粋（300 文字まで）。
fn snippet(enc: TextEncoding, bytes: &[u8]) -> String {
    let head = &bytes[..bytes.len().min(300 * 4)];
    let text: Cow<'_, str> = enc.decode_body(head);
    let mut chars = text.chars();
    let mut out: String = chars.by_ref().take(300).collect();
    if chars.next().is_some() || head.len() < bytes.len() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::huge::HugeDoc;
    use crate::search::SearchOptions;

    fn doc_with(text: &[u8], name: &str) -> (HugeDoc, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("vz_huge_search_{name}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.txt");
        std::fs::write(&path, text).unwrap();
        let mut doc = HugeDoc::open_with_chunk(&path, 4096).unwrap();
        doc.wait_index();
        (doc, dir)
    }

    #[test]
    fn test_find_replace_scan() {
        let mut text = String::new();
        for i in 0..20_000 {
            if i % 997 == 0 {
                text.push_str(&format!("{i} MARK あ MARK\n"));
            } else {
                text.push_str(&format!("{i} plain line\n"));
            }
        }
        let (mut doc, dir) = doc_with(text.as_bytes(), "utf8");
        // ピースをまたぐ一致: 編集で 1 つ目の MARK の途中に区切りを入れる
        let first = text.find("MARK").unwrap() as u64;
        doc.insert(first + 2, "").unwrap();
        doc.insert(0, "x").unwrap();
        doc.delete(0, 1).unwrap();
        let pattern = SearchPattern::new("mark", &SearchOptions::default()).unwrap();
        let m = Matcher::new(&pattern, doc.encoding).unwrap();
        let snap = doc.doc_snapshot();
        let cancel = AtomicBool::new(false);
        let hit = find(&snap, &m, 0, false, false, &cancel).unwrap();
        assert_eq!(hit, (first, first + 4));
        let next = find(&snap, &m, hit.1, false, false, &cancel).unwrap();
        assert_eq!(&snap.read(next.0, 4)[..], b"MARK");
        assert!(next.0 > hit.1);
        let last = text.rfind("MARK").unwrap() as u64;
        assert_eq!(
            find(&snap, &m, snap.len(), true, false, &cancel),
            Some((last, last + 4))
        );
        // 折り返し: 末尾から先頭へ
        assert_eq!(
            find(&snap, &m, last + 4, false, true, &cancel),
            Some((first, first + 4))
        );
        assert_eq!(find(&snap, &m, last + 4, false, false, &cancel), None);

        let mut lines = Vec::new();
        scan_lines(&snap, &m, false, false, &cancel, |h| {
            lines.push((h.line, h.col));
            true
        });
        let expected: Vec<_> = (0..20_000)
            .filter(|i| i % 997 == 0)
            .map(|i| (i, format!("{i} ").len()))
            .collect();
        assert_eq!(lines, expected);

        let out = dir.join("out.txt");
        let (p, c) = (AtomicU64::new(0), AtomicBool::new(false));
        let n = replace_all_to_file(&snap, &m, "X", false, None, &out, &p, &c).unwrap();
        assert_eq!(n, expected.len() as u64 * 2);
        let replaced = std::fs::read_to_string(&out).unwrap();
        assert_eq!(replaced, text.replace("MARK", "X"));
        drop(doc);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_shift_jis_search_and_replace() {
        let text = "一行目 猫です\r\n二行目 犬\r\n三行目 猫\r\n";
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode(text);
        let (doc, dir) = doc_with(&sjis, "sjis");
        let pattern = SearchPattern::new("猫", &SearchOptions::default()).unwrap();
        let m = Matcher::new(&pattern, doc.encoding).unwrap();
        let snap = doc.doc_snapshot();
        let cancel = AtomicBool::new(false);
        let (a, b) = find(&snap, &m, 0, false, false, &cancel).unwrap();
        assert_eq!(doc.offset_to_pos(a), Some((0, 4)));
        assert_eq!(b - a, 2);
        let (a2, _) = find(&snap, &m, b, false, false, &cancel).unwrap();
        assert_eq!(doc.offset_to_pos(a2), Some((2, 4)));
        let out = dir.join("out.txt");
        let (p, c) = (AtomicU64::new(0), AtomicBool::new(false));
        let n = replace_all_to_file(&snap, &m, "ねこ", false, None, &out, &p, &c).unwrap();
        assert_eq!(n, 2);
        let replaced = text.replace('猫', "ねこ");
        let (expected, _, _) = encoding_rs::SHIFT_JIS.encode(&replaced);
        assert_eq!(std::fs::read(&out).unwrap(), expected.as_ref());
        drop(doc);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
