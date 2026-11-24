# VZ Editor

Rust 製の高速ネイティブ GUI テキストエディター。[LZ Editor](https://github.com/TouryuTamane/lz-editor)（MIT License）をもとに、Windows 11 / Linux / macOS 対応と、背面透過・最前面固定を追加しています。

## 追加機能

| キー | 機能 |
| :--- | :--- |
| `Alt+T` | 最前面固定の切替 |
| `Alt+↑` / `Alt+↓` | 背景を濃く / 薄く（20〜100%、10% 刻み） |
| `Alt+0` | 背景透過を解除（100%） |
| `Alt+Shift+「+」` / `Alt+Shift+「-」` | 文字を濃く / 薄く（20〜100%、10% 刻み） |
| `Alt+Shift+0` | 文字透過を解除（100%） |
| `Alt+C` → `C` / `Alt+C` → `X` | バッファ全体を Claude Code / Codex のデスクトップアプリへ送信 |

ステータスバー右端の `TOP`・`透過 n%`・`文字 n%`、メニュー「ツール」からも操作できます。背景と文字の濃さは別々に変えられます。設定は次回起動時に復元されます。

複数ファイルは、ウィンドウへのドラッグ＆ドロップ、`vz a.txt b.md` のような起動引数、F3 の開くダイアログ（複数選択）、2画面ファイラーで Space マーク → Enter のいずれでもまとめて開けます。

未保存の変更（新規バッファへの入力を含む）がある状態で終了しようとすると、×ボタンや Alt+F4 でも「保存して終了 / 保存せず終了 / キャンセル」を確認します。

## キー割り当て

エディター画面のキー割り当ては「サクラ互換」（既定）と「VZ 互換」を切り替えられます。切替はメニュー「ツール」→「キー割り当て」、またはステータスバー右端の `サクラ` / `VZ` ボタンです。

- サクラ互換: サクラエディタと同じ Ctrl+S / Ctrl+Z / Ctrl+X・C・V、Shift+移動キーやマウスでの範囲選択、Ctrl+E 行切り取りなど。Esc で2画面ファイラー（選択中は選択解除）、Ctrl+Shift+V で Markdown ビュー。
- VZ 互換: 原作と同じ `Ctrl+E/S/D/X`、`Ctrl+Q`/`Ctrl+K` 系、`ESC` で2画面ファイラーなど。

一覧は `F1` で表示できます。対応状況は [docs/キー割り当て対比.md](docs/キー割り当て対比.md) を参照してください。

## ビルド

```powershell
cargo build --release
```

- Windows: MSVC ツールチェーン（Visual Studio の「C++ によるデスクトップ開発」と Windows SDK）が必要です。配布用の `vz.exe`（出力先 `C:\gitroot\ut\vzeditor\`）の作り方は [AGENTS.md](AGENTS.md) の「Release 手順」を参照してください。
- Linux: 日本語フォント（`fonts-noto-cjk` など）と、ファイルダイアログ用に `zenity` を推奨。
- macOS: 追加の依存はありません。

詳細は [docs/基本設計.md](docs/基本設計.md) を参照してください。

## ライセンス

MIT License。原作 LZ Editor の著作権表示を [LICENSE](LICENSE) に含みます。
