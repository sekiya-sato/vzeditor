//! ピースの B-Tree（Piece Tree）。節は配列に置き番号で参照する（Box を使わない）。
//! 内部節は子ごとに (ピース数, バイト数, 改行数, 改行数が未確定のピース数) の合計を持つ。

/// ピースが指す先: ファイルの対応付け `sources[i]` か、追加ブロック `blocks[i]`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src {
    File(u16),
    Add(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    pub src: Src,
    pub start: u64,
    pub len: u64,
    /// 改行数（`pending` のときは確定した範囲の分だけ。合計には数えない）
    pub nl: u64,
    /// 改行数がまだ分からない（索引の範囲外にかかるファイルのピース）
    pub pending: bool,
}

impl Piece {
    fn sum(&self) -> Sum {
        Sum {
            n: 1,
            bytes: self.len,
            nl: if self.pending { 0 } else { self.nl },
            pending: u32::from(self.pending),
        }
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Sum {
    pub n: u64,
    pub bytes: u64,
    pub nl: u64,
    pub pending: u32,
}

impl Sum {
    fn add(&mut self, o: Sum) {
        self.n += o.n;
        self.bytes += o.bytes;
        self.nl += o.nl;
        self.pending += o.pending;
    }
}

#[derive(Clone, Copy)]
struct Child {
    id: u32,
    sum: Sum,
}

enum Node {
    Leaf(Vec<Piece>),
    Inner(Vec<Child>),
}

/// 位置の検索結果: ピース番号・ピース（末尾なら None）・ピースの文書内開始位置・それより前の改行数
#[derive(Clone, Copy, Debug, Default)]
pub struct Hit {
    pub idx: u64,
    pub piece: Option<Piece>,
    pub start: u64,
    /// 前にあるピースの改行数の合計（前に `pending` のピースがあれば不正確: `pending_before`）
    pub nl_before: u64,
    pub pending_before: bool,
}

pub struct Tree {
    nodes: Vec<Node>,
    free: Vec<u32>,
    root: u32,
    max: usize,
}

impl Tree {
    pub fn new(max: usize) -> Self {
        assert!(max >= 4);
        Self {
            nodes: vec![Node::Leaf(Vec::new())],
            free: Vec::new(),
            root: 0,
            max,
        }
    }

    fn min(&self) -> usize {
        self.max / 2
    }

    fn alloc(&mut self, node: Node) -> u32 {
        if let Some(id) = self.free.pop() {
            self.nodes[id as usize] = node;
            id
        } else {
            self.nodes.push(node);
            (self.nodes.len() - 1) as u32
        }
    }

    fn release(&mut self, id: u32) {
        self.nodes[id as usize] = Node::Leaf(Vec::new());
        self.free.push(id);
    }

    fn node_sum(&self, id: u32) -> Sum {
        let mut s = Sum::default();
        match &self.nodes[id as usize] {
            Node::Leaf(v) => v.iter().for_each(|p| s.add(p.sum())),
            Node::Inner(c) => c.iter().for_each(|c| s.add(c.sum)),
        }
        s
    }

    fn node_len(&self, id: u32) -> usize {
        match &self.nodes[id as usize] {
            Node::Leaf(v) => v.len(),
            Node::Inner(c) => c.len(),
        }
    }

    pub fn total(&self) -> Sum {
        self.node_sum(self.root)
    }

    // ---- 挿入 ----

    pub fn insert(&mut self, idx: u64, p: Piece) {
        debug_assert!(idx <= self.total().n);
        if let Some(right) = self.ins(self.root, idx, p) {
            let left = self.root;
            let (ls, rs) = (self.node_sum(left), self.node_sum(right));
            self.root = self.alloc(Node::Inner(vec![
                Child { id: left, sum: ls },
                Child { id: right, sum: rs },
            ]));
        }
    }

    fn ins(&mut self, id: u32, idx: u64, p: Piece) -> Option<u32> {
        let max = self.max;
        let (i, rem, cid) = match &mut self.nodes[id as usize] {
            Node::Leaf(v) => {
                v.insert(idx as usize, p);
                if v.len() > max {
                    let r = v.split_off(v.len() / 2);
                    return Some(self.alloc(Node::Leaf(r)));
                }
                return None;
            }
            Node::Inner(ch) => {
                let (mut i, mut rem) = (0, idx);
                while i + 1 < ch.len() && rem > ch[i].sum.n {
                    rem -= ch[i].sum.n;
                    i += 1;
                }
                (i, rem, ch[i].id)
            }
        };
        let split = self.ins(cid, rem, p);
        let cs = self.node_sum(cid);
        let split = split.map(|r| (r, self.node_sum(r)));
        let Node::Inner(ch) = &mut self.nodes[id as usize] else {
            unreachable!()
        };
        ch[i].sum = cs;
        if let Some((r, rs)) = split {
            ch.insert(i + 1, Child { id: r, sum: rs });
        }
        if ch.len() > max {
            let rr = ch.split_off(ch.len() / 2);
            return Some(self.alloc(Node::Inner(rr)));
        }
        None
    }

    // ---- 削除 ----

    pub fn remove(&mut self, idx: u64) -> Piece {
        debug_assert!(idx < self.total().n);
        let p = self.rem(self.root, idx);
        while let Node::Inner(c) = &self.nodes[self.root as usize] {
            if c.len() != 1 {
                break;
            }
            let child = c[0].id;
            self.release(self.root);
            self.root = child;
        }
        p
    }

    fn rem(&mut self, id: u32, idx: u64) -> Piece {
        let (i, rem, cid) = match &mut self.nodes[id as usize] {
            Node::Leaf(v) => return v.remove(idx as usize),
            Node::Inner(ch) => {
                let (mut i, mut rem) = (0, idx);
                while rem >= ch[i].sum.n {
                    rem -= ch[i].sum.n;
                    i += 1;
                }
                (i, rem, ch[i].id)
            }
        };
        let p = self.rem(cid, rem);
        let cs = self.node_sum(cid);
        if let Node::Inner(ch) = &mut self.nodes[id as usize] {
            ch[i].sum = cs;
        }
        if self.node_len(cid) < self.min() {
            self.fix(id, i);
        }
        p
    }

    /// 子 i が少なすぎるとき、隣と合わせるか、隣と均等に分け直す。
    fn fix(&mut self, id: u32, i: usize) {
        let Node::Inner(ch) = &self.nodes[id as usize] else {
            unreachable!()
        };
        if ch.len() < 2 {
            return;
        }
        let li = i.saturating_sub(1);
        let (l, r) = (ch[li].id, ch[li + 1].id);
        let a = std::mem::replace(&mut self.nodes[l as usize], Node::Leaf(Vec::new()));
        let b = std::mem::replace(&mut self.nodes[r as usize], Node::Leaf(Vec::new()));
        let max = self.max;
        let merged = match (a, b) {
            (Node::Leaf(mut x), Node::Leaf(y)) => {
                x.extend(y);
                let merged = x.len() <= max;
                let y = if merged {
                    Vec::new()
                } else {
                    x.split_off(x.len() / 2)
                };
                self.nodes[l as usize] = Node::Leaf(x);
                self.nodes[r as usize] = Node::Leaf(y);
                merged
            }
            (Node::Inner(mut x), Node::Inner(y)) => {
                x.extend(y);
                let merged = x.len() <= max;
                let y = if merged {
                    Vec::new()
                } else {
                    x.split_off(x.len() / 2)
                };
                self.nodes[l as usize] = Node::Inner(x);
                self.nodes[r as usize] = Node::Inner(y);
                merged
            }
            _ => unreachable!("兄弟の節の種類が違う"),
        };
        let (ls, rs) = (self.node_sum(l), self.node_sum(r));
        if merged {
            self.release(r);
        }
        let Node::Inner(ch) = &mut self.nodes[id as usize] else {
            unreachable!()
        };
        ch[li].sum = ls;
        if merged {
            ch.remove(li + 1);
        } else {
            ch[li + 1].sum = rs;
        }
    }

    // ---- 参照・置き換え ----

    pub fn get(&self, idx: u64) -> Piece {
        let (mut id, mut rem) = (self.root, idx);
        loop {
            match &self.nodes[id as usize] {
                Node::Leaf(v) => return v[rem as usize],
                Node::Inner(ch) => {
                    let mut i = 0;
                    while rem >= ch[i].sum.n {
                        rem -= ch[i].sum.n;
                        i += 1;
                    }
                    id = ch[i].id;
                }
            }
        }
    }

    pub fn set(&mut self, idx: u64, p: Piece) {
        self.set_in(self.root, idx, p);
    }

    fn set_in(&mut self, id: u32, idx: u64, p: Piece) {
        let (i, rem, cid) = match &mut self.nodes[id as usize] {
            Node::Leaf(v) => {
                v[idx as usize] = p;
                return;
            }
            Node::Inner(ch) => {
                let (mut i, mut rem) = (0, idx);
                while rem >= ch[i].sum.n {
                    rem -= ch[i].sum.n;
                    i += 1;
                }
                (i, rem, ch[i].id)
            }
        };
        self.set_in(cid, rem, p);
        let cs = self.node_sum(cid);
        if let Node::Inner(ch) = &mut self.nodes[id as usize] {
            ch[i].sum = cs;
        }
    }

    /// 全ピースを順に返す（保存・写しの作成用）。
    pub fn pieces(&self) -> Vec<Piece> {
        let mut out = Vec::with_capacity(self.total().n as usize);
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            match &self.nodes[id as usize] {
                Node::Leaf(v) => out.extend_from_slice(v),
                Node::Inner(ch) => stack.extend(ch.iter().rev().map(|c| c.id)),
            }
        }
        out
    }

    /// `pending` のピースの番号（`pending` を含む部分木だけを下りる）。
    pub fn pending_indices(&self) -> Vec<u64> {
        let mut out = Vec::new();
        self.collect_pending(self.root, 0, &mut out);
        out
    }

    fn collect_pending(&self, id: u32, base: u64, out: &mut Vec<u64>) {
        match &self.nodes[id as usize] {
            Node::Leaf(v) => out.extend(
                v.iter()
                    .enumerate()
                    .filter(|(_, p)| p.pending)
                    .map(|(i, _)| base + i as u64),
            ),
            Node::Inner(ch) => {
                let mut b = base;
                for c in ch {
                    if c.sum.pending > 0 {
                        self.collect_pending(c.id, b, out);
                    }
                    b += c.sum.n;
                }
            }
        }
    }

    /// 最初の `pending` のピース（無ければ None）。
    pub fn first_pending(&self) -> Option<Hit> {
        let total = self.total();
        if total.pending == 0 {
            return None;
        }
        let mut id = self.root;
        let mut hit = Hit::default();
        loop {
            match &self.nodes[id as usize] {
                Node::Leaf(v) => {
                    for p in v {
                        if p.pending {
                            hit.piece = Some(*p);
                            return Some(hit);
                        }
                        hit.idx += 1;
                        hit.start += p.len;
                        hit.nl_before += p.nl;
                    }
                    return None;
                }
                Node::Inner(ch) => {
                    let i = ch.iter().position(|c| c.sum.pending > 0)?;
                    for c in &ch[..i] {
                        hit.idx += c.sum.n;
                        hit.start += c.sum.bytes;
                        hit.nl_before += c.sum.nl;
                    }
                    id = ch[i].id;
                }
            }
        }
    }

    /// 文書内のバイト位置 off を含むピース（境界ならそこから始まるピース。末尾なら piece = None）。
    pub fn locate_offset(&self, off: u64) -> Hit {
        let total = self.total();
        if off >= total.bytes {
            return Hit {
                idx: total.n,
                piece: None,
                start: total.bytes,
                nl_before: total.nl,
                pending_before: total.pending > 0,
            };
        }
        let (mut id, mut rem) = (self.root, off);
        let mut hit = Hit::default();
        loop {
            match &self.nodes[id as usize] {
                Node::Leaf(v) => {
                    for p in v {
                        if rem < p.len {
                            hit.piece = Some(*p);
                            return hit;
                        }
                        rem -= p.len;
                        hit.idx += 1;
                        hit.start += p.len;
                        hit.nl_before += p.sum().nl;
                        hit.pending_before |= p.pending;
                    }
                    unreachable!()
                }
                Node::Inner(ch) => {
                    let mut i = 0;
                    while rem >= ch[i].sum.bytes {
                        rem -= ch[i].sum.bytes;
                        hit.idx += ch[i].sum.n;
                        hit.start += ch[i].sum.bytes;
                        hit.nl_before += ch[i].sum.nl;
                        hit.pending_before |= ch[i].sum.pending > 0;
                        i += 1;
                    }
                    id = ch[i].id;
                }
            }
        }
    }

    /// 文書の k 番目（1 始まり）の改行を含むピースと、そのピース内で何番目の改行か。
    /// 途中に `pending` のピースがあれば、そのピースを返す（その中にあるかは呼び出し側が索引で調べる）。
    pub fn locate_newline(&self, k: u64) -> Option<(Hit, u64)> {
        let total = self.total();
        if k == 0 || (total.pending == 0 && k > total.nl) {
            return None;
        }
        let (mut id, mut rem) = (self.root, k);
        let mut hit = Hit::default();
        loop {
            match &self.nodes[id as usize] {
                Node::Leaf(v) => {
                    for p in v {
                        if p.pending || rem <= p.nl {
                            hit.piece = Some(*p);
                            return Some((hit, rem));
                        }
                        rem -= p.nl;
                        hit.idx += 1;
                        hit.start += p.len;
                        hit.nl_before += p.nl;
                    }
                    return None;
                }
                Node::Inner(ch) => {
                    let mut i = 0;
                    while ch[i].sum.pending == 0 && rem > ch[i].sum.nl {
                        rem -= ch[i].sum.nl;
                        hit.idx += ch[i].sum.n;
                        hit.start += ch[i].sum.bytes;
                        hit.nl_before += ch[i].sum.nl;
                        i += 1;
                        if i == ch.len() {
                            return None;
                        }
                    }
                    id = ch[i].id;
                }
            }
        }
    }

    /// 構造の検査（テスト用）: 合計値・節の大きさ・葉の深さ。
    #[cfg(test)]
    pub fn check(&self) {
        fn walk(t: &Tree, id: u32, root: bool, depth: usize, leaf_depth: &mut Option<usize>) {
            let len = t.node_len(id);
            assert!(len <= t.max, "節が大きすぎる");
            if !root {
                assert!(len >= t.min(), "節が小さすぎる {len}");
            }
            match &t.nodes[id as usize] {
                Node::Leaf(v) => {
                    assert!(v.iter().all(|p| p.len > 0), "長さ 0 のピース");
                    match leaf_depth {
                        Some(d) => assert_eq!(*d, depth, "葉の深さが違う"),
                        None => *leaf_depth = Some(depth),
                    }
                }
                Node::Inner(ch) => {
                    if root {
                        assert!(ch.len() >= 2, "根の子が 1 つ");
                    }
                    for c in ch {
                        assert_eq!(c.sum, t.node_sum(c.id), "合計値が違う");
                        walk(t, c.id, false, depth + 1, leaf_depth);
                    }
                }
            }
        }
        let mut leaf_depth = None;
        walk(self, self.root, true, 1, &mut leaf_depth);
    }
}
