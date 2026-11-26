//! 画面本体 `VzEditorApp`。状態・初期化・`eframe::App` と共通の補助関数を置き、
//! 処理は役割ごとの子モジュールに分ける（docs/基本設計.md の「クレート構成」）。

mod bars;
mod browser_view;
mod dialogs;
mod editor_view;
mod filer_view;
mod files;
mod huge;
mod input;
mod keys;
mod menu;
mod restore;
mod search;
mod send;
mod terminal_view;
#[cfg(test)]
mod tests;
mod theme;
mod window;
mod wrap;

use crate::fonts::configure_japanese_fonts;
use crate::markdown::{parse_markdown, render_markdown_view, MdBlock};
use crate::window_style::{self, bg};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, Event, FontId, ImeEvent, Key, Pos2, Rect, Sense, Stroke,
    StrokeKind, Vec2,
};
use search::{SearchDialog, SearchState};
use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use theme::*;
use vz_config::session::{self, EntryKind, SessionEntry};
use vz_config::settings::{KeyScheme, Settings, WindowGeometry};
use vz_core::buffer::{char_to_byte_idx, expand_tabs_for_display, FileStamp, TextBuffer, TextPos};
use vz_core::diff::{self, DiffKind};
use vz_core::encoding::TextEncoding;
use vz_core::filer::{
    copy_entry, create_dir, create_file, delete_entry, format_size, move_entry, rename_entry,
    ActivePane, FileEntry, FilerState,
};
use vz_core::huge::HugeDoc;
use vz_platform::dialog::{spawn_open_dialog, spawn_save_dialog, DialogResult};
use vz_platform::send_to_app::{prepare_text, spawn_send, SendResult, SendTarget};
use vz_platform::{clipboard, monitor};
use wrap::WrapLayout;

// --- Font Size Scaling Constants ---
const DEFAULT_FONT_SIZE: f32 = 16.0;
const MIN_FONT_SIZE: f32 = 10.0;
const MAX_FONT_SIZE: f32 = 36.0;
const FONT_SIZE_STEP: f32 = 2.0;

