//! 未保存の編集内容の一時保存（`--save-and-quit` で巨大ファイルを保存できなかったとき）。
//! ピース列と、ピースが使う追加した文字だけを書き、元ファイルは書かない。
//! 復元するときは元ファイルを開き直し、開いた時点と同じファイルなら当て直す。

use super::tree::{Piece, Src};
use super::{Change, HugeDoc, INDEX_CHUNK};
use crate::encoding::TextEncoding;
use memchr::memchr_iter;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const MAGIC: &[u8; 8] = b"VZEDIT1\n";

/// ファイルの状態（更新日時, サイズ）。
pub(super) fn disk_stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

fn nanos(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos().min(u128::from(u64::MAX)) as u64)
}

fn put(w: &mut impl Write, v: u64) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn get(r: &mut impl Read) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

impl HugeDoc {
    /// 未保存の編集内容を out に書く。すべて置換の作業ファイルを使っているとき、
    /// または元ファイルが開いた後に変わっているときは書かない（エラー）。
    pub fn write_edits(&self, out: &Path) -> io::Result<()> {
        let path = self.path().ok_or_else(|| bad("ファイル名がありません"))?;
        let stamp = self
            .opened_stamp
            .ok_or_else(|| bad("元ファイルの状態が分かりません"))?;
        if disk_stamp(path) != Some(stamp) {
            return Err(bad("元ファイルが開いた後に変わっています"));
        }
        let snap = self.doc_snapshot();
        if snap
            .pieces
            .iter()
            .any(|p| matches!(p.src, Src::File(i) if i != 0))
        {
            return Err(bad("すべて置換の結果を含むため一時保存できません"));
        }
        let mut w = BufWriter::new(File::create(out)?);
        w.write_all(MAGIC)?;
        put(&mut w, nanos(stamp.0))?;
        put(&mut w, stamp.1)?;
        let label = self.encoding.label().as_bytes();
        put(&mut w, label.len() as u64)?;
        w.write_all(label)?;
        w.write_all(if self.line_ending == "\r\n" {
            b"C"
        } else {
            b"L"
        })?;
        put(&mut w, snap.pieces.len() as u64)?;
        for p in &snap.pieces {
            w.write_all(&[u8::from(matches!(p.src, Src::Add(_)))])?;
            put(&mut w, p.start)?;
            put(&mut w, p.len)?;
        }
        put(&mut w, snap.add.len() as u64)?;
        w.write_all(&snap.add)?;
        w.into_inner().map_err(|e| e.into_error())?.sync_all()
    }

    /// 元ファイルを開き直し、一時保存した編集内容を当て直す（Undo 1 回で元ファイルの内容に戻る）。
    /// 元ファイルが一時保存したときと違えばエラー。
    pub fn open_with_edits(path: &Path, edits: &Path) -> io::Result<Self> {
        let mut r = BufReader::new(File::open(edits)?);
        let mut magic = [0u8; 8];
        r.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(bad("一時保存した編集内容の形式が違います"));
        }
        let (mtime, len) = (get(&mut r)?, get(&mut r)?);
        let now = disk_stamp(path).ok_or_else(|| bad("元ファイルを開けません"))?;
        if (nanos(now.0), now.1) != (mtime, len) {
            return Err(bad(
                "元ファイルが一時保存の後に変わったため、未保存の編集を戻せません",
            ));
        }
        let label_len = get(&mut r)? as usize;
        let mut label = vec![0u8; label_len.min(64)];
        r.read_exact(&mut label)?;
        let encoding = TextEncoding::from_label(std::str::from_utf8(&label).unwrap_or(""))
            .ok_or_else(|| bad("文字コードが分かりません"))?;
        let mut eol = [0u8; 1];
        r.read_exact(&mut eol)?;
        let count = get(&mut r)? as usize;
        let mut raw = Vec::with_capacity(count.min(1 << 20));
        for _ in 0..count {
            let mut kind = [0u8; 1];
            r.read_exact(&mut kind)?;
            raw.push((kind[0] == 1, get(&mut r)?, get(&mut r)?));
        }
        let add_len = get(&mut r)? as usize;
        let mut add = Vec::with_capacity(add_len);
        r.by_ref().take(add_len as u64).read_to_end(&mut add)?;
        if add.len() != add_len {
            return Err(bad("一時保存した編集内容が途中で切れています"));
        }

        let mut doc = Self::open_with_chunk(path, INDEX_CHUNK)?;
        let file_len = doc.sources[0].as_ref().map_or(0, |s| s.map.len() as u64);
        let block = u32::try_from(doc.blocks.len()).map_err(|_| bad("追加ブロックが多すぎます"))?;
        let mut pieces = Vec::with_capacity(raw.len());
        for (is_add, start, len) in raw {
            let end = start
                .checked_add(len)
                .ok_or_else(|| bad("ピースの範囲が不正です"))?;
            let piece = if is_add {
                if end > add.len() as u64 {
                    return Err(bad("ピースの範囲が不正です"));
                }
                let nl = memchr_iter(b'\n', &add[start as usize..end as usize]).count() as u64;
                Piece {
                    src: Src::Add(block),
                    start,
                    len,
                    nl,
                    pending: false,
                }
            } else {
                if end > file_len {
                    return Err(bad("ピースの範囲が不正です"));
                }
                Piece {
                    src: Src::File(0),
                    start,
                    len,
                    nl: 0,
                    pending: true,
                }
            };
            pieces.push(piece);
        }
        doc.encoding = encoding;
        doc.line_ending = if eol[0] == b'C' { "\r\n" } else { "\n" };
        // 追加した文字は 1 つのブロックにまとめる（容量ちょうどなので、以後の入力は新しいブロックへ）
        add.shrink_to_fit();
        doc.blocks.push(add);
        let old = doc.tree.pieces();
        doc.replace_run(0, old.len(), &pieces);
        doc.current.push(Change {
            pos: 0,
            old,
            new: pieces,
        });
        doc.snapshot();
        doc.resolve_pending();
        Ok(doc)
    }
}
