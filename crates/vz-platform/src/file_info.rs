//! ファイルの情報（ハードリンクの数）。巨大ファイルの保存は一時ファイルを rename で置き換えるため、
//! ハードリンクがあるとリンクが切れる。保存の前にこの数を見て確かめる。

use std::path::Path;

/// ファイルのハードリンクの数。調べられなければ None。
pub fn hard_link_count(path: &Path) -> Option<u64> {
    imp::hard_link_count(path)
}

#[cfg(windows)]
mod imp {
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    pub fn hard_link_count(path: &Path) -> Option<u64> {
        let file = std::fs::File::open(path).ok()?;
        // SAFETY: BY_HANDLE_FILE_INFORMATION は整数だけの構造体で、すべて 0 は正しい値
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: file が生きている間の有効なハンドルと、書き込める構造体を渡す
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
        (ok != 0).then_some(u64::from(info.nNumberOfLinks))
    }
}

#[cfg(unix)]
mod imp {
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    pub fn hard_link_count(path: &Path) -> Option<u64> {
        std::fs::metadata(path).ok().map(|m| m.nlink())
    }
}

#[cfg(not(any(windows, unix)))]
mod imp {
    use std::path::Path;

    pub fn hard_link_count(_path: &Path) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hard_link_count() {
        let dir = std::env::temp_dir().join(format!("vz_links_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.txt");
        std::fs::write(&a, "x").unwrap();
        assert_eq!(hard_link_count(&a), Some(1));
        std::fs::hard_link(&a, dir.join("b.txt")).unwrap();
        assert_eq!(hard_link_count(&a), Some(2));
        assert_eq!(hard_link_count(&dir.join("none.txt")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
