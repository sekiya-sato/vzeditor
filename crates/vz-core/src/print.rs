//! 印刷の割付（A4 縦 / 横）。改ページ・折り返し・行番号・ヘッダーを OS に依存せず決める。
//!
//! 文字は等幅の升目に置く（半角 = 0.5em、全角 = 1em。エディターと同じ unicode-width の判定）。
//! 長さの単位はポイント（1/72 インチ）。紙への出力は `vz-platform::print` が行う。

use crate::buffer::expand_tabs_for_display;
use unicode_width::UnicodeWidthChar;

/// 本文の文字サイズ（ポイント）の範囲と既定値。
pub const MIN_FONT_PT: u8 = 8;
pub const MAX_FONT_PT: u8 = 14;
pub const DEFAULT_FONT_PT: u8 = 10;
/// 印刷できるページ数の目安。論理行数がこのページ数に収まらなければ「大きすぎる」とする。
pub const PAGE_LIMIT: usize = 100;
/// 折り返しで増えたページの上限（1 行が極端に長いファイルを止める）。
const PAGE_HARD_LIMIT: usize = PAGE_LIMIT * 2;

const PT_PER_MM: f32 = 72.0 / 25.4;
const A4_SHORT_MM: f32 = 210.0;
const A4_LONG_MM: f32 = 297.0;
/// 上下左右の余白（プリンターの印字できない縁より広く取る）
const MARGIN_MM: f32 = 7.5;
/// ヘッダー欄の高さ（ヘッダーの文字と本文との間を含む）
const HEADER_BLOCK_MM: f32 = 8.0;
/// ヘッダーの文字サイズ（ポイント）
pub const HEADER_FONT_PT: f32 = 9.0;
/// 行送り = 文字サイズ × この値
const LINE_PITCH_RATIO: f32 = 1.4;
/// 行番号の最小桁数
const MIN_NUMBER_DIGITS: usize = 3;

/// 用紙の向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    pub fn label(self) -> &'static str {
        match self {
            Orientation::Portrait => "A4 縦",
            Orientation::Landscape => "A4 横",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Orientation::Portrait => Orientation::Landscape,
            Orientation::Landscape => Orientation::Portrait,
        }
    }
}

/// 印刷の設定（向き・本文の文字サイズ・ページ番号の有無）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageSetup {
    pub orientation: Orientation,
    pub font_pt: u8,
    pub page_numbers: bool,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self {
            orientation: Orientation::Portrait,
            font_pt: DEFAULT_FONT_PT,
            page_numbers: true,
        }
    }
}

impl PageSetup {
    /// 文字サイズを範囲に収めた設定。
    pub fn clamped(self) -> Self {
        Self {
            font_pt: self.font_pt.clamp(MIN_FONT_PT, MAX_FONT_PT),
            ..self
        }
    }

    /// 用紙の幅と高さ（ポイント）。
    pub fn paper_pt(&self) -> (f32, f32) {
        let (w, h) = match self.orientation {
            Orientation::Portrait => (A4_SHORT_MM, A4_LONG_MM),
            Orientation::Landscape => (A4_LONG_MM, A4_SHORT_MM),
        };
        (w * PT_PER_MM, h * PT_PER_MM)
    }

    pub fn margin_pt(&self) -> f32 {
        MARGIN_MM * PT_PER_MM
    }

    pub fn font_size_pt(&self) -> f32 {
        f32::from(self.font_pt)
    }

    /// 半角 1 文字分の幅（ポイント）。
    pub fn cell_pt(&self) -> f32 {
        self.font_size_pt() * 0.5
    }

    pub fn line_pitch_pt(&self) -> f32 {
        self.font_size_pt() * LINE_PITCH_RATIO
    }

    /// ヘッダーの文字の基準線（用紙の上端から、ポイント）。
    pub fn header_baseline_pt(&self) -> f32 {
        self.margin_pt() + HEADER_FONT_PT
    }

    /// 本文の上端（用紙の上端から、ポイント）。
    pub fn body_top_pt(&self) -> f32 {
        self.margin_pt() + HEADER_BLOCK_MM * PT_PER_MM
    }

