//! UTF-8 テキストを対象にしたフォルダー検索。

use crate::huge::search::{scan_lines, Matcher};
use crate::huge::{Backing, DocSnapshot, HUGE_FILE_THRESHOLD};
use crate::{buffer::read_lines, search::SearchPattern};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use walkdir::WalkDir;

const RESULT_LIMIT: usize = 2_000;

#[derive(Clone, Copy, Default, PartialEq)]
pub enum GrepOutput {
    #[default]
    Line,
    Match,
    NonMatch,
}

pub struct GrepOptions {
    pub root: PathBuf,
    pub recursive: bool,
    pub include: GlobSet,
    pub exclude_files: GlobSet,
    pub exclude_dirs: GlobSet,
    pub output: GrepOutput,
    pub first_only: bool,
}

pub struct GrepHit {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub text: String,
}

#[derive(Default)]
pub struct GrepResults {
    /// 検索したフォルダ（結果のパスの表示を短くするのに使う）
    pub root: PathBuf,
    pub hits: Vec<GrepHit>,
    pub skipped: usize,
    pub limited: bool,
}

pub fn file_patterns(patterns: &str) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let pattern = if pattern == "*.*" { "*" } else { pattern };
        let glob =
            Glob::new(pattern).map_err(|err| format!("ファイル名パターンのエラー: {err}"))?;
        builder.add(glob);
    }
    builder.build().map_err(|err| err.to_string())
}

pub fn grep(pattern: &SearchPattern, options: &GrepOptions, cancelled: &AtomicBool) -> GrepResults {
    let mut results = GrepResults {
        root: options.root.clone(),
        ..Default::default()
    };
    let walker = WalkDir::new(&options.root)
        .follow_root_links(false)
        .max_depth(if options.recursive { usize::MAX } else { 1 })
        .sort_by_file_name();
    for entry in walker.into_iter().filter_entry(|entry| {
        entry.depth() == 0
            || !entry.file_type().is_dir()
            || !options.exclude_dirs.is_match(Path::new(entry.file_name()))
    }) {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let Ok(entry) = entry else {
            results.skipped += 1;
            continue;
        };
        let name = Path::new(entry.file_name());
        if !entry.file_type().is_file()
            || (!options.include.is_empty() && !options.include.is_match(name))
            || options.exclude_files.is_match(name)
        {
            continue;
        }
        // 巨大ファイルは全体を読み込まず、対応付けて順に調べる
        if entry
            .metadata()
            .is_ok_and(|m| m.len() >= HUGE_FILE_THRESHOLD)
        {
            if grep_huge_file(entry.path(), pattern, options, &mut results, cancelled).is_err() {
                results.skipped += 1;
            }
            if results.limited || cancelled.load(Ordering::Relaxed) {
                return results;
            }
            continue;
        }
        let Ok((lines, _)) = read_lines(entry.path()) else {
            results.skipped += 1;
            continue;
        };
        if lines.iter().any(|line| line.contains('\0')) {
            results.skipped += 1;
            continue;
        }
        for (line_no, text) in lines.iter().enumerate() {
            if cancelled.load(Ordering::Relaxed) {
                return results;
            }
            let mut push = |byte: usize, text: &str| {
                if results.hits.len() == RESULT_LIMIT {
                    results.limited = true;
                    return false;
                }
                let mut chars = text.chars();
                let mut snippet: String = chars.by_ref().take(300).collect();
                if chars.next().is_some() {
                    snippet.push('…');
                }
                results.hits.push(GrepHit {
                    path: entry.path().to_path_buf(),
                    line: line_no,
                    col: lines[line_no][..byte].chars().count(),
                    text: snippet,
                });
                true
            };
            let mut matches = pattern.matches(text);
            let first = matches.next();
            match options.output {
                GrepOutput::Line => {
                    if let Some(hit) = first {
                        push(hit.start(), text);
                    }
                }
                GrepOutput::Match => {
                    for hit in first.into_iter().chain(matches) {
                        if !push(hit.start(), hit.as_str()) || options.first_only {
                            break;
                        }
                    }
                }
                GrepOutput::NonMatch => {
                    if first.is_none() {
                        push(0, text);
                    }
                }
            }
            if results.limited {
                return results;
            }
            let matched = first.is_some() != (options.output == GrepOutput::NonMatch);
            if options.first_only && matched {
                break;
            }
        }
    }
    results
}

