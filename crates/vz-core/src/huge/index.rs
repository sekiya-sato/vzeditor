//! ファイルの疎な行索引。裏のスレッドがチャンクごとに並列で作り、束ごとに送る。

use memchr::memchr_iter;
use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

/// 何行ごとに行頭位置を記録するか
pub const LINE_STRIDE: u64 = 1024;
/// 索引を作るときの分割単位（行頭にそろえる）
pub const INDEX_CHUNK: u64 = 16 << 20;

pub struct ChunkIndex {
    pub start: u64,
    pub end: u64,
    /// チャンク先頭より前の改行数
    pub first_nl: u64,
    /// チャンク内の改行数
    pub nl: u64,
    /// marks[k] = チャンク内のローカル行 k*LINE_STRIDE の行頭位置（marks[0] = start）
    marks: Vec<u64>,
}

#[derive(Default)]
pub struct LineIndex {
    chunks: Vec<ChunkIndex>,
    complete: bool,
}

impl LineIndex {
    /// その場で全体を作る（書き出しながら作る代わりや、テスト用）。
    pub fn build(data: &[u8], base: u64, chunk: u64) -> Self {
        let mut index = Self::default();
        let mut builder = Builder::new(data.len() as u64, chunk);
        while let Some(ranges) = builder.next_batch(data, usize::MAX) {
            index.extend(build_batch(data, base, &ranges, index.total_nl()));
        }
        index.complete = true;
        index
    }

    /// 先頭から max_chunks 個のチャンクだけ作る（索引の作成中を再現するテスト用）。
    #[cfg(test)]
    pub fn build_partial(data: &[u8], base: u64, chunk: u64, max_chunks: usize) -> Self {
        let mut index = Self::default();
        let mut builder = Builder::new(data.len() as u64, chunk);
        if let Some(ranges) = builder.next_batch(data, max_chunks) {
            index.extend(build_batch(data, base, &ranges, 0));
        }
        index.complete = builder.next_start >= data.len() as u64;
        index
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// 索引ができた範囲の終わり（ファイル内のバイト位置）。
    pub fn indexed_end(&self) -> u64 {
        self.chunks.last().map_or(0, |c| c.end)
    }

    fn total_nl(&self) -> u64 {
        self.chunks.last().map_or(0, |c| c.first_nl + c.nl)
    }

    fn extend(&mut self, batch: Vec<ChunkIndex>) {
        self.chunks.extend(batch);
    }

    /// [from, to) の改行数。to が索引の範囲外なら None。
    pub fn newlines_in(&self, data: &[u8], base: u64, from: u64, to: u64) -> Option<u64> {
        Some(self.nl_before(data, base, to)? - self.nl_before(data, base, from)?)
    }

    /// ファイル内の位置 off より前の改行数（base は索引の起点。BOM の分）。範囲外なら None。
    fn nl_before(&self, data: &[u8], base: u64, off: u64) -> Option<u64> {
        if off <= base {
            return Some(0);
        }
        if off > self.indexed_end() {
            return None;
        }
        let ci = self
            .chunks
            .partition_point(|c| c.end <= off)
            .min(self.chunks.len() - 1);
        let c = &self.chunks[ci];
        let k = c.marks.partition_point(|&m| m <= off) - 1;
        let from = c.marks[k];
        Some(
            c.first_nl
                + k as u64 * LINE_STRIDE
                + memchr_iter(b'\n', &data[from as usize..off as usize]).count() as u64,
        )
    }

    /// `from` より前の改行数が `before` のとき、そこから n 個目の改行の直後の位置。範囲外なら None。
    pub fn nth_line_start(&self, data: &[u8], before: u64, n: u64) -> Option<u64> {
        let line = before + n;
        let ci = self.chunks.partition_point(|c| c.first_nl + c.nl < line);
        let c = self.chunks.get(ci)?;
        let j = line - c.first_nl; // 1..=c.nl
        let m = (j / LINE_STRIDE) as usize;
        let mut pos = c.marks[m];
        let skip = j - m as u64 * LINE_STRIDE;
        if skip > 0 {
            let i =
                memchr_iter(b'\n', &data[pos as usize..c.end as usize]).nth(skip as usize - 1)?;
            pos += i as u64 + 1;
        }
        Some(pos)
    }

    /// 索引が使っているメモリの概算（バイト）。
    pub fn heap_bytes(&self) -> usize {
        self.chunks
            .iter()
            .map(|c| c.marks.capacity() * 8 + 48)
            .sum()
    }
}

/// チャンクの区切りを先頭から順に決める。
struct Builder {
    len: u64,
    chunk: u64,
    next_start: u64,
}

impl Builder {
    fn new(len: u64, chunk: u64) -> Self {
        Self {
            len,
            chunk,
            next_start: 0,
        }
    }

