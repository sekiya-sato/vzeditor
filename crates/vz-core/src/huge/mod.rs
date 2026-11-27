//! 巨大ファイルモードの文書（128MB 以上）。
//!
//! 元ファイルは mmap（読み取り専用）、追加した文字は 1MB 単位の追加ブロック（追記だけ）に置き、
//! 文書をその 2 つを指す「ピース」の並びで表す。並びは B-Tree（`tree::Tree`）で持つ。
//! 変更は vis の Change / Revision 方式で記録し、Undo / Redo はピース列の入れ替えで行う。
//! 設計は docs/巨大ファイル詳細設計.md の 5 章。

mod edits;
pub mod index;
pub mod search;
mod snapshot;
pub mod tree;

use crate::encoding::{TextEncoding, DETECT_SAMPLE};
use index::{IndexJob, LineIndex, INDEX_CHUNK};
use memchr::memchr_iter;
use memmap2::Mmap;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tree::{Piece, Src, Tree};

pub use snapshot::{save_by_rename, DocSnapshot};

/// これ以上の大きさのファイルは巨大ファイルモードで開く
pub const HUGE_FILE_THRESHOLD: u64 = 128 << 20;
/// 追加ブロックの大きさ（vis の BLOCK_SIZE と同じ 1MB）。確保した後は伸ばし直さない
const BLOCK_SIZE: usize = 1 << 20;
/// B-Tree の節の子の数の上限
const NODE_MAX: usize = 32;
/// Undo の回数（通常ファイルモードと同じ）
const MAX_UNDO: usize = 200;
/// mmap できないとき、全体をメモリに読み込んで代わりにする上限
const HEAP_FALLBACK_LIMIT: u64 = 2 << 30;
/// 長い行で、文字位置→バイト位置の目印を何文字ごとに取るか
const COL_STRIDE: usize = 4096;
/// 文字位置の目印を覚えておく行の数
const COL_CACHE_LINES: usize = 16;

/// 編集できなかった理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// 索引がまだその位置まで届いていない
    Indexing,
    /// 文字コードで表せない文字
    Unencodable(char),
    /// 保存中・すべて置換の書き出し中
    Busy,
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Indexing => f.write_str("索引作成中のため、まだこの位置は編集できません"),
            Self::Unencodable(c) => write!(
                f,
                "「{c}」はこのファイルの文字コードで表せません（ファイル→文字コードを指定して保存で UTF-8 などに変えてください）"
            ),
            Self::Busy => f.write_str("保存中のため編集できません"),
        }
    }
}

/// ファイルの中身: mmap で対応付けたもの、または（mmap できないとき）メモリに読み込んだもの。
pub enum Backing {
    Mapped(Mmap),
    Heap(Vec<u8>),
}

impl std::ops::Deref for Backing {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Mapped(m) => m,
            Self::Heap(v) => v,
        }
    }
}

impl Backing {
    /// ファイルを読み取り専用で対応付ける。mmap できなければ、上限以下ならメモリに読み込む。
    pub fn open(file: &File, path: &Path) -> io::Result<Self> {
        // SAFETY: 読み取り専用で対応付ける。ほかのプロセスが対応付け中のファイルを切り詰めると
        // 読み取りで異常終了しうる（Windows は切り詰めを拒む）。外部での変更は UI 側で検知する。
        match unsafe { Mmap::map(file) } {
            Ok(map) => Ok(Self::Mapped(map)),
            Err(e) => Self::read_whole(file, path, e),
        }
    }

    fn read_whole(file: &File, path: &Path, map_error: io::Error) -> io::Result<Self> {
        let len = file.metadata()?.len();
        if len > HEAP_FALLBACK_LIMIT {
            return Err(io::Error::other(format!(
                "ファイルを対応付けできず（{map_error}）、大きすぎてメモリにも読み込めません"
            )));
        }
        Ok(Self::Heap(std::fs::read(path)?))
    }

    /// メモリに読み込んだか（mmap できなかったか）。
    pub fn is_heap(&self) -> bool {
        matches!(self, Self::Heap(_))
    }

    /// 先頭から順に読む間だけ、OS に先読みを頼む（Linux / macOS）。
    /// WSL2 での実測（2GB・キャッシュなし）: 順に読む速さは約 15% 上がった。
    /// Random にすると順に読むのが約 70 倍遅くなり、とびとびの読み出しも速くならなかったので使わない。
    #[cfg(unix)]
    pub(crate) fn advise_sequential(&self, on: bool) {
        if let Self::Mapped(m) = self {
            let advice = if on {
                memmap2::Advice::Sequential
            } else {
                memmap2::Advice::Normal
            };
            let _ = m.advise(advice);
        }
    }

