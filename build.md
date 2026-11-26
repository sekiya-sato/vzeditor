## Release 手順（Windows 版）

コードを修正したら最後に PowerShell で実行し、`C:\gitroot\ut\vzeditor\vz.exe` を更新する。

```powershell
$dest = 'C:\gitroot\ut\vzeditor'
cargo test
if ($LASTEXITCODE -ne 0) { throw 'cargo test failed' }
cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
New-Item -ItemType Directory -Force $dest | Out-Null
# 起動中の vz は未保存を保存して終了させ、コピー後に元の状態で起動し直す
$running = @(Get-Process vz -ErrorAction SilentlyContinue)
if ($running) {
    Start-Process target\release\vz.exe '--save-and-quit' -Wait
    $running | Wait-Process -Timeout 30 -ErrorAction SilentlyContinue
    if (Get-Process vz -ErrorAction SilentlyContinue) { throw 'vz が終了していません（終了してから再実行）' }
}
Copy-Item target\release\vz.exe $dest -Force
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

- 更新後に `vz.exe` を起動し、タイトルバーの `VZ Editor` と変更した機能を確認する。
- vz が終了しない（古いビルド・ダイアログ表示中など）とスクリプトが止まるので、ユーザーに終了を依頼する。
- 出力先はリポジトリ外。リポジトリ内に `Release\` などの出力フォルダを作らない。
- Linux / macOS のビルドは各 OS 上で `cargo build --release`。