fn calc_line_height(font_size: f32) -> f32 {
    (font_size * 1.45).round().max(font_size + 4.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppMode {
    Editor,
    Filer,
    /// 組み込みターミナル（Alt+K）
    Terminal,
    /// 簡易ブラウザ（Alt+B）
    Browser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrefixKey {
    None,
    CtrlQ,
    CtrlK,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorAction {
    ToggleHelp,
    NewBuffer,
    MoveLineStart,
    MoveLineEnd,
    DeleteLine,
    MoveWordLeft,
    MoveWordRight,
    SwitchToFiler,
    SaveFile,
    // サクラ互換の F キー
    Cut,
    Copy,
    Paste,
    BeginSelect,
    CloseBuffer,
    /// 次を検索（F3）または前を検索（Shift+F3）
    RepeatSearch(bool),
    /// 行の複製（Shift+F10）
    DuplicateLine,
    /// 検索語の行マーク切替（サクラ互換 Ctrl+F3）
    ToggleSearchMarks,
    /// 終了（Alt+F4。ctx が要るので F キー欄の側で request_quit を呼ぶ）
    Quit,
    /// 割り当てなし（F キー欄の空欄）
    Nop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilerAction {
    ToggleHelp,
    Rename,
    NewFile,
    Copy,
    Move,
    Delete,
    NewDir,
    CycleSort,
    BackToEditor,
    Quit,
    /// 割り当てなし（F キー欄の空欄）
    Nop,
}

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// ビルド日時（build.rs が埋め込む。例: 2026/11/24 12:00）
const BUILD_AT: &str = env!("VZ_BUILD_AT");
/// 原作 LZ Editor (MIT License)
const BASE_REPO_URL: &str = "https://github.com/TouryuTamane/lz-editor";
/// このフォーク（VZ Editor）のリポジトリ。Cargo.toml の repository から取る。
const FORK_REPO_URL: &str = env!("CARGO_PKG_REPOSITORY");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TopMenu {
    File,
    Edit,
    Search,
    Tools,
    About,
}

impl TopMenu {
    const ALL: [TopMenu; 5] = [
        TopMenu::File,
        TopMenu::Edit,
        TopMenu::Search,
        TopMenu::Tools,
        TopMenu::About,
    ];

    fn label(self) -> &'static str {
        match self {
            TopMenu::File => "ファイル",
            TopMenu::Edit => "編集",
            TopMenu::Search => "検索",
            TopMenu::Tools => "ツール",
            TopMenu::About => "About",
        }
    }

    fn next(self) -> Self {
        match self {
            TopMenu::File => TopMenu::Edit,
            TopMenu::Edit => TopMenu::Search,
            TopMenu::Search => TopMenu::Tools,
            TopMenu::Tools => TopMenu::About,
            TopMenu::About => TopMenu::File,
        }
    }

    fn prev(self) -> Self {
        match self {
            TopMenu::File => TopMenu::About,
            TopMenu::Edit => TopMenu::File,
            TopMenu::Tools => TopMenu::Search,
            TopMenu::Search => TopMenu::Edit,
            TopMenu::About => TopMenu::Tools,
        }
    }
}

/// 「保存して終了」の進み具合。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuitSaveStep {
    /// 未保存のバッファが無くなった
    Done,
    /// 無題のバッファ（索引）の保存先を尋ねる必要がある
    NeedSaveAs(usize),
    /// 巨大ファイルの確認または保存を待つ
    WaitingForHugeSave,
    /// 保存に失敗した
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    // File
    NewBuffer,
    OpenFile,
    SaveFile,
    SaveAs,
    /// 文字コードを選んで保存する（キーの割り当てなし）
    SaveWithEncoding,
    CloseBuffer,
    ToggleFiler,
    Quit,
    // Edit
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    DeleteLine,
    MoveLineStart,
    MoveLineEnd,
    ToggleOverwrite,
    // Search
    OpenSearch(SearchDialog),
    RepeatSearch(bool),
    ToggleSearchMarks,
    // Tools
    ToggleMarkdownView,
    FontZoomIn,
    FontZoomOut,
    FontReset,
    NextBuffer,
    ToggleAlwaysOnTop,
    ToggleWordWrap,
    OpacityUp,
    OpacityDown,
    OpacityReset,
    TextOpacityUp,
    TextOpacityDown,
    TextOpacityReset,
    SendToClaudeCode,
    SendToCodex,
    PushGitHubDesktop,
    NewSessionClaudeCode,
    NewSessionCodex,
    ToggleKeyScheme,
    SelectAll,
    ToggleTerminal,
    ToggleBrowser,
    NewBrowserTab,
    CloseBrowserTab,
    // About
    ShowHelp,
    ShowAbout,
}

#[derive(Debug, Clone)]
enum MenuEntry {
    Item {
        label: &'static str,
        shortcut: &'static str,
        action: MenuAction,
        enabled: bool,
    },
    Separator,
}

#[derive(Debug, Clone)]
struct InlinePathPrompt {
    is_save: bool,
    path_input: String,
    just_opened: bool,
}

#[derive(Debug, Clone)]
enum FilerModal {
    Rename {
        src: PathBuf,
        input: String,
        just_opened: bool,
    },
    NewFile {
        dir: PathBuf,
        input: String,
        just_opened: bool,
    },
    NewDir {
        dir: PathBuf,
        input: String,
        just_opened: bool,
    },
    ConfirmCopyOrMove {
        is_move: bool,
        targets: Vec<FileEntry>,
        dst_dir: PathBuf,
    },
    ConfirmDelete {
        targets: Vec<FileEntry>,
    },
    ConfirmCloseBuffer {
        buf_idx: usize,
        name: String,
    },
    ConfirmQuit,
    /// 巨大ファイルの保存でハードリンクが切れる。保存するか確認する
    ConfirmHardLinkSave {
        path: PathBuf,
        encoding: TextEncoding,
    },
    /// 文字コードを選んで保存する（保存できなかった理由を message に出す）
    ChooseEncoding {
        selected: TextEncoding,
        message: String,
    },
    /// 開いているファイルが外部で変更された。再読み込みするか確認する
    ConfirmReload {
        buf_idx: usize,
        stamp: FileStamp,
        /// 表示した時刻（直後の打鍵で答えてしまわないようにする）
        shown: std::time::Instant,
    },
    /// 前回の保存して終了で外部の変更と衝突した。差分を見せて、どちらを採用するか確認する
    ResolveConflict {
        path: PathBuf,
        diff: Vec<(DiffKind, String)>,
        shown: std::time::Instant,
    },
}

struct BufferSlot {
    /// 同名のタブや、ドラッグ中に位置が変わったタブも識別する。
    tab_id: usize,
    buffer: TextBuffer,
    scroll_x: f32,
    /// 縦スクロール位置（ピクセル。巨大ファイルの行数でも精度が足りるよう f64）
    scroll_y: f64,
    markdown_view: bool,
    md_scroll_y: f32,
    md_max_scroll_y: f32,
    md_viewport_h: f32,
    md_scroll_dirty: bool,
    md_cache_rev: Option<u64>,
    md_blocks: Vec<MdBlock>,
    /// 折り返し表示の行割り（折り返しモードのときだけ作る）
    wrap: Option<WrapLayout>,
    /// 行マークを付けたときの文書リビジョンと論理行。
    search_marks: Option<(u64, Vec<usize>)>,
    /// 外部での変更を「再読み込みしない」と答えたときのファイルの状態（同じ変更では再度尋ねない）
    ignored_stamp: Option<FileStamp>,
    /// 外部の変更と衝突して上書きしなかった vz の編集内容を持つ無題バッファなら、元のファイルのパス
    conflict_of: Option<PathBuf>,
    /// 巨大ファイルを裏のスレッドで保存中
    huge_save: Option<huge::HugeSaveJob>,
    /// 巨大ファイルの検索（裏のスレッド）
    huge_find: Option<huge::HugeFindJob>,
    /// 巨大ファイルのすべて置換（裏のスレッド）
    huge_replace: Option<huge::HugeReplaceJob>,
    /// 巨大ファイルの行ジャンプで、索引が届くのを待っている行
    pending_jump: Option<usize>,
    /// 巨大ファイルの横スクロール位置（各行の何文字目から表示するか）
    huge_hcol: usize,
    /// 巨大ファイルの折り返し表示で、先頭に表示している (論理行, 行内の表示行)
    huge_wrap_top: (usize, usize),
    /// 巨大ファイルの折り返しの計算結果（表示中の行だけ）
    huge_wrap: wrap::HugeWrapCache,
    /// 巨大ファイルの折り返し表示のホイールの端数（ピクセル）
    huge_wrap_wheel: f32,
}

impl BufferSlot {
    fn new(buffer: TextBuffer) -> Self {
        static NEXT_TAB_ID: AtomicUsize = AtomicUsize::new(0);
        Self {
            tab_id: NEXT_TAB_ID.fetch_add(1, Ordering::Relaxed),
            wrap: None,
            search_marks: None,
            buffer,
            scroll_x: 0.0,
            scroll_y: 0.0,
            markdown_view: false,
            md_scroll_y: 0.0,
            md_max_scroll_y: 0.0,
            md_viewport_h: 400.0,
            md_scroll_dirty: false,
            md_cache_rev: None,
            md_blocks: Vec::new(),
            ignored_stamp: None,
            conflict_of: None,
            huge_save: None,
            huge_find: None,
            huge_replace: None,
            pending_jump: None,
            huge_hcol: 0,
            huge_wrap_top: (0, 0),
            huge_wrap: wrap::HugeWrapCache::default(),
            huge_wrap_wheel: 0.0,
        }
    }

    fn ensure_md_cache(&mut self) {
        if self.md_cache_rev != Some(self.buffer.revision) {
            self.md_blocks = parse_markdown(self.buffer.text_lines());
            self.md_cache_rev = Some(self.buffer.revision);
        }
    }

    fn is_untouched_untitled(&self) -> bool {
        self.buffer.file_path.is_none()
            && !self.buffer.dirty
            && matches!(self.buffer.text_lines(), [line] if line.is_empty())
    }
}

pub struct VzEditorApp {
    mode: AppMode,
    buffers: Vec<BufferSlot>,
    active_buf: usize,
    filer: FilerState,

    prefix: PrefixKey,
    status_message: String,
    ime_preedit: String,
    ime_active: bool,
    last_window_title: String,
    internal_clipboard: String,

    // Rendering / scrolling state
    font_size: f32,
    line_height: f32,
    need_scroll_to_cursor: bool,
    last_visible_lines: usize,
    last_filer_visible_rows: usize,

    // Menu bar & UI overlays / async native dialogs
    open_menu: Option<TopMenu>,
    menu_selected_idx: usize,
    show_help: bool,
    show_about: bool,
    /// ヘルプ / About が前のフレームから表示中か。開いたフレームのクリック（メニュー操作）で閉じないための判定。
    help_click_armed: bool,
    about_click_armed: bool,
    pending_dialog: Option<Receiver<DialogResult>>,
    inline_prompt: Option<InlinePathPrompt>,
    filer_modal: Option<FilerModal>,
    search: SearchState,

    // Window style: background-only transparency & always-on-top
    bg_opacity: u8,
    text_opacity: u8,
    always_on_top: bool,
    /// false in tests so that user settings are never overwritten.
    persist_settings: bool,

    // Quit confirmation (未保存時の終了確認)
    /// 「保存して終了」で保存ダイアログの完了を待っている
    quit_after_save: bool,
    /// 確認済みのため次の close_requested を止めない
    allow_close: bool,

    // Claude Code / Codex への送信 (Alt+C → C / X)
    /// Alt+C を押して2打目を待っている
    send_prefix: bool,
    /// 送信中の処理（別スレッド）
    pending_send: Option<Receiver<SendResult>>,

    // キー割り当て（サクラ互換 / VZ 互換）
    key_scheme: KeyScheme,
    /// サクラの F6「範囲選択開始」: 移動キーで選択を広げるモード
    select_mode: bool,
    /// Ins キーで切り替える上書きモード（false = 挿入）。保存しない
    overwrite_mode: bool,
    /// フレームの最後にクリップボードへ書き出す文字列（ctx を持たない処理から使う）
    clipboard_out: Option<String>,

    /// `vz --save-and-quit` からの終了要求（監視スレッドが true にする）。テストでは None
    quit_request: Option<Arc<AtomicBool>>,
    /// 外部での変更を最後に調べた時刻
    last_disk_check: std::time::Instant,
    /// 復元後に確認する、外部の変更と衝突したファイル
    conflict_queue: Vec<PathBuf>,

    /// 長い行をウィンドウ幅で折り返して表示する（Alt+W）
    word_wrap: bool,
    /// 直前の描画で求めた折り返し幅（セル数）。キー操作の表示行移動に使う
    wrap_cols: usize,

    /// 最後に確認したウィンドウの位置とサイズ（終了時に settings.ini へ保存する）
    window_geometry: Option<WindowGeometry>,
    /// 起動直後に合わせる前回の位置（合わせ終わるまで window_geometry を更新しない）
    pending_window_restore: Option<WindowGeometry>,

    /// 組み込みターミナル（Alt+K で初めて開いたときに起動する）
    terminal: Option<vz_terminal::Terminal>,
    /// ターミナルのマウス選択（離した時点でクリップボードへコピー）
    term_selection: Option<terminal_view::TermSelection>,
    /// 復元したターミナルを起動するフォルダ（最初のフレームで起動する）
    pending_terminal_cwd: Option<PathBuf>,
    /// ターミナル・ブラウザから戻る画面（エディターかファイラー）
    return_mode: AppMode,
    /// 簡易ブラウザのタブ（空ならブラウザは閉じている。WebView は表示するときに作る）
    browsers: Vec<browser_view::BrowserTab>,
    /// 選択中のブラウザのタブ
    active_browser: usize,
    /// WebView を置く領域（直前の描画で決めた、論理座標）
    browser_view_rect: Option<Rect>,
    /// URL 欄に入力中（ページの読み込みで書き換えない）
    browser_url_editing: bool,
}

impl VzEditorApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        initial_paths: Vec<PathBuf>,
        settings: Settings,
        restore: bool,
    ) -> Self {
        configure_japanese_fonts(&cc.egui_ctx);
        Self::configure_visuals(&cc.egui_ctx);
        // Disable egui's built-in Ctrl++/-/0 UI zoom so VZ Editor controls font scaling directly
        // without double-scaling or breaking IME candidate window coordinates.
        cc.egui_ctx.options_mut(|o| {
            o.zoom_with_keyboard = false;
        });

        let status_message = match settings.key_scheme {
            KeyScheme::Sakura => String::from(
                "サクラ互換: ^S保存  Shift+移動で選択  ^Tabバッファ切替  Escファイラー  F1ヘルプ（ツール→キー割り当てで VZ 互換へ）",
            ),
            KeyScheme::Vz => String::from(
                "VZ互換: ^E/^S/^D/^X移動  ^Wバッファ切替  ^P MDビュー  ESCファイラー  F1ヘルプ",
            ),
        };
        let mut initial_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        if let Some(first) = initial_paths.first() {
            let dir = if first.is_dir() {
                Some(first.as_path())
            } else {
                first.parent()
            };
            if let Some(dir) = dir {
                if !dir.as_os_str().is_empty() && dir.exists() {
                    initial_dir = dir.to_path_buf();
                }
            }
        }
        let initial_buffer = TextBuffer::new();

        let font_size = settings.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        let bg_opacity = window_style::set_opacity_percent(settings.opacity);
        let text_opacity = window_style::set_text_opacity_percent(settings.text_opacity);

        let mut app = Self {
            mode: AppMode::Editor,
            buffers: vec![BufferSlot::new(initial_buffer)],
            active_buf: 0,
            filer: FilerState::new(initial_dir),
            prefix: PrefixKey::None,
            status_message,
            ime_preedit: String::new(),
            ime_active: false,
            last_window_title: String::new(),
            internal_clipboard: String::new(),
            font_size,
            line_height: calc_line_height(font_size),
            need_scroll_to_cursor: false,
            last_visible_lines: 24,
            last_filer_visible_rows: 20,
            open_menu: None,
            menu_selected_idx: 0,
            show_help: false,
            show_about: false,
            help_click_armed: false,
            about_click_armed: false,
            pending_dialog: None,
            inline_prompt: None,
            filer_modal: None,
            search: SearchState::default(),
            bg_opacity,
            text_opacity,
            always_on_top: settings.always_on_top,
            persist_settings: true,
            quit_after_save: false,
            allow_close: false,
            send_prefix: false,
            pending_send: None,
            key_scheme: settings.key_scheme,
            select_mode: false,
            overwrite_mode: false,
            clipboard_out: None,
            quit_request: None,
            last_disk_check: std::time::Instant::now(),
            conflict_queue: Vec::new(),
            word_wrap: settings.word_wrap,
            wrap_cols: 80,
            window_geometry: settings.window,
            pending_window_restore: settings.window,
            terminal: None,
            term_selection: None,
            pending_terminal_cwd: None,
            return_mode: AppMode::Editor,
            browsers: Vec::new(),
            active_browser: 0,
            browser_view_rect: None,
            browser_url_editing: false,
        };
        let session_dir = session::session_dir();
        if restore {
            if let Some(dir) = &session_dir {
                app.restore_session_from(dir);
            }
        }
        if !initial_paths.is_empty() {
            // 存在しないパスは新規ファイルとして開く（従来の `vz 新規.txt` と同じ）
            app.open_paths(initial_paths, true);
        }
        if let Some(dir) = session_dir {
            let ctx = cc.egui_ctx.clone();
            app.quit_request = Some(session::start_watcher(dir, move || ctx.request_repaint()));
        }
        app.sync_window_title(&cc.egui_ctx);
        app
    }

    fn configure_visuals(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::TRANSPARENT;
        visuals.window_fill = STATUS_BG;
        visuals.override_text_color = Some(TEXT_WHITE);
        visuals.widgets.noninteractive.bg_fill = BG_NAVY;
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.fg_stroke.color = TEXT_WHITE;
        }
        // OS のテーマに切り替わっても、紺色の背景と白文字の組み合わせを維持する。
        ctx.set_theme(egui::Theme::Dark);
        ctx.set_visuals_of(egui::Theme::Dark, visuals);
    }

    fn cur_slot(&self) -> &BufferSlot {
        &self.buffers[self.active_buf]
    }

    fn cur_slot_mut(&mut self) -> &mut BufferSlot {
        &mut self.buffers[self.active_buf]
    }

    fn cur_buf(&self) -> &TextBuffer {
        &self.buffers[self.active_buf].buffer
    }

    fn cur_buf_mut(&mut self) -> &mut TextBuffer {
        &mut self.buffers[self.active_buf].buffer
    }

    fn has_any_dirty_buffer(&self) -> bool {
        self.buffers.iter().any(|s| s.buffer.dirty)
    }

    fn is_markdown_view(&self) -> bool {
        self.mode == AppMode::Editor && self.cur_slot().markdown_view
    }

    // --- Font Size Scaling ---
}