    #[cfg(not(unix))]
    pub(crate) fn advise_sequential(&self, _on: bool) {}
}

/// ファイルの対応付けと、その行索引。
struct Source {
    map: Arc<Backing>,
    path: PathBuf,
    /// すべて置換の作業ファイルなら true（使われなくなったら消す）
    work_file: bool,
    /// 本文の開始位置（BOM の分）
    base: u64,
    index: LineIndex,
    job: Option<IndexJob>,
}

/// vis の TextChange: 文書内の位置 pos から始まる old のピース列を new に置き換えた記録。
struct Change {
    pos: u64,
    old: Vec<Piece>,
    new: Vec<Piece>,
}

/// 直前の挿入（続けて入力したらピースを伸ばす。vis の cache_insert）。
#[derive(Clone, Copy)]
struct InsertCache {
    piece: u64,
    change: usize,
    slot: usize,
    end: u64,
}

pub struct HugeDoc {
    sources: Vec<Option<Source>>,
    blocks: Vec<Vec<u8>>,
    tree: Tree,
    pub encoding: TextEncoding,
    pub line_ending: &'static str,
    current: Vec<Change>,
    /// 閉じた変更のまとまり（番号付き。番号で保存した時点を覚える）
    undo: VecDeque<(u64, Vec<Change>)>,
    redo: Vec<(u64, Vec<Change>)>,
    next_rev: u64,
    /// 保存した（開いた）時点のまとまりの番号
    saved_rev: u64,
    last_insert: Option<InsertCache>,
    /// 保存中・すべて置換の書き出し中（編集できない）
    busy: bool,
    /// 長い行の文字位置の目印: (行頭の文書内位置, marks[k] = k*COL_STRIDE 文字目の行内バイト位置)
    col_cache: RefCell<VecDeque<(u64, Vec<u64>)>>,
    /// 開いた時点のファイルの状態（更新日時, サイズ）。未保存の編集の一時保存で、元ファイルが同じかを確かめる
    opened_stamp: Option<(std::time::SystemTime, u64)>,
}

impl HugeDoc {
    pub fn open(path: &Path) -> io::Result<Self> {
        Self::open_with_chunk(path, INDEX_CHUNK)
    }

    /// 索引のチャンクの大きさを指定して開く（テストで小さくする）。
    pub fn open_with_chunk(path: &Path, chunk: u64) -> io::Result<Self> {
        Self::open_inner(path, Some(chunk))
    }

    /// chunk が None なら索引スレッドを起動しない（テストで索引を手で伸ばす）。
    fn open_inner(path: &Path, chunk: Option<u64>) -> io::Result<Self> {
        let file = File::open(path)?;
        if file.metadata()?.len() == 0 {
            return Err(io::Error::other(
                "空のファイルは巨大ファイルモードで開けません",
            ));
        }
        let map = Arc::new(Backing::open(&file, path)?);
        Self::from_backing(path, map, chunk)
    }

    /// 対応付け（またはメモリに読み込んだ中身）から文書を作る。
    fn from_backing(path: &Path, map: Arc<Backing>, chunk: Option<u64>) -> io::Result<Self> {
        let opened_stamp = edits::disk_stamp(path);
        let head = &map[..map.len().min(DETECT_SAMPLE)];
        let encoding = TextEncoding::detect(head, map.len() > DETECT_SAMPLE);
        let line_ending = if memchr::memmem::find(head, b"\r\n").is_some() {
            "\r\n"
        } else {
            "\n"
        };
        let base = encoding.bom().len() as u64;
        let len = map.len() as u64;
        let job = match chunk {
            Some(chunk) => Some(IndexJob::spawn(Arc::clone(&map), base, chunk)?),
            None => None,
        };
        let mut tree = Tree::new(NODE_MAX);
        if len > base {
            tree.insert(
                0,
                Piece {
                    src: Src::File(0),
                    start: base,
                    len: len - base,
                    nl: 0,
                    pending: true,
                },
            );
        }
        Ok(Self {
            sources: vec![Some(Source {
                map,
                path: path.to_path_buf(),
                work_file: false,
                base,
                index: LineIndex::default(),
                job,
            })],
            blocks: Vec::new(),
            tree,
            encoding,
            line_ending,
            current: Vec::new(),
            undo: VecDeque::new(),
            redo: Vec::new(),
            next_rev: 1,
            saved_rev: 0,
            last_insert: None,
            busy: false,
            opened_stamp,
            col_cache: RefCell::new(VecDeque::new()),
        })
    }

    // ---- 行索引 ----

