# LZ Editor

**A VZ Editor-inspired, ultra-lightweight native GUI text editor & 2-pane filer for Linux, written in Rust.**  
**Linux（Ubuntu / Raspberry Pi OS）向けに Rust で作成した、往年の VZ Editor 風・超軽量ネイティブ GUI テキストエディタ＆2画面ファイラー。**

---

## Screenshots / スクリーンショット

### Editor Mode / エディタ画面

![LZ Editor - Editor Mode](docs/screenshots/editor.png)

<!-- 2-Pane Filer screenshot: add docs/screenshots/filer.png and uncomment below
### 2-Pane Filer Mode / 2画面ファイラー

![LZ Editor - 2-Pane Filer](docs/screenshots/filer.png)
-->

---

## Why LZ Editor? / 開発の動機

Linux 向けの VZ Editor クローンとしては、これまでにも `ne` や `n8` といった優れたコマンドベース（TUI）の実装が存在します。しかし、現代の Linux デスクトップ環境（GNOME / Wayland / X11）のターミナル上でこれらを動かそうとすると、以下のような課題に直面します。

- 端末エミュレータ側が `F1`（ヘルプ）や `F10`（メニュー）、`Ctrl+S`（フロー制御）などのキーを奪ってしまう
- 日本語入力（Fcitx5 などの IME）のインライン変換や候補ウィンドウ表示が端末の仕様に左右される

そこで **LZ Editor** は、ターミナルの制約から解放された**「単一バイナリで瞬間起動するネイティブ GUI アプリ（Rust + egui）」**として設計しました。往年の VZ Editor が持っていたダイヤモンドカーソルや 2 画面ファイラーの軽快な操作感を、現代のデスクトップ環境でそのまま再現することを目指しています。

### English

There are already excellent terminal-based (TUI) VZ Editor clones for Linux, such as `ne` and `n8`. However, running them inside modern Linux desktop terminal emulators (GNOME / Wayland / X11) comes with recurring friction:

- Terminal emulators often intercept essential VZ keys such as `F1` (Help), `F10` (Menu), and `Ctrl+S` (XOFF flow control).
- Inline Japanese IME conversion (e.g., Fcitx5) and candidate popup positioning depend heavily on terminal emulator quirks.

**LZ Editor** is designed to break free from terminal constraints as a **single-binary, instant-startup native GUI application (Rust + egui)**. It aims to faithfully bring back the snappy feel of the classic VZ Editor—diamond cursor movement, instant `ESC` 2-pane filer, and multi-buffer switching—directly onto modern Linux desktops.

---

## Features / 主な特徴

- **Single Native Binary & Instant Startup / 単一バイナリ・瞬間起動**  
  Built with Rust + `eframe` (`egui` / OpenGL / Wayland & X11). Compiles into a compact ~5 MB standalone binary (`lz`).
- **Classic PC-98 / VZ Editor Aesthetic / 往年の PC-98・VZ 風デザイン**  
  Deep navy (`#081838`) background, crisp white & cyan monospace text, line numbers, newline (`↵`) & `[EOF]` markers, top status bar, and a 2-block `F1`–`F10` function key guide bar (both keyboard & mouse clickable).
- **Japanese Font Auto-Detection & Inline Fcitx5 IME / 日本語フォント自動読込＆Fcitx5 インライン変換**  
  Automatically loads system Japanese monospace fonts (`Noto Sans Mono CJK JP`, `IPAGothic`, `VL Gothic`, etc.) via `fc-match` to prevent tofu/garbled glyphs. Supports inline preedit rendering and cursor-tracking candidate popups with Fcitx5.
- **Authentic VZ Keybindings / VZ 互換キーバインド**  
  Full diamond cursor (`Ctrl+E/S/D/X`), word jump (`Ctrl+A/F`), two-stroke commands (`Ctrl+Q+S/D`, `Ctrl+K+D/Q`), line deletion (`Ctrl+Y`), and character deletion (`Ctrl+G`), overriding standard GUI shortcuts.
- **Instant `ESC` 2-Pane Filer / `ESC` 一発切替の2画面ファイラー**  
  Press `ESC` to switch instantaneously between the editor and a dual-pane file manager (`[L]` / `[R]`) with independent directories, multi-file marking (`Space`), sorting, and file operations (copy, move, delete, rename, create file/folder).