/// 巨大ファイルを対応付けて先頭から調べる（行は 0 始まり、桁は文字位置）。
fn grep_huge_file(
    path: &Path,
    pattern: &SearchPattern,
    options: &GrepOptions,
    results: &mut GrepResults,
    cancelled: &AtomicBool,
) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    // 読み取り専用で対応付け（できなければメモリに読み込み）、調べ終わったら外す
    let map = std::sync::Arc::new(Backing::open(&file, path)?);
    let snap = DocSnapshot::from_map(map);
    let matcher = Matcher::new(pattern, snap.encoding).map_err(std::io::Error::other)?;
    scan_lines(
        &snap,
        &matcher,
        options.output == GrepOutput::Match && !options.first_only,
        options.output == GrepOutput::NonMatch,
        cancelled,
        |hit| {
            if results.hits.len() == RESULT_LIMIT {
                results.limited = true;
                return false;
            }
            results.hits.push(GrepHit {
                path: path.to_path_buf(),
                line: hit.line,
                col: hit.col,
                text: hit.text,
            });
            !options.first_only
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SearchOptions;
    use std::fs;

    #[test]
    fn huge_file_path_matches_normal_path() {
        let root = std::env::temp_dir().join(format!("vz-grep-huge-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("a.txt");
        let mut text = String::new();
        for i in 0..30_000 {
            match i % 1000 {
                0 => text.push_str(&format!("{i} 猫 と 猫\r\n")),
                1 => text.push_str("\r\n"),
                _ => text.push_str(&format!("{i} 猫\r\n")),
            }
        }
        fs::write(&path, &text).unwrap();
        let cancel = AtomicBool::new(false);
        for (output, query) in [
            (GrepOutput::Line, "と"),
            (GrepOutput::Match, "と|^1 "),
            (GrepOutput::NonMatch, "猫"),
        ] {
            let options = GrepOptions {
                root: root.clone(),
                recursive: false,
                include: file_patterns("*").unwrap(),
                exclude_files: file_patterns("").unwrap(),
                exclude_dirs: file_patterns("").unwrap(),
                output,
                first_only: false,
            };
            let pattern = SearchPattern::new(
                query,
                &SearchOptions {
                    regex: true,
                    ..Default::default()
                },
            )
            .unwrap();
            let normal = grep(&pattern, &options, &cancel);
            let mut huge = GrepResults::default();
            grep_huge_file(&path, &pattern, &options, &mut huge, &cancel).unwrap();
            let key = |r: &GrepResults| -> Vec<(usize, usize, String)> {
                r.hits
                    .iter()
                    .map(|h| (h.line, h.col, h.text.clone()))
                    .collect()
            };
            assert!(!normal.hits.is_empty());
            assert_eq!(key(&huge), key(&normal), "{query}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn grep_matches_filters_and_reports_character_columns() {
        let root = std::env::temp_dir().join(format!("vz-grep-{}", std::process::id()));
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join(".git/ignored.txt"), "猫").unwrap();
        fs::write(root.join("sub/a.txt"), "字 猫 猫").unwrap();
        fs::write(root.join("ignored.log"), "猫").unwrap();
        fs::write(root.join("ignored.txt"), "猫").unwrap();
        let options = GrepOptions {
            root: root.clone(),
            recursive: true,
            include: file_patterns("*.txt").unwrap(),
            exclude_files: file_patterns("ignored*").unwrap(),
            exclude_dirs: file_patterns(".git").unwrap(),
            output: GrepOutput::Match,
            first_only: false,
        };
        let pattern = SearchPattern::new("猫", &SearchOptions::default()).unwrap();
        let results = grep(&pattern, &options, &AtomicBool::new(false));
        fs::remove_dir_all(root).unwrap();
        assert_eq!(results.hits.len(), 2);
        assert_eq!(results.hits[0].col, 2);
        assert_eq!(results.hits[1].col, 4);
        assert!(results.hits[0].path.ends_with("sub/a.txt"));
        assert_eq!(
            results.hits[0].path.strip_prefix(&results.root).ok(),
            Some(Path::new("sub").join("a.txt").as_path())
        );
    }
}