    /// 索引スレッドから届いた分を取り込む。改行数が確定したピースがあれば true。
    pub fn poll_index(&mut self) -> bool {
        let mut grew = false;
        for src in self.sources.iter_mut().flatten() {
            if let Some(job) = &src.job {
                grew |= job.poll(&mut src.index);
                if src.index.is_complete() {
                    src.job = None;
                }
            }
        }
        if grew {
            self.resolve_pending();
        }
        grew
    }

    /// 索引が完成するまで待つ（保存して終了・テスト用）。
    pub fn wait_index(&mut self) {
        for src in self.sources.iter_mut().flatten() {
            if let Some(job) = src.job.take() {
                job.wait(&mut src.index);
            }
        }
        self.resolve_pending();
    }

    /// 索引スレッドを使わずに開く（テストで索引を手で伸ばす）。
    #[cfg(test)]
    fn open_manual(path: &Path) -> io::Result<Self> {
        Self::open_inner(path, None)
    }

    /// 開いたファイルの索引を、先頭から max_chunks 個のチャンクまで作り直す（テスト用）。
    #[cfg(test)]
    fn feed_index(&mut self, chunk: u64, max_chunks: usize) {
        if let Some(s) = self.sources[0].as_mut() {
            s.index = LineIndex::build_partial(&s.map, s.base, chunk, max_chunks);
        }
        self.resolve_pending();
    }

    /// 開いたファイルを mmap できず、メモリに読み込んだか。
    pub fn is_heap_backed(&self) -> bool {
        self.sources[0].as_ref().is_some_and(|s| s.map.is_heap())
    }

    /// mmap できなかったときと同じく、メモリに読み込んで開く（テスト用）。
    #[cfg(test)]
    fn open_heap_for_test(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let map = Arc::new(Backing::read_whole(&file, path, io::Error::other("test"))?);
        Self::from_backing(path, map, Some(INDEX_CHUNK))
    }

    pub fn is_indexing(&self) -> bool {
        self.sources
            .iter()
            .flatten()
            .any(|s| !s.index.is_complete())
    }

    /// 開いたファイルの索引の進み具合（索引ができたバイト数, 全体のバイト数）。
    pub fn index_progress(&self) -> (u64, u64) {
        self.sources[0]
            .as_ref()
            .map_or((0, 0), |s| (s.index.indexed_end(), s.map.len() as u64))
    }

    /// 索引が届いた `pending` のピースに改行数を入れる。
    fn resolve_pending(&mut self) {
        for idx in self.tree.pending_indices() {
            let mut p = self.tree.get(idx);
            if let Some(nl) = self.file_newlines(&p, 0, p.len) {
                p.nl = nl;
                p.pending = false;
                self.tree.set(idx, p);
            }
        }
    }

    fn source(&self, i: u16) -> &Source {
        self.sources[i as usize]
            .as_ref()
            .expect("ピースが片付けた対応付けを指している")
    }

    /// ファイルのピース p の [from, to) の改行数。索引の範囲外なら None。
    fn file_newlines(&self, p: &Piece, from: u64, to: u64) -> Option<u64> {
        let Src::File(i) = p.src else {
            return Some(
                memchr_iter(b'\n', &self.bytes(p)[from as usize..to as usize]).count() as u64,
            );
        };
        let s = self.source(i);
        s.index
            .newlines_in(&s.map, s.base, p.start + from, p.start + to)
    }

    fn bytes(&self, p: &Piece) -> &[u8] {
        let (s, e) = (p.start as usize, (p.start + p.len) as usize);
        match p.src {
            Src::File(i) => &self.source(i).map[s..e],
            Src::Add(b) => &self.blocks[b as usize][s..e],
        }
    }

    // ---- 大きさ・行 ----

