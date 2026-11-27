//! 実行ファイルの差し替えを行うリリーススクリプト向けのコマンド。

use std::ffi::OsString;
use std::io;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use vz_config::session::{
    self,
    release::{Batch, State, STARTUP_TIMEOUT},
};

pub enum Startup {
    Normal,
    Handled,
    Restore(Box<State>),
}

fn pid(arg: &OsString) -> io::Result<u32> {
    arg.to_str()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|&pid| pid != 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "PID は正の整数が必要です"))
}

pub fn run(args: &[OsString]) -> io::Result<Startup> {
    let Some(command) = args.first().and_then(|arg| arg.to_str()) else {
        return Ok(Startup::Normal);
    };
    if !matches!(
        command,
        "--release-save-and-quit" | "--release-restore" | "--release-restore-window"
    ) {
        return Ok(Startup::Normal);
    }
    let session_dir =
        session::session_dir().ok_or_else(|| io::Error::other("設定フォルダがありません"))?;
    let id = args
        .get(1)
        .and_then(|arg| arg.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "リリース ID が必要です"))?;
    let batch = Batch::new(&session_dir, id)?;
    match (command, &args[2..]) {
        ("--release-save-and-quit", pids) => {
            batch.capture(&pids.iter().map(pid).collect::<io::Result<Vec<_>>>()?)?;
            Ok(Startup::Handled)
        }
        ("--release-restore", []) => {
            launch(&batch, id)?;
            Ok(Startup::Handled)
        }
        ("--release-restore-window", [arg]) => {
            Ok(Startup::Restore(Box::new(batch.load(pid(arg)?)?)))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "リリース用の引数が不正です",
        )),
    }
}

fn launch(batch: &Batch, id: &str) -> io::Result<()> {
    let lock = batch.lock_restore()?;
    let exe = std::env::current_exe()?;
    let mut failures = Vec::new();
    for pid in batch.recorded_pids()? {
        let result = (|| {
            let mut child = Command::new(&exe)
                .args(["--release-restore-window", id, &pid.to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            let start = Instant::now();
            while batch.restore_pending(pid)? {
                if let Some(status) = child.try_wait()? {
                    return Err(io::Error::other(format!(
                        "起動を完了できませんでした ({status})"
                    )));
                }
                if start.elapsed() >= STARTUP_TIMEOUT {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "起動完了を確認できません",
                    ));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(())
        })();
        if let Err(e) = result {
            failures.push(format!("PID {pid}: {e}"));
        }
    }
    if failures.is_empty() {
        batch.finish_restore(lock);
        Ok(())
    } else {
        Err(io::Error::other(failures.join(" / ")))
    }
}