    /// 本文 row 行目の基準線（用紙の上端から、ポイント）。
    pub fn row_baseline_pt(&self, row: usize) -> f32 {
        self.body_top_pt() + row as f32 * self.line_pitch_pt() + self.font_size_pt()
    }

    /// 本文の欄の幅（ポイント）。
    pub fn body_width_pt(&self) -> f32 {
        self.paper_pt().0 - 2.0 * self.margin_pt()
    }

    /// 本文の下端（用紙の上端から、ポイント）。
    pub fn body_bottom_pt(&self) -> f32 {
        self.paper_pt().1 - self.margin_pt()
    }

    /// 1 行に入る半角の桁数（行番号の欄を含む）。
    pub fn body_cols(&self) -> usize {
        (self.body_width_pt() / self.cell_pt()).floor() as usize
    }

    /// 1 ページの行数。
    pub fn body_rows(&self) -> usize {
        let (_, h) = self.paper_pt();
        let usable = h - 2.0 * self.margin_pt() - HEADER_BLOCK_MM * PT_PER_MM;
        ((usable / self.line_pitch_pt()).floor() as usize).max(1)
    }
}

/// ヘッダーに出す印刷日時（ローカル時刻）。
pub fn printed_at_now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

/// 文字が占める升目の数（半角 = 1、全角 = 2）。制御文字は 1。
pub fn char_cells(c: char) -> usize {
    c.width().unwrap_or(1)
}

/// 文字列が占める升目の数。
pub fn str_cells(s: &str) -> usize {
    s.chars().map(char_cells).sum()
}

/// 印字する 1 行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintLine {
    /// 論理行番号（1 から）。折り返しの続きは None
    pub number: Option<usize>,
    /// タブを展開した本文（本文の欄に収まる長さ）
    pub text: String,
}

/// 色（RGB）。
pub type Rgb = [u8; 3];

/// 図として置く印刷項目（Markdown ビューの印刷）。座標は用紙の左上からのポイント。
#[derive(Debug, Clone, PartialEq)]
pub enum PrintItem {
    /// 文字列。y は基準線、advances は各文字の送り幅（文字と同じ数）
    Text {
        x: f32,
        y: f32,
        size: f32,
        color: Rgb,
        italic: bool,
        text: String,
        advances: Vec<f32>,
    },
    /// 矩形（塗り・枠線は任意）
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        fill: Option<Rgb>,
        stroke: Option<(f32, Rgb)>,
    },
    /// 線分
    Line {
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
        color: Rgb,
    },
}

/// 1 ページの中身。
#[derive(Debug, Clone)]
pub enum PrintPage {
    /// 行番号付きの行（テキストの印刷）
    Lines(Vec<PrintLine>),
    /// 図として置く項目（Markdown ビューの印刷。行番号は無い）
    Items(Vec<PrintItem>),
}

impl PrintPage {
    /// 行の印刷なら行の一覧、図なら空。
    pub fn lines(&self) -> &[PrintLine] {
        match self {
            PrintPage::Lines(lines) => lines,
            PrintPage::Items(_) => &[],
        }
    }
}

/// 割付の結果。
#[derive(Debug, Clone)]
pub struct PrintDoc {
    pub setup: PageSetup,
    /// ヘッダー左のファイル名
    pub title: String,
    /// ヘッダー中央の印刷日時
    pub printed_at: String,
    /// 行番号の桁数
    pub number_digits: usize,
    pub pages: Vec<PrintPage>,
    /// 印刷するページ（0 始まり、末尾を含まない）。ページ番号は全体の番号のまま
    pub range: std::ops::Range<usize>,
}

impl PrintDoc {
    /// 図のページ（Markdown ビューの印刷）から文書を作る。目安のページ数を超えたら `TooLarge`。
    pub fn from_items(
        setup: PageSetup,
        title: &str,
        printed_at: &str,
        mut pages: Vec<Vec<PrintItem>>,
    ) -> Result<Self, TooLarge> {
        if pages.len() > PAGE_LIMIT {
            return Err(TooLarge);
        }
        if pages.is_empty() {
            pages.push(Vec::new());
        }
        Ok(Self {
            setup: setup.clamped(),
            title: title.to_string(),
            printed_at: printed_at.to_string(),
            number_digits: 0,
            range: 0..pages.len(),
            pages: pages.into_iter().map(PrintPage::Items).collect(),
        })
    }