    pub fn len(&self) -> u64 {
        self.tree.total().bytes
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn piece_count(&self) -> u64 {
        self.tree.total().n
    }

    /// 位置が確定している行の数（索引の作成中は、確定した改行の数まで）。
    pub fn line_count(&self) -> usize {
        match self.known_newlines() {
            None => self.tree.total().nl as usize + 1,
            Some(known) => (known as usize).max(1),
        }
    }

    /// 索引の作成中なら、確定している改行の数。完成していれば None。
    fn known_newlines(&self) -> Option<u64> {
        let hit = self.tree.first_pending()?;
        let p = hit.piece?;
        let Src::File(i) = p.src else {
            return Some(hit.nl_before);
        };
        let s = self.source(i);
        let end = s.index.indexed_end().min(p.start + p.len);
        let within = if end > p.start {
            s.index
                .newlines_in(&s.map, s.base, p.start, end)
                .unwrap_or(0)
        } else {
            0
        };
        Some(hit.nl_before + within)
    }

    /// スクロールバー用の総行数（索引の作成中は推定値）。
    pub fn estimated_line_count(&self) -> usize {
        let Some(known) = self.known_newlines() else {
            return self.line_count();
        };
        let (done, total) = self.index_progress();
        if done == 0 || known == 0 {
            return self.line_count();
        }
        let per_line = done as f64 / known as f64;
        (known as f64 + (total - done) as f64 / per_line) as usize
    }

    /// 行 line の行頭の文書内位置。まだ分からなければ None。
    pub fn line_start(&self, line: usize) -> Option<u64> {
        if line == 0 {
            return Some(0);
        }
        let (hit, k) = self.tree.locate_newline(line as u64)?;
        let p = hit.piece?;
        let within = match p.src {
            Src::File(i) => {
                let s = self.source(i);
                let before = s.index.newlines_in(&s.map, s.base, s.base, p.start)?;
                let pos = s.index.nth_line_start(&s.map, before, k)?;
                if pos > p.start + p.len {
                    return None;
                }
                pos - p.start
            }
            Src::Add(_) => memchr_iter(b'\n', self.bytes(&p)).nth(k as usize - 1)? as u64 + 1,
        };
        Some(hit.start + within)
    }

    /// 行の範囲（改行と行末の \r を除く文書内の位置）。
    pub fn line_range(&self, line: usize) -> Option<(u64, u64)> {
        let s = self.line_start(line)?;
        let mut e = match self.line_start(line + 1) {
            Some(n) => n - 1,
            None if self.known_newlines().is_none() => self.len(),
            // 索引の作成中で行末がまだ分からない: 索引ができた所まで
            None => {
                let (done, _) = self.index_progress();
                done.min(self.len()).max(s)
            }
        };
        if e > s && self.read(e - 1, 1).as_ref() == b"\r" {
            e -= 1;
        }
        Some((s, e))
    }

    /// [off, off+len) を読む。1 つのピースの中なら写さずに返す。
    pub fn read(&self, off: u64, len: u64) -> Cow<'_, [u8]> {
        let hit = self.tree.locate_offset(off);
        let Some(p) = hit.piece else {
            return Cow::Borrowed(&[]);
        };
        let skip = off - hit.start;
        if skip + len <= p.len {
            return Cow::Borrowed(&self.bytes(&p)[skip as usize..(skip + len) as usize]);
        }
        let mut out = Vec::with_capacity(len as usize);
        let (mut idx, mut skip) = (hit.idx, skip);
        let total = self.piece_count();
        while (out.len() as u64) < len && idx < total {
            let p = self.tree.get(idx);
            let b = &self.bytes(&p)[skip as usize..];
            let take = b.len().min((len - out.len() as u64) as usize);
            out.extend_from_slice(&b[..take]);
            skip = 0;
            idx += 1;
        }
        Cow::Owned(out)
    }

