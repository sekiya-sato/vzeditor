//! 起動するシェルを決める。bash を優先する（Windows は Git Bash）。

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl ShellCommand {
    /// `cwd` で起動するときの引数。wsl.exe は利用者の設定でホームへ移ることがあるので `--cd` で渡す。
    pub fn args_for(&self, cwd: &str) -> Vec<String> {
        let is_wsl = self
            .program
            .file_stem()
            .is_some_and(|s| s.eq_ignore_ascii_case("wsl"));
        let mut args = Vec::with_capacity(self.args.len() + 2);
        if is_wsl && !self.args.iter().any(|a| a == "--cd") {
            args.push("--cd".to_string());
            args.push(cwd.to_string());
        }
        args.extend(self.args.iter().cloned());
        args
    }
}

/// 環境変数 `VZ_TERMINAL_SHELL` で指定したシェル（引数なし）。
fn from_env() -> Option<ShellCommand> {
    let program = std::env::var_os("VZ_TERMINAL_SHELL")?;
    if program.is_empty() {
        return None;
    }
    Some(ShellCommand {
        program: PathBuf::from(program),
        args: Vec::new(),
    })
}

/// PATH から `name` を探す。
fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// 既定のシェル。
/// - Windows: WSL2 の既定シェル（`wsl.exe`）。無ければ Git Bash（`bash --login -i`）、PowerShell の順。
/// - Linux / macOS: bash。無ければ `$SHELL`、`/bin/sh`。
///
/// どちらも環境変数 `VZ_TERMINAL_SHELL` があればそれを使う。
pub fn default_shell() -> ShellCommand {
    if let Some(cmd) = from_env() {
        return cmd;
    }
    platform_default()
}

#[cfg(windows)]
fn platform_default() -> ShellCommand {
    // WSL2: 既定のディストリビューションの既定シェル。作業フォルダは WSL が /mnt/c/... に変換する
    let wsl = std::env::var_os("SystemRoot")
        .map(|root| Path::new(&root).join(r"System32\wsl.exe"))
        .filter(|p| p.is_file())
        .or_else(|| find_in_path("wsl.exe"));
    if let Some(wsl) = wsl {
        return ShellCommand {
            program: wsl,
            args: Vec::new(),
        };
    }
    if let Some(bash) = git_bash() {
        return ShellCommand {
            program: bash,
            args: vec!["--login".into(), "-i".into()],
        };
    }
    ShellCommand {
        program: PathBuf::from("powershell.exe"),
        args: vec!["-NoLogo".into()],
    }
}

/// Git for Windows の bash.exe。
#[cfg(windows)]
fn git_bash() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(var) {
            candidates.push(Path::new(&dir).join(r"Git\bin\bash.exe"));
        }
    }
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(Path::new(&dir).join(r"Programs\Git\bin\bash.exe"));
    }
    // PATH の git.exe（…\Git\cmd\git.exe など）から Git のフォルダを求める
    if let Some(git) = find_in_path("git.exe") {
        candidates.extend(git_root_candidates(&git).map(|root| root.join(r"bin\bash.exe")));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// git.exe の場所から Git のインストール先の候補（`cmd\` `bin\` `mingw64\bin\` の親）。
#[cfg_attr(not(windows), allow(dead_code))]
fn git_root_candidates(git_exe: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    git_exe.ancestors().skip(2).take(2).map(Path::to_path_buf)
}

#[cfg(not(windows))]
fn platform_default() -> ShellCommand {
    let program = [
        "/bin/bash",
        "/usr/bin/bash",
        "/usr/local/bin/bash",
        "/opt/homebrew/bin/bash",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
    .or_else(|| find_in_path("bash"))
    .or_else(|| std::env::var_os("SHELL").map(PathBuf::from))
    .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    ShellCommand {
        program,
        args: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_root_candidates() {
        let git = Path::new("/opt/Git/cmd/git.exe");
        let roots: Vec<PathBuf> = git_root_candidates(git).collect();
        assert_eq!(
            roots,
            vec![PathBuf::from("/opt/Git"), PathBuf::from("/opt")]
        );
    }

    #[test]
    fn test_wsl_gets_cd_argument() {
        // `\` は Windows でしか区切りにならないので join で組み立てる（他 OS のテストでも末尾が WSL.exe になる）
        let wsl = ShellCommand {
            program: Path::new("C:\\Windows").join("System32").join("WSL.exe"),
            args: Vec::new(),
        };
        assert_eq!(
            wsl.args_for(r"C:\work"),
            vec!["--cd".to_string(), r"C:\work".to_string()]
        );
        let bash = ShellCommand {
            program: PathBuf::from("/bin/bash"),
            args: vec!["-l".into()],
        };
        assert_eq!(bash.args_for("/tmp"), vec!["-l".to_string()]);
    }

    #[test]
    fn test_default_shell_has_program() {
        assert!(!default_shell().program.as_os_str().is_empty());
    }
}