    /// 印刷するページを from〜to（1 始まり、両端を含む）に絞る。範囲は 1〜総ページ数に収める。
    pub fn with_page_range(mut self, from: usize, to: usize) -> Self {
        let total = self.pages.len();
        let to = to.clamp(1, total);
        let from = from.clamp(1, to);
        self.range = from - 1..to;
        self
    }

    /// 印刷するページ（全体でのページ位置と中身）。
    pub fn printed_pages(&self) -> impl Iterator<Item = (usize, &PrintPage)> {
        self.pages
            .iter()
            .enumerate()
            .skip(self.range.start)
            .take(self.range.len())
    }

    /// 本文の欄の開始桁（行番号の桁数 + 区切り 2 桁）。
    pub fn text_col(&self) -> usize {
        self.number_digits + 2
    }

    /// 行番号の欄の文字列（右寄せ。続きの行は空白）。
    pub fn number_label(&self, line: &PrintLine) -> String {
        match line.number {
            Some(n) => format!("{n:>width$}", width = self.number_digits),
            None => String::new(),
        }
    }

    /// ヘッダー右のページ番号（ページ番号を出さない設定なら None）。
    pub fn page_label(&self, page_idx: usize) -> Option<String> {
        self.setup
            .page_numbers
            .then(|| format!("{} / {}", page_idx + 1, self.pages.len()))
    }

    /// ヘッダーに置く文字列と左端の x（ポイント）。左にファイル名、中央に印刷日時、右にページ番号。
    /// 文字は [`HEADER_FONT_PT`] の升目で置く。
    pub fn header_items(&self, page_idx: usize) -> Vec<(f32, String)> {
        let (w, _) = self.setup.paper_pt();
        let m = self.setup.margin_pt();
        let cell = HEADER_FONT_PT * 0.5;
        let date_x = (w - str_cells(&self.printed_at) as f32 * cell) / 2.0;
        let title_max = (((date_x - m) / cell).floor() as usize).saturating_sub(2);
        let mut items = vec![
            (m, fit_cells_tail(&self.title, title_max)),
            (date_x, self.printed_at.clone()),
        ];
        if let Some(label) = self.page_label(page_idx) {
            items.push((w - m - str_cells(&label) as f32 * cell, label));
        }
        items
    }

    /// 本文の x（ポイント）: 行番号の左端と本文の左端。
    pub fn body_x(&self) -> (f32, f32) {
        let m = self.setup.margin_pt();
        (m, m + self.text_col() as f32 * self.setup.cell_pt())
    }
}

/// 升目 max に収まらなければ、末尾を残して先頭を「...」にする（パスはファイル名側を残す）。
pub fn fit_cells_tail(s: &str, max: usize) -> String {
    if str_cells(s) <= max {
        return s.to_string();
    }
    let budget = max.saturating_sub(3);
    let mut used = 0;
    let mut start = s.len();
    for (i, c) in s.char_indices().rev() {
        let w = char_cells(c);
        if used + w > budget {
            break;
        }
        used += w;
        start = i;
    }
    format!("...{}", &s[start..])
}

/// ファイルが大きすぎて印刷しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLarge;

