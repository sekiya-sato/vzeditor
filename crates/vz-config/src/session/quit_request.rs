use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const REQUEST_FILE: &str = "quit-request";

fn claim_path(dir: &Path) -> PathBuf {
    dir.join(format!("{REQUEST_FILE}.{}", std::process::id()))
}

/// 終了要求の監視スレッドを開始する。要求を受け取ったら true になり、`repaint` を呼ぶ。
pub fn start_watcher(dir: PathBuf, repaint: impl Fn() + Send + 'static) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let flag2 = flag.clone();
    std::thread::Builder::new()
        .name("vz-quit-request".into())
        .spawn(move || {
            let request = dir.join(REQUEST_FILE);
            let claim = claim_path(&dir);
            let mut last_request = None;
            loop {
                std::thread::sleep(Duration::from_millis(300));
                let Ok(id) = fs::read_to_string(&request) else {
                    continue;
                };
                // 同じコマンドから繰り返し届く要求は一度だけ処理する。
                if last_request.as_ref() == Some(&id) || claim.exists() {
                    continue;
                }
                // 改名に成功した1プロセスだけが受け取る
                if fs::rename(&request, &claim).is_ok() {
                    last_request = Some(id);
                    flag2.store(true, Ordering::SeqCst);
                    repaint();
                }
            }
        })
        .ok();
    flag
}

/// 状態を書き終えたことを要求側へ知らせる。
pub fn finish_request(dir: &Path) {
    let _ = fs::remove_file(claim_path(dir));
}

fn wait_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    done()
}

/// 受け取り済みの要求ファイル（`{REQUEST_FILE}.<pid>`）の名前か。
fn is_claim_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy()
        .strip_prefix(REQUEST_FILE)
        .is_some_and(|rest| rest.starts_with('.'))
}

fn has_claim(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|rd| rd.flatten().any(|e| is_claim_name(&e.file_name())))
}

/// 起動中のすべての vz に「保存して終了」を要求する。受け取った vz の数を返す。
pub fn request_save_and_quit(dir: &Path) -> io::Result<usize> {
    fs::create_dir_all(dir)?;
    let request = dir.join(REQUEST_FILE);
    let request_id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let pending = dir.join(format!(".{REQUEST_FILE}-{request_id}"));
    let mut handled = 0usize;
    // 前回の失敗で残った受け取り済みファイルは消しておく（受け取った vz が居なければ完了しない）
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            if is_claim_name(&e.file_name()) {
                let _ = fs::remove_file(e.path());
            }
        }
    }
    loop {
        // 監視側が要求 ID の書きかけを読まないよう、書き終えてから公開する。
        if let Err(e) =
            fs::write(&pending, &request_id).and_then(|()| fs::rename(&pending, &request))
        {
            let _ = fs::remove_file(&pending);
            return Err(e);
        }
        // 監視は 0.3 秒ごと。最小化中などで受け取りが遅れても待てるよう余裕を持たせる
        let claimed = wait_until(Duration::from_secs(3), || !request.exists());
        if !claimed {
            let _ = fs::remove_file(&request);
            return Ok(handled);
        }
        // 状態の書き込み（保存）が終わるまで待つ
        wait_until(Duration::from_secs(30), || !has_claim(dir));
        handled += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_claim_name() {
        assert!(is_claim_name(std::ffi::OsStr::new("quit-request.123")));
        assert!(!is_claim_name(std::ffi::OsStr::new("quit-request")));
        assert!(!is_claim_name(std::ffi::OsStr::new("quit-requestx.1")));
    }

    #[test]
    fn test_request_without_running_instance_returns_zero() {
        let dir = std::env::temp_dir().join(format!("vz_session_req_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(request_save_and_quit(&dir).unwrap(), 0);
        assert!(!dir.join(REQUEST_FILE).exists());

        // 同じウィンドウが開いたままなら、別のコマンドから再試行できる。
        let flag = start_watcher(dir.clone(), || {});
        let dir2 = dir.clone();
        let flag2 = flag.clone();
        let responder = std::thread::spawn(move || {
            for _ in 0..2 {
                assert!(wait_until(Duration::from_secs(10), || flag2
                    .swap(false, Ordering::SeqCst)));
                finish_request(&dir2);
            }
        });
        assert_eq!(request_save_and_quit(&dir).unwrap(), 1);
        assert_eq!(request_save_and_quit(&dir).unwrap(), 1);
        responder.join().unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}
