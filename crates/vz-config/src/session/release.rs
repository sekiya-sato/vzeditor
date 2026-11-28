//! リリースに限って、旧 PID ごとの状態を終了前に受け渡す。

use super::{read_session, unix_nanos, SessionEntry, SessionWindow};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(90);

fn release_root(session_dir: &Path) -> PathBuf {
    session_dir.join("release")
}

fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit())
}

fn lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

fn try_lock(file: &File) -> io::Result<()> {
    file.try_lock().map_err(|e| match e {
        TryLockError::WouldBlock => io::ErrorKind::WouldBlock.into(),
        TryLockError::Error(e) => e,
    })
}

fn instance_dir(root: &Path, pid: u32) -> PathBuf {
    root.join("instances").join(pid.to_string())
}

fn instance_token(dir: &Path, pid: u32) -> io::Result<String> {
    let active = File::open(dir.join("active"))?;
    match try_lock(&active) {
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
            let token = fs::read_to_string(dir.join("token"))?;
            if token.is_empty() {
                return Err(io::Error::other(format!(
                    "PID {pid} の起動を確認できません"
                )));
            }
            Ok(token)
        }
        Err(e) => Err(e),
        Ok(()) => Err(io::Error::other(format!("PID {pid} は終了しています"))),
    }
}

#[derive(Debug)]
pub struct Batch {
    root: PathBuf,
    id: String,
}

impl Batch {
    pub fn new(session_dir: &Path, id: &str) -> io::Result<Self> {
        if !valid_id(id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "リリース ID は32桁の16進数が必要です",
            ));
        }
        Ok(Self {
            root: release_root(session_dir),
            id: id.into(),
        })
    }

    fn dir(&self) -> PathBuf {
        self.root.join("batches").join(&self.id)
    }

    fn state_dir(&self, pid: u32) -> PathBuf {
        self.dir().join(pid.to_string())
    }

    pub fn has_state(&self, pid: u32) -> bool {
        self.state_dir(pid).is_dir()
    }

    pub fn restore_pending(&self, pid: u32) -> io::Result<bool> {
        let dir = self.state_dir(pid);
        Ok(dir.try_exists()? || dir.with_extension("restored").try_exists()?)
    }

    /// 全 PID の対応を確かめてから要求を出し、各 PID の公開完了を待つ。
    pub fn capture(&self, pids: &[u32]) -> io::Result<()> {
        fs::create_dir_all(self.root.join("batches"))?;
        let coordinator = lock_file(&self.root.join("capture.lock"))?;
        try_lock(&coordinator)?;
        let mut unique = pids.to_vec();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() != pids.len() || unique.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "PID は正の整数で重複できません",
            ));
        }
        let targets = unique
            .into_iter()
            .map(|pid| {
                let dir = instance_dir(&self.root, pid);
                let token = instance_token(&dir, pid).map_err(|e| {
                    io::Error::other(format!("PID {pid} はリリース終了を受け付けられません: {e}"))
                })?;
                Ok((pid, dir.join(&self.id), token))
            })
            .collect::<io::Result<Vec<_>>>()?;
        fs::create_dir(self.dir())?;
        let result = (|| {
            for (_, request, token) in &targets {
                let pending = request.with_extension("tmp");
                fs::write(&pending, token.as_bytes())?;
                fs::rename(&pending, request)?;
            }
            let start = Instant::now();
            loop {
                for (pid, _, _) in &targets {
                    if let Ok(error) = fs::read_to_string(self.dir().join(format!(".failed-{pid}")))
                    {
                        return Err(io::Error::other(format!("PID {pid}: {error}")));
                    }
                }
                if targets.iter().all(|(pid, _, _)| self.has_state(*pid)) {
                    return Ok(());
                }
                if start.elapsed() >= STARTUP_TIMEOUT {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "状態の保存完了を確認できません",
                    ));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        })();
        for (_, request, _) in &targets {
            let _ = fs::remove_file(request);
            let _ = fs::remove_file(request.with_extension("tmp"));
        }
        result
    }

    /// 公開済みの記録だけを列挙する。保存中の一時ディレクトリには触らない。
    pub fn recorded_pids(&self) -> io::Result<Vec<u32>> {
        let mut pids = Vec::new();
        for entry in fs::read_dir(self.dir())? {
            let entry = entry?;
            if let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() {
                if entry.file_type()?.is_dir() {
                    pids.push(pid);
                }
            }
        }
        pids.sort_unstable();
        Ok(pids)
    }

    pub fn lock_restore(&self) -> io::Result<File> {
        let lock = lock_file(&self.dir().join(".restore.lock"))?;
        try_lock(&lock)?;
        Ok(lock)
    }

    pub fn finish_restore(&self, lock: File) {
        drop(lock);
        let _ = fs::remove_file(self.dir().join(".restore.lock"));
        let _ = fs::remove_dir(self.dir());
    }

    /// 起動途中でプロセスが落ちても、記録を元の場所から再取得できる。
    pub fn load(&self, pid: u32) -> io::Result<State> {
        let dir = self.state_dir(pid);
        let lock_path = self.dir().join(format!(".restore-{pid}.lock"));
        let lock = lock_file(&lock_path)?;
        try_lock(&lock)?;
        let (entries, window) = read_session(&dir)?;
        if entries.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "状態ファイルに復元項目がありません",
            ));
        }
        Ok(State {
            dir,
            lock_path,
            lock,
            entries,
            window,
        })
    }
}

