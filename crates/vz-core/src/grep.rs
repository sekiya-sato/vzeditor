//! UTF-8 テキストを対象にしたフォルダー検索。

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
    let mut results = GrepResults::default();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SearchOptions;
    use std::fs;

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
    }
}