/// ウィンドウスタイル用ショートカットの判定。該当しなければ None。
/// Alt+C の2打目 → 送信先。C: Claude Code、X: Codex、P: GitHub Desktop の Push、
/// N / M: Claude Code / Codex の新規セッション。Ctrl 付きは取り消し。
/// 画面中央に固定したダイアログ（折りたたみ・サイズ変更なし、枠線の色 `stroke`）。
fn modal_window(
    ctx: &egui::Context,
    title: impl Into<egui::WidgetText>,
    stroke: Color32,
) -> egui::Window<'static> {
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(STATUS_BG)
                .stroke(Stroke::new(1.5_f32, stroke)),
        )
}

/// 確認ダイアログのボタン行（Enter / Y で実行、ESC / N で取消）。(実行, 取消) を返す。
fn yes_no_row(ui: &mut egui::Ui, yes_label: &str) -> (bool, bool) {
    let mut yes = false;
    let mut no = false;
    ui.horizontal(|ui| {
        if ui.button(yes_label).clicked()
            || ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Y))
        {
            yes = true;
        }
        if ui.button("キャンセル (ESC / N)").clicked()
            || ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::N))
        {
            no = true;
        }
    });
    (yes, no)
}

/// 一覧を 6 件で打ち切ったときの「... 他 N件」。
fn more_items_label(ui: &mut egui::Ui, total: usize) {
    if total > 6 {
        ui.label(
            egui::RichText::new(format!("  ... 他 {}件", total - 6))
                .font(FontId::monospace(12.5))
                .color(TEXT_DIM_CYAN),
        );
    }
}