- **Multi-Buffer Switching / 複数バッファ（裏画面）切替**  
  Open multiple files from the filer and cycle through them smoothly with `Ctrl+W`, with per-buffer cursor & scroll memory and a buffer strip in the status area.
- **Real-Time Font Size Scaling / リアルタイム・フォントサイズ変更 (v0.2.0)**  
  Zoom in/out/reset (`Ctrl++` / `Ctrl+-` / `Ctrl+0`, or click `[-] 16pt [+]` in the status bar) across both the editor and 2-pane filer, with automatic line height, cursor sizing, and Fcitx5 IME position tracking.
- **Markdown Edit / View Mode Toggle / Markdown 編集・ビュー切替 (v0.2.0)**  
  Press `Ctrl+P`, `Ctrl+K+V`, `Alt+P`, or click `[EDIT]`/`[VIEW]` in the status bar to switch instantaneously between plain text editing and a VZ-navy-themed Markdown preview (headings, bold/italic/code, framed code blocks, lists/tasks, quotes, rules, links, and tables) while retaining keyboard scrolling, `ESC` filer switching, and `Ctrl+W` buffer switching.
- **Compact GUI Menu Bar, Undo/Redo, Clipboard & Dynamic Title Bar / スリム GUI メニューバー・Undo/Redo・クリップボード・動的タイトル表示 (v0.2.0)**  
  Slim VZ-navy top menu bar (`ファイル` / `編集` / `ツール` / `About`) designed never to steal focus from VZ keybindings, providing Save As, Close Buffer, Undo/Redo history, Cut/Copy/Paste clipboard integration, and an About dialog. The OS window title bar updates live with the current filename, dirty state (`*`), or active 2-pane filer directory.

---

## Keybindings / キーバインド

### Editor Mode / エディタ画面

| Key / キー | Action (EN) | 機能 (JA) |
| :--- | :--- | :--- |
| `Ctrl+E` / `S` / `D` / `X` | Diamond cursor Up / Left / Right / Down | ダイヤモンドカーソル移動（ビュー時は上下スクロール） |
| `Ctrl+A` / `Ctrl+F` | Move word Left / Right (JP & ASCII aware) | 単語単位の左・右移動（日本語・英数対応） |
| `Ctrl+Q+S` / `Ctrl+Q+D` | Line start / Line end (`Home` / `End`) | 行頭・行末移動（`Home` / `End` も可） |
| `Ctrl+Q+R` / `Ctrl+Q+C` | Document top / bottom | ファイル先頭・末尾移動 |
| `Ctrl+Y` | Delete current line | 現在行の削除 |
| `Ctrl+G` / `Delete` | Delete character at cursor | カーソル位置の1文字削除 |
| `BackSpace` / `Ctrl+H` | Delete previous character | 前1文字削除 |
| `Ctrl+W` (`Ctrl+Shift+W`) | Switch next (previous) buffer | 複数バッファ（裏画面）切替 |
| `Ctrl+P` / `Ctrl+K+V` / `Alt+P` | Toggle Markdown Edit / View mode | Markdown 編集 / ビュー（プレビュー）切替 |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | Zoom font size in / out / reset | フォントサイズ拡大・縮小・リセット |
| `Ctrl+K+D` / `F10` | Save / Save As dialog | 上書き保存 / 名前を付けて保存 |
| `ESC` / `Ctrl+K+Q` / `F9` | Switch to 2-Pane Filer | 2画面ファイラーへ切替 |
| `F1` | Toggle keybinding help | キーバインドヘルプ表示 |
| `F2` / `F3` | New buffer / Open file dialog | 新規バッファ / ファイルを開く |

### 2-Pane Filer Mode / 2画面ファイラー画面

| Key / キー | Action (EN) | 機能 (JA) |
| :--- | :--- | :--- |
| `Ctrl+E` / `Ctrl+X` (`↑` / `↓`) | Move file cursor Up / Down | ファイルカーソル移動 |
| `Tab` | Toggle Left / Right active pane | 左右ペイン（アクティブ窓）トグル切替 |
| `Ctrl+S` / `Ctrl+D` (`←` / `→`) | Focus Left / Right pane | 左窓 / 右窓へアクティブ切替 |
| `Enter` / Double-click | Enter directory or open file in buffer | フォルダ移動 / ファイルを新規バッファで開く |
| `BackSpace` / `Ctrl+H` | Go to parent directory (`..`) | 親ディレクトリへ移動 |
| `Space` | Toggle file mark & move down | ファイルマーク（複数選択・反転表示） |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | Zoom font size in / out / reset | フォントサイズ拡大・縮小・リセット |
| `F2` / `F3` / `F7` | Rename / Create new file / Create folder | リネーム / 新規ファイル作成 / フォルダ作成 |
| `F4` / `F5` / `F6` | Copy / Move (to opposite pane) / Delete | 反対側窓へコピー / 移動 / 削除 |
| `F8` | Cycle sort order (Name / Date / Size / Ext) | ソート順切替（名前・更新・サイズ・拡張子） |
| `ESC` / `F9` | Return to Editor | エディタ画面へ戻る |
| `F10` | Quit LZ Editor | アプリ終了（未保存時は確認） |

