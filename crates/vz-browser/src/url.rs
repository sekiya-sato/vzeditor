use std::path::Path;

/// ブラウザで確認する対象になるファイル（拡張子で判定）。
pub fn is_web_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["html", "htm", "xhtml", "svg"]
            .iter()
            .any(|x| e.eq_ignore_ascii_case(x))
    })
}

/// ファイルのパス → `file:///` URL（空白や日本語は % で符号化）。
pub fn file_url(path: &Path) -> String {
    let s = path.to_string_lossy();
    // 正規化した Windows のパス（\\?\C:\...）の接頭辞を外す
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// URL 欄の入力を URL にする。解釈できなければ None。
/// スキーム付きはそのまま、存在するパスは file URL、localhost・IP は http、ドメインらしいものは https。
pub fn normalize_url(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if s.contains("://") || s.starts_with("about:") || s.starts_with("data:") {
        return Some(s.to_string());
    }
    let path = Path::new(s);
    if path.is_absolute() && path.exists() {
        return Some(file_url(path));
    }
    if s.contains(char::is_whitespace) {
        return None;
    }
    let host = s.split(['/', '?', '#']).next().unwrap_or(s);
    let host_name = host.rsplit_once(':').map_or(host, |(h, _)| h);
    let is_local = host_name == "localhost"
        || host_name.ends_with(".localhost")
        || host_name.parse::<std::net::IpAddr>().is_ok();
    if is_local {
        return Some(format!("http://{s}"));
    }
    if host_name.contains('.') {
        return Some(format!("https://{s}"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_url() {
        assert_eq!(
            normalize_url("https://example.com"),
            Some("https://example.com".into())
        );
        assert_eq!(
            normalize_url("example.com/a"),
            Some("https://example.com/a".into())
        );
        assert_eq!(
            normalize_url("localhost:3000"),
            Some("http://localhost:3000".into())
        );
        assert_eq!(
            normalize_url("127.0.0.1:8080/x"),
            Some("http://127.0.0.1:8080/x".into())
        );
        assert_eq!(normalize_url("about:blank"), Some("about:blank".into()));
        assert_eq!(normalize_url("  "), None);
        assert_eq!(normalize_url("hello world"), None);
        assert_eq!(normalize_url("hello"), None);
    }

    #[test]
    fn test_normalize_existing_path() {
        let dir = std::env::temp_dir();
        let url = normalize_url(&dir.to_string_lossy()).expect("path");
        assert!(url.starts_with("file:///"), "{url}");
    }

    #[test]
    fn test_file_url_encodes() {
        assert_eq!(
            file_url(Path::new(r"\\?\C:\a b\テ.html")),
            "file:///C:/a%20b/%E3%83%86.html"
        );
        assert_eq!(
            file_url(Path::new("/home/u/x.html")),
            "file:///home/u/x.html"
        );
    }

    #[test]
    fn test_is_web_file() {
        assert!(is_web_file(Path::new("a/Index.HTML")));
        assert!(is_web_file(Path::new("x.svg")));
        assert!(!is_web_file(Path::new("x.md")));
        assert!(!is_web_file(Path::new("html")));
    }
}
