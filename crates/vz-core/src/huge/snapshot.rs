use super::{Backing, Piece, Src};
use crate::encoding::{TextEncoding, Transcoder, DETECT_SAMPLE};
use std::borrow::Cow;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// 裏のスレッドで読む文書の写し。
pub struct DocSnapshot {
    maps: Vec<Option<Arc<Backing>>>,
    pub(super) add: Vec<u8>,
    pub(super) pieces: Vec<Piece>,
    /// starts[i] = ピース i の文書内の開始位置
    starts: Vec<u64>,
    pub encoding: TextEncoding,
    pub line_ending: &'static str,
}

impl DocSnapshot {
    pub(super) fn new(
        maps: Vec<Option<Arc<Backing>>>,
        add: Vec<u8>,
        pieces: Vec<Piece>,
        encoding: TextEncoding,
        line_ending: &'static str,
    ) -> Self {
        let mut starts = Vec::with_capacity(pieces.len());
        let mut acc = 0;
        for p in &pieces {
            starts.push(acc);
            acc += p.len;
        }
        Self {
            maps,
            add,
            pieces,
            starts,
            encoding,
            line_ending,
        }
    }

    /// 対応付けたファイル全体（BOM を除く）を 1 つのピースとする写し（Grep 用）。
    pub fn from_map(map: Arc<Backing>) -> Self {
        let head = &map[..map.len().min(DETECT_SAMPLE)];
        let encoding = TextEncoding::detect(head, map.len() > DETECT_SAMPLE);
        let line_ending = if memchr::memmem::find(head, b"\r\n").is_some() {
            "\r\n"
        } else {
            "\n"
        };
        let base = encoding.bom().len() as u64;
        let len = map.len() as u64;
        let pieces = if len > base {
            vec![Piece {
                src: Src::File(0),
                start: base,
                len: len - base,
                nl: 0,
                pending: true,
            }]
        } else {
            Vec::new()
        };
        Self::new(vec![Some(map)], Vec::new(), pieces, encoding, line_ending)
    }

    pub fn len(&self) -> u64 {
        self.starts
            .last()
            .zip(self.pieces.last())
            .map_or(0, |(s, p)| s + p.len)
    }

    /// [off, off+len) を読む。1 つのピースの中なら写さずに返す。
    pub fn read(&self, off: u64, len: u64) -> Cow<'_, [u8]> {
        let len = len.min(self.len().saturating_sub(off));
        if len == 0 {
            return Cow::Borrowed(&[]);
        }
        let mut i = self.starts.partition_point(|&s| s <= off) - 1;
        let skip = off - self.starts[i];
        let p = &self.pieces[i];
        if skip + len <= p.len {
            return Cow::Borrowed(&self.bytes(p)[skip as usize..(skip + len) as usize]);
        }
        let mut out = Vec::with_capacity(len as usize);
        let mut skip = skip as usize;
        while (out.len() as u64) < len && i < self.pieces.len() {
            let b = &self.bytes(&self.pieces[i])[skip..];
            let take = b.len().min(len as usize - out.len());
            out.extend_from_slice(&b[..take]);
            skip = 0;
            i += 1;
        }
        Cow::Owned(out)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn bytes(&self, p: &Piece) -> &[u8] {
        let (s, e) = (p.start as usize, (p.start + p.len) as usize);
        match p.src {
            Src::File(i) => &self.maps[i as usize].as_ref().expect("対応付けが無い")[s..e],
            Src::Add(_) => &self.add[s..e],
        }
    }

    /// ピースの中身を順に返す。
    pub fn spans(&self) -> impl Iterator<Item = &[u8]> {
        self.pieces.iter().map(|p| self.bytes(p))
    }

    /// 保存先と同じフォルダの一時ファイルへ順に書き出し、rename で置き換える。
    /// 元ファイルの対応付けは外さない（古い対応付けから読み続けられるので Undo も残る）。
    /// シンボリックリンクなら、リンク先の実体を置き換える。
    pub fn save(
        &self,
        path: &Path,
        to: TextEncoding,
        progress: &AtomicU64,
        cancel: &AtomicBool,
    ) -> io::Result<()> {
        let target = save_target(path)?;
        let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("vz");
        let temp = target.with_file_name(format!(".{name}.vzsave.tmp"));
        let result = (|| {
            let mut w = BufWriter::with_capacity(8 << 20, File::create(&temp)?);
            w.write_all(to.bom())?;
            let mut transcoder = Transcoder::new(self.encoding, to);
            let mut done = 0u64;
            let count = self.pieces.len();
            for (i, span) in self.spans().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    return Err(io::Error::other("保存を中止しました"));
                }
                // 大きなピースは 4MB ずつ変換する
                let parts = span.chunks(4 << 20);
                let n = span.len().div_ceil(4 << 20).max(1);
                for (j, part) in parts.enumerate() {
                    let last = i + 1 == count && j + 1 == n;
                    let out = transcoder.push(part, last).map_err(|e| {
                        io::Error::other(format!(
                            "「{}」は {to} で保存できません（文字コードを指定して保存で UTF-8 などに変えてください）",
                            e.ch
                        ))
                    })?;
                    w.write_all(out)?;
                    done += part.len() as u64;
                    progress.store(done, Ordering::Relaxed);
                }
            }
            if count == 0 {
                w.write_all(transcoder.push(&[], true).map_err(io::Error::other)?)?;
            }
            w.into_inner().map_err(|e| e.into_error())?.sync_all()?;
            std::fs::rename(&temp, &target)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }
}

/// 保存先の実体（シンボリックリンクならリンク先）。
fn save_target(path: &Path) -> io::Result<PathBuf> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path),
        _ => Ok(path.to_path_buf()),
    }
}

/// すべて置換の作業ファイルを、ディスクへ書き切ってから保存先へ rename する（全体を書き出さない保存）。
pub fn save_by_rename(work: &Path, path: &Path) -> io::Result<()> {
    // Windows はディスクへの書き切り（FlushFileBuffers）に書き込みの権限が要る
    std::fs::OpenOptions::new()
        .write(true)
        .open(work)?
        .sync_all()?;
    std::fs::rename(work, save_target(path)?)
}