    /// 行のバイト列（改行を除く）。
    pub fn line_bytes(&self, line: usize) -> Cow<'_, [u8]> {
        match self.line_range(line) {
            Some((s, e)) => self.read(s, e - s),
            None => Cow::Borrowed(&[]),
        }
    }

    /// 行の文字列。
    pub fn line_text(&self, line: usize) -> Cow<'_, str> {
        match self.line_bytes(line) {
            Cow::Borrowed(b) => self.encoding.decode_body(b),
            Cow::Owned(b) => Cow::Owned(self.encoding.decode_body(&b).into_owned()),
        }
    }

    pub fn line_char_len(&self, line: usize) -> usize {
        count_chars(self.encoding, &self.line_bytes(line))
    }

    /// 行の start_col 文字目から最大 max_cols 文字（表示用）。長い行は文字位置の目印から数える。
    pub fn line_segment(&self, line: usize, start_col: usize, max_cols: usize) -> String {
        let Some((s, e)) = self.line_range(line) else {
            return String::new();
        };
        let from = s + self.col_to_byte_in_line(s, e, start_col);
        // 1 文字は最大 4 バイト
        let take = (max_cols as u64 * 4).min(e.saturating_sub(from));
        let bytes = self.read(from, take);
        let end = byte_of_char(self.encoding, &bytes, max_cols);
        self.encoding.decode_body(&bytes[..end]).into_owned()
    }

    /// 行内の col 文字目のバイト位置（行頭からの位置）。
    fn col_to_byte_in_line(&self, s: u64, e: u64, col: usize) -> u64 {
        let len = e - s;
        if col == 0 {
            return 0;
        }
        if len < (COL_STRIDE * 4) as u64 {
            return byte_of_char(self.encoding, &self.read(s, len), col) as u64;
        }
        // 長い行: 目印から数える
        let mut cache = self.col_cache.borrow_mut();
        let pos = cache.iter().position(|(start, _)| *start == s);
        let mut marks = match pos {
            Some(i) => cache.remove(i).map(|(_, m)| m).unwrap_or_default(),
            None => vec![0],
        };
        let k = col / COL_STRIDE;
        while marks.len() <= k {
            let last = *marks.last().unwrap_or(&0);
            if last >= len {
                break;
            }
            let chunk = self.read(s + last, ((COL_STRIDE * 4) as u64).min(len - last));
            let step = byte_of_char(self.encoding, &chunk, COL_STRIDE) as u64;
            if step == 0 {
                break;
            }
            marks.push(last + step);
        }
        let k = k.min(marks.len() - 1);
        let base = marks[k];
        let rest = col - k * COL_STRIDE;
        let chunk = self.read(s + base, ((rest * 4) as u64).min(len - base));
        let off = base + byte_of_char(self.encoding, &chunk, rest) as u64;
        cache.push_front((s, marks));
        cache.truncate(COL_CACHE_LINES);
        off
    }

    /// (行, 文字位置) → 文書内の位置。行がまだ分からなければ None。
    pub fn pos_to_offset(&self, line: usize, col: usize) -> Option<u64> {
        let (s, e) = self.line_range(line)?;
        Some(s + self.col_to_byte_in_line(s, e, col).min(e - s))
    }

    /// 文書内の位置 → (行, 文字位置)。まだ分からなければ None。
    pub fn offset_to_pos(&self, off: u64) -> Option<(usize, usize)> {
        let off = off.min(self.len());
        let hit = self.tree.locate_offset(off);
        if hit.pending_before {
            return None;
        }
        let within = match hit.piece {
            Some(p) => self.file_newlines(&p, 0, off - hit.start)?,
            None => 0,
        };
        let line = (hit.nl_before + within) as usize;
        let s = self.line_start(line)?;
        let col = count_chars(self.encoding, &self.read(s, off - s));
        Some((line, col))
    }

    /// [start, end) の文字列（改行は \n にそろえる）。limit バイトを超えたら None。
    pub fn text_between(&self, start: u64, end: u64, limit: u64) -> Option<String> {
        if end.saturating_sub(start) > limit {
            return None;
        }
        let bytes = self.read(start, end.saturating_sub(start));
        let text = self.encoding.decode_body(&bytes);
        Some(if text.contains('\r') {
            text.replace("\r\n", "\n")
        } else {
            text.into_owned()
        })
    }

    // ---- 編集 ----

    /// 保存中などで編集できないようにする。
    pub fn set_busy(&mut self, busy: bool) {
        self.busy = busy;
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// 位置が確定していなければ Indexing、保存中なら Busy。
    fn check_resolved(&self, off: u64) -> Result<(), EditError> {
        if self.busy {
            return Err(EditError::Busy);
        }
        self.offset_to_pos(off)
            .map(|_| ())
            .ok_or(EditError::Indexing)
    }

    /// 文字列をファイルの文字コードと改行コードのバイト列にする。
    fn encode_text(&self, text: &str) -> Result<Vec<u8>, EditError> {
        let text = if self.line_ending == "\r\n" && text.contains('\n') {
            Cow::Owned(text.replace("\r\n", "\n").replace('\n', "\r\n"))
        } else {
            Cow::Borrowed(text)
        };
        self.encoding
            .encode(&text)
            .map(Cow::into_owned)
            .map_err(|e| EditError::Unencodable(e.ch))
    }

    fn store(&mut self, bytes: &[u8]) -> (u32, u64) {
        let fits = self
            .blocks
            .last()
            .is_some_and(|b| b.capacity() - b.len() >= bytes.len());
        if !fits {
            self.blocks
                .push(Vec::with_capacity(bytes.len().max(BLOCK_SIZE)));
        }
        let bi = self.blocks.len() - 1;
        let b = &mut self.blocks[bi];
        let start = b.len() as u64;
        b.extend_from_slice(bytes);
        (bi as u32, start)
    }

    /// ピース p の [from, to) を切り出す。改行数は索引で求め、範囲外にかかれば pending にする。
    fn slice(&self, p: &Piece, from: u64, to: u64) -> Piece {
        let nl = if p.pending {
            self.file_newlines(p, from, to)
        } else if from == 0 && to == p.len {
            Some(p.nl)
        } else {
            self.file_newlines(p, from, to)
        };
        Piece {
            src: p.src,
            start: p.start + from,
            len: to - from,
            nl: nl.unwrap_or(0),
            pending: nl.is_none(),
        }
    }

    /// 文書内の位置 off に text を挿入する。挿入したバイト数を返す。
    pub fn insert(&mut self, off: u64, text: &str) -> Result<u64, EditError> {
        if text.is_empty() {
            return Ok(0);
        }
        self.check_resolved(off)?;
        let bytes = self.encode_text(text)?;
        self.insert_bytes(off, &bytes);
        Ok(bytes.len() as u64)
    }

    /// text を挿入できるか（保存中でなく、文字コードで表せるか）。
    pub fn check_encodable(&self, text: &str) -> Result<(), EditError> {
        if self.busy {
            return Err(EditError::Busy);
        }
        self.encode_text(text).map(|_| ())
    }

    fn insert_bytes(&mut self, off: u64, bytes: &[u8]) {
        self.col_cache.borrow_mut().clear();
        let nl = memchr_iter(b'\n', bytes).count() as u64;
        // 続けて入力: 直前の挿入ピースがブロックの末尾で終わっていれば伸ばす
        if let Some(c) = self.last_insert {
            let mut p = self.tree.get(c.piece);
            if let Src::Add(bi) = p.src {
                let block = &mut self.blocks[bi as usize];
                if c.end == off
                    && p.start + p.len == block.len() as u64
                    && block.capacity() - block.len() >= bytes.len()
                {
                    block.extend_from_slice(bytes);
                    p.len += bytes.len() as u64;
                    p.nl += nl;
                    self.tree.set(c.piece, p);
                    self.current[c.change].new[c.slot] = p;
                    self.last_insert = Some(InsertCache {
                        end: c.end + bytes.len() as u64,
                        ..c
                    });
                    return;
                }
            }
        }
        let (bi, start) = self.store(bytes);
        let q = Piece {
            src: Src::Add(bi),
            start,
            len: bytes.len() as u64,
            nl,
            pending: false,
        };
        let hit = self.tree.locate_offset(off);
        let change = self.current.len();
        match hit.piece {
            Some(p) if off > hit.start => {
                let d = off - hit.start;
                let (left, right) = (self.slice(&p, 0, d), self.slice(&p, d, p.len));
                self.tree.set(hit.idx, left);
                self.tree.insert(hit.idx + 1, q);
                self.tree.insert(hit.idx + 2, right);
                self.current.push(Change {
                    pos: hit.start,
                    old: vec![p],
                    new: vec![left, q, right],
                });
                self.last_insert = Some(InsertCache {
                    piece: hit.idx + 1,
                    change,
                    slot: 1,
                    end: off + q.len,
                });
            }
            _ => {
                self.tree.insert(hit.idx, q);
                self.current.push(Change {
                    pos: off,
                    old: Vec::new(),
                    new: vec![q],
                });
                self.last_insert = Some(InsertCache {
                    piece: hit.idx,
                    change,
                    slot: 0,
                    end: off + q.len,
                });
            }
        }
    }

    /// [start, end) を削除する。
    pub fn delete(&mut self, start: u64, end: u64) -> Result<(), EditError> {
        let end = end.min(self.len());
        if start >= end {
            return Ok(());
        }
        self.check_resolved(start)?;
        self.check_resolved(end)?;
        self.col_cache.borrow_mut().clear();
        self.last_insert = None;
        let hit = self.tree.locate_offset(start);
        let mut old = Vec::new();
        let (mut idx, mut piece_start) = (hit.idx, hit.start);
        loop {
            let p = self.tree.get(idx);
            old.push(p);
            if piece_start + p.len >= end {
                break;
            }
            piece_start += p.len;
            idx += 1;
        }
        let (first, last) = (old[0], old[old.len() - 1]);
        let mut new = Vec::new();
        if start > hit.start {
            new.push(self.slice(&first, 0, start - hit.start));
        }
        if end < piece_start + last.len {
            new.push(self.slice(&last, end - piece_start, last.len));
        }
        self.replace_run(hit.idx, old.len(), &new);
        self.current.push(Change {
            pos: hit.start,
            old,
            new,
        });
        Ok(())
    }

    /// vis の span_swap に当たる: idx から count 個のピースを pieces に置き換える。
    fn replace_run(&mut self, idx: u64, count: usize, pieces: &[Piece]) {
        for _ in 0..count {
            self.tree.remove(idx);
        }
        for (i, p) in pieces.iter().enumerate() {
            self.tree.insert(idx + i as u64, *p);
        }
    }

    /// vis の text_snapshot: 開いている変更を 1 つの Undo 単位として閉じる。閉じたら true。
    pub fn snapshot(&mut self) -> bool {
        self.last_insert = None;
        if self.current.is_empty() {
            return false;
        }
        let id = self.next_rev;
        self.next_rev += 1;
        self.undo.push_back((id, std::mem::take(&mut self.current)));
        if self.undo.len() > MAX_UNDO {
            self.undo.pop_front();
        }
        self.redo.clear();
        self.release_unused_sources();
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.current.is_empty() || !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        self.current.is_empty() && !self.redo.is_empty()
    }

    /// 元に戻す。戻した変更の位置を返す。
    pub fn undo(&mut self) -> Option<u64> {
        if self.busy {
            return None;
        }
        self.snapshot();
        let (id, rev) = self.undo.pop_back()?;
        self.col_cache.borrow_mut().clear();
        for c in rev.iter().rev() {
            let hit = self.tree.locate_offset(c.pos);
            debug_assert_eq!(hit.start, c.pos);
            self.replace_run(hit.idx, c.new.len(), &c.old);
        }
        let pos = rev.first().map(|c| c.pos);
        self.redo.push((id, rev));
        self.resolve_pending();
        pos
    }

    /// やり直す。やり直した変更の末尾の位置を返す。
    pub fn redo(&mut self) -> Option<u64> {
        if self.busy {
            return None;
        }
        self.snapshot();
        let (id, rev) = self.redo.pop()?;
        self.col_cache.borrow_mut().clear();
        for c in &rev {
            let hit = self.tree.locate_offset(c.pos);
            debug_assert_eq!(hit.start, c.pos);
            self.replace_run(hit.idx, c.old.len(), &c.new);
        }
        let pos = rev
            .last()
            .map(|c| c.pos + c.new.iter().map(|p| p.len).sum::<u64>());
        self.undo.push_back((id, rev));
        self.resolve_pending();
        pos
    }

    fn top_rev(&self) -> u64 {
        if !self.current.is_empty() {
            return u64::MAX;
        }
        self.undo.back().map_or(0, |(id, _)| *id)
    }

    /// 保存した（開いた）時点から変わっているか。
    pub fn is_modified(&self) -> bool {
        self.top_rev() != self.saved_rev
    }

    // ---- 保存 ----

    /// 裏のスレッドへ渡す写し（ピース列・対応付け・追加した文字）。
    pub fn doc_snapshot(&self) -> DocSnapshot {
        let mut add = Vec::new();
        let pieces = self
            .tree
            .pieces()
            .into_iter()
            .map(|p| match p.src {
                Src::File(_) => p,
                Src::Add(_) => {
                    let start = add.len() as u64;
                    add.extend_from_slice(self.bytes(&p));
                    Piece {
                        src: Src::Add(0),
                        start,
                        ..p
                    }
                }
            })
            .collect();
        DocSnapshot::new(
            self.sources
                .iter()
                .map(|s| s.as_ref().map(|s| Arc::clone(&s.map)))
                .collect(),
            add,
            pieces,
            self.encoding,
            self.line_ending,
        )
    }

    /// 開いたファイルのパス。
    pub fn path(&self) -> Option<&Path> {
        self.sources[0].as_ref().map(|s| s.path.as_path())
    }

    /// 保存できたら呼ぶ（以後の保存の文字コード）。ピースと Undo の履歴はそのまま残す。
    /// `snapshot_rev` は保存を始めたときの `current_rev()`（保存中に編集されていなければ今と同じ）。
    pub fn saved_as(&mut self, encoding: TextEncoding, snapshot_rev: u64) {
        self.encoding = encoding;
        self.saved_rev = snapshot_rev;
    }

    /// 今の内容を表すまとまりの番号（開いている変更は閉じる）。保存の開始時に覚えておく。
    pub fn current_rev(&mut self) -> u64 {
        self.snapshot();
        self.top_rev()
    }

    /// すべて置換の結果（作業ファイル。本文だけで BOM なし）を文書の中身にする。Undo 1 回で置換前に戻る。
    pub fn adopt_work_file(&mut self, path: &Path) -> io::Result<()> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        let mut new = Vec::new();
        if len > 0 {
            let map = Arc::new(Backing::open(&file, path)?);
            let job = IndexJob::spawn(Arc::clone(&map), 0, INDEX_CHUNK)?;
            let id = u16::try_from(self.sources.len())
                .map_err(|_| io::Error::other("すべて置換の回数が多すぎます"))?;
            self.sources.push(Some(Source {
                map,
                path: path.to_path_buf(),
                work_file: true,
                base: 0,
                index: LineIndex::default(),
                job: Some(job),
            }));
            new.push(Piece {
                src: Src::File(id),
                start: 0,
                len,
                nl: 0,
                pending: true,
            });
        } else {
            let _ = std::fs::remove_file(path);
        }
        self.snapshot();
        self.col_cache.borrow_mut().clear();
        let old = self.tree.pieces();
        self.replace_run(0, old.len(), &new);
        self.current.push(Change { pos: 0, old, new });
        self.snapshot();
        Ok(())
    }

    /// 文書が「すべて置換の作業ファイル全体を指す 1 個のピース」だけなら、その作業ファイルのパス。
    /// このときは作業ファイルを rename するだけで保存できる（BOM と文字コードの変換が要らなければ）。
    pub fn whole_work_file(&self) -> Option<PathBuf> {
        if self.tree.total().n != 1 {
            return None;
        }
        let p = self.tree.get(0);
        let Src::File(i) = p.src else {
            return None;
        };
        let s = self.sources[i as usize].as_ref()?;
        (s.work_file && p.start == 0 && p.len == s.map.len() as u64).then(|| s.path.clone())
    }

    /// 作業ファイルを rename で保存した後: その対応付けを保存先の元ファイルとして扱う（消さない）。
    pub fn promote_work_file(&mut self, work: &Path, saved_to: &Path) {
        for s in self.sources.iter_mut().flatten() {
            if s.work_file && s.path == work {
                s.work_file = false;
                s.path = saved_to.to_path_buf();
            }
        }
    }

    /// どのピース（木と履歴）からも指されなくなった作業ファイルの対応付けを外して消す。
    fn release_unused_sources(&mut self) {
        if !self.sources.iter().flatten().any(|s| s.work_file) {
            return;
        }
        let mut used = vec![false; self.sources.len()];
        let mut mark = |p: &Piece| {
            if let Src::File(i) = p.src {
                used[i as usize] = true;
            }
        };
        self.tree.pieces().iter().for_each(&mut mark);
        let history = self
            .undo
            .iter()
            .chain(self.redo.iter())
            .flat_map(|(_, rev)| rev.iter())
            .chain(self.current.iter());
        for c in history {
            c.old.iter().chain(c.new.iter()).for_each(&mut mark);
        }
        for (i, src) in self.sources.iter_mut().enumerate() {
            if used[i] {
                continue;
            }
            if let Some(s) = src.take_if(|s| s.work_file) {
                let path = s.path.clone();
                drop(s);
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// 片付ける対応付け（すべて置換の作業ファイル）を消す。
    fn remove_work_files(&mut self) {
        for src in self.sources.iter_mut() {
            if let Some(s) = src.take_if(|s| s.work_file) {
                let path = s.path.clone();
                drop(s);
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

impl Drop for HugeDoc {
    fn drop(&mut self) {
        self.remove_work_files();
    }
}

/// bytes の n 文字目のバイト位置（足りなければ末尾）。
pub fn byte_of_char(encoding: TextEncoding, bytes: &[u8], n: usize) -> usize {
    let mut i = 0;
    for _ in 0..n {
        if i >= bytes.len() {
            return bytes.len();
        }
        i = next_char(encoding, bytes, i);
    }
    i.min(bytes.len())
}

/// bytes の文字数。
pub fn count_chars(encoding: TextEncoding, bytes: &[u8]) -> usize {
    match encoding {
        TextEncoding::ShiftJis => {
            let (mut i, mut n) = (0, 0);
            while i < bytes.len() {
                i = next_char(encoding, bytes, i);
                n += 1;
            }
            n
        }
        // 正しい UTF-8 なら継続バイト以外を数える（速い）。不正なら 1 文字ずつ進める
        _ if std::str::from_utf8(bytes).is_ok() => {
            bytes.iter().filter(|&&b| b & 0xC0 != 0x80).count()
        }
        _ => {
            let (mut i, mut n) = (0, 0);
            while i < bytes.len() {
                i = next_char(encoding, bytes, i);
                n += 1;
            }
            n
        }
    }
}

/// i の文字の次の文字の位置。
fn next_char(encoding: TextEncoding, bytes: &[u8], i: usize) -> usize {
    let b = bytes[i];
    let len = match encoding {
        TextEncoding::ShiftJis => {
            if matches!(b, 0x81..=0x9F | 0xE0..=0xFC) && i + 1 < bytes.len() {
                2
            } else {
                1
            }
        }
        _ => match b {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => 1,
        },
    };
    // UTF-8 の継続バイトが足りない（不正）なら、そこで区切る
    let mut end = (i + len).min(bytes.len());
    if encoding != TextEncoding::ShiftJis {
        if let Some(bad) = (i + 1..end).find(|&j| bytes[j] & 0xC0 != 0x80) {
            end = bad;
        }
    }
    end
}

#[cfg(test)]
mod tests;