/// 正規化したパス（できなければそのまま）。
fn canonical_or_self(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `p` が `raw`（正規化すると `canon`）と同じファイルか。
fn same_file(p: &Path, raw: &Path, canon: &Path) -> bool {
    p == raw || canonical_or_self(p) == canon
}

fn send_target_for_key(key: Key, ctrl: bool) -> Option<SendTarget> {
    match (key, ctrl) {
        (Key::C, false) => Some(SendTarget::ClaudeCode),
        (Key::X, false) => Some(SendTarget::Codex),
        (Key::P, false) => Some(SendTarget::GitHubDesktop),
        (Key::N, false) => Some(SendTarget::ClaudeCodeNewSession),
        (Key::M, false) => Some(SendTarget::CodexNewSession),
        _ => None,
    }
}

fn window_style_key_action(
    key: Key,
    physical_key: Option<Key>,
    modifiers: egui::Modifiers,
) -> Option<MenuAction> {
    if !modifiers.alt || modifiers.ctrl || modifiers.command {
        return None;
    }
    if modifiers.shift {
        if key == Key::N {
            return Some(MenuAction::RepeatSearch(true));
        }
        // 文字の不透明度: 物理キーで判定（無ければ論理キー）
        let phys = physical_key.unwrap_or(key);
        if phys == Key::Minus {
            return Some(MenuAction::TextOpacityDown);
        }
        if matches!(phys, Key::Plus | Key::Equals | Key::Semicolon) || key == Key::Plus {
            return Some(MenuAction::TextOpacityUp);
        }
        if phys == Key::Num0 {
            return Some(MenuAction::TextOpacityReset);
        }
        return None;
    }
    match key {
        Key::T => Some(MenuAction::ToggleAlwaysOnTop),
        Key::W => Some(MenuAction::ToggleWordWrap),
        Key::ArrowUp => Some(MenuAction::OpacityUp),
        Key::ArrowDown => Some(MenuAction::OpacityDown),
        Key::Num0 => Some(MenuAction::OpacityReset),
        Key::K => Some(MenuAction::ToggleTerminal),
        Key::B => Some(MenuAction::ToggleBrowser),
        Key::F => Some(MenuAction::OpenSearch(SearchDialog::Find)),
        Key::R => Some(MenuAction::OpenSearch(SearchDialog::Replace)),
        Key::G => Some(MenuAction::OpenSearch(SearchDialog::Grep)),
        Key::J => Some(MenuAction::OpenSearch(SearchDialog::Jump)),
        Key::N => Some(MenuAction::RepeatSearch(false)),
        _ => None,
    }
}

fn truncate_path_left(s: &str, max_cols: usize) -> Cow<'_, str> {
    use unicode_width::UnicodeWidthChar;
    let total_w: usize = s.chars().map(|c| c.width().unwrap_or(1)).sum();
    if total_w <= max_cols {
        return Cow::Borrowed(s);
    }
    let budget = max_cols.saturating_sub(3);
    let mut start = s.len();
    let mut acc = 0usize;
    for (i, c) in s.char_indices().rev() {
        let cw = c.width().unwrap_or(1);
        if acc + cw > budget {
            break;
        }
        acc += cw;
        start = i;
    }
    Cow::Owned(format!("...{}", &s[start..]))
}

