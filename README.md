# VZ Editor

VZEditor was originally forked from [LZ Editor](https://github.com/TouryuTamane/lz-editor) developed by Touryu Tamane.

VZEditor has since evolved into an independent project with its own architecture, features, and development roadmap.

本プロジェクトは、Touryu Tamane氏が開発したLZ Editorを起源としています。
現在は独自の設計、機能、開発方針を持つ独立したプロジェクトとして開発を進めています。
具体的にはClaudeCodeとCodex、GithubDesktop などと連携する機能など。

We thank the original author and contributors for their work.

## 開発方針

- 高速起動・軽量動作、GPU（OpenGL）による描画
- VZ Editor のキー操作の継承と、サクラエディタ互換のキー割り当ての両立
- 日本語入力（IME）への対応
- Windows 11 / Linux / macOS のクロスプラットフォーム対応
- ウィンドウの最前面固定・背景透過など、作業の横に置いて使うための機能
- 前回のモニター・位置・サイズで起動（モニターを外した場合は主モニターに表示）
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
- デザインはvzeditorを踏襲します。ただしうろ覚えなので違うところもあるかもしれません。
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
| `Alt+W` | 長い行をウィンドウ幅で折り返して表示する / しないの切替 |
| `Alt+↑` / `Alt+↓` | 背景を濃く / 薄く（20〜100%、10% 刻み） |
| `Alt+0` | 背景透過を解除（100%） |
| `Alt+Shift+「+」` / `Alt+Shift+「-」` | 文字を濃く / 薄く（20〜100%、10% 刻み） |
| `Alt+Shift+0` | 文字透過を解除（100%） |
| `Alt+C` → `C` / `Alt+C` → `X` | バッファ全体を Claude Code / Codex のデスクトップアプリへ送信 |
| `Alt+C` → `N` / `Alt+C` → `M` | Claude Code / Codex のデスクトップアプリで新規セッション（アプリを前面にして Ctrl+N を押す） |
| `Alt+C` → `P` | GitHub Desktop で表示中のリポジトリを Push（GitHub Desktop を前面にして Ctrl+P を押す） |
| `Alt+K` | ターミナルの表示 / 元の画面へ戻る（Windows は WSL2 の既定シェル、Linux / macOS は bash） |
| `Alt+B` | 簡易ブラウザの表示 / 元の画面へ戻る（エディターで HTML・SVG を編集中なら、そのファイルを表示・再読み込み） |

ステータスバー右端の `TOP`・`背 n%`・`字 n%`、メニュー「ツール」からも操作できます。背景と文字の濃さは別々に変えられます。設定は次回起動時に復元されます。

複数ファイルは、ウィンドウへのドラッグ＆ドロップ、`vz a.txt b.md` のような起動引数、開くダイアログ（`F3`、サクラ互換は `Ctrl+O` も。複数選択可）、2画面ファイラーで Space マーク → Enter のいずれでもまとめて開けます。

未保存の変更（新規バッファへの入力を含む）がある状態で終了しようとすると、×ボタンや Alt+F4 でも「保存して終了 / 保存せず終了 / キャンセル」を確認します。

実行ファイルを更新するときは、`vz --save-and-quit` で起動中の vz を確認なしで終了できます（未保存のファイルは保存し、無題の内容は設定フォルダに一時保存）。置き換え後に `vz --restore` で起動すると、開いていたファイルと無題の内容が元どおりに開きます。vz での編集中に外部でもファイルが変更されていた場合は上書きせず、vz の編集内容を無題のバッファに残し、再起動後に差分を表示してどちらを採用するか確認します。

開いているファイルが外部で変更されると、再読み込みするか確認します（Y で再読み込み、N / ESC でそのまま）。

### ターミナルと簡易ブラウザ

ファイルを編集しながら、`Alt+K` でターミナル、`Alt+B` でブラウザに切り替えて確認できます。同じキーでもう一度押すと元の画面（エディター / ファイラー）へ戻ります。メニュー「ツール」からも切り替えられます。

- **ターミナル**: 初めて開いたときに、編集中のファイルのフォルダ（無題ならファイラーのフォルダ）でシェルを起動し、切り替えても動き続けます。Windows は WSL2 の既定のディストリビューション・既定のシェル（`wsl.exe`。無ければ Git Bash、PowerShell）、Linux / macOS は bash です。環境変数 `VZ_TERMINAL_SHELL` で別のシェルを指定できます。
  - `Alt+T` などの Alt 系の共通キーを除き、キーはすべてシェルへ送ります（`Ctrl+C` は中断、F キー・矢印も送る）。
  - `Ctrl+V` / `Shift+Ins` で貼り付け、`Ctrl+Shift+C` で見えている画面の文字をコピー、マウスホイールで履歴（5000 行）を表示します。日本語入力もできます。
  - シェルが終了したら `Enter` で起動し直します。
- **簡易ブラウザ**: Windows は WebView2、macOS は WKWebView をウィンドウに埋め込みます。上の欄に URL・ファイルのパス・`localhost:3000` などを入れて Enter で開きます。
  - エディターで HTML・SVG を開いた状態で `Alt+B` を押すと、そのファイルを表示します（表示中なら再読み込み）。表示するのは保存済みの内容です。
  - `F5` 再読み込み、`Alt+←` / `Alt+→` 戻る / 進む、「外部ブラウザ」で既定のブラウザで開きます。
  - ページの上では背景透過は効かず、メニューやダイアログを開いている間はページを隠します。Cookie などは設定フォルダの `webview` に保存します。
  - Linux は埋め込めないため（WebView が GTK のウィンドウを必要とし、Wayland の winit ウィンドウに入れられない）、「外部ブラウザ」で開きます。

## キー割り当て

エディター画面のキー割り当ては「サクラ互換」（既定）と「VZ 互換」を切り替えられます。切替はメニュー「ツール」→「キー割り当て」です。2画面ファイラーのキーと、上の Alt 系のキーはどちらでも同じです。

| 操作 | サクラ互換（既定） | VZ 互換（原作） |
| :--- | :--- | :--- |
| 新規 / 開く | `Ctrl+N`・`F2` / `Ctrl+O`・`F3` | `F2` / `F3` |
| 上書き保存 / 名前を付けて保存 | `Ctrl+S` / `Ctrl+Shift+S` | `F10`・`Ctrl+K` → `D` / メニュー |
| ファイル（現在のバッファ）を閉じる | `Ctrl+F4`・`F4`（未保存なら確認） | `Ctrl+F4`（未保存なら確認） |
| 挿入 / 上書きの切替 | `Ins`（上書き時はカーソルが文字の下半分） | 同左 |
| 元に戻す / やり直し | `Ctrl+Z` / `Ctrl+Y` | メニュー |
| 切り取り / コピー / 貼り付け | `Ctrl+X`・`F7` / `Ctrl+C`・`F8` / `Ctrl+V`・`F9`（選択なしは現在行） | なし / なし / `Ctrl+V` |
| 範囲選択 | `Shift`+移動キー、`F6`、`Ctrl+A`、`Ctrl+W`、マウス | なし |
| 行の切り取り / 削除 | `Ctrl+E` / `Ctrl+Shift+E` | なし / `Ctrl+Y`・`F6` |
| カーソル移動 | 矢印、`Ctrl+←→` 単語、`Home` / `End`、`Ctrl+Home` / `End` | ダイヤモンドカーソル `Ctrl+E/S/D/X`、`Ctrl+A` / `Ctrl+F`・`F7` / `F8` 単語、`Ctrl+R` 前ページ、`F4` / `F5` 行頭・行末、`Ctrl+Q` → `S/D/R/C` |
| バッファ切替 | `Ctrl+Tab` / `Ctrl+Shift+Tab` | `Ctrl+W` / `Ctrl+Shift+W` |
| 2画面ファイラー | `Esc`（選択中は選択解除） | `Esc`・`F9`・`Ctrl+K` → `Q` |
| Markdown 編集 / ビュー切替 | `Ctrl+Shift+V`・`Ctrl+P` | `Ctrl+P`・`Alt+P`・`Ctrl+K` → `V` |
| ヘルプ / バージョン情報 | `F1` / `Ctrl+Shift+F1`（クリックでも閉じる） | `F1` / メニュー（同左） |
| フォントサイズ | `Ctrl++` / `Ctrl+-` / `Ctrl+0` | 同左 |

サクラ互換の検索・置換・Grep・行ジャンプなどは未対応です（押すとステータスバーに表示）。全キーの一覧は `F1`、対応状況は [docs/キー割り当て対比.md](docs/キー割り当て対比.md) を参照してください。

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

- Windows: MSVC ツールチェーン（Visual Studio の「C++ によるデスクトップ開発」と Windows SDK）が必要です。
- Linux: 日本語フォント（`fonts-noto-cjk` など）と、ファイルダイアログ用に `zenity` を推奨。
- macOS: 追加の依存はありません。

ソースは Cargo ワークスペースで、ルートの `vz`（起動）と `crates/` の vz-ui（画面）・vz-platform（OS 固有処理）・vz-config（設定）・vz-core（編集モデル）に分かれています。詳細は [docs/基本設計.md](docs/基本設計.md) を参照してください。

## macOS での検証

開発機（Windows 11）では macOS 版のリンクと実行ができません（macOS SDK は Apple 製ハードウェアでしか使えません）。手元でできるのは型チェックまでで、テスト・ビルド・起動の確認には GitHub Actions の macOS ランナーを使います。

手元の型チェック（`cargo check` / `cargo clippy`）には Clang が要ります。依存の `objc2-exception-helper` が Objective-C のファイルを 1 つコンパイルするためです。Visual Studio Installer で「Windows 用 C++ Clang コンパイラ」を追加し、次のように実行します（パスは VS 2026 Community の例）。

```powershell
$llvm = 'C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\Llvm\x64\bin'
$env:CC_aarch64_apple_darwin = "$llvm\clang.exe"
$env:AR_aarch64_apple_darwin = "$llvm\llvm-ar.exe"
cargo check --all-targets --target aarch64-apple-darwin
```

- ワークフロー: [.github/workflows/macos-ci.yml](.github/workflows/macos-ci.yml)（手動実行のみ）。Actions タブの「macOS CI」→「Run workflow」で実行し、ランナー（`macos-latest` = Apple Silicon / `macos-15-intel` = Intel）と起動確認の有無を選びます。
- 実行内容: `cargo check --all-targets` → `cargo clippy --all-targets` → `cargo test` → `cargo build --release` → release ビルドを 15 秒起動して画面を撮る。
- 結果: 実行ページの Summary に段階ごとの ✅ / ❌ / ⏭ が出ます。すべて通ればジョブは緑で「✅ macOS CI 成功」、どれかが失敗すればジョブは赤で、失敗した段階とエラーの抜粋が出ます。
- 引き継ぎ: 成果物 `macos-ci-report-…` の `handoff.md`（結果の表・環境・エラー抜粋）を、修正するエージェントや開発者にそのまま渡します。各段階の全ログと起動時の画面も同じ成果物に入っています。
- ワークフローはリポジトリにファイルを作らず、コミットも push もしません（レポートはランナーの一時フォルダに置きます）。
- 透過・キー入力・IME・ドラッグ＆ドロップなど、画面を操作する確認は macOS の実機が必要です。実機で確認するまでは「macOS 実機未確認」の扱いです。

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