#[derive(Debug)]
pub struct State {
    dir: PathBuf,
    lock_path: PathBuf,
    lock: File,
    pub entries: Vec<SessionEntry>,
    pub window: SessionWindow,
}

impl State {
    /// 復元済みの記録を再列挙から外し、そこに含まれる一時ファイルを削除する。
    pub fn finish(self) -> io::Result<()> {
        let done = self.dir.with_extension("restored");
        fs::rename(&self.dir, &done)?;
        let result = fs::remove_dir_all(done);
        drop(self.lock);
        let _ = fs::remove_file(self.lock_path);
        result
    }
}

pub struct Request {
    batch: Batch,
    pid: u32,
}

impl Request {
    /// 書き終えたディレクトリを改名してから、要求元が保存完了と判断できるようにする。
    pub fn publish(self, save: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> {
        let writing = self.batch.dir().join(format!(".writing-{}", self.pid));
        let result = fs::create_dir(&writing).and_then(|()| {
            let result = save(&writing).and_then(|()| {
                for entry in fs::read_dir(&writing)? {
                    File::options()
                        .write(true)
                        .open(entry?.path())?
                        .sync_all()?;
                }
                fs::rename(&writing, self.batch.state_dir(self.pid))
            });
            if result.is_err() {
                let _ = fs::remove_dir_all(&writing);
            }
            result
        });
        if let Err(e) = &result {
            let _ = fs::write(
                self.batch.dir().join(format!(".failed-{}", self.pid)),
                e.to_string(),
            );
        }
        result
    }
}

pub struct Watcher {
    requests: mpsc::Receiver<Request>,
    stopping: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Watcher {
    pub fn start(session_dir: &Path, repaint: impl Fn() + Send + 'static) -> io::Result<Self> {
        Self::start_for_pid(session_dir, std::process::id(), repaint)
    }

    fn start_for_pid(
        session_dir: &Path,
        pid: u32,
        repaint: impl Fn() + Send + 'static,
    ) -> io::Result<Self> {
        let root = release_root(session_dir);
        let dir = instance_dir(&root, pid);
        fs::create_dir_all(&dir)?;
        let lock = lock_file(&dir.join("active"))?;
        try_lock(&lock)?;
        let token = unix_nanos().to_string();
        fs::write(dir.join("token"), &token)?;
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&stopping);
        let (send, requests) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("vz-release-request".into())
            .spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if let Ok(entries) = fs::read_dir(&dir) {
                        for entry in entries.flatten() {
                            let id = entry.file_name().to_string_lossy().into_owned();
                            if !valid_id(&id) {
                                continue;
                            }
                            let path = entry.path();
                            let Ok(expected_token) = fs::read_to_string(&path) else {
                                continue;
                            };
                            if fs::remove_file(&path).is_ok() && expected_token == token {
                                if send
                                    .send(Request {
                                        batch: Batch {
                                            root: root.clone(),
                                            id,
                                        },
                                        pid,
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                                repaint();
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                let _ = fs::remove_file(dir.join("token"));
                drop(lock);
                let _ = fs::remove_file(dir.join("active"));
                let _ = fs::remove_dir(dir);
            })?;
        Ok(Self {
            requests,
            stopping,
            thread: Some(thread),
        })
    }

    pub fn next(&self) -> Option<Request> {
        self.requests.try_recv().ok()
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{write_session, EntryKind};

    const ID: &str = "0123456789abcdef0123456789abcdef";

    fn wait_request(watcher: &Watcher) -> Request {
        let start = Instant::now();
        loop {
            if let Some(request) = watcher.next() {
                return request;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "終了要求が届きません"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn multiple_pids_keep_independent_records_until_each_startup_succeeds() {
        const WORKER: &str = "VZ_RELEASE_TEST_WORKER";
        if let Some(root) = std::env::var_os(WORKER) {
            let pid = std::process::id();
            let watcher = Watcher::start(Path::new(&root), || {}).unwrap();
            wait_request(&watcher)
                .publish(|dir| {
                    let temp = dir.join("unsaved.txt");
                    fs::write(&temp, format!("edit-{pid}"))?;
                    write_session(
                        dir,
                        &[SessionEntry {
                            kind: EntryKind::Unsaved,
                            active: true,
                            temp: Some(temp),
                            ..Default::default()
                        }],
                        &SessionWindow::default(),
                    )
                })
                .unwrap();
            return;
        }

        let root = std::env::temp_dir().join(format!("vz_release_multi_{}", std::process::id()));
        let batch = Batch::new(&root, ID).unwrap();
        let mut workers: Vec<_> = (0..2)
            .map(|_| {
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "session::release::tests::multiple_pids_keep_independent_records_until_each_startup_succeeds",
                    ])
                    .env(WORKER, &root)
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        let mut pids: Vec<_> = workers.iter().map(std::process::Child::id).collect();
        pids.sort_unstable();
        let deadline = Instant::now() + Duration::from_secs(10);
        for &pid in &pids {
            while instance_token(&instance_dir(&batch.root, pid), pid).is_err() {
                assert!(
                    Instant::now() < deadline,
                    "PID {pid} が監視を開始しませんでした"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        batch.capture(&pids).unwrap();
        for worker in &mut workers {
            assert!(worker.wait().unwrap().success());
        }
        let first_pid = pids[0];
        let second_pid = pids[1];
        assert_eq!(batch.recorded_pids().unwrap(), pids);
        let lock = batch.lock_restore().unwrap();
        let first = batch.load(first_pid).unwrap();
        assert_eq!(
            batch.load(first_pid).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        let temp = first.entries[0].temp.clone().unwrap();
        assert_eq!(
            fs::read_to_string(&temp).unwrap(),
            format!("edit-{first_pid}")
        );
        drop(first);
        assert!(batch.restore_pending(first_pid).unwrap());
        assert!(temp.is_file());

        batch.load(first_pid).unwrap().finish().unwrap();
        assert!(!batch.restore_pending(first_pid).unwrap());
        assert!(!temp.exists());
        let second = batch.load(second_pid).unwrap();
        assert_eq!(
            fs::read_to_string(second.entries[0].temp.as_ref().unwrap()).unwrap(),
            format!("edit-{second_pid}")
        );
        second.finish().unwrap();
        batch.finish_restore(lock);
        assert!(!batch.dir().exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_pid_leaves_all_targets_running_and_write_failure_is_reported() {
        let root = std::env::temp_dir().join(format!("vz_release_failure_{}", std::process::id()));
        let watcher = Watcher::start_for_pid(&root, 39, || {}).unwrap();
        let batch = Batch::new(&root, ID).unwrap();
        assert!(batch
            .capture(&[39, 40])
            .unwrap_err()
            .to_string()
            .contains("PID 40"));
        assert!(!batch.dir().exists());
        assert!(watcher.next().is_none());

        let worker = std::thread::spawn(move || {
            let err = wait_request(&watcher)
                .publish(|dir| {
                    fs::write(dir.join("unsaved.txt"), "partial data")?;
                    Err(io::Error::other("write failure"))
                })
                .unwrap_err();
            assert!(err.to_string().contains("write failure"));
        });
        assert!(batch
            .capture(&[39])
            .unwrap_err()
            .to_string()
            .contains("write failure"));
        worker.join().unwrap();
        assert!(batch.recorded_pids().unwrap().is_empty());
        assert!(!batch.dir().join(".writing-39").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
