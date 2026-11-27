## Release 手順1（Windows 版）

PowerShell で実行します。既定の配置先は `C:\gitroot\ut\vzeditor\vz.exe` です。

```powershell
.\scripts\release-windows.ps1
```

## Release 手順2（Ubuntu WSL2 版）

Ubuntu WSL2 で、リポジトリのルートから実行します。配置先は `~/bin/vz` です。ビルド出力は `CARGO_TARGET_DIR`、未設定なら `~/vz-target` に置きます。

glibc 2.35（Ubuntu 22.04）以降で動くよう `cargo zigbuild` でビルドします。初回は `uv tool install cargo-zigbuild`（zig も入ります）を実行してください。

```bash
bash scripts/release-wsl.sh
```

## 複数ウィンドウの引き継ぎ

- スクリプトはテストとリリースビルドの後、配置先の実行ファイルから起動した VZ を列挙します。起動時と同じユーザー・設定フォルダを使います。
- 対象の全 PID が状態を記録し、終了したことを確認してから実行ファイルを入れ替えます。終了要求が失敗した場合、配置先はそのままです。
- 差し替え後は旧 PID ごとの記録からウィンドウを起動します。復元と画面起動が完了したプロセスの記録だけを削除します。Windows では About、WSL2 では About と `type -a vz` で確認できます。
- 中断時は、表示されたリリース ID を使い、同機能に対応する実行ファイルで `vz --release-restore <リリースID>` を実行すると、保存済みで残っている PID の記録を再試行できます。差し替えが済んでいれば配置先の VZ を使います。
- 旧ビルドがリリース終了要求に未対応の場合、スクリプトは対象を終了する前に停止します。初回のみ、各ウィンドウを Alt+Shift+F4 で記録して閉じた後に配置し、記録した数だけ Shift を押しながら起動します。
