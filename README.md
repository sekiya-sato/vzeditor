# VZ Editor

Rust 製の高速ネイティブ GUI テキストエディター。[LZ Editor](https://github.com/TouryuTamane/lz-editor)（MIT License）をもとに、Windows 11 / Linux / macOS 対応と、背面透過・最前面固定を追加しています。

## 追加機能

| キー | 機能 |
| :--- | :--- |
| `Alt+T` | 最前面固定の切替 |
| `Alt+↑` / `Alt+↓` | 背景を濃く / 薄く（20〜100%、10% 刻み） |
| `Alt+0` | 背景透過を解除（100%） |

ステータスバー右端の `TOP` と `透過 n%`、メニュー「ツール」からも操作できます。透過するのは背景だけで、文字は不透明のままです。設定は次回起動時に復元されます。

VZ 互換キー（`Ctrl+E/S/D/X`、`Ctrl+Q`/`Ctrl+K` 系、`ESC` で2画面ファイラー など）は原作と同じです。一覧は `F1` で表示できます。

## ビルド

```powershell
cargo build --release
```

- Windows: MSVC ツールチェーン（Visual Studio の「C++ によるデスクトップ開発」と Windows SDK）が必要です。ビルド後、`target\release\vz.exe` を `Release\` に置きます。
- Linux: 日本語フォント（`fonts-noto-cjk` など）と、ファイルダイアログ用に `zenity` を推奨。
- macOS: 追加の依存はありません。

詳細は [docs/基本設計.md](docs/基本設計.md) を参照してください。

## ライセンス

MIT License。原作 LZ Editor の著作権表示を [LICENSE](LICENSE) に含みます。