---

## Installation / インストール方法

Prebuilt packages (`.deb`, `.rpm`) and standalone binaries for both `x86_64` and `aarch64` are available on the [GitHub Releases](https://github.com/TouryuTamane/lz-editor/releases) page.  
`x86_64` / `aarch64` 向けのパッケージ（`.deb` / `.rpm`）および単一バイナリを [GitHub Releases](https://github.com/TouryuTamane/lz-editor/releases) からダウンロードできます。

### Debian / Ubuntu / Raspberry Pi OS (`.deb`)

Installing the `.deb` package places `lz` in `/usr/bin/lz` and registers the desktop launcher & icon automatically.  
`.deb` パッケージをインストールすると `/usr/bin/lz` に配置され、アプリメニュー（デスクトップランチャー・アイコン）にも自動登録されます。

```bash
# x86_64 (amd64)
sudo apt install ./lz-editor_*_amd64.deb

# Raspberry Pi OS / ARM64 (arm64)
sudo apt install ./lz-editor_*_arm64.deb
```

### Fedora / RHEL / Rocky Linux / AlmaLinux (`.rpm`)

```bash
# x86_64
sudo dnf install ./lz-editor-*.x86_64.rpm

# ARM64 (aarch64)
sudo dnf install ./lz-editor-*.aarch64.rpm
```

### Standalone Binary / 単一バイナリ

```bash
# Example for x86_64 (use lz-linux-aarch64 on ARM64 / Raspberry Pi)
chmod +x lz-linux-x86_64
sudo mv lz-linux-x86_64 /usr/local/bin/lz
lz
```

---

## Requirements & Build / 動作環境とビルド

### Requirements / 前提環境
- Linux desktop environment (tested on **Raspberry Pi OS / Debian Trixie** & **Ubuntu GNOME**, Wayland / X11)
- Rust toolchain (`rustc` / `cargo`, install via [rustup.rs](https://rustup.rs/)) *(only needed when building from source)*
- Japanese monospace font installed on the system (e.g., `fonts-noto-cjk`, `fonts-ipafont-gothic`, or `fonts-vlgothic`)
- Optional: `zenity` or `kdialog` for native Save/Open dialogs (falls back to built-in inline path prompt if unavailable)

### Build from Source / ソースからビルド

```bash
git clone https://github.com/TouryuTamane/lz-editor.git
cd lz-editor

# Run tests / テスト実行
cargo test

# Release build / リリースビルド
cargo build --release
cp target/release/lz ./lz

# Launch / 起動
./lz
./lz examples/welcome_ja.txt
```

---

## Roadmap / ロードマップ

- [x] **Phase 1 (v0.1.0)**: Core VZ Editor GUI, Japanese font auto-loading, Fcitx5 inline IME, VZ diamond cursor & editing keybindings, Save/Open
- [x] **Phase 2 (v0.1.1)**: Instant `ESC` 2-pane VZ Filer (copy/move/delete/rename/mkdir/mark/sort), `Ctrl+W` multi-buffer switching, `.deb` / `.rpm` packaging
- [x] **v0.2.0**: Real-time font size scaling (`Ctrl++`/`-`/`0`), Markdown Edit/View mode (`Ctrl+P` / `Ctrl+K+V` / `Alt+P`), VZ-safe GUI menu bar, Undo/Redo, clipboard operations, dynamic OS window title bar
- [ ] **Phase 3 (Planned)**: Search & replace (`Ctrl+Q+F` / `Ctrl+Q+A`), Shift_JIS / EUC-JP encoding detection & conversion, customizable config file

---

## License

[MIT License](LICENSE) © 2026 Touryu Tamane