    /// 次の最大 count 個のチャンクの範囲。終わりなら None。
    fn next_batch(&mut self, data: &[u8], count: usize) -> Option<Vec<(u64, u64)>> {
        if self.next_start >= self.len && !(self.len == 0 && self.next_start == 0) {
            return None;
        }
        let mut ranges = Vec::new();
        while ranges.len() < count && self.next_start < self.len {
            let s = self.next_start;
            let target = s + self.chunk;
            let e = if target >= self.len {
                self.len
            } else {
                match memchr::memchr(b'\n', &data[target as usize..]) {
                    Some(i) => (target + i as u64 + 1).min(self.len),
                    None => self.len,
                }
            };
            ranges.push((s, e));
            self.next_start = e;
        }
        if ranges.is_empty() {
            // 空のファイル
            self.next_start = 1;
            return Some(vec![(0, 0)]);
        }
        Some(ranges)
    }
}

/// チャンクを並列に処理する。first_nl は束の先頭より前の改行数。
fn build_batch(data: &[u8], base: u64, ranges: &[(u64, u64)], first_nl: u64) -> Vec<ChunkIndex> {
    let parts: Vec<(u64, Vec<u64>)> = ranges
        .par_iter()
        .map(|&(s, e)| {
            let mut marks = vec![s.max(base)];
            let mut count = 0u64;
            let from = s.max(base);
            for i in memchr_iter(b'\n', &data[from as usize..e as usize]) {
                count += 1;
                if count.is_multiple_of(LINE_STRIDE) {
                    marks.push(from + i as u64 + 1);
                }
            }
            (count, marks)
        })
        .collect();
    let mut nl_acc = first_nl;
    ranges
        .iter()
        .zip(parts)
        .map(|(&(start, end), (nl, marks))| {
            let c = ChunkIndex {
                start,
                end,
                first_nl: nl_acc,
                nl,
                marks,
            };
            nl_acc += nl;
            c
        })
        .collect()
}

enum Msg {
    Batch(Vec<ChunkIndex>),
    Done,
}

/// 裏のスレッドで作る索引。`poll` で受け取って伸ばす。
pub struct IndexJob {
    rx: Receiver<Msg>,
    cancel: Arc<AtomicBool>,
}

impl IndexJob {
    /// 索引スレッドを起動する。base は本文の開始位置（BOM の分）。
    pub fn spawn(map: Arc<super::Backing>, base: u64, chunk: u64) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        std::thread::Builder::new()
            .name("vz-huge-index".into())
            .spawn(move || {
                let data: &[u8] = &map;
                // 索引を作る間だけ先読みを頼み、終わったら（中止でも）元に戻す
                struct Sequential<'a>(&'a super::Backing);
                impl Drop for Sequential<'_> {
                    fn drop(&mut self) {
                        self.0.advise_sequential(false);
                    }
                }
                map.advise_sequential(true);
                let _sequential = Sequential(&map);
                let mut builder = Builder::new(data.len() as u64, chunk);
                let mut first_nl = 0;
                // 最初の束は 1 チャンクだけにして、先頭をすぐ表示できるようにする
                let mut count = 1;
                while let Some(ranges) = builder.next_batch(data, count) {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let batch = build_batch(data, base, &ranges, first_nl);
                    first_nl = batch.last().map_or(first_nl, |c| c.first_nl + c.nl);
                    if tx.send(Msg::Batch(batch)).is_err() {
                        return;
                    }
                    count = rayon::current_num_threads().max(1) * 4;
                }
                let _ = tx.send(Msg::Done);
            })?;
        Ok(Self { rx, cancel })
    }

    /// 届いた分を索引に足す。伸びたら true。
    pub fn poll(&self, index: &mut LineIndex) -> bool {
        let mut grew = false;
        loop {
            match self.rx.try_recv() {
                Ok(Msg::Batch(batch)) => {
                    index.extend(batch);
                    grew = true;
                }
                Ok(Msg::Done) => {
                    index.complete = true;
                    return true;
                }
                Err(TryRecvError::Empty) => return grew,
                Err(TryRecvError::Disconnected) => return grew,
            }
        }
    }

    /// 完成まで待つ（保存して終了・テスト用）。
    pub fn wait(&self, index: &mut LineIndex) {
        while !index.complete {
            match self.rx.recv() {
                Ok(Msg::Batch(batch)) => index.extend(batch),
                Ok(Msg::Done) => index.complete = true,
                Err(_) => return,
            }
        }
    }
}

impl Drop for IndexJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_line_start(data: &[u8], line: u64) -> Option<u64> {
        if line == 0 {
            return Some(0);
        }
        memchr_iter(b'\n', data)
            .nth(line as usize - 1)
            .map(|i| i as u64 + 1)
    }

    #[test]
    fn test_index_matches_model() {
        let mut data = Vec::new();
        for i in 0..5000u32 {
            match i % 7 {
                0 => {}
                1 => data.extend(std::iter::repeat_n(b'x', 3000)),
                _ => data.extend(format!("line {i} あいう").as_bytes()),
            }
            data.extend(if i % 3 == 0 { &b"\r\n"[..] } else { &b"\n"[..] });
        }
        data.extend(b"last line without newline");
        let total = memchr_iter(b'\n', &data).count() as u64;
        for chunk in [64, 1000, 4096, 1 << 20] {
            let index = LineIndex::build(&data, 0, chunk);
            assert!(index.is_complete());
            assert_eq!(index.indexed_end(), data.len() as u64);
            for line in (0..=total).step_by(37).chain([1, 2, total]) {
                let got = if line == 0 {
                    Some(0)
                } else {
                    index.nth_line_start(&data, 0, line)
                };
                assert_eq!(
                    got,
                    model_line_start(&data, line),
                    "chunk={chunk} line={line}"
                );
            }
            assert_eq!(index.nth_line_start(&data, 0, total + 1), None);
            for off in (0..data.len() as u64).step_by(997) {
                let expected = memchr_iter(b'\n', &data[..off as usize]).count() as u64;
                assert_eq!(index.newlines_in(&data, 0, 0, off), Some(expected));
            }
        }
    }
}