fn truncate_display_right(s: &str, max_cols: usize) -> Cow<'_, str> {
    use unicode_width::UnicodeWidthChar;
    let total_w: usize = s.chars().map(|c| c.width().unwrap_or(1)).sum();
    if total_w <= max_cols {
        return Cow::Borrowed(s);
    }
    let budget = max_cols.saturating_sub(2);
    let mut out = String::new();
    let mut acc = 0usize;
    for c in s.chars() {
        let cw = c.width().unwrap_or(1);
        if acc + cw > budget {
            break;
        }
        acc += cw;
        out.push(c);
    }
    out.push('…');
    Cow::Owned(out)
}

impl eframe::App for VzEditorApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // 背面透過: ウィンドウ自体は透明にし、背景色のアルファで濃さを決める
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        window_style::set_opacity_percent(self.bg_opacity);
        window_style::set_text_opacity_percent(self.text_opacity);
        self.restore_window_position(ctx);
        self.track_window_geometry(ctx);
        self.handle_close_request(ctx);
        self.poll_quit_request(ctx);
        self.check_external_changes(ctx);
        self.handle_dropped_files(ctx);
        self.poll_dialog();
        self.poll_send(ctx);
        self.continue_save_and_quit(ctx);
        self.start_pending_terminal(ctx);
        self.poll_terminal();
        self.handle_events(ctx);
        self.poll_huge_buffers(ctx);

        self.render_menu_bar(ctx);
        self.render_status_bar(ctx);
        self.render_buffer_strip(ctx);
        self.render_fkey_bar(ctx);

        match self.mode {
            AppMode::Editor => {
                if self.is_markdown_view() {
                    self.render_markdown_canvas(ctx);
                } else {
                    self.render_editor_canvas(ctx);
                }
            }
            AppMode::Filer => self.render_filer_canvas(ctx),
            AppMode::Terminal => self.render_terminal_canvas(ctx),
            AppMode::Browser => self.render_browser_canvas(ctx),
        }

        self.render_help_overlay(ctx);
        self.render_about_dialog(ctx);
        self.render_filer_modal(ctx);
        self.render_inline_path_prompt(ctx);
        self.render_search_dialog(ctx);
        self.render_drop_overlay(ctx);
        // WebView は egui の上に重なるので、描画の後で位置と表示を合わせる
        self.sync_browser(ctx, frame);

        // ctx を持たない処理（F キー・メニュー）からのコピーを書き出す
        if let Some(text) = self.clipboard_out.take() {
            ctx.copy_text(text);
        }

        // Keep OS window title bar synchronized with current file, dirty state, or filer directory
        self.sync_window_title(ctx);

        if self.pending_dialog.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// 終了時にウィンドウの位置とサイズを settings.ini へ保存する。
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_settings();
    }
}

