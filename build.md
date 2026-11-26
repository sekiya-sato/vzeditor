## Release 手順1（Windows 版）

コードを修正したら最後に PowerShell で実行し、`C:\gitroot\ut\vzeditor\vz.exe` を更新する。

```powershell
$dest = 'C:\gitroot\ut\vzeditor'
cargo test
if ($LASTEXITCODE -ne 0) { throw 'cargo test failed' }
cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
New-Item -ItemType Directory -Force -ErrorAction Stop $dest | Out-Null
# 起動中の vz は未保存を保存して終了させ、コピー後に元の状態で起動し直す
$running = @(Get-Process vz -ErrorAction SilentlyContinue)
if ($running) {
    Start-Process target\release\vz.exe '--save-and-quit' -Wait
    $running | Wait-Process -Timeout 30 -ErrorAction SilentlyContinue
    if (Get-Process vz -ErrorAction SilentlyContinue) { throw 'vz が終了していません（終了してから再実行）' }
}
Copy-Item target\release\vz.exe $dest -Force -ErrorAction Stop
if ($running) { Start-Process (Join-Path $dest 'vz.exe') '--restore' }
# LICENSE・README.md は変更があった場合のみコピーする
foreach ($f in 'LICENSE', 'README.md') {
    $to = Join-Path $dest $f
    if (-not (Test-Path $to) -or (Get-FileHash $f).Hash -ne (Get-FileHash $to).Hash) {
        Copy-Item $f $dest -Force
        "copied: $f"
    }
}
```

## Release 手順2（Linux WSL2 版）

Ubuntu WSL2 の Bash でリポジトリのルート（`/mnt/c/gitroot/new2022/vzeditor`）から実行する。WSL2 側の Rust と C リンカーを使う。Cargo の出力先は既定で Linux 側の `~/vz-target` とし、`CARGO_TARGET_DIR` が設定済みならその値を使う。

```bash
(
    set -eu
    target_dir="${CARGO_TARGET_DIR:-$HOME/vz-target}"
    install_dir="$HOME/bin"
    dest="$install_dir/vz"

    cargo test --locked --target-dir "$target_dir"
    cargo build --locked --release --bin vz --target-dir "$target_dir"

    mkdir -p -- "$install_dir"
    pending=$(mktemp "$install_dir/.vz.XXXXXX")
    trap 'rm -f -- "$pending"' EXIT
    install -m 0755 -- "$target_dir/release/vz" "$pending"
    mv -fT -- "$pending" "$dest"
    cmp -- "$target_dir/release/vz" "$dest"
    printf '配置済み: %s\n' "$dest"
)
```

- テストとビルドが通ってから、`~/bin` 内の一時ファイルを `vz` に rename する。起動中の Linux 版は次回起動時に更新版へ切り替わる。
- WSLg で `"$HOME/bin/vz"` を起動し、About のビルド日時と変更した機能を確認する。`vz` で起動する場合は `type -a vz` で優先される実行ファイルを確認し、必要に応じて `~/bin` を `PATH` の先頭に追加する。
- 作業中のタブを引き継ぐ場合は、起動中の Linux 版と同じユーザー・`XDG_CONFIG_HOME` で `"$HOME/bin/vz" --save-and-quit` を実行する。ウィンドウが実際に終了したことを確認してから `"$HOME/bin/vz" --restore` で起動する。

## 補足事項

- Windows 版の更新後は配置先の `vz.exe` を起動し、About のビルド日時と変更した機能を確認する。
- `--save-and-quit` の終了コードは終了要求を受け取ったインスタンスの数。ウィンドウが残った場合は画面で理由を確認し、ダイアログを処理するか保存先の問題を解消してから再試行する。
- 終了後に配置が失敗した場合は、配置をやり直してから、その版の実行ファイルを `--restore` 付きで起動する。`--restore` は保存したセッション一覧を読み取ると削除する。
- 出力先はリポジトリ外。リポジトリ内に `Release\` などの出力フォルダを作らない。
