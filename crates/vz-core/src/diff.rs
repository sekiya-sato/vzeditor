//! 行単位の差分（外部での変更との衝突を確認するダイアログ用）。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// 両方にある行（前後の文脈）
    Same,
    /// 古い側（ファイル）にだけある行
    Removed,
    /// 新しい側（vz の編集）にだけある行
    Added,
    /// 省略した同じ行の区切り
    Skip,
}

/// DP 表の大きさの上限。超えたら変わった範囲をまとめて「削除 → 追加」として扱う。
const MAX_CELLS: usize = 4_000_000;

/// `old` から `new` への差分を、変更箇所の前後 `context` 行だけ残して返す。
/// 各要素は（種類, 行番号の表示, 本文）。行番号は old / new の 1 始まり。
pub fn unified(old: &[String], new: &[String], context: usize) -> Vec<(DiffKind, String)> {
    let ops = diff_ops(old, new);
    if ops.iter().all(|(k, _, _)| *k == DiffKind::Same) {
        return Vec::new();
    }
    // 変更行の近く（context 行以内）の Same だけ残す
    let n = ops.len();
    let mut keep = vec![false; n];
    for (i, (k, _, _)) in ops.iter().enumerate() {
        if *k != DiffKind::Same {
            let lo = i.saturating_sub(context);
            let hi = (i + context + 1).min(n);
            for flag in &mut keep[lo..hi] {
                *flag = true;
            }
        }
    }
    let mut out = Vec::new();
    let mut skipped = false;
    for (i, (kind, oi, ni)) in ops.iter().enumerate() {
        if !keep[i] {
            skipped = true;
            continue;
        }
        if skipped {
            out.push((DiffKind::Skip, "...".to_string()));
            skipped = false;
        }
        let text = match kind {
            DiffKind::Removed => format!("{:>5}      - {}", oi + 1, old[*oi]),
            DiffKind::Added => format!("      {:>5} + {}", ni + 1, new[*ni]),
            _ => format!("{:>5} {:>5}   {}", oi + 1, ni + 1, old[*oi]),
        };
        out.push((*kind, text));
    }
    if skipped {
        out.push((DiffKind::Skip, "...".to_string()));
    }
    out
}

/// 差分の操作列（種類, old の位置, new の位置）。
fn diff_ops(old: &[String], new: &[String]) -> Vec<(DiffKind, usize, usize)> {
    // 先頭・末尾の共通部分を除いてから中央部分を LCS で比べる
    let mut pre = 0;
    while pre < old.len() && pre < new.len() && old[pre] == new[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < old.len() - pre && suf < new.len() - pre && old[old.len() - 1 - suf] == new[new.len() - 1 - suf] {
        suf += 1;
    }
    let a = &old[pre..old.len() - suf];
    let b = &new[pre..new.len() - suf];

    let mut ops = Vec::with_capacity(old.len().max(new.len()));
    for i in 0..pre {
        ops.push((DiffKind::Same, i, i));
    }
    if a.len().saturating_mul(b.len()) > MAX_CELLS {
        for i in 0..a.len() {
            ops.push((DiffKind::Removed, pre + i, pre));
        }
        for j in 0..b.len() {
            ops.push((DiffKind::Added, pre + a.len(), pre + j));
        }
    } else {
        // lcs[i][j] = a[i..] と b[j..] の LCS の長さ
        let (n, m) = (a.len(), b.len());
        let w = m + 1;
        let mut lcs = vec![0u32; (n + 1) * w];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[i * w + j] = if a[i] == b[j] {
                    lcs[(i + 1) * w + j + 1] + 1
                } else {
                    lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && a[i] == b[j] {
                ops.push((DiffKind::Same, pre + i, pre + j));
                i += 1;
                j += 1;
            } else if i < n && (j == m || lcs[(i + 1) * w + j] >= lcs[i * w + j + 1]) {
                // 同じ長さなら削除を先に出す
                ops.push((DiffKind::Removed, pre + i, pre + j));
                i += 1;
            } else {
                ops.push((DiffKind::Added, pre + i, pre + j));
                j += 1;
            }
        }
    }
    for k in 0..suf {
        ops.push((DiffKind::Same, old.len() - suf + k, new.len() - suf + k));
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_unified_diff() {
        let old = lines(&["a", "b", "c", "d", "e", "f", "g"]);
        let new = lines(&["a", "b", "C", "d", "e", "f", "g", "h"]);
        let d = unified(&old, &new, 1);
        let kinds: Vec<DiffKind> = d.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            kinds,
            vec![
                DiffKind::Skip,
                DiffKind::Same,
                DiffKind::Removed,
                DiffKind::Added,
                DiffKind::Same,
                DiffKind::Skip,
                DiffKind::Same,
                DiffKind::Added,
            ]
        );
        assert!(d[2].1.ends_with("- c"));
        assert!(d[3].1.ends_with("+ C"));
        assert!(d[7].1.ends_with("+ h"));
        // 同じなら空
        assert!(unified(&old, &old, 2).is_empty());
        // 片方が空
        assert_eq!(unified(&[], &lines(&["x"]), 2).len(), 1);
    }
}
