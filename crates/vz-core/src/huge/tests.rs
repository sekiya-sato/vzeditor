use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> Self {
        let d = std::env::temp_dir().join(format!("vz_huge_{name}_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        Self(d)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sample(seed: u64) -> Vec<u8> {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut v = Vec::new();
    for i in 0..3000 {
        match rng.below(10) {
            0 => {}
            1 => v.extend(std::iter::repeat_n(b'x', 3000)),
            _ => v.extend(format!("line {i} あいう").as_bytes()),
        }
        v.extend(b"\n");
    }
    v
}

fn model_line_start(m: &[u8], line: usize) -> Option<u64> {
    if line == 0 {
        return Some(0);
    }
    memchr_iter(b'\n', m).nth(line - 1).map(|i| i as u64 + 1)
}

fn check(doc: &HugeDoc, model: &[u8], rng: &mut Rng) {
    doc.tree.check();
    assert_eq!(doc.len(), model.len() as u64, "長さ");
    assert_eq!(doc.read(0, doc.len()).as_ref(), model, "内容");
    let lines = memchr_iter(b'\n', model).count() + 1;
    assert_eq!(doc.line_count(), lines, "行数");
    for _ in 0..10 {
        let l = rng.below(lines as u64 + 2) as usize;
        assert_eq!(doc.line_start(l), model_line_start(model, l), "行頭 {l}");
        if l < lines {
            let col = rng.below(doc.line_char_len(l) as u64 + 1) as usize;
            let off = doc.pos_to_offset(l, col).unwrap();
            assert_eq!(
                doc.offset_to_pos(off),
                Some((l, col)),
                "位置の往復 {l}:{col}"
            );
        }
    }
}

#[test]
fn test_random_edits_match_model() {
    let dir = TempDir::new("model");
    for seed in [1u64, 7, 42] {
        let orig = sample(seed);
        let path = dir.0.join(format!("orig{seed}.txt"));
        std::fs::write(&path, &orig).unwrap();
        let mut doc = HugeDoc::open_with_chunk(&path, 4096).unwrap();
        doc.wait_index();
        let mut rng = Rng(seed);
        let mut model = orig.clone();
        let mut history = vec![model.clone()];
        let mut at = 0usize;
        check(&doc, &model, &mut rng);
        for step in 0..800 {
            match rng.below(10) {
                0..=3 => {
                    let pos = rng.below(model.len() as u64 + 1);
                    let text = match rng.below(3) {
                        0 => "\n".to_string(),
                        1 => format!("ins{step}\nあ\n"),
                        _ => format!("t{step}"),
                    };
                    doc.insert(pos, &text).unwrap();
                    model.splice(pos as usize..pos as usize, text.bytes());
                    let mut p = pos + text.len() as u64;
                    for _ in 0..rng.below(4) {
                        doc.insert(p, "ab").unwrap();
                        model.splice(p as usize..p as usize, *b"ab");
                        p += 2;
                    }
                }
                4..=5 => {
                    let pos = rng.below(model.len() as u64 + 1);
                    let limit = if rng.below(10) == 0 { 50_000 } else { 200 };
                    let end = (pos + rng.below(limit)).min(model.len() as u64);
                    doc.delete(pos, end).unwrap();
                    model.drain(pos as usize..end as usize);
                }
                6..=7 => {
                    if doc.snapshot() {
                        history.truncate(at + 1);
                        history.push(model.clone());
                        at += 1;
                    }
                }
                8 => {
                    if doc.snapshot() {
                        history.truncate(at + 1);
                        history.push(model.clone());
                        at += 1;
                    }
                    if doc.undo().is_some() {
                        at -= 1;
                        model = history[at].clone();
                    }
                }
                _ => {
                    if doc.snapshot() {
                        history.truncate(at + 1);
                        history.push(model.clone());
                        at += 1;
                    }
                    if doc.redo().is_some() {
                        at += 1;
                        model = history[at].clone();
                    }
                }
            }
            check(&doc, &model, &mut rng);
        }
    }
}

#[test]
fn test_typing_coalesces_and_undo_limit() {
    let dir = TempDir::new("typing");
    let path = dir.0.join("a.txt");
    std::fs::write(&path, b"hello\nworld\n").unwrap();
    let mut doc = HugeDoc::open_with_chunk(&path, 4096).unwrap();
    doc.wait_index();
    for (i, c) in "abcdefghij".chars().enumerate() {
        doc.insert(6 + i as u64, &c.to_string()).unwrap();
    }
    // 元のピースが 2 つに分かれ、間に入力のピースが 1 つ
    assert_eq!(doc.piece_count(), 3);
    assert_eq!(doc.line_text(1), "abcdefghijworld");
    doc.undo();
    assert_eq!(doc.line_text(1), "world");
    // Undo は 200 回まで
    for i in 0..(MAX_UNDO + 10) {
        doc.insert(0, &format!("{i}")).unwrap();
        doc.snapshot();
    }
    let mut n = 0;
    while doc.undo().is_some() {
        n += 1;
    }
    assert_eq!(n, MAX_UNDO);
}

#[test]
fn test_editing_while_indexing() {
    let dir = TempDir::new("pending");
    let orig = sample(5);
    let path = dir.0.join("a.txt");
    std::fs::write(&path, &orig).unwrap();
    let mut doc = HugeDoc::open_manual(&path).unwrap();
    // 索引がまだ無い: 1 行だけ（終わりは分からない）
    assert!(doc.is_indexing());
    doc.feed_index(4096, 2);
    assert!(doc.is_indexing());
    let known = doc.line_count();
    let full_lines = memchr_iter(b'\n', &orig).count() + 1;
    assert!(known > 2 && known < full_lines, "known={known}");
    // 索引ができた範囲は編集できる
    let mut model = orig.clone();
    let off = doc.pos_to_offset(1, 2).unwrap();
    doc.insert(off, "[A]\n").unwrap();
    model.splice(off as usize..off as usize, *b"[A]\n");
    assert_eq!(doc.line_count(), known + 1);
    // 索引の範囲外は待つ
    let far = orig.len() as u64 - 10;
    assert_eq!(doc.insert(far, "x"), Err(EditError::Indexing));
    assert_eq!(doc.offset_to_pos(far), None);
    // 索引が完成すると、未確定のピースに改行数が入る
    doc.feed_index(4096, usize::MAX);
    assert!(!doc.is_indexing());
    let mut rng = Rng(3);
    check(&doc, &model, &mut rng);
    doc.insert(far + 4, "x").unwrap();
    model.splice(far as usize + 4..far as usize + 4, *b"x");
    check(&doc, &model, &mut rng);
    // 索引の作成中の編集も Undo できる
    doc.undo();
    doc.undo();
    assert_eq!(doc.read(0, doc.len()).as_ref(), &orig[..]);
}

#[test]
fn test_save_keeps_undo_and_converts_encoding() {
    let dir = TempDir::new("save");
    let path = dir.0.join("a.txt");
    let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("一般書籍\r\n二行目\r\n");
    std::fs::write(&path, &sjis).unwrap();
    let mut doc = HugeDoc::open_with_chunk(&path, 4096).unwrap();
    doc.wait_index();
    assert_eq!(doc.encoding, TextEncoding::ShiftJis);
    assert_eq!(doc.line_ending, "\r\n");
    assert_eq!(doc.line_text(0), "一般書籍");
    assert_eq!(doc.line_char_len(1), 3);
    // 2 行目の 2 文字目に挿入（改行は CRLF にそろう）
    let off = doc.pos_to_offset(1, 2).unwrap();
    doc.insert(off, "漢\n").unwrap();
    assert_eq!(doc.line_text(1), "二行漢");
    assert_eq!(doc.line_text(2), "目");
    assert_eq!(doc.insert(0, "😀"), Err(EditError::Unencodable('😀')));
    doc.snapshot();

    let (progress, cancel) = (AtomicU64::new(0), AtomicBool::new(false));
    doc.doc_snapshot()
        .save(&path, TextEncoding::ShiftJis, &progress, &cancel)
        .unwrap();
    let (expected, _, _) = encoding_rs::SHIFT_JIS.encode("一般書籍\r\n二行漢\r\n目\r\n");
    assert_eq!(std::fs::read(&path).unwrap(), expected.as_ref());
    // 保存後も古い対応付けから読めるので Undo できる
    doc.undo();
    assert_eq!(doc.line_text(1), "二行目");
    // UTF-8 BOM に変えて保存
    let out = dir.0.join("b.txt");
    doc.doc_snapshot()
        .save(&out, TextEncoding::Utf8Bom, &progress, &cancel)
        .unwrap();
    let mut want = b"\xEF\xBB\xBF".to_vec();
    want.extend("一般書籍\r\n二行目\r\n".as_bytes());
    assert_eq!(std::fs::read(&out).unwrap(), want);
    let names: Vec<_> = std::fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(names.iter().all(|n| !n.contains("vzsave")), "{names:?}");
}

#[test]
fn test_replace_result_saved_by_rename() {
    let dir = TempDir::new("rename");
    let path = dir.0.join("a.txt");
    std::fs::write(&path, "x cat\ny cat\n").unwrap();
    let mut doc = HugeDoc::open_with_chunk(&path, 4096).unwrap();
    doc.wait_index();
    assert_eq!(doc.whole_work_file(), None);
    // すべて置換の結果を作業ファイルとして取り込む
    let work = dir.0.join(".a.txt.vzwork-test.tmp");
    std::fs::write(&work, "x dog\ny dog\n").unwrap();
    doc.adopt_work_file(&work).unwrap();
    doc.wait_index();
    assert_eq!(doc.whole_work_file().as_deref(), Some(work.as_path()));
    // rename だけで保存し、作業ファイルを元ファイルとして扱う
    save_by_rename(&work, &path).unwrap();
    doc.promote_work_file(&work, &path);
    assert!(!work.exists());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "x dog\ny dog\n");
    assert_eq!(doc.whole_work_file(), None);
    // 保存後も置換前に戻せる
    doc.undo();
    assert_eq!(doc.line_text(0), "x cat");
    doc.redo();
    assert_eq!(doc.line_text(1), "y dog");
    drop(doc);
    // 元ファイルとして扱うので、閉じても消さない
    assert!(path.exists());
}

#[test]
fn test_heap_backing_when_mmap_fails() {
    let dir = TempDir::new("heap");
    let path = dir.0.join("a.txt");
    std::fs::write(&path, "one\ntwo\nthree").unwrap();
    let mut doc = HugeDoc::open_heap_for_test(&path).unwrap();
    doc.wait_index();
    assert!(doc.is_heap_backed());
    assert_eq!(doc.line_count(), 3);
    doc.insert(doc.pos_to_offset(1, 3).unwrap(), "!").unwrap();
    doc.snapshot();
    let (p, c) = (AtomicU64::new(0), AtomicBool::new(false));
    doc.doc_snapshot()
        .save(&path, TextEncoding::Utf8, &p, &c)
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo!\nthree");
    doc.undo();
    assert_eq!(doc.line_text(1), "two");
}

#[test]
fn test_unsaved_edits_written_and_restored() {
    let dir = TempDir::new("edits");
    let path = dir.0.join("a.txt");
    let orig = sample(11);
    std::fs::write(&path, &orig).unwrap();
    let mut doc = HugeDoc::open_with_chunk(&path, 4096).unwrap();
    doc.wait_index();
    doc.insert(doc.pos_to_offset(3, 2).unwrap(), "あいう\n追加")
        .unwrap();
    doc.delete(100, 5000).unwrap();
    doc.snapshot();
    let edited = doc.read(0, doc.len()).into_owned();
    let edits = dir.0.join("a.vzedit");
    doc.write_edits(&edits).unwrap();
    drop(doc);

    let mut back = HugeDoc::open_with_edits(&path, &edits).unwrap();
    back.wait_index();
    assert_eq!(back.read(0, back.len()).as_ref(), &edited[..]);
    assert!(back.is_modified());
    let mut rng = Rng(5);
    check(&back, &edited, &mut rng);
    // Undo 1 回で元ファイルの内容に戻る
    back.undo();
    assert_eq!(back.read(0, back.len()).as_ref(), &orig[..]);
    drop(back);

    // 元ファイルが変わっていたら戻さない
    std::fs::write(&path, b"changed\n").unwrap();
    assert!(HugeDoc::open_with_edits(&path, &edits).is_err());
}

#[test]
fn test_long_line_columns() {
    let dir = TempDir::new("long");
    let path = dir.0.join("a.txt");
    let mut data = "あ".repeat(100_000).into_bytes();
    data.extend(b"\nshort\n");
    std::fs::write(&path, &data).unwrap();
    let mut doc = HugeDoc::open_with_chunk(&path, 4096).unwrap();
    doc.wait_index();
    assert_eq!(doc.line_char_len(0), 100_000);
    let seg = doc.line_segment(0, 90_000, 5);
    assert_eq!(seg, "あああああ");
    assert_eq!(doc.pos_to_offset(0, 90_000), Some(270_000));
    assert_eq!(doc.offset_to_pos(270_000), Some((0, 90_000)));
    doc.insert(270_000, "X").unwrap();
    assert_eq!(doc.line_segment(0, 89_999, 3), "あXあ");
}
