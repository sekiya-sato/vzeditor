# VZ Editor

A fast, lightweight, cross-platform native GUI text editor and dual-pane file manager written in Rust. Forked from [LZ Editor](https://github.com/TouryuTamane/lz-editor).

Rust 製の高速・軽量なネイティブ GUI テキストエディター＆2画面ファイラーです。[LZ Editor](https://github.com/TouryuTamane/lz-editor)（MIT License）をフォークし、背面透過・最前面固定などを追加しています。往年の VZ Editor の操作性を受け継ぎつつ、今のデスクトップ環境で使いやすいエディターを目指します。

## 開発方針

- 高速起動・軽量動作、GPU（OpenGL）による描画
- VZ Editor のキー操作の継承と、サクラエディタ互換のキー割り当ての両立
- 日本語入力（IME）への対応
- Windows 11 / Linux / macOS のクロスプラットフォーム対応
- ウィンドウの最前面固定・背景透過など、作業の横に置いて使うための機能
- 2画面ファイラーの機能拡張

## 開発状況

開発中です。独自機能の追加とクロスプラットフォーム対応を進めています。

| OS | 状況 |
| :--- | :--- |
| Windows 11 | 動作確認済み |
| Linux（Ubuntu など） | 原作は対応。フォーク後の機能は実機未確認 |
| macOS | ビルド対象。実機未確認 |

## フォーク元との関係

- このリポジトリ（[sekiya-sato/vzeditor](https://github.com/sekiya-sato/vzeditor)）は [TouryuTamane/lz-editor](https://github.com/TouryuTamane/lz-editor) のフォークです。フォーク元のコミット `adab5d2` までの履歴を含み、それ以降の変更がこのリポジトリ独自のものです。
- フォーク後に追加・変更した機能は Windows 11 でしか動作確認していません。Linux / macOS の実機では未確認です。
- 実行ファイル名は `lz` から `vz` に、パッケージ名は `vzeditor` に変えています。原作と同時にインストールしても衝突しません。
- 不具合報告や要望は、原作由来の部分も含めてこのリポジトリの Issue へお願いします。
- フォーク元の更新を取り込む場合:

```powershell
git remote add upstream https://github.com/TouryuTamane/lz-editor.git
git fetch upstream
git merge upstream/main
```

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

## 入手

[Releases](https://github.com/sekiya-sato/vzeditor/releases) に、`v` で始まるタグごとに各 OS 向けのビルドを公開します（GitHub Actions で自動ビルド）。

| ファイル | 対象 |
| :--- | :--- |
| `vz-windows-x86_64.exe` | Windows 11（x86_64） |
| `vz-macos-aarch64` | macOS（Apple Silicon） |
| `vz-linux-x86_64` / `vz-linux-aarch64` | Linux |
| `*.deb` / `*.rpm` | Ubuntu・Debian / Fedora など（`/usr/bin/vz` とデスクトップ登録） |

## ビルド

```powershell
git clone https://github.com/sekiya-sato/vzeditor.git
cd vzeditor
cargo build --release
```

実行ファイルは `target/release/vz`（Windows は `vz.exe`）です。

- Windows: MSVC ツールチェーン（Visual Studio の「C++ によるデスクトップ開発」と Windows SDK）が必要です。配布用の `vz.exe`（出力先 `C:\gitroot\ut\vzeditor\`）の作り方は [AGENTS.md](AGENTS.md) の「Release 手順」を参照してください。
- Linux: 日本語フォント（`fonts-noto-cjk` など）と、ファイルダイアログ用に `zenity` を推奨。
- macOS: 追加の依存はありません。

詳細は [docs/基本設計.md](docs/基本設計.md) を参照してください。

## ライセンス

MIT License（[LICENSE](LICENSE)）。

```text
Copyright (c) 2026 Touryu Tamane
Copyright (c) 2026 Sekiya Sato
```

同じ表示をアプリの About ダイアログにも載せています。

## 謝辞

原作 LZ Editor を開発した Touryu Tamane 氏とコントリビューターの皆さんに感謝します。

Special thanks to Touryu Tamane and the contributors of LZ Editor.