/// 行を割り付ける。目安（[`PAGE_LIMIT`] ページ）を超えるなら `TooLarge`。
pub fn layout(
    lines: &[String],
    setup: PageSetup,
    title: &str,
    printed_at: &str,
) -> Result<PrintDoc, TooLarge> {
    let setup = setup.clamped();
    let rows = setup.body_rows();
    // 目安: 論理行数で判定する（折り返しで増えた分は上限まで許す）
    if lines.len() > rows * PAGE_LIMIT {
        return Err(TooLarge);
    }
    let number_digits = lines.len().max(1).to_string().len().max(MIN_NUMBER_DIGITS);
    let text_cols = setup.body_cols().saturating_sub(number_digits + 2).max(2);
    let max_rows = rows * PAGE_HARD_LIMIT;

    let mut printed: Vec<PrintLine> = Vec::with_capacity(lines.len());
    for (idx, line) in lines.iter().enumerate() {
        let expanded = expand_tabs_for_display(line);
        let mut number = Some(idx + 1);
        let mut cur = String::new();
        let mut used = 0;
        for c in expanded.chars() {
            let w = char_cells(c);
            if used + w > text_cols && used > 0 {
                printed.push(PrintLine {
                    number: number.take(),
                    text: std::mem::take(&mut cur),
                });
                used = 0;
                if printed.len() > max_rows {
                    return Err(TooLarge);
                }
            }
            cur.push(c);
            used += w;
        }
        printed.push(PrintLine { number, text: cur });
        if printed.len() > max_rows {
            return Err(TooLarge);
        }
    }

    let mut pages: Vec<PrintPage> = Vec::with_capacity(printed.len().div_ceil(rows));
    let mut iter = printed.into_iter().peekable();
    while iter.peek().is_some() {
        pages.push(PrintPage::Lines(iter.by_ref().take(rows).collect()));
    }
    if pages.is_empty() {
        pages.push(PrintPage::Lines(Vec::new()));
    }
    Ok(PrintDoc {
        setup,
        title: title.to_string(),
        printed_at: printed_at.to_string(),
        number_digits,
        range: 0..pages.len(),
        pages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn setup(orientation: Orientation, font_pt: u8) -> PageSetup {
        PageSetup {
            orientation,
            font_pt,
            page_numbers: true,
        }
    }

    #[test]
    fn test_page_geometry_10pt() {
        let p = setup(Orientation::Portrait, 10);
        assert_eq!((p.body_cols(), p.body_rows()), (110, 55));
        let l = setup(Orientation::Landscape, 10);
        assert_eq!((l.body_cols(), l.body_rows()), (159, 37));
        // 文字を小さくすると桁も行も増える
        let small = setup(Orientation::Portrait, 8);
        assert!(small.body_cols() > p.body_cols() && small.body_rows() > p.body_rows());
    }

    #[test]
    fn test_font_size_is_clamped() {
        let doc = layout(&lines(&["a"]), setup(Orientation::Portrait, 40), "t", "d").unwrap();
        assert_eq!(doc.setup.font_pt, MAX_FONT_PT);
        let doc = layout(&lines(&["a"]), setup(Orientation::Portrait, 1), "t", "d").unwrap();
        assert_eq!(doc.setup.font_pt, MIN_FONT_PT);
    }

    #[test]
    fn test_wrap_and_line_numbers() {
        let s = setup(Orientation::Portrait, 10);
        let text_cols = s.body_cols() - (MIN_NUMBER_DIGITS + 2);
        let long = "a".repeat(text_cols + 3);
        let doc = layout(&lines(&["x", &long, ""]), s, "t", "d").unwrap();
        let page = doc.pages[0].lines();
        assert_eq!(page.len(), 4);
        assert_eq!(page[1].number, Some(2));
        assert_eq!(page[1].text.len(), text_cols);
        assert_eq!(page[2].number, None);
        assert_eq!(page[2].text, "aaa");
        assert_eq!(page[3].number, Some(3));
        assert_eq!(doc.number_label(&page[0]), "  1");
        assert_eq!(doc.number_label(&page[2]), "");
    }

    #[test]
    fn test_wide_char_moves_to_next_line_at_edge() {
        let s = setup(Orientation::Portrait, 10);
        let text_cols = s.body_cols() - (MIN_NUMBER_DIGITS + 2);
        // 1 桁残った所に全角を置くと次の行へ送る
        let line = format!("{}あ", "a".repeat(text_cols - 1));
        let doc = layout(&lines(&[&line]), s, "t", "d").unwrap();
        let page = doc.pages[0].lines();
        assert_eq!(page.len(), 2);
        assert_eq!(str_cells(&page[0].text), text_cols - 1);
        assert_eq!(page[1].text, "あ");
    }

    #[test]
    fn test_tabs_are_expanded() {
        let doc = layout(&lines(&["\tx"]), PageSetup::default(), "t", "d").unwrap();
        assert_eq!(doc.pages[0].lines()[0].text, "    x");
    }

    #[test]
    fn test_pagination_and_page_label() {
        let s = setup(Orientation::Landscape, 10);
        let rows = s.body_rows();
        let v: Vec<String> = (0..rows * 2 + 1).map(|i| i.to_string()).collect();
        let doc = layout(&v, s, "t", "d").unwrap();
        assert_eq!(doc.pages.len(), 3);
        assert_eq!(doc.pages[2].lines().len(), 1);
        assert_eq!(doc.page_label(1).as_deref(), Some("2 / 3"));
        let no_numbers = PageSetup {
            page_numbers: false,
            ..s
        };
        let doc = layout(&v, no_numbers, "t", "d").unwrap();
        assert_eq!(doc.page_label(0), None);
    }

    #[test]
    fn test_header_items() {
        let s = PageSetup::default();
        let v = lines(&["a"]);
        let doc = layout(&v, s, "C:\\work\\メモ.txt", "2026-11-27 12:00").unwrap();
        let items = doc.header_items(0);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].1, "C:\\work\\メモ.txt");
        assert_eq!(items[2].1, "1 / 1");
        // 左 < 中央 < 右
        assert!(items[0].0 < items[1].0 && items[1].0 < items[2].0);
        let doc = layout(
            &v,
            PageSetup {
                page_numbers: false,
                ..s
            },
            "t",
            "d",
        )
        .unwrap();
        assert_eq!(doc.header_items(0).len(), 2);
    }

    #[test]
    fn test_from_items() {
        let s = PageSetup::default();
        let doc = PrintDoc::from_items(s, "t", "d", Vec::new()).unwrap();
        assert_eq!(doc.pages.len(), 1);
        assert!(doc.pages[0].lines().is_empty());
        let over = vec![Vec::new(); PAGE_LIMIT + 1];
        assert_eq!(
            PrintDoc::from_items(s, "t", "d", over).unwrap_err(),
            TooLarge
        );
        // 本文の欄は余白の内側
        assert!(s.body_bottom_pt() > s.body_top_pt());
        assert!(s.body_width_pt() < s.paper_pt().0);
    }

    #[test]
    fn test_fit_cells_tail() {
        assert_eq!(fit_cells_tail("abc", 3), "abc");
        assert_eq!(fit_cells_tail("abcdefgh", 6), "...fgh");
        // 全角は 2 桁で数える
        assert_eq!(fit_cells_tail("あいうえお", 7), "...えお");
    }

    #[test]
    fn test_page_range() {
        let s = PageSetup::default();
        let v: Vec<String> = vec![String::new(); s.body_rows() * 4];
        let doc = layout(&v, s, "t", "d").unwrap();
        assert_eq!(doc.printed_pages().count(), 4);
        let doc = doc.with_page_range(2, 3);
        let idx: Vec<usize> = doc.printed_pages().map(|(i, _)| i).collect();
        assert_eq!(idx, [1, 2]);
        // ページ番号は全体の番号のまま
        assert_eq!(doc.page_label(1).as_deref(), Some("2 / 4"));
        // 範囲外は 1〜総ページ数に収める
        let doc = doc.with_page_range(0, 99);
        assert_eq!(doc.range, 0..4);
        let doc = doc.with_page_range(5, 2);
        assert_eq!(doc.range, 1..2);
    }

    #[test]
    fn test_empty_document_has_one_page() {
        let doc = layout(&[], PageSetup::default(), "t", "d").unwrap();
        assert_eq!(doc.pages.len(), 1);
    }

    #[test]
    fn test_too_large() {
        let s = PageSetup::default();
        let rows = s.body_rows();
        let ok: Vec<String> = vec![String::new(); rows * PAGE_LIMIT];
        assert_eq!(layout(&ok, s, "t", "d").unwrap().pages.len(), PAGE_LIMIT);
        let over: Vec<String> = vec![String::new(); rows * PAGE_LIMIT + 1];
        assert_eq!(layout(&over, s, "t", "d").unwrap_err(), TooLarge);
        // 1 行が極端に長いと折り返しの上限で止める
        let huge_line = vec!["a".repeat(s.body_cols() * rows * PAGE_HARD_LIMIT)];
        assert_eq!(layout(&huge_line, s, "t", "d").unwrap_err(), TooLarge);
    }
}
