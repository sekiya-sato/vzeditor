//! ローカルの VZ インスタンス間で編集タブを受け渡す。
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tempfile::{Builder, NamedTempFile, TempDir, TempPath};

fn valid_name(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.len() == 6 && suffix.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

fn private_dir(root: &Path, prefix: &str) -> io::Result<TempDir> {
    fs::create_dir_all(root)?;
    let mut builder = Builder::new();
    builder.prefix(prefix).rand_bytes(6);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    builder.tempdir_in(root)
}

fn name(dir: &TempDir) -> &str {
    // tempfile が付けた ASCII の名前。
    dir.path()
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
}

pub struct Inbox {
    dir: TempDir,
    registration: PathBuf,
    requests: mpsc::Receiver<Request>,
    worker: Option<JoinHandle<()>>,
    stop: Option<mpsc::Sender<()>>,
}

impl Inbox {
    pub fn start(root: &Path, pid: u32, wake: impl Fn() + Send + 'static) -> io::Result<Self> {
        let dir = private_dir(root, "inbox-")?;
        let registration = root.join(pid.to_string());
        let mut pending = NamedTempFile::new_in(root)?;
        use std::io::Write;
        pending.write_all(name(&dir).as_bytes())?;
        let (send, requests) = mpsc::channel();
        let mut inbox = Self {
            dir,
            registration,
            requests,
            worker: None,
            stop: None,
        };
        pending.persist(&inbox.registration)?;
        let path = inbox.dir.path().to_path_buf();
        let (stop, stopping) = mpsc::channel();
        inbox.worker = Some(thread::Builder::new().name("vz-tab-inbox".into()).spawn(
            move || loop {
                while let Some(request) = Self::read_request(&path) {
                    if send.send(request).is_err() {
                        return;
                    }
                    wake();
                }
                if stopping.recv_timeout(Duration::from_millis(200))
                    != Err(mpsc::RecvTimeoutError::Timeout)
                {
                    break;
                }
            },
        )?);
        inbox.stop = Some(stop);
        Ok(inbox)
    }

    pub fn next(&self) -> Option<Request> {
        self.requests.try_recv().ok()
    }

    /// 受信票は一度だけ取り出す。編集内容は送信元が応答まで保持する。
    fn read_request(inbox: &Path) -> Option<Request> {
        let root = inbox.parent()?;
        for entry in fs::read_dir(inbox).ok()?.flatten() {
            let id = entry.file_name();
            let Some(id) = id.to_str().filter(|id| valid_name(id, "offer-")) else {
                continue;
            };
            if fs::remove_file(entry.path()).is_ok() {
                if let Some(request) = Request::open(root, id) {
                    return Some(request);
                }
            }
        }
        None
    }
}

impl Drop for Inbox {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_file(&self.registration);
    }
}

pub struct Offer(TempDir);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pending,
    Accepted,
    Rejected,
}

impl Offer {
    pub fn new(root: &Path) -> io::Result<Self> {
        Ok(Self(private_dir(root, "offer-")?))
    }

    pub fn path(&self) -> &Path {
        self.0.path()
    }

    pub fn id(&self) -> &str {
        name(&self.0)
    }

    pub fn publish(&self) -> io::Result<()> {
        File::create(self.path().join("pending"))?;
        Ok(())
    }

    /// 同じ設定フォルダの VZ が相手なら受信票を置く。
    pub fn send_to(&self, root: &Path, pid: u32) -> io::Result<bool> {
        let inbox = match fs::read_to_string(root.join(pid.to_string())) {
            Ok(name) if valid_name(&name, "inbox-") => root.join(name),
            Ok(_) => return Err(io::Error::other("移動先の識別子が無効です")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        match File::create(inbox.join(self.id())) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub fn outcome(&self) -> Outcome {
        if self.path().join("accepted").is_file() {
            Outcome::Accepted
        } else if self.path().join("rejected").is_file() {
            Outcome::Rejected
        } else {
            Outcome::Pending
        }
    }

    pub fn cancel(&self) -> Outcome {
        let _ = fs::rename(self.path().join("pending"), self.path().join("rejected"));
        self.outcome()
    }
}

/// 受信した巨大ファイルの作業コピーを、受理後も残る一時ファイルへ移す。
pub fn take_work_file(source: &Path) -> io::Result<TempPath> {
    let root = source
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::other("作業ファイルのフォルダがありません"))?;
    let dest = Builder::new()
        .prefix("work-")
        .tempfile_in(root)?
        .into_temp_path();
    fs::rename(source, &dest)?;
    Ok(dest)
}

pub struct Request(PathBuf);

impl Request {
    pub fn open(root: &Path, id: &str) -> Option<Self> {
        if !valid_name(id, "offer-") {
            return None;
        }
        let dir = root.join(id);
        dir.join("pending").is_file().then_some(Self(dir))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// 受理・拒否・送信側からの取消のうち、先に pending を移動できたものだけが成立する。
    pub fn finish(&self, accepted: bool) -> io::Result<()> {
        let result = if accepted { "accepted" } else { "rejected" };
        fs::rename(self.0.join("pending"), self.0.join(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offer_is_delivered_once_and_acknowledged() {
        let root = tempfile::tempdir().unwrap();
        let inbox = Inbox::start(root.path(), 42, || {}).unwrap();
        let offer = Offer::new(root.path()).unwrap();
        assert!(!offer.send_to(root.path(), 43).unwrap());
        assert!(Request::open(root.path(), "../offer-abcdef").is_none());
        offer.publish().unwrap();
        assert!(offer.send_to(root.path(), 42).unwrap());
        let request = inbox.requests.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(request.path(), offer.path());
        assert!(inbox.next().is_none());
        request.finish(true).unwrap();
        assert_eq!(offer.cancel(), Outcome::Accepted);
    }

    #[test]
    fn canceled_offer_cannot_be_accepted() {
        let root = tempfile::tempdir().unwrap();
        let offer = Offer::new(root.path()).unwrap();
        offer.publish().unwrap();
        let request = Request::open(root.path(), offer.id()).unwrap();
        assert_eq!(offer.cancel(), Outcome::Rejected);
        assert!(request.finish(true).is_err());
    }
}