/// ウィンドウの情報から保存する位置とサイズを求める。最小化・全画面・情報なしは None（前の値を残す）。
/// 最大化中は元に戻したときのサイズを残し、位置は最大化したモニター上に置く。
fn next_window_geometry(
    prev: Option<WindowGeometry>,
    info: &egui::ViewportInfo,
    zoom: f32,
) -> Option<WindowGeometry> {
    if info.minimized == Some(true) || info.fullscreen == Some(true) {
        return None;
    }
    let (outer, inner, native_ppp) = (
        info.outer_rect?,
        info.inner_rect?,
        info.native_pixels_per_point?,
    );
    let ppp = native_ppp * zoom;
    if ppp.is_nan() || ppp <= 0.0 {
        return None;
    }
    let px = |v: f32| (v * ppp).round() as i32;
    if info.maximized != Some(true) {
        return Some(WindowGeometry {
            x: px(outer.min.x),
            y: px(outer.min.y),
            width: inner.width(),
            height: inner.height(),
            scale: ppp,
            maximized: false,
        });
    }
    // 最大化中: 元のサイズを残す。元の位置が最大化したモニター上ならそのまま、違えば中央へ
    let (width, height) = prev.map_or((inner.width(), inner.height()), |p| (p.width, p.height));
    let max_rect_px = egui::Rect::from_min_max(
        egui::pos2(outer.min.x * ppp, outer.min.y * ppp),
        egui::pos2(outer.max.x * ppp, outer.max.y * ppp),
    );
    let (x, y) = match prev {
        Some(p) if max_rect_px.contains(egui::pos2(p.x as f32, p.y as f32)) => (p.x, p.y),
        _ => {
            let c = outer.center();
            (px(c.x - width / 2.0), px(c.y - height / 2.0))
        }
    };
    Some(WindowGeometry {
        x,
        y,
        width,
        height,
        scale: ppp,
        maximized: true,
    })
}

/// 保存していた位置でタイトルバー（上端 32 論理ピクセル）がモニターの作業領域に見えるか。
fn window_title_bar_visible(g: &WindowGeometry) -> bool {
    monitor::title_bar_visible([
        g.x,
        g.y,
        g.x + (g.width * g.scale).round() as i32,
        g.y + (32.0 * g.scale).round() as i32,
    ])
}
