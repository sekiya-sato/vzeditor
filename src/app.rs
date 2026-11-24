use crate::buffer::{char_to_byte_idx, expand_tabs_for_display, TextBuffer, TextPos};
use crate::dialog::{spawn_open_dialog, spawn_save_dialog, DialogResult};
use crate::filer::{
    copy_entry, create_dir, create_file, delete_entry, format_size, move_entry, rename_entry,
    ActivePane, FileEntry, FilerState,
};
use crate::fonts::configure_japanese_fonts;
use crate::markdown::{parse_markdown, render_markdown_view, MdBlock};
use crate::send_to_app::{prepare_text, spawn_send, SendResult, SendTarget};
use crate::settings::{KeyScheme, Settings};
use crate::window_style::{self, bg};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, Event, FontId, ImeEvent, Key, Pos2, Rect, Sense, Stroke,
    StrokeKind, Vec2,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

// --- Font Size Scaling Constants ---
const DEFAULT_FONT_SIZE: f32 = 16.0;
const MIN_FONT_SIZE: f32 = 10.0;
const MAX_FONT_SIZE: f32 = 36.0;
const FONT_SIZE_STEP: f32 = 2.0;

fn calc_line_height(font_size: f32) -> f32 {
    (font_size * 1.45).round().max(font_size + 4.0)
}

// --- PC-98 / VZ Editor Color Palette ---
const BG_NAVY: Color32 = Color32::from_rgb(8, 24, 56); // #081838
const BG_ACTIVE_LINE: Color32 = Color32::from_rgb(14, 38, 84);
const BG_GUTTER: Color32 = Color32::from_rgb(6, 19, 46);
const BG_TAB_BAR: Color32 = Color32::from_rgb(9, 28, 66);
const BG_TAB_ACTIVE: Color32 = Color32::from_rgb(24, 66, 134);
const BG_TAB_INACTIVE: Color32 = Color32::from_rgb(12, 34, 78);

const BORDER_BLUE: Color32 = Color32::from_rgb(42, 98, 168);
const BORDER_ACTIVE_PANE: Color32 = Color32::from_rgb(90, 212, 255);
const GUTTER_SEP: Color32 = Color32::from_rgb(26, 62, 118);

const STATUS_BG: Color32 = Color32::from_rgb(15, 42, 92);
const STATUS_BADGE_BG: Color32 = Color32::from_rgb(64, 210, 255);
const STATUS_BADGE_FILER_BG: Color32 = Color32::from_rgb(255, 206, 86);
const STATUS_BADGE_VIEW_BG: Color32 = Color32::from_rgb(56, 204, 152);
const STATUS_BADGE_FG: Color32 = Color32::from_rgb(6, 20, 48);

const FKEY_BAR_BG: Color32 = Color32::from_rgb(10, 28, 64);
const FKEY_BTN_BG: Color32 = Color32::from_rgb(18, 48, 102);
const FKEY_BTN_HOVER: Color32 = Color32::from_rgb(30, 74, 148);
const FKEY_NUM_BG: Color32 = Color32::from_rgb(46, 140, 208);
const FKEY_NUM_FILER_BG: Color32 = Color32::from_rgb(36, 138, 168);
const FKEY_NUM_FG: Color32 = Color32::from_rgb(240, 250, 255);

const FILER_HDR_ACTIVE: Color32 = Color32::from_rgb(18, 56, 114);
const FILER_HDR_INACTIVE: Color32 = Color32::from_rgb(11, 32, 72);
const FILER_COL_HDR: Color32 = Color32::from_rgb(9, 26, 60);
const FILER_ROW_CURSOR_ACTIVE: Color32 = Color32::from_rgb(28, 82, 156);
const FILER_ROW_CURSOR_INACTIVE: Color32 = Color32::from_rgb(16, 44, 92);
const FILER_ROW_MARKED: Color32 = Color32::from_rgb(62, 54, 18);
const FILER_ROW_MARKED_CURSOR: Color32 = Color32::from_rgb(88, 76, 24);

const TEXT_WHITE: Color32 = Color32::from_rgb(234, 244, 255);
const TEXT_CYAN: Color32 = Color32::from_rgb(112, 232, 255);
const TEXT_DIM_CYAN: Color32 = Color32::from_rgb(88, 146, 204);
const TEXT_CR_MARK: Color32 = Color32::from_rgb(56, 110, 168);
const TEXT_EOF: Color32 = Color32::from_rgb(76, 178, 224);
const TEXT_AMBER: Color32 = Color32::from_rgb(255, 220, 96);

const CURSOR_FILL: Color32 = Color32::from_rgba_premultiplied(80, 220, 255, 110);
const CURSOR_BAR: Color32 = Color32::from_rgb(120, 244, 255);
/// 範囲選択の背景色
const SELECTION_BG: Color32 = Color32::from_rgb(38, 108, 190);
const PREEDIT_BG: Color32 = Color32::from_rgb(28, 72, 134);
const PREEDIT_FG: Color32 = Color32::from_rgb(255, 246, 160);
const PREEDIT_UNDERLINE: Color32 = Color32::from_rgb(112, 236, 255);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppMode {
    Editor,
    Filer,
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
    OpenFile,
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
}

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// ビルド日時（build.rs が埋め込む。例: 2026/11/24 12:00）
const BUILD_AT: &str = env!("VZ_BUILD_AT");
/// 原作 LZ Editor (MIT License)
const BASE_REPO_URL: &str = "https://github.com/TouryuTamane/lz-editor";
/// このフォーク（VZ Editor）のリポジトリ。Cargo.toml の repository から取る。
const FORK_REPO_URL: &str = env!("CARGO_PKG_REPOSITORY");

const MENU_BAR_BG: Color32 = Color32::from_rgb(7, 21, 50);
const MENU_BAR_HOVER: Color32 = Color32::from_rgb(20, 56, 114);
const MENU_POPUP_BG: Color32 = Color32::from_rgb(10, 30, 70);
const MENU_ITEM_HOVER: Color32 = Color32::from_rgb(26, 74, 146);
const MENU_TEXT_DISABLED: Color32 = Color32::from_rgb(74, 108, 152);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TopMenu {
    File,
    Edit,
    Tools,
    About,
}

impl TopMenu {
    const ALL: [TopMenu; 4] = [TopMenu::File, TopMenu::Edit, TopMenu::Tools, TopMenu::About];

    fn label(self) -> &'static str {
        match self {
            TopMenu::File => "ファイル",
            TopMenu::Edit => "編集",
            TopMenu::Tools => "ツール",
            TopMenu::About => "About",
        }
    }

    fn next(self) -> Self {
        match self {
            TopMenu::File => TopMenu::Edit,
            TopMenu::Edit => TopMenu::Tools,
            TopMenu::Tools => TopMenu::About,
            TopMenu::About => TopMenu::File,
        }
    }

    fn prev(self) -> Self {
        match self {
            TopMenu::File => TopMenu::About,
            TopMenu::Edit => TopMenu::File,
            TopMenu::Tools => TopMenu::Edit,
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
    // Tools
    ToggleMarkdownView,
    FontZoomIn,
    FontZoomOut,
    FontReset,
    NextBuffer,
    ToggleAlwaysOnTop,
    OpacityUp,
    OpacityDown,
    OpacityReset,
    TextOpacityUp,
    TextOpacityDown,
    TextOpacityReset,
    SendToClaudeCode,
    SendToCodex,
    ToggleKeyScheme,
    SelectAll,
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
}

struct BufferSlot {
    buffer: TextBuffer,
    scroll_x: f32,
    scroll_y: f32,
    markdown_view: bool,
    md_scroll_y: f32,
    md_max_scroll_y: f32,
    md_viewport_h: f32,
    md_scroll_dirty: bool,
    md_cache_rev: Option<u64>,
    md_blocks: Vec<MdBlock>,
}

impl BufferSlot {
    fn new(buffer: TextBuffer) -> Self {
        Self {
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
        }
    }

    fn ensure_md_cache(&mut self) {
        if self.md_cache_rev != Some(self.buffer.revision) {
            self.md_blocks = parse_markdown(&self.buffer.lines);
            self.md_cache_rev = Some(self.buffer.revision);
        }
    }

    fn is_untouched_untitled(&self) -> bool {
        self.buffer.file_path.is_none()
            && !self.buffer.dirty
            && self.buffer.lines.len() == 1
            && self.buffer.lines[0].is_empty()
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
    /// フレームの最後にクリップボードへ書き出す文字列（ctx を持たない処理から使う）
    clipboard_out: Option<String>,
}

impl VzEditorApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        initial_paths: Vec<PathBuf>,
        settings: Settings,
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
            clipboard_out: None,
        };
        if !initial_paths.is_empty() {
            // 存在しないパスは新規ファイルとして開く（従来の `vz 新規.txt` と同じ）
            app.open_paths(initial_paths, true);
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
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_WHITE);
        ctx.set_visuals(visuals);
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

    fn set_font_size(&mut self, new_size: f32) {
        let clamped = new_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE).round();
        self.font_size = clamped;
        self.line_height = calc_line_height(clamped);
        self.need_scroll_to_cursor = true;
        self.filer.left.need_scroll_to_cursor = true;
        self.filer.right.need_scroll_to_cursor = true;
        self.status_message = format!("フォントサイズ: {}pt", clamped as i32);
        self.save_settings();
    }

    // --- Window style (背面透過・最前面固定) ---

    fn save_settings(&mut self) {
        if !self.persist_settings {
            return;
        }
        let settings = Settings {
            opacity: self.bg_opacity,
            text_opacity: self.text_opacity,
            always_on_top: self.always_on_top,
            font_size: self.font_size,
            key_scheme: self.key_scheme,
        };
        if let Err(e) = settings.save() {
            self.status_message = format!("設定の保存に失敗しました: {e}");
        }
    }

    fn set_bg_opacity(&mut self, percent: u8) {
        let p = window_style::clamp_opacity(percent);
        if self.persist_settings {
            window_style::set_opacity_percent(p);
        }
        if p == self.bg_opacity {
            self.status_message = format!(
                "背景の不透明度は {p}% です ({}〜{}%)",
                window_style::MIN_OPACITY,
                window_style::MAX_OPACITY
            );
            return;
        }
        self.bg_opacity = p;
        self.status_message = format!("背景の不透明度: {p}% (Alt+↑ 濃く / Alt+↓ 薄く / Alt+0 戻す)");
        self.save_settings();
    }

    fn change_bg_opacity(&mut self, delta: i16) {
        let next = (self.bg_opacity as i16 + delta).clamp(0, 100) as u8;
        self.set_bg_opacity(next);
    }

    fn set_text_opacity(&mut self, percent: u8) {
        let p = window_style::clamp_opacity(percent);
        if self.persist_settings {
            window_style::set_text_opacity_percent(p);
        }
        if p == self.text_opacity {
            self.status_message = format!(
                "文字の不透明度は {p}% です ({}〜{}%)",
                window_style::MIN_OPACITY,
                window_style::MAX_OPACITY
            );
            return;
        }
        self.text_opacity = p;
        self.status_message =
            format!("文字の不透明度: {p}% (Alt+Shift++ 濃く / Alt+Shift+- 薄く / Alt+Shift+0 戻す)");
        self.save_settings();
    }

    fn change_text_opacity(&mut self, delta: i16) {
        let next = (self.text_opacity as i16 + delta).clamp(0, 100) as u8;
        self.set_text_opacity(next);
    }

    // --- Claude Code / Codex への送信 ---

    /// 現在のバッファ全体を送信先アプリへ送る（別スレッド）。送信後も内容は残す。
    fn start_send(&mut self, target: SendTarget) {
        self.send_prefix = false;
        if self.pending_send.is_some() {
            self.status_message = "送信中です。完了までお待ちください".to_string();
            return;
        }
        let Some(text) = prepare_text(&self.cur_buf().lines) else {
            self.status_message = format!("{} へ送る文字がありません", target.label());
            return;
        };
        self.status_message = format!("{} へ送信中... (Alt などのキーを離してください)", target.label());
        self.pending_send = Some(spawn_send(target, text));
    }

    fn poll_send(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.pending_send else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.pending_send = None;
                self.status_message = match result {
                    Ok(msg) => msg,
                    Err(msg) => format!("送信エラー: {msg}"),
                };
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_send = None;
                self.status_message = "送信エラー: 送信処理が中断されました".to_string();
            }
        }
    }

    /// Alt+C → C（Claude Code）/ X（Codex）の2打鍵を処理し、使ったキーは入力から取り除く。
    /// 2打目は Alt を押したままでも離してもよい。C / X 以外のキーなら取り消す。
    fn handle_send_keys(&mut self, ctx: &egui::Context) {
        let mut action: Option<Option<SendTarget>> = None; // Some(None) = 取り消し
        let mut consumed = false;
        let mut prefix = self.send_prefix;
        ctx.input_mut(|i| {
            i.events.retain(|ev| {
                let Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } = ev
                else {
                    return true;
                };
                let ctrl = modifiers.ctrl || modifiers.command;
                if prefix {
                    if !*pressed {
                        // 2打目の離し
                        if matches!(key, Key::C | Key::X) {
                            consumed = true;
                            return false;
                        }
                        return true;
                    }
                    if action.is_none() {
                        action = Some(match (key, ctrl) {
                            (Key::C, false) => Some(SendTarget::ClaudeCode),
                            (Key::X, false) => Some(SendTarget::Codex),
                            _ => None,
                        });
                        prefix = false;
                        consumed = true;
                        return false;
                    }
                    return true;
                }
                if *key == Key::C && modifiers.alt && !ctrl && !modifiers.shift {
                    if *pressed {
                        prefix = true;
                    }
                    consumed = true;
                    return false;
                }
                true
            });
            if consumed {
                i.events.retain(|ev| !matches!(ev, Event::Text(_)));
            }
        });
        let started_prefix = prefix && !self.send_prefix;
        self.send_prefix = prefix;
        if started_prefix {
            self.status_message =
                "Alt+C: 続けて C で Claude Code へ / X で Codex へ送信（他のキーで取り消し）".to_string();
        }
        match action {
            Some(Some(target)) => self.start_send(target),
            Some(None) => self.status_message = "送信を取り消しました".to_string(),
            None => {}
        }
    }

    // --- キー割り当て（サクラ互換 / VZ 互換） ---

    fn toggle_key_scheme(&mut self) {
        self.key_scheme = match self.key_scheme {
            KeyScheme::Sakura => KeyScheme::Vz,
            KeyScheme::Vz => KeyScheme::Sakura,
        };
        // VZ 互換には範囲選択が無いので解除する
        for slot in &mut self.buffers {
            slot.buffer.clear_selection();
        }
        self.select_mode = false;
        self.prefix = PrefixKey::None;
        self.status_message = format!(
            "キー割り当てを {} にしました（ツール→キー割り当てで切替）",
            self.key_scheme.label()
        );
        self.save_settings();
    }

    fn is_sakura(&self) -> bool {
        self.key_scheme == KeyScheme::Sakura
    }

    /// フレームの最後にクリップボードへ書き出す（内部クリップボードにも保持）。
    fn copy_to_clipboard(&mut self, text: String) {
        self.internal_clipboard = text.clone();
        self.clipboard_out = Some(text);
    }

    fn view_mode_guard(&mut self) -> bool {
        if self.is_markdown_view() {
            self.status_message =
                "ビューモード中は編集できません (Ctrl+Shift+V で編集モードへ)".to_string();
            return true;
        }
        false
    }

    /// 選択範囲、選択が無ければ現在行（改行を含む）を返す。
    fn selection_or_line_range(&self) -> (TextPos, TextPos) {
        let buf = self.cur_buf();
        if let Some(range) = buf.selection_range() {
            return range;
        }
        let line = buf.cursor_line;
        if line + 1 < buf.lines.len() {
            ((line, 0), (line + 1, 0))
        } else {
            ((line, 0), (line, buf.line_char_len(line)))
        }
    }

    /// サクラの切り取り（Ctrl+X / Shift+Del / F7）。選択が無ければ現在行を切り取る。
    fn sakura_cut(&mut self) {
        if self.view_mode_guard() {
            return;
        }
        let had_selection = self.cur_buf().has_selection();
        let (start, end) = self.selection_or_line_range();
        if let Some(text) = self.cur_buf_mut().delete_range(start, end) {
            self.copy_to_clipboard(text);
            self.status_message = if had_selection {
                "選択範囲を切り取りました".to_string()
            } else {
                "現在行を切り取りました".to_string()
            };
        }
        self.select_mode = false;
        self.need_scroll_to_cursor = true;
    }

    /// サクラのコピー（Ctrl+C / Ctrl+Ins / F8）。選択が無ければ現在行、ビューモードでは全文。
    fn sakura_copy(&mut self) {
        let (text, msg) = if self.is_markdown_view() {
            (
                self.cur_buf().lines.join(self.cur_buf().line_ending),
                "ドキュメント全体をコピーしました",
            )
        } else if let Some(text) = self.cur_buf().selected_text() {
            (text, "選択範囲をコピーしました")
        } else {
            let (start, end) = self.selection_or_line_range();
            (self.cur_buf().text_in_range(start, end), "現在行をコピーしました")
        };
        self.copy_to_clipboard(text);
        self.select_mode = false;
        self.status_message = msg.to_string();
    }

    /// 貼り付け（選択中は置き換え）。
    fn sakura_paste_text(&mut self, text: &str) {
        if self.view_mode_guard() || text.is_empty() {
            return;
        }
        self.cur_buf_mut().insert_text(text);
        self.select_mode = false;
        self.need_scroll_to_cursor = true;
    }

    /// F9 の貼り付け（システムのクリップボード、無ければ内部クリップボード）。
    fn sakura_paste_from_clipboard(&mut self) {
        let text = read_system_clipboard().unwrap_or_else(|| self.internal_clipboard.clone());
        if text.is_empty() {
            self.status_message = "クリップボードが空です".to_string();
        } else {
            self.sakura_paste_text(&text);
        }
    }

    /// 範囲を切り取る（delete_only なら削除のみ）。
    fn sakura_cut_range(&mut self, start: TextPos, end: TextPos, delete_only: bool, what: &str) {
        if self.view_mode_guard() {
            return;
        }
        if let Some(text) = self.cur_buf_mut().delete_range(start, end) {
            if delete_only {
                self.status_message = format!("{what}を削除しました");
            } else {
                self.copy_to_clipboard(text);
                self.status_message = format!("{what}を切り取りました");
            }
        }
        self.select_mode = false;
        self.need_scroll_to_cursor = true;
    }

    /// 単語単位で移動した先の位置（カーソルは動かさない）。
    fn word_boundary_pos(&mut self, right: bool) -> TextPos {
        let buf = self.cur_buf_mut();
        let cur = buf.cursor_pos();
        if right {
            buf.move_word_right();
        } else {
            buf.move_word_left();
        }
        let pos = buf.cursor_pos();
        buf.set_cursor(cur.0, cur.1);
        pos
    }

    fn toggle_select_mode(&mut self) {
        if self.view_mode_guard() {
            return;
        }
        self.select_mode = !self.select_mode;
        if self.select_mode {
            let buf = self.cur_buf_mut();
            let pos = buf.cursor_pos();
            buf.anchor = Some(pos);
            self.status_message = "範囲選択開始（移動キーで選択、Esc で解除）".to_string();
        } else {
            self.status_message = "範囲選択モードを終了しました".to_string();
        }
    }

    /// Markdown ビューでのサクラ互換キー（スクロール）。
    fn sakura_md_view_key(&mut self, key: Key, ctrl: bool) -> bool {
        let page = (self.cur_slot().md_viewport_h * 0.85).max(self.line_height * 4.0);
        match (key, ctrl) {
            (Key::ArrowUp, _) => self.scroll_markdown_by(-self.line_height * 2.5),
            (Key::ArrowDown, _) => self.scroll_markdown_by(self.line_height * 2.5),
            (Key::PageUp, _) => self.scroll_markdown_by(-page),
            (Key::PageDown, _) | (Key::Space, false) => self.scroll_markdown_by(page),
            (Key::Home, _) => self.scroll_markdown_to_top(),
            (Key::End, _) => self.scroll_markdown_to_bottom(),
            (Key::P, true) => self.toggle_markdown_view(),
            _ => return ctrl,
        }
        true
    }

    /// サクラエディタ互換のキー処理（エディター画面）。処理したら true。
    /// Ctrl+X / C / V、Shift+Del、Ctrl+Ins、Shift+Ins は egui が Cut / Copy / Paste イベントに
    /// 変換するため、ここではなく handle_editor_events で扱う。
    fn handle_sakura_key(&mut self, key: Key, modifiers: egui::Modifiers, in_md_view: bool) -> bool {
        let ctrl = modifiers.ctrl || modifiers.command;
        let shift = modifiers.shift;
        if modifiers.alt {
            // Alt 系は透過・最前面・送信（handle_events で処理済み）。サクラの Alt+P などは未対応
            return false;
        }

        // ファンクションキー
        match (key, ctrl, shift) {
            (Key::F1, false, false) => {
                self.show_help = !self.show_help;
                return true;
            }
            (Key::F1, true, true) => {
                self.show_help = false;
                self.show_about = true;
                return true;
            }
            (Key::F6, false, false) => {
                self.toggle_select_mode();
                return true;
            }
            (Key::F7, false, false) => {
                self.sakura_cut();
                return true;
            }
            (Key::F8, false, _) => {
                self.sakura_copy();
                return true;
            }
            (Key::F9, false, false) => {
                self.sakura_paste_from_clipboard();
                return true;
            }
            (Key::F4, true, false) => {
                self.close_current_buffer();
                return true;
            }
            // F2〜F5 は VZ 互換と同じ（新規 / 開く / 行頭 / 行末）
            (Key::F2, false, false) => {
                self.execute_editor_action(EditorAction::NewBuffer);
                return true;
            }
            (Key::F3, false, false) => {
                self.execute_editor_action(EditorAction::OpenFile);
                return true;
            }
            (Key::F4 | Key::F5, false, _) => {
                if in_md_view {
                    self.execute_editor_action(if key == Key::F4 {
                        EditorAction::MoveLineStart
                    } else {
                        EditorAction::MoveLineEnd
                    });
                } else {
                    // Home / End と同じく、Shift または選択モードなら範囲選択
                    let extend = shift || self.select_mode;
                    let buf = self.cur_buf_mut();
                    if extend {
                        buf.begin_selection_if_none();
                    } else {
                        buf.clear_selection();
                    }
                    if key == Key::F4 {
                        buf.move_line_start();
                    } else {
                        buf.move_line_end();
                    }
                    self.need_scroll_to_cursor = true;
                }
                return true;
            }
            (Key::F10, false, true) => {
                if !self.view_mode_guard() {
                    self.cur_buf_mut().duplicate_line();
                    self.need_scroll_to_cursor = true;
                }
                return true;
            }
            (Key::F2 | Key::F3 | Key::F4 | Key::F5 | Key::F6 | Key::F7 | Key::F9 | Key::F10
            | Key::F11 | Key::F12, _, _) => {
                self.status_message = format!("{key:?} はサクラ互換でまだ対応していません");
                return true;
            }
            _ => {}
        }

        if key == Key::Escape {
            if self.cur_buf().anchor.is_some() || self.select_mode {
                self.cur_buf_mut().clear_selection();
                self.select_mode = false;
                self.status_message = "範囲選択を解除しました".to_string();
            } else if self.show_help {
                self.show_help = false;
            } else {
                self.enter_filer();
            }
            return true;
        }

        if self.handle_font_zoom_key(key, modifiers) {
            return true;
        }

        if in_md_view {
            return self.sakura_md_view_key(key, ctrl);
        }

        // --- 移動（Shift または F6 の選択モードで範囲選択） ---
        let page = self.last_visible_lines;
        let is_move = matches!(
            key,
            Key::ArrowLeft
                | Key::ArrowRight
                | Key::ArrowUp
                | Key::ArrowDown
                | Key::Home
                | Key::End
                | Key::PageUp
                | Key::PageDown
        );
        if is_move {
            // Ctrl+↑ / ↓（Shift なし）は1行スクロール
            if ctrl && !shift && matches!(key, Key::ArrowUp | Key::ArrowDown) {
                let dy = if key == Key::ArrowUp {
                    -self.line_height
                } else {
                    self.line_height
                };
                let slot = self.cur_slot_mut();
                slot.scroll_y = (slot.scroll_y + dy).max(0.0);
                return true;
            }
            let extend = shift || self.select_mode;
            let buf = self.cur_buf_mut();
            if extend {
                buf.begin_selection_if_none();
            } else {
                buf.clear_selection();
            }
            match (key, ctrl) {
                (Key::ArrowLeft, false) => buf.move_left(),
                (Key::ArrowLeft, true) => buf.move_word_left(),
                (Key::ArrowRight, false) => buf.move_right(),
                (Key::ArrowRight, true) => buf.move_word_right(),
                (Key::ArrowUp, false) => buf.move_up(),
                (Key::ArrowUp, true) => {
                    buf.move_up();
                    buf.move_up();
                }
                (Key::ArrowDown, false) => buf.move_down(),
                (Key::ArrowDown, true) => {
                    buf.move_down();
                    buf.move_down();
                }
                (Key::Home, false) => buf.move_line_start(),
                (Key::Home, true) => buf.move_doc_start(),
                (Key::End, false) => buf.move_line_end(),
                (Key::End, true) => buf.move_doc_end(),
                (Key::PageUp, false) => buf.move_page_up(page),
                (Key::PageUp, true) => buf.move_page_up((page / 2).max(1)),
                (Key::PageDown, false) => buf.move_page_down(page),
                (Key::PageDown, true) => buf.move_page_down((page / 2).max(1)),
                _ => {}
            }
            self.need_scroll_to_cursor = true;
            return true;
        }

        if ctrl {
            let cur = self.cur_buf().cursor_pos();
            match key {
                Key::N => self.new_empty_buffer(),
                Key::O => self.trigger_open(),
                Key::S if shift => self.trigger_save_as(),
                Key::S => self.trigger_save(),
                Key::Tab => self.switch_buffer_step(if shift { -1 } else { 1 }),
                Key::Z => self.menu_undo(),
                Key::Y => self.menu_redo(),
                Key::A => {
                    self.cur_buf_mut().select_all();
                    self.status_message = "すべて選択しました".to_string();
                }
                Key::W => self.cur_buf_mut().select_word_at_cursor(),
                Key::P => self.toggle_markdown_view(),
                Key::E if shift => {
                    if !self.view_mode_guard() {
                        self.cur_buf_mut().delete_current_line();
                        self.need_scroll_to_cursor = true;
                        self.status_message = "行を削除しました".to_string();
                    }
                }
                Key::E => {
                    let (start, end) = {
                        let buf = self.cur_buf();
                        if cur.0 + 1 < buf.lines.len() {
                            ((cur.0, 0), (cur.0 + 1, 0))
                        } else {
                            ((cur.0, 0), (cur.0, buf.line_char_len(cur.0)))
                        }
                    };
                    self.sakura_cut_range(start, end, false, "行");
                }
                Key::K => {
                    let buf = self.cur_buf();
                    let len = buf.line_char_len(cur.0);
                    // 行末にいるときは改行を対象にする
                    let end = if cur.1 >= len && cur.0 + 1 < buf.lines.len() {
                        (cur.0 + 1, 0)
                    } else {
                        (cur.0, len)
                    };
                    self.sakura_cut_range(cur, end, shift, "行末まで");
                }
                Key::U => self.sakura_cut_range((cur.0, 0), cur, shift, "行頭まで"),
                Key::D => {
                    let (s, e) = self.cur_buf().word_range_at(cur);
                    self.sakura_cut_range((cur.0, s), (cur.0, e), shift, "単語");
                }
                Key::I => {
                    if !self.view_mode_guard() {
                        self.cur_buf_mut().duplicate_line();
                        self.need_scroll_to_cursor = true;
                    }
                }
                Key::Backspace => {
                    let start = self.word_boundary_pos(false);
                    self.sakura_cut_range(start, cur, true, "単語の左側");
                }
                Key::Delete => {
                    let end = self.word_boundary_pos(true);
                    self.sakura_cut_range(cur, end, true, "単語の右側");
                }
                Key::F | Key::R | Key::G | Key::J => {
                    self.status_message =
                        "検索・置換・Grep・行ジャンプはまだ対応していません".to_string();
                }
                _ => {}
            }
            // Ctrl の組み合わせは文字入力に流さない
            return true;
        }

        // --- 修飾キーなしの編集キー ---
        match key {
            Key::Enter => {
                self.cur_buf_mut().insert_newline();
            }
            Key::Backspace => {
                self.cur_buf_mut().backspace();
            }
            Key::Delete => {
                self.cur_buf_mut().delete_char_at_cursor();
            }
            Key::Tab if !shift => {
                self.cur_buf_mut().insert_text("\t");
            }
            Key::Insert => {
                self.status_message = "挿入 / 上書きの切替はまだ対応していません".to_string();
                return true;
            }
            _ => return false,
        }
        self.select_mode = false;
        self.need_scroll_to_cursor = true;
        true
    }

    fn toggle_always_on_top(&mut self, ctx: &egui::Context) {
        self.always_on_top = !self.always_on_top;
        let level = if self.always_on_top {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
        self.status_message = if self.always_on_top {
            "最前面に固定しました (Alt+T で解除)".to_string()
        } else {
            "最前面固定を解除しました (Alt+T で固定)".to_string()
        };
        self.save_settings();
    }

    /// ウィンドウスタイルのキーをモードに関係なく処理し、処理したキーは入力から取り除く。
    ///
    /// - Alt+T / Alt+↑ / Alt+↓ / Alt+0: 最前面固定・背景の不透明度
    /// - Alt+Shift+「+」 / Alt+Shift+「-」 / Alt+Shift+0: 文字の不透明度
    ///
    /// 「+」「-」は配列差（US: `=+` / JIS: `;+`、Shift+「-」が `=` になる JIS）を吸収するため
    /// 物理キーを優先して判定する。既存の Ctrl+± / Ctrl+0（フォントサイズ）は Alt 併用時に
    /// 反応しないため重複しない。
    fn handle_window_style_keys(&mut self, ctx: &egui::Context) {
        let mut commands: Vec<MenuAction> = Vec::new();
        ctx.input_mut(|i| {
            i.events.retain(|ev| {
                if let Event::Key {
                    key,
                    physical_key,
                    pressed,
                    modifiers,
                    ..
                } = ev
                {
                    if let Some(action) = window_style_key_action(*key, *physical_key, *modifiers) {
                        if *pressed {
                            commands.push(action);
                        }
                        return false;
                    }
                }
                true
            });
            // Option/Alt+キーで生じる文字（macOS の Option+T → † など）を本文に入れない
            if !commands.is_empty() {
                i.events.retain(|ev| !matches!(ev, Event::Text(_)));
            }
        });
        for action in commands {
            self.execute_menu_action(ctx, action);
        }
    }

    fn zoom_in_font(&mut self) {
        self.set_font_size(self.font_size + FONT_SIZE_STEP);
    }

    fn zoom_out_font(&mut self) {
        self.set_font_size(self.font_size - FONT_SIZE_STEP);
    }

    fn reset_font_size(&mut self) {
        self.set_font_size(DEFAULT_FONT_SIZE);
    }

    fn handle_font_zoom_key(&mut self, key: Key, modifiers: egui::Modifiers) -> bool {
        let ctrl = modifiers.ctrl || modifiers.command;
        if !ctrl || modifiers.alt {
            return false;
        }
        match key {
            Key::Plus | Key::Equals => {
                self.zoom_in_font();
                true
            }
            // JIS keyboard fallback where Shift+; is '+'
            Key::Semicolon if modifiers.shift => {
                self.zoom_in_font();
                true
            }
            Key::Minus => {
                self.zoom_out_font();
                true
            }
            Key::Num0 => {
                self.reset_font_size();
                true
            }
            _ => false,
        }
    }

    // --- Markdown View Mode ---

    fn toggle_markdown_view(&mut self) {
        self.prefix = PrefixKey::None;
        self.ime_preedit.clear();
        let slot = self.cur_slot_mut();
        slot.markdown_view = !slot.markdown_view;
        let now_view = slot.markdown_view;
        if now_view {
            slot.md_scroll_dirty = true;
            self.status_message =
                "Markdown ビューモード [VIEW] (^P / Alt+P / ^K^V で編集へ戻る)".to_string();
        } else {
            self.need_scroll_to_cursor = true;
            self.status_message =
                "テキスト編集モード [EDIT] (^P / Alt+P / ^K^V でプレビューへ)".to_string();
        }
    }

    fn scroll_markdown_by(&mut self, delta_y: f32) {
        let slot = self.cur_slot_mut();
        slot.md_scroll_y = (slot.md_scroll_y + delta_y).clamp(0.0, slot.md_max_scroll_y);
        slot.md_scroll_dirty = true;
    }

    fn scroll_markdown_to_top(&mut self) {
        let slot = self.cur_slot_mut();
        slot.md_scroll_y = 0.0;
        slot.md_scroll_dirty = true;
    }

    fn scroll_markdown_to_bottom(&mut self) {
        let slot = self.cur_slot_mut();
        slot.md_scroll_y = slot.md_max_scroll_y;
        slot.md_scroll_dirty = true;
    }

    // --- Mode Switching ---

    fn enter_filer(&mut self) {
        self.prefix = PrefixKey::None;
        self.ime_preedit.clear();
        self.show_help = false;
        self.filer.refresh_both();
        self.mode = AppMode::Filer;
        self.status_message = "2画面ファイラー (Tab/←→:窓切替  Space:マーク  Enter:開く  ESC:エディタ戻)"
            .to_string();
    }

    fn enter_editor(&mut self) {
        self.prefix = PrefixKey::None;
        self.show_help = false;
        self.filer_modal = None;
        self.mode = AppMode::Editor;
        self.need_scroll_to_cursor = true;
        if self.cur_slot().markdown_view {
            self.cur_slot_mut().md_scroll_dirty = true;
        }
        let name = self.cur_buf().display_name();
        let mode_tag = if self.cur_slot().markdown_view {
            " [VIEW]"
        } else {
            ""
        };
        self.status_message = format!(
            "エディタ [{}/{}]: {}{}",
            self.active_buf + 1,
            self.buffers.len(),
            name,
            mode_tag
        );
    }

    // --- Multi-Buffer Management ---

    /// 未編集の [無題] だけなら置き換え、それ以外は末尾に追加してアクティブにする。
    fn add_buffer(&mut self, buf: TextBuffer) {
        if self.buffers.len() == 1 && self.buffers[0].is_untouched_untitled() {
            self.buffers[0] = BufferSlot::new(buf);
            self.active_buf = 0;
        } else {
            self.buffers.push(BufferSlot::new(buf));
            self.active_buf = self.buffers.len() - 1;
        }
        self.mode = AppMode::Editor;
        self.need_scroll_to_cursor = true;
    }

    /// 複数のファイル・フォルダを開く（ドラッグ＆ドロップ、起動引数、開くダイアログ、ファイラーのマーク）。
    ///
    /// - ファイルはそれぞれバッファで開き、最初に開いたファイルを表示する。既に開いていれば切り替えるだけ。
    /// - フォルダだけが渡された場合は、ファイラーでそのフォルダへ移動する。
    /// - `create_missing` が true なら存在しないパスを新規ファイルとして開く。
    fn open_paths(&mut self, paths: Vec<PathBuf>, create_missing: bool) {
        let mut first_opened: Option<usize> = None;
        let mut opened = 0usize;
        let mut failed: Vec<String> = Vec::new();
        let mut first_dir: Option<PathBuf> = None;

        for path in paths {
            if path.is_dir() {
                if first_dir.is_none() {
                    first_dir = Some(path);
                }
                continue;
            }
            let ok = if !path.exists() && create_missing {
                let mut buf = TextBuffer::new();
                buf.file_path = Some(path.clone());
                self.add_buffer(buf);
                self.status_message = format!("新規ファイル: {}", path.display());
                true
            } else {
                self.open_file_in_buffer(path.clone())
            };
            if ok {
                opened += 1;
                first_opened.get_or_insert(self.active_buf);
            } else {
                failed.push(
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string()),
                );
            }
        }

        if let Some(idx) = first_opened {
            self.switch_buffer_to(idx);
            self.mode = AppMode::Editor;
            if opened > 1 || !failed.is_empty() {
                let mut msg = format!(
                    "{opened} 個のファイルを開きました [{}/{}]: {}",
                    self.active_buf + 1,
                    self.buffers.len(),
                    self.cur_buf().display_name()
                );
                if !failed.is_empty() {
                    msg.push_str(&format!("  (開けなかったファイル: {})", failed.join(", ")));
                }
                self.status_message = msg;
            }
        } else if let Some(dir) = first_dir {
            match self.filer.active_pane_mut().enter_dir(dir.clone()) {
                Ok(()) => {
                    self.enter_filer();
                    self.status_message = format!("フォルダを開きました: {}", dir.display());
                }
                Err(e) => self.status_message = format!("フォルダを開けません: {e}"),
            }
        } else if !failed.is_empty() {
            self.status_message = format!("開けなかったファイル: {}", failed.join(", "));
        }
    }

    /// ドロップされたファイルを開く。
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let paths: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if paths.is_empty() {
            return;
        }
        if self.filer_modal.is_some()
            || self.inline_prompt.is_some()
            || self.pending_dialog.is_some()
            || self.quit_after_save
        {
            self.status_message = "ダイアログを閉じてからドロップしてください".to_string();
            return;
        }
        self.open_menu = None;
        self.show_help = false;
        self.show_about = false;
        self.prefix = PrefixKey::None;
        self.open_paths(paths, false);
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// ファイルをウィンドウ上にドラッグしている間の案内表示。
    fn render_drop_overlay(&self, ctx: &egui::Context) {
        let count = ctx.input(|i| i.raw.hovered_files.len());
        if count == 0 {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("vz_drop_overlay"),
        ));
        let rect = ctx.screen_rect();
        painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(8, 24, 56, 210));
        painter.rect_stroke(
            rect.shrink(10.0),
            CornerRadius::same(6),
            Stroke::new(3.0_f32, TEXT_CYAN),
            StrokeKind::Inside,
        );
        let text = if count > 1 {
            format!("ドロップして {count} 個のファイルを開く")
        } else {
            "ドロップしてファイルを開く".to_string()
        };
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::monospace(22.0),
            TEXT_CYAN,
        );
    }

    /// ファイルをバッファで開く。開けた（または既存バッファへ切り替えた）ら true。
    fn open_file_in_buffer(&mut self, path: PathBuf) -> bool {
        let canon_target = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());

        // If already open in an existing buffer, switch directly to it.
        if let Some(existing_idx) = self.buffers.iter().position(|slot| {
            slot.buffer.file_path.as_ref().is_some_and(|p| {
                let canon_p = fs::canonicalize(p).unwrap_or_else(|_| p.clone());
                canon_p == canon_target
            })
        }) {
            self.active_buf = existing_idx;
            self.mode = AppMode::Editor;
            self.need_scroll_to_cursor = true;
            if self.cur_slot().markdown_view {
                self.cur_slot_mut().md_scroll_dirty = true;
            }
            let name = self.cur_buf().display_name();
            self.status_message = format!(
                "既存バッファへ切替 [{}/{}]: {}",
                self.active_buf + 1,
                self.buffers.len(),
                name
            );
            return true;
        }

        match TextBuffer::from_file(&path) {
            Ok(buf) => {
                let display_path = path.display().to_string();
                let is_md = buf.is_markdown();
                self.add_buffer(buf);
                if is_md {
                    self.status_message = format!(
                        "開きました [{}/{}]: {} (^PでMDビュー切替)",
                        self.active_buf + 1,
                        self.buffers.len(),
                        display_path
                    );
                } else {
                    self.status_message = format!(
                        "開きました [{}/{}]: {}",
                        self.active_buf + 1,
                        self.buffers.len(),
                        display_path
                    );
                }
                true
            }
            Err(e) => {
                self.status_message = format!("読み込みエラー: {e}");
                false
            }
        }
    }

    fn new_empty_buffer(&mut self) {
        self.buffers.push(BufferSlot::new(TextBuffer::new()));
        self.active_buf = self.buffers.len() - 1;
        self.mode = AppMode::Editor;
        self.need_scroll_to_cursor = true;
        self.status_message = format!(
            "新規バッファ作成 [{}/{}]",
            self.active_buf + 1,
            self.buffers.len()
        );
    }

    fn switch_buffer_step(&mut self, delta: isize) {
        let count = self.buffers.len();
        if count <= 1 {
            self.status_message =
                "開いているバッファは1件です（ESCファイラーから複数ファイルを開けます）"
                    .to_string();
            return;
        }
        let next = ((self.active_buf as isize + delta).rem_euclid(count as isize)) as usize;
        self.active_buf = next;
        self.prefix = PrefixKey::None;
        self.ime_preedit.clear();
        self.need_scroll_to_cursor = true;
        if self.cur_slot().markdown_view {
            self.cur_slot_mut().md_scroll_dirty = true;
        }
        let name = self.cur_buf().display_name();
        let mode_tag = if self.cur_slot().markdown_view {
            " [VIEW]"
        } else {
            ""
        };
        self.status_message =
            format!("バッファ切替 [{}/{}]: {}{}", next + 1, count, name, mode_tag);
    }

    fn switch_buffer_to(&mut self, idx: usize) {
        if idx < self.buffers.len() {
            self.active_buf = idx;
            self.prefix = PrefixKey::None;
            self.ime_preedit.clear();
            self.need_scroll_to_cursor = true;
            if self.cur_slot().markdown_view {
                self.cur_slot_mut().md_scroll_dirty = true;
            }
            let name = self.cur_buf().display_name();
            let mode_tag = if self.cur_slot().markdown_view {
                " [VIEW]"
            } else {
                ""
            };
            self.status_message = format!(
                "バッファ切替 [{}/{}]: {}{}",
                idx + 1,
                self.buffers.len(),
                name,
                mode_tag
            );
        }
    }

    fn update_open_buffer_path(&mut self, old_path: &Path, new_path: &Path) {
        let canon_old = fs::canonicalize(old_path).unwrap_or_else(|_| old_path.to_path_buf());
        for slot in &mut self.buffers {
            if let Some(p) = &slot.buffer.file_path {
                let canon_p = fs::canonicalize(p).unwrap_or_else(|_| p.clone());
                if canon_p == canon_old || p == old_path {
                    slot.buffer.file_path = Some(new_path.to_path_buf());
                }
            }
        }
    }

    // --- Native Dialogs (Editor) ---

    fn poll_dialog(&mut self) {
        let result = match &self.pending_dialog {
            Some(rx) => match rx.try_recv() {
                Ok(res) => Some(res),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.pending_dialog = None;
                    None
                }
            },
            None => None,
        };

        if let Some(res) = result {
            self.pending_dialog = None;
            match res {
                DialogResult::SavePath(Some(path)) => self.perform_save(path),
                DialogResult::SavePath(None) => {
                    self.status_message = if self.quit_after_save {
                        "保存をキャンセルしたため終了を中止しました".to_string()
                    } else {
                        "保存をキャンセルしました".to_string()
                    };
                    self.quit_after_save = false;
                }
                DialogResult::OpenPaths(paths) if !paths.is_empty() => {
                    self.open_paths(paths, false)
                }
                DialogResult::OpenPaths(_) => {
                    self.status_message = "ファイルを開くのをキャンセルしました".to_string();
                }
                DialogResult::Unavailable { is_save } => {
                    let initial = self
                        .cur_buf()
                        .file_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| {
                            std::env::current_dir()
                                .map(|d| d.join("untitled.txt").display().to_string())
                                .unwrap_or_else(|_| "untitled.txt".to_string())
                        });
                    self.inline_prompt = Some(InlinePathPrompt {
                        is_save,
                        path_input: initial,
                        just_opened: true,
                    });
                }
            }
        }
    }

    // --- Dynamic Window Title ---

    fn compute_window_title(&self) -> String {
        match self.mode {
            AppMode::Editor => {
                let buf = self.cur_buf();
                let dirty_prefix = if buf.dirty { "*" } else { "" };
                format!("{}{} - VZ Editor", dirty_prefix, buf.display_name())
            }
            AppMode::Filer => {
                let dir = self.filer.active_pane().current_dir.display();
                format!("[2画面ファイラー] {} - VZ Editor", dir)
            }
        }
    }

    fn sync_window_title(&mut self, ctx: &egui::Context) {
        let title = self.compute_window_title();
        if title != self.last_window_title {
            self.last_window_title = title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
    }

    fn perform_save(&mut self, path: PathBuf) {
        match self.cur_buf_mut().save_to_path(&path) {
            Ok(()) => {
                self.status_message = format!("保存しました: {}", path.display());
            }
            Err(e) => {
                self.status_message = format!("保存エラー: {e}");
                self.quit_after_save = false;
            }
        }
    }

    fn trigger_save(&mut self) {
        if let Some(path) = self.cur_buf().file_path.clone() {
            self.perform_save(path);
        } else if self.pending_dialog.is_none() {
            self.status_message = "保存先を選択中...".to_string();
            self.pending_dialog = Some(spawn_save_dialog(None));
        }
    }

    fn trigger_save_as(&mut self) {
        if self.pending_dialog.is_none() {
            self.status_message = "名前を付けて保存先を選択中...".to_string();
            let current = self.cur_buf().file_path.clone();
            self.pending_dialog = Some(spawn_save_dialog(current.as_deref()));
        }
    }

    fn trigger_open(&mut self) {
        if self.pending_dialog.is_none() {
            self.status_message = "開くファイルを選択中...".to_string();
            let dir = self
                .cur_buf()
                .file_path
                .as_ref()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf());
            self.pending_dialog = Some(spawn_open_dialog(dir.as_deref()));
        }
    }

    fn close_current_buffer(&mut self) {
        let idx = self.active_buf;
        if self.buffers[idx].buffer.dirty {
            let name = self.buffers[idx].buffer.display_name();
            self.filer_modal = Some(FilerModal::ConfirmCloseBuffer { buf_idx: idx, name });
        } else {
            self.force_close_buffer(idx);
        }
    }

    fn force_close_buffer(&mut self, idx: usize) {
        if idx >= self.buffers.len() {
            return;
        }
        let closed_name = self.buffers[idx].buffer.display_name();
        self.prefix = PrefixKey::None;
        self.ime_preedit.clear();
        if self.buffers.len() <= 1 {
            self.buffers[0] = BufferSlot::new(TextBuffer::new());
            self.active_buf = 0;
            self.need_scroll_to_cursor = true;
            self.status_message = format!("バッファを閉じました ({closed_name}) -> 新規 [無題]");
        } else {
            self.buffers.remove(idx);
            if self.active_buf >= self.buffers.len() {
                self.active_buf = self.buffers.len() - 1;
            }
            self.need_scroll_to_cursor = true;
            if self.cur_slot().markdown_view {
                self.cur_slot_mut().md_scroll_dirty = true;
            }
            let next_name = self.cur_buf().display_name();
            self.status_message = format!(
                "バッファを閉じました ({closed_name}) -> [{}/{}] {next_name}",
                self.active_buf + 1,
                self.buffers.len()
            );
        }
    }

    /// F10・メニュー「終了」・ウィンドウの×ボタン / Alt+F4 / Cmd+Q からの終了要求。
    /// 未保存（新規バッファへの入力を含む）があれば確認ダイアログを出す。
    fn request_quit(&mut self, ctx: &egui::Context) {
        if self.has_any_dirty_buffer() {
            self.open_quit_confirm();
        } else {
            self.close_now(ctx);
        }
    }

    fn open_quit_confirm(&mut self) {
        self.quit_after_save = false;
        self.open_menu = None;
        self.show_about = false;
        self.show_help = false;
        self.inline_prompt = None;
        self.filer_modal = Some(FilerModal::ConfirmQuit);
    }

    /// 確認済みとしてウィンドウを閉じる（close_requested を再度止めない）。
    fn close_now(&mut self, ctx: &egui::Context) {
        self.allow_close = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// OS からの終了要求（×ボタン・Alt+F4・Cmd+Q）を横取りして確認する。
    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.allow_close {
            return;
        }
        if !self.has_any_dirty_buffer() {
            self.allow_close = true;
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        if self.pending_dialog.is_some() {
            self.status_message = "保存ダイアログを閉じてから終了してください".to_string();
        } else if !matches!(self.filer_modal, Some(FilerModal::ConfirmQuit)) {
            self.open_quit_confirm();
        }
    }

    /// 「保存して終了」: パスのある未保存バッファを保存し、無題のバッファは順に保存先を尋ねる。
    fn save_dirty_buffers_for_quit(&mut self) -> QuitSaveStep {
        for idx in 0..self.buffers.len() {
            if !self.buffers[idx].buffer.dirty {
                continue;
            }
            let Some(path) = self.buffers[idx].buffer.file_path.clone() else {
                continue;
            };
            if let Err(e) = self.buffers[idx].buffer.save_to_path(&path) {
                self.switch_buffer_to(idx);
                self.status_message = format!("保存エラーのため終了を中止しました: {e}");
                return QuitSaveStep::Failed;
            }
        }
        match self.buffers.iter().position(|s| s.buffer.dirty) {
            Some(idx) => QuitSaveStep::NeedSaveAs(idx),
            None => QuitSaveStep::Done,
        }
    }

    fn continue_save_and_quit(&mut self, ctx: &egui::Context) {
        if !self.quit_after_save || self.pending_dialog.is_some() || self.inline_prompt.is_some() {
            return;
        }
        match self.save_dirty_buffers_for_quit() {
            QuitSaveStep::Done => {
                self.quit_after_save = false;
                self.close_now(ctx);
            }
            QuitSaveStep::NeedSaveAs(idx) => {
                if self.mode != AppMode::Editor {
                    self.enter_editor();
                }
                self.switch_buffer_to(idx);
                self.trigger_save_as();
                self.status_message =
                    format!("終了前に保存: {} の保存先を選択中...", self.cur_buf().display_name());
            }
            QuitSaveStep::Failed => {
                self.quit_after_save = false;
            }
        }
    }

    // --- Clipboard & Edit Menu Helpers ---

    fn menu_undo(&mut self) {
        if self.is_markdown_view() || self.mode != AppMode::Editor {
            return;
        }
        if self.cur_buf_mut().undo() {
            self.need_scroll_to_cursor = true;
            self.status_message = "元に戻しました (Undo)".to_string();
        } else {
            self.status_message = "元に戻せる履歴がありません".to_string();
        }
    }

    fn menu_redo(&mut self) {
        if self.is_markdown_view() || self.mode != AppMode::Editor {
            return;
        }
        if self.cur_buf_mut().redo() {
            self.need_scroll_to_cursor = true;
            self.status_message = "やり直しました (Redo)".to_string();
        } else {
            self.status_message = "やり直せる履歴がありません".to_string();
        }
    }

    fn menu_copy(&mut self, ctx: &egui::Context) {
        if self.mode != AppMode::Editor {
            return;
        }
        if self.is_sakura() {
            self.sakura_copy();
            return;
        }
        let text = if self.is_markdown_view() {
            self.cur_buf().lines.join(self.cur_buf().line_ending)
        } else {
            self.cur_buf().current_line_str().to_string()
        };
        self.internal_clipboard = text.clone();
        ctx.copy_text(text);
        self.status_message = if self.is_markdown_view() {
            "ドキュメント全体をクリップボードへコピーしました".to_string()
        } else {
            "現在行をクリップボードへコピーしました".to_string()
        };
    }

    fn menu_cut(&mut self, ctx: &egui::Context) {
        if self.mode != AppMode::Editor || self.is_markdown_view() {
            return;
        }
        if self.is_sakura() {
            self.sakura_cut();
            return;
        }
        let line = self.cur_buf().current_line_str().to_string();
        self.internal_clipboard = line.clone();
        ctx.copy_text(line);
        self.cur_buf_mut().delete_current_line();
        self.need_scroll_to_cursor = true;
        self.status_message = "現在行を切り取りました".to_string();
    }

    fn menu_paste(&mut self) {
        if self.mode != AppMode::Editor || self.is_markdown_view() {
            return;
        }
        let pasted = read_system_clipboard().or_else(|| {
            if self.internal_clipboard.is_empty() {
                None
            } else {
                Some(self.internal_clipboard.clone())
            }
        });
        match pasted {
            Some(text) if !text.is_empty() => {
                self.cur_buf_mut().insert_text(&text);
                self.need_scroll_to_cursor = true;
                self.status_message = "クリップボードから貼り付けました".to_string();
            }
            _ => {
                self.status_message =
                    "クリップボードが空です（Ctrl+V での貼り付けも利用できます）".to_string();
            }
        }
    }

    // --- Menu Bar Model & Actions ---

    fn build_menu_entries(&self, menu: TopMenu) -> Vec<MenuEntry> {
        let in_editor = self.mode == AppMode::Editor;
        let in_md_view = self.is_markdown_view();
        let can_edit = in_editor && !in_md_view;
        let sakura = self.is_sakura();
        let sc = |sakura_key: &'static str, vz_key: &'static str| if sakura { sakura_key } else { vz_key };

        match menu {
            TopMenu::File => vec![
                MenuEntry::Item {
                    label: "新規作成",
                    shortcut: sc("Ctrl+N", "F2"),
                    action: MenuAction::NewBuffer,
                    enabled: true,
                },
                MenuEntry::Item {
                    label: "ファイルを開く...",
                    shortcut: sc("Ctrl+O", "F3"),
                    action: MenuAction::OpenFile,
                    enabled: true,
                },
                MenuEntry::Item {
                    label: "上書き保存",
                    shortcut: sc("Ctrl+S", "^K^D / F10"),
                    action: MenuAction::SaveFile,
                    enabled: true,
                },
                MenuEntry::Item {
                    label: "名前を付けて保存...",
                    shortcut: sc("Ctrl+Shift+S", ""),
                    action: MenuAction::SaveAs,
                    enabled: true,
                },
                MenuEntry::Item {
                    label: "現在のバッファを閉じる",
                    shortcut: "Ctrl+F4",
                    action: MenuAction::CloseBuffer,
                    enabled: true,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: if in_editor {
                        "2画面ファイラーへ切替"
                    } else {
                        "エディタ画面へ戻る"
                    },
                    shortcut: sc("Esc", "ESC / F9"),
                    action: MenuAction::ToggleFiler,
                    enabled: true,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: "終了",
                    shortcut: sc("Alt+F4", "^K^Q"),
                    action: MenuAction::Quit,
                    enabled: true,
                },
            ],
            TopMenu::Edit => vec![
                MenuEntry::Item {
                    label: "元に戻す (Undo)",
                    shortcut: sc("Ctrl+Z", ""),
                    action: MenuAction::Undo,
                    enabled: can_edit && self.cur_buf().can_undo(),
                },
                MenuEntry::Item {
                    label: "やり直し (Redo)",
                    shortcut: sc("Ctrl+Y", ""),
                    action: MenuAction::Redo,
                    enabled: can_edit && self.cur_buf().can_redo(),
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: if sakura {
                        "切り取り"
                    } else {
                        "切り取り (現在行)"
                    },
                    shortcut: sc("Ctrl+X", ""),
                    action: MenuAction::Cut,
                    enabled: can_edit,
                },
                MenuEntry::Item {
                    label: if in_md_view {
                        "コピー (全文)"
                    } else if sakura {
                        "コピー"
                    } else {
                        "コピー (現在行)"
                    },
                    shortcut: sc("Ctrl+C", ""),
                    action: MenuAction::Copy,
                    enabled: in_editor,
                },
                MenuEntry::Item {
                    label: "貼り付け",
                    shortcut: "Ctrl+V",
                    action: MenuAction::Paste,
                    enabled: can_edit,
                },
                MenuEntry::Item {
                    label: "すべて選択",
                    shortcut: sc("Ctrl+A", ""),
                    action: MenuAction::SelectAll,
                    enabled: can_edit && sakura,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: "現在行の削除",
                    shortcut: sc("Ctrl+Shift+E", "Ctrl+Y"),
                    action: MenuAction::DeleteLine,
                    enabled: can_edit,
                },
                MenuEntry::Item {
                    label: if in_md_view {
                        "ドキュメント先頭へ"
                    } else {
                        "行頭へ移動"
                    },
                    shortcut: sc("Home", "^Q^S / F4"),
                    action: MenuAction::MoveLineStart,
                    enabled: in_editor,
                },
                MenuEntry::Item {
                    label: if in_md_view {
                        "ドキュメント末尾へ"
                    } else {
                        "行末へ移動"
                    },
                    shortcut: sc("End", "^Q^D / F5"),
                    action: MenuAction::MoveLineEnd,
                    enabled: in_editor,
                },
            ],
            TopMenu::Tools => vec![
                MenuEntry::Item {
                    label: if in_md_view {
                        "テキスト編集モードへ戻す"
                    } else {
                        "Markdown ビューモードへ切替"
                    },
                    shortcut: sc("Ctrl+Shift+V", "Ctrl+P"),
                    action: MenuAction::ToggleMarkdownView,
                    enabled: in_editor,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: "フォントサイズ拡大",
                    shortcut: "Ctrl++",
                    action: MenuAction::FontZoomIn,
                    enabled: self.font_size < MAX_FONT_SIZE,
                },
                MenuEntry::Item {
                    label: "フォントサイズ縮小",
                    shortcut: "Ctrl+-",
                    action: MenuAction::FontZoomOut,
                    enabled: self.font_size > MIN_FONT_SIZE,
                },
                MenuEntry::Item {
                    label: "フォントサイズリセット (16pt)",
                    shortcut: "Ctrl+0",
                    action: MenuAction::FontReset,
                    enabled: true,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: "次のバッファへ切替",
                    shortcut: sc("Ctrl+Tab", "Ctrl+W"),
                    action: MenuAction::NextBuffer,
                    enabled: self.buffers.len() > 1,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: if self.always_on_top {
                        "最前面固定を解除"
                    } else {
                        "最前面に固定"
                    },
                    shortcut: "Alt+T",
                    action: MenuAction::ToggleAlwaysOnTop,
                    enabled: true,
                },
                MenuEntry::Item {
                    label: "背景を濃くする (+10%)",
                    shortcut: "Alt+↑",
                    action: MenuAction::OpacityUp,
                    enabled: self.bg_opacity < window_style::MAX_OPACITY,
                },
                MenuEntry::Item {
                    label: "背景を透過する (-10%)",
                    shortcut: "Alt+↓",
                    action: MenuAction::OpacityDown,
                    enabled: self.bg_opacity > window_style::MIN_OPACITY,
                },
                MenuEntry::Item {
                    label: "背景透過を解除 (100%)",
                    shortcut: "Alt+0",
                    action: MenuAction::OpacityReset,
                    enabled: self.bg_opacity < window_style::MAX_OPACITY,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: "文字を濃くする (+10%)",
                    shortcut: "Alt+Shift++",
                    action: MenuAction::TextOpacityUp,
                    enabled: self.text_opacity < window_style::MAX_OPACITY,
                },
                MenuEntry::Item {
                    label: "文字を透過する (-10%)",
                    shortcut: "Alt+Shift+-",
                    action: MenuAction::TextOpacityDown,
                    enabled: self.text_opacity > window_style::MIN_OPACITY,
                },
                MenuEntry::Item {
                    label: "文字透過を解除 (100%)",
                    shortcut: "Alt+Shift+0",
                    action: MenuAction::TextOpacityReset,
                    enabled: self.text_opacity < window_style::MAX_OPACITY,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: if sakura {
                        "キー割り当て: サクラ互換 → VZ互換へ切替"
                    } else {
                        "キー割り当て: VZ互換 → サクラ互換へ切替"
                    },
                    shortcut: "",
                    action: MenuAction::ToggleKeyScheme,
                    enabled: true,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: "Claude Code へ送信",
                    shortcut: "Alt+C → C",
                    action: MenuAction::SendToClaudeCode,
                    enabled: self.pending_send.is_none(),
                },
                MenuEntry::Item {
                    label: "Codex へ送信",
                    shortcut: "Alt+C → X",
                    action: MenuAction::SendToCodex,
                    enabled: self.pending_send.is_none(),
                },
            ],
            TopMenu::About => vec![
                MenuEntry::Item {
                    label: "キーバインド・操作ヘルプ",
                    shortcut: "F1",
                    action: MenuAction::ShowHelp,
                    enabled: true,
                },
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: "バージョン情報...",
                    shortcut: "",
                    action: MenuAction::ShowAbout,
                    enabled: true,
                },
            ],
        }
    }

    fn first_selectable_menu_idx(entries: &[MenuEntry]) -> usize {
        entries
            .iter()
            .position(|e| matches!(e, MenuEntry::Item { enabled: true, .. }))
            .or_else(|| entries.iter().position(|e| matches!(e, MenuEntry::Item { .. })))
            .unwrap_or(0)
    }

    fn step_menu_selection(&mut self, entries: &[MenuEntry], delta: isize) {
        let len = entries.len();
        if len == 0 {
            return;
        }
        let mut idx = self.menu_selected_idx.min(len - 1);
        for _ in 0..len {
            idx = ((idx as isize + delta).rem_euclid(len as isize)) as usize;
            if matches!(entries[idx], MenuEntry::Item { .. }) {
                self.menu_selected_idx = idx;
                return;
            }
        }
    }

    fn open_top_menu(&mut self, menu: TopMenu) {
        self.prefix = PrefixKey::None;
        let entries = self.build_menu_entries(menu);
        self.menu_selected_idx = Self::first_selectable_menu_idx(&entries);
        self.open_menu = Some(menu);
    }

    fn execute_menu_action(&mut self, ctx: &egui::Context, action: MenuAction) {
        self.open_menu = None;
        self.prefix = PrefixKey::None;
        match action {
            MenuAction::NewBuffer => self.new_empty_buffer(),
            MenuAction::OpenFile => self.trigger_open(),
            MenuAction::SaveFile => self.trigger_save(),
            MenuAction::SaveAs => self.trigger_save_as(),
            MenuAction::CloseBuffer => self.close_current_buffer(),
            MenuAction::ToggleFiler => match self.mode {
                AppMode::Editor => self.enter_filer(),
                AppMode::Filer => self.enter_editor(),
            },
            MenuAction::Quit => self.request_quit(ctx),
            MenuAction::Undo => self.menu_undo(),
            MenuAction::Redo => self.menu_redo(),
            MenuAction::Cut => self.menu_cut(ctx),
            MenuAction::Copy => self.menu_copy(ctx),
            MenuAction::Paste => self.menu_paste(),
            MenuAction::DeleteLine => self.execute_editor_action(EditorAction::DeleteLine),
            MenuAction::MoveLineStart => self.execute_editor_action(EditorAction::MoveLineStart),
            MenuAction::MoveLineEnd => self.execute_editor_action(EditorAction::MoveLineEnd),
            MenuAction::ToggleMarkdownView => {
                if self.mode == AppMode::Editor {
                    self.toggle_markdown_view();
                }
            }
            MenuAction::FontZoomIn => self.zoom_in_font(),
            MenuAction::FontZoomOut => self.zoom_out_font(),
            MenuAction::FontReset => self.reset_font_size(),
            MenuAction::NextBuffer => {
                if self.mode != AppMode::Editor {
                    self.enter_editor();
                }
                self.switch_buffer_step(1);
            }
            MenuAction::ToggleAlwaysOnTop => self.toggle_always_on_top(ctx),
            MenuAction::OpacityUp => self.change_bg_opacity(window_style::OPACITY_STEP as i16),
            MenuAction::OpacityDown => {
                self.change_bg_opacity(-(window_style::OPACITY_STEP as i16))
            }
            MenuAction::OpacityReset => self.set_bg_opacity(window_style::MAX_OPACITY),
            MenuAction::TextOpacityUp => {
                self.change_text_opacity(window_style::OPACITY_STEP as i16)
            }
            MenuAction::TextOpacityDown => {
                self.change_text_opacity(-(window_style::OPACITY_STEP as i16))
            }
            MenuAction::TextOpacityReset => self.set_text_opacity(window_style::MAX_OPACITY),
            MenuAction::SendToClaudeCode => self.start_send(SendTarget::ClaudeCode),
            MenuAction::SendToCodex => self.start_send(SendTarget::Codex),
            MenuAction::ToggleKeyScheme => self.toggle_key_scheme(),
            MenuAction::SelectAll => {
                if self.mode == AppMode::Editor && !self.is_markdown_view() {
                    self.cur_buf_mut().select_all();
                }
            }
            MenuAction::ShowHelp => {
                self.show_help = !self.show_help;
            }
            MenuAction::ShowAbout => {
                self.show_help = false;
                self.show_about = true;
            }
        }
    }

    fn handle_menu_events(&mut self, ctx: &egui::Context) {
        let Some(menu) = self.open_menu else {
            return;
        };
        let events = ctx.input(|i| i.events.clone());
        let mut active_menu = menu;

        for event in events {
            if let Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                let ctrl = modifiers.ctrl || modifiers.command;
                if ctrl || modifiers.alt {
                    continue;
                }
                match key {
                    Key::Escape => {
                        self.open_menu = None;
                        return;
                    }
                    Key::ArrowDown => {
                        let entries = self.build_menu_entries(active_menu);
                        self.step_menu_selection(&entries, 1);
                    }
                    Key::ArrowUp => {
                        let entries = self.build_menu_entries(active_menu);
                        self.step_menu_selection(&entries, -1);
                    }
                    Key::ArrowRight => {
                        active_menu = active_menu.next();
                        self.open_top_menu(active_menu);
                    }
                    Key::ArrowLeft => {
                        active_menu = active_menu.prev();
                        self.open_top_menu(active_menu);
                    }
                    Key::Enter => {
                        let entries = self.build_menu_entries(active_menu);
                        if let Some(MenuEntry::Item {
                            action,
                            enabled: true,
                            ..
                        }) = entries.get(self.menu_selected_idx)
                        {
                            self.execute_menu_action(ctx, *action);
                            return;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // --- Editor Actions ---

    fn execute_editor_action(&mut self, action: EditorAction) {
        self.prefix = PrefixKey::None;
        let in_md_view = self.is_markdown_view();
        match action {
            EditorAction::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            EditorAction::NewBuffer => {
                self.new_empty_buffer();
            }
            EditorAction::OpenFile => {
                self.trigger_open();
            }
            EditorAction::MoveLineStart => {
                if in_md_view {
                    self.scroll_markdown_to_top();
                } else {
                    self.cur_buf_mut().move_line_start();
                    self.need_scroll_to_cursor = true;
                }
            }
            EditorAction::MoveLineEnd => {
                if in_md_view {
                    self.scroll_markdown_to_bottom();
                } else {
                    self.cur_buf_mut().move_line_end();
                    self.need_scroll_to_cursor = true;
                }
            }
            EditorAction::DeleteLine => {
                if in_md_view {
                    self.status_message =
                        "ビューモード中は編集できません (^P / Alt+P で編集モードへ)".to_string();
                } else {
                    self.cur_buf_mut().delete_current_line();
                    self.need_scroll_to_cursor = true;
                    self.status_message = "現在行を削除しました (^Y)".to_string();
                }
            }
            EditorAction::MoveWordLeft => {
                if in_md_view {
                    let step = (self.cur_slot().md_viewport_h * 0.5).max(self.line_height * 4.0);
                    self.scroll_markdown_by(-step);
                } else {
                    self.cur_buf_mut().move_word_left();
                    self.need_scroll_to_cursor = true;
                }
            }
            EditorAction::MoveWordRight => {
                if in_md_view {
                    let step = (self.cur_slot().md_viewport_h * 0.5).max(self.line_height * 4.0);
                    self.scroll_markdown_by(step);
                } else {
                    self.cur_buf_mut().move_word_right();
                    self.need_scroll_to_cursor = true;
                }
            }
            EditorAction::SwitchToFiler => {
                self.enter_filer();
            }
            EditorAction::SaveFile => {
                self.trigger_save();
            }
            EditorAction::Cut => self.sakura_cut(),
            EditorAction::Copy => self.sakura_copy(),
            EditorAction::Paste => self.sakura_paste_from_clipboard(),
            EditorAction::BeginSelect => self.toggle_select_mode(),
            EditorAction::Nop => {}
        }
    }

    // --- Filer Actions ---

    fn filer_open_selected(&mut self) {
        // Space でマークしたファイルがあれば、まとめてバッファで開く
        let marked_files: Vec<PathBuf> = if self.filer.active_pane().marked.is_empty() {
            Vec::new()
        } else {
            self.filer
                .active_pane()
                .target_entries()
                .into_iter()
                .filter(|e| !e.is_dir && !e.is_parent)
                .map(|e| e.path)
                .collect()
        };
        if !marked_files.is_empty() {
            self.filer.active_pane_mut().marked.clear();
            self.open_paths(marked_files, false);
            return;
        }

        let Some(entry) = self.filer.active_pane().selected_entry().cloned() else {
            return;
        };
        if entry.is_parent {
            if let Err(e) = self.filer.active_pane_mut().go_parent() {
                self.status_message = format!("移動エラー: {e}");
            }
        } else if entry.is_dir {
            if let Err(e) = self.filer.active_pane_mut().enter_dir(entry.path) {
                self.status_message = format!("ディレクトリを開けません: {e}");
            }
        } else {
            self.open_file_in_buffer(entry.path);
        }
    }

    fn execute_filer_action(&mut self, ctx: &egui::Context, action: FilerAction) {
        match action {
            FilerAction::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            FilerAction::Rename => {
                if let Some(entry) = self.filer.active_pane().selected_entry().cloned() {
                    if entry.is_parent {
                        self.status_message = "親ディレクトリ (..) はリネームできません".to_string();
                    } else {
                        self.filer_modal = Some(FilerModal::Rename {
                            src: entry.path,
                            input: entry.name,
                            just_opened: true,
                        });
                    }
                }
            }
            FilerAction::NewFile => {
                let dir = self.filer.active_pane().current_dir.clone();
                self.filer_modal = Some(FilerModal::NewFile {
                    dir,
                    input: String::new(),
                    just_opened: true,
                });
            }
            FilerAction::Copy => {
                let targets = self.filer.active_pane().target_entries();
                if targets.is_empty() {
                    self.status_message = "コピー対象を選択してください".to_string();
                } else {
                    let dst_dir = self.filer.inactive_pane().current_dir.clone();
                    self.filer_modal = Some(FilerModal::ConfirmCopyOrMove {
                        is_move: false,
                        targets,
                        dst_dir,
                    });
                }
            }
            FilerAction::Move => {
                let targets = self.filer.active_pane().target_entries();
                if targets.is_empty() {
                    self.status_message = "移動対象を選択してください".to_string();
                } else {
                    let dst_dir = self.filer.inactive_pane().current_dir.clone();
                    self.filer_modal = Some(FilerModal::ConfirmCopyOrMove {
                        is_move: true,
                        targets,
                        dst_dir,
                    });
                }
            }
            FilerAction::Delete => {
                let targets = self.filer.active_pane().target_entries();
                if targets.is_empty() {
                    self.status_message = "削除対象を選択してください".to_string();
                } else {
                    self.filer_modal = Some(FilerModal::ConfirmDelete { targets });
                }
            }
            FilerAction::NewDir => {
                let dir = self.filer.active_pane().current_dir.clone();
                self.filer_modal = Some(FilerModal::NewDir {
                    dir,
                    input: String::new(),
                    just_opened: true,
                });
            }
            FilerAction::CycleSort => {
                let mode = self.filer.active_pane_mut().cycle_sort();
                self.status_message = format!("ソート順を変更しました: {}", mode.label());
            }
            FilerAction::BackToEditor => {
                self.enter_editor();
            }
            FilerAction::Quit => {
                self.request_quit(ctx);
            }
        }
    }

    // --- Event Dispatch ---

    fn handle_events(&mut self, ctx: &egui::Context) {
        if self.inline_prompt.is_some() || self.filer_modal.is_some() || self.show_about {
            return;
        }
        self.handle_send_keys(ctx);
        self.handle_window_style_keys(ctx);
        if self.open_menu.is_some() {
            self.handle_menu_events(ctx);
            return;
        }
        match self.mode {
            AppMode::Editor => self.handle_editor_events(ctx),
            AppMode::Filer => self.handle_filer_events(ctx),
        }
    }

    fn handle_editor_events(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        let modifiers_now = ctx.input(|i| i.modifiers);
        let mut suppress_text_this_frame = false;

        for event in events {
            let in_md_view = self.is_markdown_view();
            match event {
                Event::Ime(ime_ev) => match ime_ev {
                    ImeEvent::Enabled => {
                        self.ime_active = true;
                    }
                    ImeEvent::Preedit(text) => {
                        if !in_md_view {
                            self.ime_preedit = text;
                            self.need_scroll_to_cursor = true;
                        }
                    }
                    ImeEvent::Commit(text) => {
                        self.ime_preedit.clear();
                        if !in_md_view && !text.is_empty() {
                            self.prefix = PrefixKey::None;
                            self.select_mode = false;
                            self.cur_buf_mut().insert_text(&text);
                            self.need_scroll_to_cursor = true;
                        }
                    }
                    ImeEvent::Disabled => {
                        self.ime_active = false;
                        self.ime_preedit.clear();
                    }
                },

                // egui_winit translates Ctrl+X to Event::Cut -> VZ diamond cursor DOWN (or MD scroll down)!
                Event::Cut => {
                    if !self.ime_preedit.is_empty() {
                        continue;
                    }
                    suppress_text_this_frame = true;
                    if self.is_sakura() {
                        // Ctrl+X / Shift+Del
                        self.sakura_cut();
                        continue;
                    }
                    if self.prefix == PrefixKey::None {
                        if in_md_view {
                            self.scroll_markdown_by(self.line_height * 2.5);
                        } else {
                            self.cur_buf_mut().move_down();
                            self.need_scroll_to_cursor = true;
                        }
                    } else {
                        self.prefix = PrefixKey::None;
                        self.status_message.clear();
                    }
                }

                // egui_winit translates Ctrl+C to Event::Copy -> supports Ctrl+Q+C (doc end).
                Event::Copy => {
                    if !self.ime_preedit.is_empty() {
                        continue;
                    }
                    suppress_text_this_frame = true;
                    if self.is_sakura() {
                        // Ctrl+C / Ctrl+Ins（Ctrl+Shift+C はサクラでは C/C++ ヘッダを開く: 対象外）
                        if !modifiers_now.shift {
                            self.sakura_copy();
                        }
                        continue;
                    }
                    if self.prefix == PrefixKey::CtrlQ {
                        self.prefix = PrefixKey::None;
                        if in_md_view {
                            self.scroll_markdown_to_bottom();
                        } else {
                            self.cur_buf_mut().move_doc_end();
                            self.need_scroll_to_cursor = true;
                        }
                        self.status_message = "ファイル末尾へ移動 (^Q^C)".to_string();
                    } else {
                        self.prefix = PrefixKey::None;
                    }
                }

                Event::Paste(text) => {
                    if !self.ime_preedit.is_empty() {
                        continue;
                    }
                    suppress_text_this_frame = true;
                    if self.is_sakura() {
                        if (modifiers_now.ctrl || modifiers_now.command) && modifiers_now.shift {
                            // Ctrl+Shift+V: Markdown 編集 / ビュー切替（egui は貼り付けとして届ける）
                            self.toggle_markdown_view();
                        } else {
                            // Ctrl+V / Shift+Ins（選択中は置き換え）
                            self.sakura_paste_text(&text);
                        }
                        continue;
                    }
                    self.prefix = PrefixKey::None;
                    if !in_md_view && !text.is_empty() {
                        self.cur_buf_mut().insert_text(&text);
                        self.need_scroll_to_cursor = true;
                    }
                }

                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if !self.ime_preedit.is_empty() {
                        continue;
                    }

                    let ctrl = modifiers.ctrl || modifiers.command;

                    // 0. サクラ互換のキー割り当て（VZ の処理は通らない）
                    if self.is_sakura() {
                        if self.handle_sakura_key(key, modifiers, in_md_view) {
                            suppress_text_this_frame = true;
                            if self.mode == AppMode::Filer {
                                return;
                            }
                        }
                        continue;
                    }

                    // 1. Two-stroke VZ prefix keys (^Q, ^K)
                    match self.prefix {
                        PrefixKey::CtrlQ => {
                            suppress_text_this_frame = true;
                            self.prefix = PrefixKey::None;
                            match key {
                                Key::S => {
                                    if !in_md_view {
                                        self.cur_buf_mut().move_line_start();
                                        self.need_scroll_to_cursor = true;
                                        self.status_message = "行頭へ移動 (^Q^S)".to_string();
                                    }
                                }
                                Key::D => {
                                    if !in_md_view {
                                        self.cur_buf_mut().move_line_end();
                                        self.need_scroll_to_cursor = true;
                                        self.status_message = "行末へ移動 (^Q^D)".to_string();
                                    }
                                }
                                Key::R => {
                                    if in_md_view {
                                        self.scroll_markdown_to_top();
                                    } else {
                                        self.cur_buf_mut().move_doc_start();
                                        self.need_scroll_to_cursor = true;
                                    }
                                    self.status_message = "ファイル先頭へ移動 (^Q^R)".to_string();
                                }
                                Key::C => {
                                    if in_md_view {
                                        self.scroll_markdown_to_bottom();
                                    } else {
                                        self.cur_buf_mut().move_doc_end();
                                        self.need_scroll_to_cursor = true;
                                    }
                                    self.status_message = "ファイル末尾へ移動 (^Q^C)".to_string();
                                }
                                Key::Escape => {
                                    self.status_message = "コマンドをキャンセルしました".to_string();
                                }
                                _ => {
                                    self.status_message = format!("未定義の ^Q コマンド: {key:?}");
                                }
                            }
                            continue;
                        }
                        PrefixKey::CtrlK => {
                            suppress_text_this_frame = true;
                            self.prefix = PrefixKey::None;
                            match key {
                                Key::D => {
                                    self.trigger_save();
                                }
                                Key::V => {
                                    self.toggle_markdown_view();
                                }
                                Key::Q => {
                                    self.enter_filer();
                                    return;
                                }
                                Key::Escape => {
                                    self.status_message = "コマンドをキャンセルしました".to_string();
                                }
                                _ => {
                                    self.status_message = format!("未定義の ^K コマンド: {key:?}");
                                }
                            }
                            continue;
                        }
                        PrefixKey::None => {}
                    }

                    // 1.5. Ctrl+F4: ファイル（現在のバッファ）を閉じる（F4 の行頭移動より先に判定）
                    if key == Key::F4 && ctrl && !modifiers.alt && !modifiers.shift {
                        suppress_text_this_frame = true;
                        self.close_current_buffer();
                        continue;
                    }

                    // 2. Function keys F1..F10
                    let fkey_action = match key {
                        Key::F1 => Some(EditorAction::ToggleHelp),
                        Key::F2 => Some(EditorAction::NewBuffer),
                        Key::F3 => Some(EditorAction::OpenFile),
                        Key::F4 => Some(EditorAction::MoveLineStart),
                        Key::F5 => Some(EditorAction::MoveLineEnd),
                        Key::F6 => Some(EditorAction::DeleteLine),
                        Key::F7 => Some(EditorAction::MoveWordLeft),
                        Key::F8 => Some(EditorAction::MoveWordRight),
                        Key::F9 => Some(EditorAction::SwitchToFiler),
                        Key::F10 => Some(EditorAction::SaveFile),
                        _ => None,
                    };
                    if let Some(action) = fkey_action {
                        suppress_text_this_frame = true;
                        self.execute_editor_action(action);
                        if self.mode == AppMode::Filer {
                            return;
                        }
                        continue;
                    }

                    // 3. ESC: instant switch to 2-pane Filer! (Closes help first if open)
                    if key == Key::Escape {
                        suppress_text_this_frame = true;
                        if self.show_help {
                            self.show_help = false;
                        } else {
                            self.enter_filer();
                            return;
                        }
                        continue;
                    }

                    // 3.5. Font size zoom (Ctrl++ / Ctrl+= / Ctrl+- / Ctrl+0)
                    if self.handle_font_zoom_key(key, modifiers) {
                        suppress_text_this_frame = true;
                        continue;
                    }

                    // 3.6. Alt+P: Markdown edit/view toggle
                    if modifiers.alt && !ctrl && key == Key::P {
                        suppress_text_this_frame = true;
                        self.toggle_markdown_view();
                        continue;
                    }

                    // 4. VZ Ctrl-based bindings
                    if ctrl && !modifiers.alt {
                        let handled = match key {
                            // Markdown Edit / View toggle: Ctrl+P
                            Key::P => {
                                self.toggle_markdown_view();
                                true
                            }
                            // Diamond cursor: Ctrl+E / S / D / X (scroll in MD View)
                            Key::E => {
                                if in_md_view {
                                    self.scroll_markdown_by(-self.line_height * 2.5);
                                } else {
                                    self.cur_buf_mut().move_up();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            Key::S => {
                                if !in_md_view {
                                    self.cur_buf_mut().move_left();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            Key::D => {
                                if !in_md_view {
                                    self.cur_buf_mut().move_right();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            Key::X => {
                                if in_md_view {
                                    self.scroll_markdown_by(self.line_height * 2.5);
                                } else {
                                    self.cur_buf_mut().move_down();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            // Word left / right: Ctrl+A / Ctrl+F
                            Key::A => {
                                if !in_md_view {
                                    self.cur_buf_mut().move_word_left();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            Key::F => {
                                if !in_md_view {
                                    self.cur_buf_mut().move_word_right();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            // Multi-buffer switch (裏画面切替): Ctrl+W / Ctrl+Shift+W
                            Key::W => {
                                if modifiers.shift {
                                    self.switch_buffer_step(-1);
                                } else {
                                    self.switch_buffer_step(1);
                                }
                                true
                            }
                            // Delete current line: Ctrl+Y
                            Key::Y => {
                                if !in_md_view {
                                    self.cur_buf_mut().delete_current_line();
                                    self.need_scroll_to_cursor = true;
                                    self.status_message = "現在行を削除しました (^Y)".to_string();
                                }
                                true
                            }
                            // Delete char at cursor: Ctrl+G
                            Key::G => {
                                if !in_md_view {
                                    self.cur_buf_mut().delete_char_at_cursor();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            // BackSpace: Ctrl+H
                            Key::H => {
                                if !in_md_view {
                                    self.cur_buf_mut().backspace();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            // Enter: Ctrl+M
                            Key::M => {
                                if !in_md_view {
                                    self.cur_buf_mut().insert_newline();
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            // Prefix keys: Ctrl+Q, Ctrl+K
                            Key::Q => {
                                self.prefix = PrefixKey::CtrlQ;
                                self.status_message =
                                    "^Q  [S:行頭  D:行末  R:先頭  C:末尾  ESC:取消]".to_string();
                                true
                            }
                            Key::K => {
                                self.prefix = PrefixKey::CtrlK;
                                self.status_message =
                                    "^K  [D:保存  V:MDビュー切替  Q:ファイラー  ESC:取消]"
                                        .to_string();
                                true
                            }
                            // Page up: Ctrl+R
                            Key::R => {
                                if in_md_view {
                                    let step = (self.cur_slot().md_viewport_h * 0.85)
                                        .max(self.line_height * 4.0);
                                    self.scroll_markdown_by(-step);
                                } else {
                                    let lines = self.last_visible_lines;
                                    self.cur_buf_mut().move_page_up(lines);
                                    self.need_scroll_to_cursor = true;
                                }
                                true
                            }
                            _ => false,
                        };
                        if handled {
                            suppress_text_this_frame = true;
                            continue;
                        }
                    }

                    // 5. Standard navigation & editing keys
                    if !ctrl && !modifiers.alt {
                        if in_md_view {
                            match key {
                                Key::ArrowUp => {
                                    self.scroll_markdown_by(-self.line_height * 2.5);
                                }
                                Key::ArrowDown => {
                                    self.scroll_markdown_by(self.line_height * 2.5);
                                }
                                Key::PageUp => {
                                    let step = (self.cur_slot().md_viewport_h * 0.85)
                                        .max(self.line_height * 4.0);
                                    self.scroll_markdown_by(-step);
                                }
                                Key::PageDown | Key::Space => {
                                    let step = (self.cur_slot().md_viewport_h * 0.85)
                                        .max(self.line_height * 4.0);
                                    self.scroll_markdown_by(step);
                                }
                                Key::Home => {
                                    self.scroll_markdown_to_top();
                                }
                                Key::End => {
                                    self.scroll_markdown_to_bottom();
                                }
                                _ => {}
                            }
                        } else {
                            match key {
                                Key::ArrowUp => {
                                    self.cur_buf_mut().move_up();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::ArrowDown => {
                                    self.cur_buf_mut().move_down();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::ArrowLeft => {
                                    self.cur_buf_mut().move_left();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::ArrowRight => {
                                    self.cur_buf_mut().move_right();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::Home => {
                                    self.cur_buf_mut().move_line_start();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::End => {
                                    self.cur_buf_mut().move_line_end();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::PageUp => {
                                    let lines = self.last_visible_lines;
                                    self.cur_buf_mut().move_page_up(lines);
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::PageDown => {
                                    let lines = self.last_visible_lines;
                                    self.cur_buf_mut().move_page_down(lines);
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::Enter => {
                                    self.cur_buf_mut().insert_newline();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::Backspace => {
                                    self.cur_buf_mut().backspace();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::Delete => {
                                    self.cur_buf_mut().delete_char_at_cursor();
                                    self.need_scroll_to_cursor = true;
                                }
                                Key::Tab => {
                                    self.cur_buf_mut().insert_text("\t");
                                    self.need_scroll_to_cursor = true;
                                }
                                _ => {}
                            }
                        }
                    }
                }

                Event::Text(text) => {
                    if in_md_view
                        || suppress_text_this_frame
                        || !self.ime_preedit.is_empty()
                        || self.prefix != PrefixKey::None
                        || modifiers_now.ctrl
                        || modifiers_now.command
                        || modifiers_now.alt
                    {
                        continue;
                    }
                    let filtered: String = text.chars().filter(|c| !c.is_control()).collect();
                    if !filtered.is_empty() {
                        // 選択中は置き換え（サクラ互換）。F6 の選択モードも終える
                        self.select_mode = false;
                        self.cur_buf_mut().insert_text(&filtered);
                        self.need_scroll_to_cursor = true;
                    }
                }

                _ => {}
            }
        }
    }

    fn handle_filer_events(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());

        for event in events {
            match event {
                // Ctrl+X is converted to Event::Cut by egui_winit -> Filer cursor DOWN!
                Event::Cut => {
                    self.filer.active_pane_mut().move_down();
                }

                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let ctrl = modifiers.ctrl || modifiers.command;

                    // Function keys F1..F10
                    let fkey_action = match key {
                        Key::F1 => Some(FilerAction::ToggleHelp),
                        Key::F2 => Some(FilerAction::Rename),
                        Key::F3 => Some(FilerAction::NewFile),
                        Key::F4 => Some(FilerAction::Copy),
                        Key::F5 => Some(FilerAction::Move),
                        Key::F6 => Some(FilerAction::Delete),
                        Key::F7 => Some(FilerAction::NewDir),
                        Key::F8 => Some(FilerAction::CycleSort),
                        Key::F9 => Some(FilerAction::BackToEditor),
                        Key::F10 => Some(FilerAction::Quit),
                        _ => None,
                    };
                    if let Some(action) = fkey_action {
                        self.execute_filer_action(ctx, action);
                        if self.mode == AppMode::Editor || self.filer_modal.is_some() {
                            return;
                        }
                        continue;
                    }

                    // ESC: close help if open, else return immediately to Editor!
                    if key == Key::Escape {
                        if self.show_help {
                            self.show_help = false;
                        } else {
                            self.enter_editor();
                            return;
                        }
                        continue;
                    }

                    // Font size zoom (Ctrl++ / Ctrl+= / Ctrl+- / Ctrl+0)
                    if self.handle_font_zoom_key(key, modifiers) {
                        continue;
                    }

                    if ctrl && !modifiers.alt {
                        match key {
                            Key::E => self.filer.active_pane_mut().move_up(),
                            Key::X => self.filer.active_pane_mut().move_down(),
                            Key::S => {
                                self.filer.active = ActivePane::Left;
                            }
                            Key::D => {
                                self.filer.active = ActivePane::Right;
                            }
                            Key::H => {
                                if let Err(e) = self.filer.active_pane_mut().go_parent() {
                                    self.status_message = format!("移動エラー: {e}");
                                }
                            }
                            Key::M => {
                                self.filer_open_selected();
                                if self.mode == AppMode::Editor {
                                    return;
                                }
                            }
                            Key::R => {
                                let rows = self.last_filer_visible_rows;
                                self.filer.active_pane_mut().move_page_up(rows);
                            }
                            _ => {}
                        }
                        continue;
                    }

                    if !ctrl && !modifiers.alt {
                        match key {
                            Key::Tab => {
                                self.filer.active = self.filer.active.toggled();
                            }
                            Key::ArrowLeft => {
                                self.filer.active = ActivePane::Left;
                            }
                            Key::ArrowRight => {
                                self.filer.active = ActivePane::Right;
                            }
                            Key::ArrowUp => {
                                self.filer.active_pane_mut().move_up();
                            }
                            Key::ArrowDown => {
                                self.filer.active_pane_mut().move_down();
                            }
                            Key::PageUp => {
                                let rows = self.last_filer_visible_rows;
                                self.filer.active_pane_mut().move_page_up(rows);
                            }
                            Key::PageDown => {
                                let rows = self.last_filer_visible_rows;
                                self.filer.active_pane_mut().move_page_down(rows);
                            }
                            Key::Home => {
                                self.filer.active_pane_mut().move_top();
                            }
                            Key::End => {
                                self.filer.active_pane_mut().move_bottom();
                            }
                            Key::Space => {
                                self.filer.active_pane_mut().toggle_mark_and_advance();
                            }
                            Key::Backspace => {
                                if let Err(e) = self.filer.active_pane_mut().go_parent() {
                                    self.status_message = format!("移動エラー: {e}");
                                }
                            }
                            Key::Enter => {
                                self.filer_open_selected();
                                if self.mode == AppMode::Editor {
                                    return;
                                }
                            }
                            _ => {}
                        }
                    }
                }

                _ => {}
            }
        }
    }

    // --- Compact VZ-Styled Top Menu Bar (VZ Keybinding Safe) ---

    fn render_menu_bar(&mut self, ctx: &egui::Context) {
        if self.open_menu.is_some() {
            // Keep IME off while navigating an open menu so arrows / Enter / ESC stay crisp
            ctx.output_mut(|o| {
                o.ime = None;
            });
        }

        let mut header_rects: [(TopMenu, Rect); 4] = [
            (TopMenu::File, Rect::NOTHING),
            (TopMenu::Edit, Rect::NOTHING),
            (TopMenu::Tools, Rect::NOTHING),
            (TopMenu::About, Rect::NOTHING),
        ];
        let mut bar_rect = Rect::NOTHING;
        let mut clicked_header: Option<TopMenu> = None;
        let mut hovered_header: Option<TopMenu> = None;

        egui::TopBottomPanel::top("menu_bar")
            .exact_height(24.0)
            .frame(
                egui::Frame::NONE
                    .fill(bg(MENU_BAR_BG))
                    .inner_margin(egui::Margin::symmetric(6, 1))
                    .stroke(Stroke::new(1.0_f32, GUTTER_SEP)),
            )
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                bar_rect = ui.max_rect();
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;

                    for (slot_idx, &menu) in TopMenu::ALL.iter().enumerate() {
                        let label = menu.label();
                        let font = FontId::monospace(12.5);
                        let text_w = ui.fonts(|f| {
                            f.layout_no_wrap(label.to_string(), font.clone(), TEXT_WHITE)
                                .size()
                                .x
                        });
                        let btn_size = Vec2::new(text_w + 16.0, 20.0);
                        // Non-focusable click region so keyboard focus is NEVER stolen from editor/filer
                        let (rect, resp) = ui.allocate_exact_size(btn_size, Sense::click());
                        header_rects[slot_idx] = (menu, rect);

                        let is_open = self.open_menu == Some(menu);
                        if resp.hovered() {
                            hovered_header = Some(menu);
                        }
                        if resp.clicked() {
                            clicked_header = Some(menu);
                        }

                        if ui.is_rect_visible(rect) {
                            let painter = ui.painter();
                            if is_open || resp.hovered() {
                                let fill = if is_open {
                                    MENU_ITEM_HOVER
                                } else {
                                    MENU_BAR_HOVER
                                };
                                painter.rect(
                                    rect,
                                    CornerRadius::same(2),
                                    fill,
                                    Stroke::new(
                                        1.0_f32,
                                        if is_open { TEXT_CYAN } else { BORDER_BLUE },
                                    ),
                                    StrokeKind::Inside,
                                );
                            }
                            painter.text(
                                rect.center(),
                                Align2::CENTER_CENTER,
                                label,
                                font,
                                if is_open { TEXT_CYAN } else { TEXT_WHITE },
                            );
                        }
                    }
                });
            });

        if let Some(menu) = clicked_header {
            if self.open_menu == Some(menu) {
                self.open_menu = None;
            } else {
                self.open_top_menu(menu);
            }
        } else if let (Some(open), Some(hovered)) = (self.open_menu, hovered_header) {
            if open != hovered {
                self.open_top_menu(hovered);
            }
        }

        // Dropdown popup rendered on Foreground layer
        let mut triggered_action: Option<MenuAction> = None;
        let mut popup_rect = Rect::NOTHING;

        if let Some(active_menu) = self.open_menu {
            let anchor_rect = header_rects
                .iter()
                .find(|(m, _)| *m == active_menu)
                .map(|(_, r)| *r)
                .unwrap_or(bar_rect);
            let entries = self.build_menu_entries(active_menu);
            let popup_w = 276.0_f32;
            let row_h = 24.0_f32;
            let sep_h = 9.0_f32;
            let selected_idx = self.menu_selected_idx;

            let area_resp = egui::Area::new(egui::Id::new("lz_menu_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(Pos2::new(anchor_rect.min.x, bar_rect.max.y))
                .show(ctx, |ui| {
                    let frame_resp = egui::Frame::NONE
                        .fill(MENU_POPUP_BG)
                        .stroke(Stroke::new(1.5_f32, TEXT_CYAN))
                        .corner_radius(CornerRadius::same(2))
                        .inner_margin(egui::Margin::symmetric(4, 4))
                        .show(ui, |ui| {
                            ui.set_width(popup_w);
                            ui.spacing_mut().item_spacing.y = 1.0;

                            for (idx, entry) in entries.iter().enumerate() {
                                match entry {
                                    MenuEntry::Separator => {
                                        let (rect, _) = ui.allocate_exact_size(
                                            Vec2::new(popup_w, sep_h),
                                            Sense::hover(),
                                        );
                                        let y = rect.center().y;
                                        ui.painter().line_segment(
                                            [
                                                Pos2::new(rect.min.x + 6.0, y),
                                                Pos2::new(rect.max.x - 6.0, y),
                                            ],
                                            Stroke::new(1.0_f32, GUTTER_SEP),
                                        );
                                    }
                                    MenuEntry::Item {
                                        label,
                                        shortcut,
                                        action,
                                        enabled,
                                    } => {
                                        let (rect, resp) = ui.allocate_exact_size(
                                            Vec2::new(popup_w, row_h),
                                            Sense::click(),
                                        );
                                        if resp.hovered() {
                                            self.menu_selected_idx = idx;
                                        }
                                        let is_selected = idx == selected_idx || resp.hovered();
                                        if ui.is_rect_visible(rect) {
                                            let p = ui.painter();
                                            if is_selected && *enabled {
                                                p.rect_filled(
                                                    rect,
                                                    CornerRadius::same(2),
                                                    MENU_ITEM_HOVER,
                                                );
                                            }
                                            let label_color = if !*enabled {
                                                MENU_TEXT_DISABLED
                                            } else if is_selected {
                                                TEXT_WHITE
                                            } else {
                                                TEXT_WHITE
                                            };
                                            p.text(
                                                Pos2::new(rect.min.x + 10.0, rect.center().y),
                                                Align2::LEFT_CENTER,
                                                *label,
                                                FontId::monospace(12.5),
                                                label_color,
                                            );
                                            if !shortcut.is_empty() {
                                                let sc_color = if !*enabled {
                                                    MENU_TEXT_DISABLED
                                                } else if is_selected {
                                                    TEXT_CYAN
                                                } else {
                                                    TEXT_DIM_CYAN
                                                };
                                                p.text(
                                                    Pos2::new(rect.max.x - 10.0, rect.center().y),
                                                    Align2::RIGHT_CENTER,
                                                    *shortcut,
                                                    FontId::monospace(11.5),
                                                    sc_color,
                                                );
                                            }
                                        }
                                        if *enabled && resp.clicked() {
                                            triggered_action = Some(*action);
                                        }
                                    }
                                }
                            }
                        });
                    popup_rect = frame_resp.response.rect;
                });
            if popup_rect == Rect::NOTHING {
                popup_rect = area_resp.response.rect;
            }
        }

        if let Some(action) = triggered_action {
            self.execute_menu_action(ctx, action);
        } else if self.open_menu.is_some() && clicked_header.is_none() {
            // Close menu if the user clicked anywhere outside both the menu bar and the popup
            let clicked_outside = ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|pos| {
                        !bar_rect.contains(pos) && !popup_rect.contains(pos)
                    })
            });
            if clicked_outside {
                self.open_menu = None;
            }
        }
    }

    // --- Status Bar & Multi-Buffer Strip ---

    /// ステータスバー右端の [最前面] [透過 n%] 表示。クリックで切替、透過表示は右クリックで濃くする。
    fn paint_window_style_controls(
        ui: &mut egui::Ui,
        opacity: u8,
        text_opacity: u8,
        always_on_top: bool,
        key_scheme: KeyScheme,
    ) -> Option<MenuAction> {
        let mut cmd = None;
        let scheme_resp = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(match key_scheme {
                        KeyScheme::Sakura => "サクラ",
                        KeyScheme::Vz => "VZ",
                    })
                    .font(FontId::monospace(12.5))
                    .strong()
                    .color(TEXT_CYAN),
                )
                .fill(BG_TAB_INACTIVE)
                .stroke(Stroke::new(1.0_f32, BORDER_BLUE)),
            )
            .on_hover_text("キー割り当ての切替（サクラ互換 / VZ互換）");
        if scheme_resp.clicked() {
            cmd = Some(MenuAction::ToggleKeyScheme);
        }
        let top_resp = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(if always_on_top { "TOP" } else { "top" })
                        .font(FontId::monospace(12.5))
                        .strong()
                        .color(if always_on_top { TEXT_AMBER } else { TEXT_DIM_CYAN }),
                )
                .fill(BG_TAB_INACTIVE)
                .stroke(Stroke::new(1.0_f32, BORDER_BLUE)),
            )
            .on_hover_text("最前面固定の切替 (Alt+T)");
        if top_resp.clicked() {
            cmd = Some(MenuAction::ToggleAlwaysOnTop);
        }
        let op_resp = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(format!("透過{:>3}%", opacity))
                        .font(FontId::monospace(12.5))
                        .color(if opacity < window_style::MAX_OPACITY {
                            TEXT_AMBER
                        } else {
                            TEXT_DIM_CYAN
                        }),
                )
                .fill(BG_TAB_INACTIVE)
                .stroke(Stroke::new(1.0_f32, BORDER_BLUE)),
            )
            .on_hover_text("背景の不透明度: クリックで薄く / 右クリックで濃く (Alt+↓ / Alt+↑ / Alt+0)");
        if op_resp.clicked() {
            cmd = Some(if opacity <= window_style::MIN_OPACITY {
                MenuAction::OpacityReset
            } else {
                MenuAction::OpacityDown
            });
        } else if op_resp.secondary_clicked() {
            cmd = Some(MenuAction::OpacityUp);
        }
        let text_resp = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(format!("文字{:>3}%", text_opacity))
                        .font(FontId::monospace(12.5))
                        .color(if text_opacity < window_style::MAX_OPACITY {
                            TEXT_AMBER
                        } else {
                            TEXT_DIM_CYAN
                        }),
                )
                .fill(BG_TAB_INACTIVE)
                .stroke(Stroke::new(1.0_f32, BORDER_BLUE)),
            )
            .on_hover_text(
                "文字の不透明度: クリックで薄く / 右クリックで濃く (Alt+Shift+- / Alt+Shift++ / Alt+Shift+0)",
            );
        if text_resp.clicked() {
            cmd = Some(if text_opacity <= window_style::MIN_OPACITY {
                MenuAction::TextOpacityReset
            } else {
                MenuAction::TextOpacityDown
            });
        } else if text_resp.secondary_clicked() {
            cmd = Some(MenuAction::TextOpacityUp);
        }
        cmd
    }

    fn paint_font_size_controls(ui: &mut egui::Ui, font_size: f32) -> Option<i8> {
        // Returns Some(-1) for shrink, Some(0) for reset, Some(1) for enlarge.
        // Rendered inside a right_to_left layout: add '+', then '16pt', then '-' so visually: [-] 16pt [+]
        let mut cmd: Option<i8> = None;
        let is_default = (font_size - DEFAULT_FONT_SIZE).abs() < 0.1;

        let plus_resp = ui
            .add(
                egui::Button::new(
                    egui::RichText::new("+")
                        .font(FontId::monospace(12.5))
                        .strong()
                        .color(if font_size < MAX_FONT_SIZE {
                            TEXT_CYAN
                        } else {
                            TEXT_DIM_CYAN
                        }),
                )
                .fill(BG_TAB_INACTIVE)
                .stroke(Stroke::new(1.0_f32, BORDER_BLUE))
                .corner_radius(CornerRadius::same(2))
                .min_size(Vec2::new(18.0, 18.0)),
            )
            .on_hover_text("フォント拡大 (Ctrl++ / Ctrl+=)");
        if plus_resp.clicked() {
            cmd = Some(1);
        }

        let pt_label = format!("{}pt", font_size.round() as i32);
        let pt_resp = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(pt_label)
                        .font(FontId::monospace(12.5))
                        .strong()
                        .color(if is_default { TEXT_WHITE } else { TEXT_AMBER }),
                )
                .fill(BG_TAB_INACTIVE)
                .stroke(Stroke::new(
                    1.0_f32,
                    if is_default { BORDER_BLUE } else { TEXT_AMBER },
                ))
                .corner_radius(CornerRadius::same(2))
                .min_size(Vec2::new(40.0, 18.0)),
            )
            .on_hover_text("クリックでデフォルトサイズ (16pt) へリセット (Ctrl+0)");
        if pt_resp.clicked() {
            cmd = Some(0);
        }

        let minus_resp = ui
            .add(
                egui::Button::new(
                    egui::RichText::new("-")
                        .font(FontId::monospace(12.5))
                        .strong()
                        .color(if font_size > MIN_FONT_SIZE {
                            TEXT_CYAN
                        } else {
                            TEXT_DIM_CYAN
                        }),
                )
                .fill(BG_TAB_INACTIVE)
                .stroke(Stroke::new(1.0_f32, BORDER_BLUE))
                .corner_radius(CornerRadius::same(2))
                .min_size(Vec2::new(18.0, 18.0)),
            )
            .on_hover_text("フォント縮小 (Ctrl+-)");
        if minus_resp.clicked() {
            cmd = Some(-1);
        }

        cmd
    }

    fn render_status_bar(&mut self, ctx: &egui::Context) {
        let mut switch_next_clicked = false;
        let mut toggle_md_clicked = false;
        let mut font_cmd: Option<i8> = None;
        let mut win_cmd: Option<MenuAction> = None;

        egui::TopBottomPanel::top("status_bar")
            .exact_height(30.0)
            .frame(
                egui::Frame::NONE
                    .fill(bg(STATUS_BG))
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .stroke(Stroke::new(1.0_f32, BORDER_BLUE)),
            )
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                ui.horizontal_centered(|ui| match self.mode {
                    AppMode::Editor => {
                        // LZ badge
                        egui::Frame::NONE
                            .fill(STATUS_BADGE_BG)
                            .corner_radius(CornerRadius::same(2))
                            .inner_margin(egui::Margin::symmetric(6, 1))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("VZ")
                                        .font(FontId::monospace(13.0))
                                        .strong()
                                        .color(STATUS_BADGE_FG),
                                );
                            });

                        ui.add_space(6.0);

                        // Multi-buffer count indicator `[1/3]` (clickable to cycle)
                        let buf_badge =
                            format!("[{}/{}]", self.active_buf + 1, self.buffers.len());
                        let badge_resp = ui.add(
                            egui::Button::new(
                                egui::RichText::new(buf_badge)
                                    .font(FontId::monospace(13.0))
                                    .strong()
                                    .color(if self.buffers.len() > 1 {
                                        TEXT_AMBER
                                    } else {
                                        TEXT_CYAN
                                    }),
                            )
                            .fill(BG_TAB_INACTIVE)
                            .stroke(Stroke::new(1.0_f32, BORDER_BLUE))
                            .corner_radius(CornerRadius::same(2)),
                        );
                        if badge_resp.clicked() {
                            switch_next_clicked = true;
                        }
                        badge_resp.on_hover_text("Ctrl+W またはクリックでバッファ切替");

                        ui.add_space(6.0);

                        // File name & dirty flag '*'
                        let name = self.cur_buf().display_name();
                        ui.label(
                            egui::RichText::new(name)
                                .font(FontId::monospace(14.0))
                                .strong()
                                .color(TEXT_WHITE),
                        );
                        if self.cur_buf().dirty {
                            ui.label(
                                egui::RichText::new("*")
                                    .font(FontId::monospace(15.0))
                                    .strong()
                                    .color(TEXT_AMBER),
                            );
                        }

                        // Markdown [EDIT] / [VIEW] clickable toggle badge
                        let is_md = self.cur_buf().is_markdown();
                        let in_view = self.cur_slot().markdown_view;
                        if is_md || in_view {
                            ui.add_space(6.0);
                            let (md_label, md_bg, md_fg, md_stroke) = if in_view {
                                (
                                    "[VIEW]",
                                    STATUS_BADGE_VIEW_BG,
                                    STATUS_BADGE_FG,
                                    STATUS_BADGE_VIEW_BG,
                                )
                            } else {
                                ("[EDIT]", BG_TAB_INACTIVE, TEXT_CYAN, BORDER_BLUE)
                            };
                            let md_resp = ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(md_label)
                                            .font(FontId::monospace(12.5))
                                            .strong()
                                            .color(md_fg),
                                    )
                                    .fill(md_bg)
                                    .stroke(Stroke::new(1.0_f32, md_stroke))
                                    .corner_radius(CornerRadius::same(2)),
                                )
                                .on_hover_text(
                                    "クリックまたは Ctrl+P / Alt+P / Ctrl+K+V で Markdown 編集/ビュー切替",
                                );
                            if md_resp.clicked() {
                                toggle_md_clicked = true;
                            }
                        }

                        // Prefix key badge (^Q / ^K)
                        if self.prefix != PrefixKey::None {
                            ui.add_space(8.0);
                            let ptext = match self.prefix {
                                PrefixKey::CtrlQ => "^Q",
                                PrefixKey::CtrlK => "^K",
                                PrefixKey::None => "",
                            };
                            egui::Frame::NONE
                                .fill(TEXT_AMBER)
                                .corner_radius(CornerRadius::same(2))
                                .inner_margin(egui::Margin::symmetric(5, 1))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(ptext)
                                            .font(FontId::monospace(13.0))
                                            .strong()
                                            .color(STATUS_BADGE_FG),
                                    );
                                });
                        }

                        // Status message (truncated so right-aligned controls never clip)
                        if !self.status_message.is_empty() {
                            let right_reserve = 380.0_f32;
                            let avail_msg_w = ui.available_width() - right_reserve;
                            if avail_msg_w > 48.0 {
                                ui.add_space(8.0);
                                let max_cols = ((avail_msg_w - 8.0) / 7.6).floor().max(6.0) as usize;
                                let clipped = truncate_display_right(&self.status_message, max_cols);
                                let msg_color = if self.status_message.contains("エラー") {
                                    TEXT_AMBER
                                } else {
                                    TEXT_CYAN
                                };
                                ui.label(
                                    egui::RichText::new(clipped)
                                        .font(FontId::monospace(12.5))
                                        .color(msg_color),
                                );
                            }
                        }

                        // Right-aligned encoding + line/column info + font size controls
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let buf = self.cur_buf();
                            let pos_text = if self.cur_slot().markdown_view {
                                format!("全{}行 [VIEW]", buf.lines.len())
                            } else {
                                format!(
                                    "Ln {:>4}/{:<4}  Col {:>3}",
                                    buf.display_line_num(),
                                    buf.lines.len(),
                                    buf.display_col_num()
                                )
                            };
                            ui.label(
                                egui::RichText::new(pos_text)
                                    .font(FontId::monospace(13.0))
                                    .color(TEXT_CYAN),
                            );

                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("|")
                                    .font(FontId::monospace(13.0))
                                    .color(BORDER_BLUE),
                            );
                            ui.add_space(6.0);

                            let eol = if buf.line_ending == "\r\n" {
                                "CRLF"
                            } else {
                                "LF"
                            };
                            ui.label(
                                egui::RichText::new(format!("{} {}", buf.encoding, eol))
                                    .font(FontId::monospace(13.0))
                                    .color(TEXT_WHITE),
                            );

                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("|")
                                    .font(FontId::monospace(13.0))
                                    .color(BORDER_BLUE),
                            );
                            ui.add_space(6.0);

                            if let Some(cmd) = Self::paint_font_size_controls(ui, self.font_size) {
                                font_cmd = Some(cmd);
                            }
                            ui.add_space(6.0);
                            if let Some(cmd) = Self::paint_window_style_controls(
                                ui,
                                self.bg_opacity,
                                self.text_opacity,
                                self.always_on_top,
                                self.key_scheme,
                            ) {
                                win_cmd = Some(cmd);
                            }

                            if !self.cur_slot().markdown_view
                                && (self.ime_active || !self.ime_preedit.is_empty())
                            {
                                ui.add_space(8.0);
                                egui::Frame::NONE
                                    .fill(PREEDIT_BG)
                                    .corner_radius(CornerRadius::same(2))
                                    .inner_margin(egui::Margin::symmetric(5, 1))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new("IME")
                                                .font(FontId::monospace(12.0))
                                                .color(PREEDIT_FG),
                                        );
                                    });
                            }
                        });
                    }
                    AppMode::Filer => {
                        // FILER badge
                        egui::Frame::NONE
                            .fill(STATUS_BADGE_FILER_BG)
                            .corner_radius(CornerRadius::same(2))
                            .inner_margin(egui::Margin::symmetric(6, 1))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("LZ FILER")
                                        .font(FontId::monospace(13.0))
                                        .strong()
                                        .color(STATUS_BADGE_FG),
                                );
                            });

                        ui.add_space(8.0);

                        // Open buffers summary
                        let open_summary = format!(
                            "編集中:{}件 ({})",
                            self.buffers.len(),
                            self.cur_buf().display_name()
                        );
                        ui.label(
                            egui::RichText::new(open_summary)
                                .font(FontId::monospace(13.0))
                                .color(TEXT_WHITE),
                        );

                        // Marked items indicator on active pane
                        let marked_count = self.filer.active_pane().marked.len();
                        if marked_count > 0 {
                            ui.add_space(10.0);
                            egui::Frame::NONE
                                .fill(FILER_ROW_MARKED_CURSOR)
                                .corner_radius(CornerRadius::same(2))
                                .inner_margin(egui::Margin::symmetric(6, 1))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("マーク: {marked_count}件"))
                                            .font(FontId::monospace(12.5))
                                            .strong()
                                            .color(TEXT_AMBER),
                                    );
                                });
                        }

                        if !self.status_message.is_empty() {
                            let right_reserve = 250.0_f32;
                            let avail_msg_w = ui.available_width() - right_reserve;
                            if avail_msg_w > 48.0 {
                                ui.add_space(10.0);
                                let max_cols = ((avail_msg_w - 10.0) / 7.8).floor().max(6.0) as usize;
                                let clipped = truncate_display_right(&self.status_message, max_cols);
                                let msg_color = if self.status_message.contains("エラー")
                                    || self.status_message.contains("失敗")
                                {
                                    TEXT_AMBER
                                } else {
                                    TEXT_CYAN
                                };
                                ui.label(
                                    egui::RichText::new(clipped)
                                        .font(FontId::monospace(13.0))
                                        .color(msg_color),
                                );
                            }
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let sort_label = self.filer.active_pane().sort_mode.label();
                            let side = match self.filer.active {
                                ActivePane::Left => "左窓",
                                ActivePane::Right => "右窓",
                            };
                            ui.label(
                                egui::RichText::new(format!("{side} [{sort_label}]"))
                                    .font(FontId::monospace(13.0))
                                    .color(TEXT_CYAN),
                            );

                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("|")
                                    .font(FontId::monospace(13.0))
                                    .color(BORDER_BLUE),
                            );
                            ui.add_space(6.0);

                            if let Some(cmd) = Self::paint_font_size_controls(ui, self.font_size) {
                                font_cmd = Some(cmd);
                            }
                            ui.add_space(6.0);
                            if let Some(cmd) = Self::paint_window_style_controls(
                                ui,
                                self.bg_opacity,
                                self.text_opacity,
                                self.always_on_top,
                                self.key_scheme,
                            ) {
                                win_cmd = Some(cmd);
                            }
                        });
                    }
                });
            });

        if let Some(action) = win_cmd {
            self.execute_menu_action(ctx, action);
        }
        if switch_next_clicked {
            self.switch_buffer_step(1);
        }
        if toggle_md_clicked {
            self.toggle_markdown_view();
        }
        if let Some(cmd) = font_cmd {
            match cmd {
                1 => self.zoom_in_font(),
                -1 => self.zoom_out_font(),
                _ => self.reset_font_size(),
            }
        }
    }

    /// Compact multi-buffer strip displayed under the status bar in Editor mode when 2+ files are open.
    fn render_buffer_strip(&mut self, ctx: &egui::Context) {
        if self.mode != AppMode::Editor || self.buffers.len() < 2 {
            return;
        }
        let mut selected_idx: Option<usize> = None;

        egui::TopBottomPanel::top("buffer_strip")
            .exact_height(24.0)
            .frame(
                egui::Frame::NONE
                    .fill(bg(BG_TAB_BAR))
                    .inner_margin(egui::Margin::symmetric(8, 2))
                    .stroke(Stroke::new(1.0_f32, GUTTER_SEP)),
            )
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                ui.horizontal_centered(|ui| {
                    ui.label(
                        egui::RichText::new("^W バッファ:")
                            .font(FontId::monospace(12.0))
                            .color(TEXT_DIM_CYAN),
                    );
                    ui.add_space(4.0);

                    for (idx, slot) in self.buffers.iter().enumerate() {
                        let is_active = idx == self.active_buf;
                        let dirty_mark = if slot.buffer.dirty { "*" } else { "" };
                        let view_mark = if slot.markdown_view { "[V]" } else { "" };
                        let title = format!(
                            "{}:{}{}{}",
                            idx + 1,
                            slot.buffer.display_name(),
                            dirty_mark,
                            view_mark
                        );
                        let resp = ui.add(
                            egui::Button::new(
                                egui::RichText::new(title)
                                    .font(FontId::monospace(12.5))
                                    .color(if is_active {
                                        TEXT_WHITE
                                    } else {
                                        TEXT_DIM_CYAN
                                    }),
                            )
                            .fill(if is_active {
                                BG_TAB_ACTIVE
                            } else {
                                BG_TAB_INACTIVE
                            })
                            .stroke(Stroke::new(
                                1.0_f32,
                                if is_active { TEXT_CYAN } else { GUTTER_SEP },
                            ))
                            .corner_radius(CornerRadius::same(2)),
                        );
                        if resp.clicked() {
                            selected_idx = Some(idx);
                        }
                    }
                });
            });

        if let Some(idx) = selected_idx {
            self.switch_buffer_to(idx);
        }
    }

    // --- Function Key Guide Bar (F1..F10, 2 blocks of 5 keys) ---

    fn render_fkey_bar(&mut self, ctx: &egui::Context) {
        match self.mode {
            AppMode::Editor => self.render_editor_fkey_bar(ctx),
            AppMode::Filer => self.render_filer_fkey_bar(ctx),
        }
    }

    fn render_editor_fkey_bar(&mut self, ctx: &egui::Context) {
        let in_md_view = self.is_markdown_view();
        let keys_left: &[(&str, &str, EditorAction)] = if in_md_view {
            &[
                ("F1", "ヘルプ", EditorAction::ToggleHelp),
                ("F2", "新規", EditorAction::NewBuffer),
                ("F3", "開く", EditorAction::OpenFile),
                ("F4", "先頭", EditorAction::MoveLineStart),
                ("F5", "末尾", EditorAction::MoveLineEnd),
            ]
        } else {
            &[
                ("F1", "ヘルプ", EditorAction::ToggleHelp),
                ("F2", "新規", EditorAction::NewBuffer),
                ("F3", "開く", EditorAction::OpenFile),
                ("F4", "行頭", EditorAction::MoveLineStart),
                ("F5", "行末", EditorAction::MoveLineEnd),
            ]
        };
        let keys_right: &[(&str, &str, EditorAction)] = if in_md_view {
            &[
                ("F6", "行削除", EditorAction::DeleteLine),
                ("F7", "半頁上", EditorAction::MoveWordLeft),
                ("F8", "半頁下", EditorAction::MoveWordRight),
                ("F9", "ファイラ", EditorAction::SwitchToFiler),
                ("F10", "保存", EditorAction::SaveFile),
            ]
        } else {
            &[
                ("F6", "行削除", EditorAction::DeleteLine),
                ("F7", "単語左", EditorAction::MoveWordLeft),
                ("F8", "単語右", EditorAction::MoveWordRight),
                ("F9", "ファイラ", EditorAction::SwitchToFiler),
                ("F10", "保存", EditorAction::SaveFile),
            ]
        };

        // サクラ互換の F キー（F1〜F5 は VZ 互換と同じ。未対応のキーは空欄）
        let sakura_left = keys_left;
        let sakura_right: &[(&str, &str, EditorAction)] = &[
            ("F6", "選択", EditorAction::BeginSelect),
            ("F7", "切取", EditorAction::Cut),
            ("F8", "コピー", EditorAction::Copy),
            ("F9", "貼付", EditorAction::Paste),
            ("F10", "", EditorAction::Nop),
        ];
        let (keys_left, keys_right) = if self.is_sakura() {
            (sakura_left, sakura_right)
        } else {
            (keys_left, keys_right)
        };

        let mut clicked_action: Option<EditorAction> = None;

        egui::TopBottomPanel::bottom("fkey_guide_bar")
            .exact_height(34.0)
            .frame(
                egui::Frame::NONE
                    .fill(bg(FKEY_BAR_BG))
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .stroke(Stroke::new(1.0_f32, BORDER_BLUE)),
            )
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                let avail_w = ui.available_width();
                let block_gap = 18.0_f32;
                let key_gap = 4.0_f32;
                let btn_w = ((avail_w - block_gap - key_gap * 8.0) / 10.0).max(56.0);
                let btn_h = ui.available_height().max(24.0);

                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = key_gap;

                    for &(fkey, label, action) in keys_left {
                        if Self::paint_fkey_button(
                            ui,
                            fkey,
                            label,
                            Vec2::new(btn_w, btn_h),
                            FKEY_NUM_BG,
                        ) {
                            clicked_action = Some(action);
                        }
                    }

                    ui.add_space(block_gap - key_gap);

                    for &(fkey, label, action) in keys_right {
                        if Self::paint_fkey_button(
                            ui,
                            fkey,
                            label,
                            Vec2::new(btn_w, btn_h),
                            FKEY_NUM_BG,
                        ) {
                            clicked_action = Some(action);
                        }
                    }
                });
            });

        if let Some(action) = clicked_action {
            self.execute_editor_action(action);
        }
    }

    fn render_filer_fkey_bar(&mut self, ctx: &egui::Context) {
        let keys_left: &[(&str, &str, FilerAction)] = &[
            ("F1", "ヘルプ", FilerAction::ToggleHelp),
            ("F2", "名前変", FilerAction::Rename),
            ("F3", "新規作成", FilerAction::NewFile),
            ("F4", "コピー", FilerAction::Copy),
            ("F5", "移動", FilerAction::Move),
        ];
        let keys_right: &[(&str, &str, FilerAction)] = &[
            ("F6", "削除", FilerAction::Delete),
            ("F7", "フォルダ", FilerAction::NewDir),
            ("F8", "ソート", FilerAction::CycleSort),
            ("F9", "編集戻", FilerAction::BackToEditor),
            ("F10", "終了", FilerAction::Quit),
        ];

        let mut clicked_action: Option<FilerAction> = None;

        egui::TopBottomPanel::bottom("fkey_guide_bar")
            .exact_height(34.0)
            .frame(
                egui::Frame::NONE
                    .fill(bg(FKEY_BAR_BG))
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .stroke(Stroke::new(1.0_f32, BORDER_BLUE)),
            )
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                let avail_w = ui.available_width();
                let block_gap = 18.0_f32;
                let key_gap = 4.0_f32;
                let btn_w = ((avail_w - block_gap - key_gap * 8.0) / 10.0).max(56.0);
                let btn_h = ui.available_height().max(24.0);

                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = key_gap;

                    for &(fkey, label, action) in keys_left {
                        if Self::paint_fkey_button(
                            ui,
                            fkey,
                            label,
                            Vec2::new(btn_w, btn_h),
                            FKEY_NUM_FILER_BG,
                        ) {
                            clicked_action = Some(action);
                        }
                    }

                    ui.add_space(block_gap - key_gap);

                    for &(fkey, label, action) in keys_right {
                        if Self::paint_fkey_button(
                            ui,
                            fkey,
                            label,
                            Vec2::new(btn_w, btn_h),
                            FKEY_NUM_FILER_BG,
                        ) {
                            clicked_action = Some(action);
                        }
                    }
                });
            });

        if let Some(action) = clicked_action {
            self.execute_filer_action(ctx, action);
        }
    }

    fn paint_fkey_button(
        ui: &mut egui::Ui,
        fkey: &str,
        label: &str,
        size: Vec2,
        badge_color: Color32,
    ) -> bool {
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let bg = if response.hovered() {
                FKEY_BTN_HOVER
            } else {
                FKEY_BTN_BG
            };
            painter.rect(
                rect,
                CornerRadius::same(2),
                bg,
                Stroke::new(1.0_f32, BORDER_BLUE),
                StrokeKind::Inside,
            );

            let badge_w = (rect.width() * 0.34).clamp(24.0, 34.0);
            let badge_rect =
                Rect::from_min_max(rect.min, Pos2::new(rect.min.x + badge_w, rect.max.y));
            painter.rect_filled(badge_rect, CornerRadius::same(1), badge_color);
            painter.text(
                badge_rect.center(),
                Align2::CENTER_CENTER,
                fkey,
                FontId::monospace(11.5),
                FKEY_NUM_FG,
            );

            let label_center = Pos2::new(
                badge_rect.max.x + (rect.max.x - badge_rect.max.x) * 0.5,
                rect.center().y,
            );
            painter.text(
                label_center,
                Align2::CENTER_CENTER,
                label,
                FontId::monospace(12.0),
                TEXT_WHITE,
            );
        }
        response.clicked()
    }

    // --- Main Center View (Editor, Markdown View, or 2-Pane Filer) ---

    fn render_markdown_canvas(&mut self, ctx: &egui::Context) {
        // Disable IME while viewing Markdown so navigation/shortcut keys work cleanly.
        ctx.output_mut(|o| {
            o.ime = None;
        });

        let active_idx = self.active_buf;
        let font_size = self.font_size;
        self.cur_slot_mut().ensure_md_cache();

        let mut clicked_url: Option<String> = None;

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                let slot = &mut self.buffers[active_idx];
                let apply_offset = slot.md_scroll_dirty;
                let out = render_markdown_view(
                    ui,
                    active_idx,
                    &slot.md_blocks,
                    font_size,
                    slot.md_scroll_y,
                    apply_offset,
                );
                slot.md_scroll_y = out.scroll_y;
                slot.md_max_scroll_y = out.max_scroll_y;
                slot.md_viewport_h = out.viewport_h;
                slot.md_scroll_dirty = false;
                clicked_url = out.clicked_url;
            });

        if let Some(url) = clicked_url {
            ctx.open_url(egui::OpenUrl::new_tab(&url));
            self.status_message = format!("リンクを開きました: {url}");
        }
    }

    fn render_editor_canvas(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                let avail_rect = ui.available_rect_before_wrap();
                let response = ui.allocate_rect(avail_rect, Sense::click_and_drag());

                let font_id = FontId::monospace(self.font_size);
                let gutter_font_size = (self.font_size * 0.875).max(9.0);
                let marker_font_size = (self.font_size * 0.875).max(9.0);

                let scrollbar_w = 12.0;
                let total_lines = self.cur_buf().lines.len();
                let line_digits = total_lines.max(999).to_string().len();

                // Measure cursor X, preedit width, gutter width, and cursor cell width from actual font metrics
                let cursor_line = self.cur_buf().cursor_line;
                let cursor_col = self.cur_buf().cursor_col;
                let cursor_line_str = self.cur_buf().current_line_str().to_string();
                let cursor_byte = char_to_byte_idx(&cursor_line_str, cursor_col);
                let before_cursor_disp = expand_tabs_for_display(&cursor_line_str[..cursor_byte]);
                let preedit_disp = expand_tabs_for_display(&self.ime_preedit);
                let char_under_cursor = cursor_line_str[cursor_byte..].chars().next();
                let cursor_sample = match char_under_cursor {
                    Some('\t') => "    ".to_string(),
                    Some(c) => c.to_string(),
                    None => "M".to_string(),
                };

                let (cursor_px_x, preedit_px_w, gutter_text_w, cursor_w) = ui.fonts(|f| {
                    let g_before =
                        f.layout_no_wrap(before_cursor_disp.clone(), font_id.clone(), TEXT_WHITE);
                    let x = g_before.size().x;
                    let pw = if preedit_disp.is_empty() {
                        0.0
                    } else {
                        f.layout_no_wrap(preedit_disp.clone(), font_id.clone(), PREEDIT_FG)
                            .size()
                            .x
                    };
                    let gw = f
                        .layout_no_wrap(
                            "9".repeat(line_digits),
                            FontId::monospace(gutter_font_size),
                            TEXT_DIM_CYAN,
                        )
                        .size()
                        .x;
                    let half_cell_w = f
                        .layout_no_wrap("M".to_string(), font_id.clone(), TEXT_WHITE)
                        .size()
                        .x
                        .max(self.font_size * 0.55);
                    let char_w = f
                        .layout_no_wrap(cursor_sample.clone(), font_id.clone(), TEXT_WHITE)
                        .size()
                        .x;
                    (x, pw, gw, char_w.max(half_cell_w))
                });

                let gutter_pad = (self.font_size * 1.1).clamp(14.0, 28.0);
                let gutter_w = (gutter_text_w + gutter_pad).max(48.0);
                let top_pad = 6.0;
                let left_pad = (self.font_size * 0.625).clamp(8.0, 18.0);

                let editor_rect = Rect::from_min_max(
                    Pos2::new(avail_rect.min.x + gutter_w, avail_rect.min.y),
                    Pos2::new(
                        (avail_rect.max.x - scrollbar_w).max(avail_rect.min.x + gutter_w + 20.0),
                        avail_rect.max.y,
                    ),
                );
                let viewport_h = (editor_rect.height() - top_pad * 2.0).max(self.line_height);
                let viewport_w = (editor_rect.width() - left_pad * 2.0).max(40.0);
                self.last_visible_lines = (viewport_h / self.line_height).floor().max(1.0) as usize;

                let mut scroll_y = self.cur_slot().scroll_y;
                let mut scroll_x = self.cur_slot().scroll_x;

                // Ctrl+マウスホイール: フォントサイズ（1ノッチ 1段階）
                if response.hovered() {
                    let wheel_steps: Vec<f32> = ctx.input(|i| {
                        i.events
                            .iter()
                            .filter_map(|e| match e {
                                Event::MouseWheel {
                                    delta, modifiers, ..
                                } if modifiers.ctrl || modifiers.command => Some(delta.y),
                                _ => None,
                            })
                            .collect()
                    });
                    for dy in wheel_steps {
                        if dy > 0.0 {
                            self.zoom_in_font();
                        } else if dy < 0.0 {
                            self.zoom_out_font();
                        }
                    }
                }

                // Mouse wheel scrolling
                if response.hovered() {
                    let scroll_delta = ctx.input(|i| i.smooth_scroll_delta);
                    if scroll_delta.y.abs() > 0.0 {
                        scroll_y -= scroll_delta.y;
                    }
                    if scroll_delta.x.abs() > 0.0 {
                        scroll_x -= scroll_delta.x;
                    }
                }

                // Auto-scroll to keep cursor inside viewport
                let cursor_px_y = cursor_line as f32 * self.line_height;
                if self.need_scroll_to_cursor {
                    if cursor_px_y < scroll_y {
                        scroll_y = cursor_px_y;
                    } else if cursor_px_y + self.line_height > scroll_y + viewport_h {
                        scroll_y = cursor_px_y + self.line_height - viewport_h;
                    }

                    let caret_end_x = cursor_px_x + preedit_px_w + cursor_w + 8.0;
                    if cursor_px_x < scroll_x {
                        scroll_x = (cursor_px_x - cursor_w * 2.0).max(0.0);
                    } else if caret_end_x > scroll_x + viewport_w {
                        scroll_x = (caret_end_x - viewport_w).max(0.0);
                    }
                    self.need_scroll_to_cursor = false;
                }

                let total_content_h = (total_lines as f32 + 2.0) * self.line_height;
                let max_scroll_y = (total_content_h - viewport_h).max(0.0);
                scroll_y = scroll_y.clamp(0.0, max_scroll_y);
                scroll_x = scroll_x.max(0.0);

                // マウス: クリックでカーソル移動。サクラ互換ではドラッグ・Shift+クリックで範囲選択、
                // ダブルクリックで単語、トリプルクリックで行を選択する。
                // ヘルプ表示中のクリックはヘルプを閉じるだけにする
                let can_click = self.open_menu.is_none()
                    && !self.show_about
                    && !self.show_help
                    && self.filer_modal.is_none()
                    && self.inline_prompt.is_none();
                if can_click {
                    let sakura = self.is_sakura();
                    let shift_held = ctx.input(|i| i.modifiers.shift);
                    // 画面上の位置 → (行, 文字位置)
                    let hit = |this: &Self, pos: Pos2| -> TextPos {
                        let rel_y = pos.y - editor_rect.min.y - top_pad + scroll_y;
                        let target_line = ((rel_y / this.line_height).floor().max(0.0) as usize)
                            .min(total_lines.saturating_sub(1));
                        let click_x = (pos.x - editor_rect.min.x - left_pad + scroll_x).max(0.0);
                        let line_str = &this.cur_buf().lines[target_line];
                        let target_col = ui.fonts(|f| {
                            let mut best_col = 0usize;
                            let mut best_dist = click_x;
                            let mut acc = String::new();
                            for (idx, c) in line_str.chars().enumerate() {
                                if c == '\t' {
                                    acc.push_str("    ");
                                } else {
                                    acc.push(c);
                                }
                                let w = f
                                    .layout_no_wrap(acc.clone(), font_id.clone(), TEXT_WHITE)
                                    .size()
                                    .x;
                                let dist = (w - click_x).abs();
                                if dist < best_dist {
                                    best_dist = dist;
                                    best_col = idx + 1;
                                }
                                if w > click_x + this.font_size * 2.0 {
                                    break;
                                }
                            }
                            best_col
                        });
                        (target_line, target_col)
                    };

                    if sakura && response.drag_started() {
                        let origin = ctx
                            .input(|i| i.pointer.press_origin())
                            .or(response.interact_pointer_pos());
                        if let Some(origin) = origin.filter(|p| editor_rect.contains(*p)) {
                            let pos = hit(self, origin);
                            self.prefix = PrefixKey::None;
                            self.select_mode = false;
                            let buf = self.cur_buf_mut();
                            if shift_held {
                                buf.begin_selection_if_none();
                            } else {
                                buf.anchor = Some(pos);
                            }
                            buf.set_cursor(pos.0, pos.1);
                        }
                    }
                    if sakura && response.dragged() && self.cur_buf().anchor.is_some() {
                        if let Some(pointer) = response.interact_pointer_pos() {
                            let pos = hit(self, pointer);
                            self.cur_buf_mut().set_cursor(pos.0, pos.1);
                            self.need_scroll_to_cursor = true;
                        }
                    }

                    if let Some(pointer) = response
                        .interact_pointer_pos()
                        .filter(|p| editor_rect.contains(*p))
                    {
                        if sakura && response.triple_clicked() {
                            let pos = hit(self, pointer);
                            self.cur_buf_mut().select_line(pos.0);
                        } else if sakura && response.double_clicked() {
                            let pos = hit(self, pointer);
                            let buf = self.cur_buf_mut();
                            buf.set_cursor(pos.0, pos.1);
                            buf.select_word_at_cursor();
                        } else if response.clicked() {
                            let pos = hit(self, pointer);
                            self.prefix = PrefixKey::None;
                            self.select_mode = false;
                            let buf = self.cur_buf_mut();
                            if sakura && shift_held {
                                buf.begin_selection_if_none();
                            } else {
                                buf.clear_selection();
                            }
                            buf.set_cursor(pos.0, pos.1);
                        }
                    }
                }

                // Paint background & gutter
                let painter = ui.painter_at(avail_rect);
                window_style::bg_painter(&painter).rect_filled(avail_rect, CornerRadius::ZERO, bg(BG_NAVY));

                let gutter_rect = Rect::from_min_max(
                    avail_rect.min,
                    Pos2::new(avail_rect.min.x + gutter_w, avail_rect.max.y),
                );
                window_style::bg_painter(&painter).rect_filled(gutter_rect, CornerRadius::ZERO, bg(BG_GUTTER));
                painter.line_segment(
                    [
                        Pos2::new(gutter_rect.max.x, avail_rect.min.y),
                        Pos2::new(gutter_rect.max.x, avail_rect.max.y),
                    ],
                    Stroke::new(1.0_f32, GUTTER_SEP),
                );

                // Virtualized line painting
                let first_line = (scroll_y / self.line_height).floor().max(0.0) as usize;
                let last_line = ((scroll_y + viewport_h) / self.line_height).ceil() as usize;
                let end_line = (last_line + 1).min(total_lines);

                let editor_painter = ui.painter_at(editor_rect);
                let gutter_painter = ui.painter_at(gutter_rect);
                let cur_line_now = self.cur_buf().cursor_line;
                let cur_col_now = self.cur_buf().cursor_col;

                for line_idx in first_line..end_line {
                    let y = editor_rect.min.y + top_pad + line_idx as f32 * self.line_height
                        - scroll_y;
                    let is_current = line_idx == cur_line_now;

                    if is_current {
                        let line_bg_rect = Rect::from_min_size(
                            Pos2::new(editor_rect.min.x, y),
                            Vec2::new(editor_rect.width(), self.line_height),
                        );
                        window_style::bg_painter(&editor_painter).rect_filled(
                            line_bg_rect,
                            CornerRadius::ZERO,
                            bg(BG_ACTIVE_LINE),
                        );
                    }

                    let num_str = format!("{:>width$}", line_idx + 1, width = line_digits);
                    gutter_painter.text(
                        Pos2::new(gutter_rect.max.x - 8.0, y + self.line_height * 0.5),
                        Align2::RIGHT_CENTER,
                        num_str,
                        FontId::monospace(gutter_font_size),
                        if is_current { TEXT_CYAN } else { TEXT_DIM_CYAN },
                    );

                    let text_origin_x = editor_rect.min.x + left_pad - scroll_x;
                    let text_y = y + (self.line_height - self.font_size) * 0.42;
                    let line = &self.cur_buf().lines[line_idx];

                    // 範囲選択の背景
                    if let Some((from, to, newline)) = self.cur_buf().line_selection(line_idx) {
                        let x_of = |col: usize| -> f32 {
                            if col == 0 {
                                return 0.0;
                            }
                            let b = char_to_byte_idx(line, col);
                            editor_painter
                                .layout_no_wrap(
                                    expand_tabs_for_display(&line[..b]),
                                    font_id.clone(),
                                    TEXT_WHITE,
                                )
                                .size()
                                .x
                        };
                        let x0 = text_origin_x + x_of(from);
                        let mut x1 = text_origin_x + x_of(to);
                        if newline {
                            x1 += self.font_size * 0.5;
                        }
                        if x1 > x0 {
                            editor_painter.rect_filled(
                                Rect::from_min_max(
                                    Pos2::new(x0, y + 1.0),
                                    Pos2::new(x1, y + self.line_height - 1.0),
                                ),
                                CornerRadius::ZERO,
                                SELECTION_BG,
                            );
                        }
                    }

                    if is_current && !self.ime_preedit.is_empty() {
                        let byte_pos = char_to_byte_idx(line, cur_col_now);
                        let before = expand_tabs_for_display(&line[..byte_pos]);
                        let after = expand_tabs_for_display(&line[byte_pos..]);
                        let mut cur_x = text_origin_x;

                        if !before.is_empty() {
                            let g = editor_painter.layout_no_wrap(
                                before,
                                font_id.clone(),
                                TEXT_WHITE,
                            );
                            let w = g.size().x;
                            editor_painter.galley(Pos2::new(cur_x, text_y), g, TEXT_WHITE);
                            cur_x += w;
                        }

                        let g_pre = editor_painter.layout_no_wrap(
                            preedit_disp.clone(),
                            font_id.clone(),
                            PREEDIT_FG,
                        );
                        let pw = g_pre.size().x;
                        let pre_rect = Rect::from_min_size(
                            Pos2::new(cur_x, y + 1.0),
                            Vec2::new(pw.max(4.0), (self.line_height - 2.0).max(4.0)),
                        );
                        editor_painter.rect_filled(pre_rect, CornerRadius::same(2), PREEDIT_BG);
                        editor_painter.galley(Pos2::new(cur_x, text_y), g_pre, PREEDIT_FG);
                        let underline_w = (self.font_size / 10.0).clamp(1.2, 3.0);
                        editor_painter.line_segment(
                            [
                                Pos2::new(cur_x, y + self.line_height - 2.0),
                                Pos2::new(cur_x + pw, y + self.line_height - 2.0),
                            ],
                            Stroke::new(underline_w, PREEDIT_UNDERLINE),
                        );
                        cur_x += pw;

                        if !after.is_empty() {
                            let g_after = editor_painter.layout_no_wrap(
                                after,
                                font_id.clone(),
                                TEXT_WHITE,
                            );
                            let w = g_after.size().x;
                            editor_painter.galley(Pos2::new(cur_x, text_y), g_after, TEXT_WHITE);
                            cur_x += w;
                        }

                        if line_idx + 1 < total_lines {
                            editor_painter.text(
                                Pos2::new(cur_x + 2.0, y + self.line_height * 0.5),
                                Align2::LEFT_CENTER,
                                "↵",
                                FontId::monospace(marker_font_size),
                                TEXT_CR_MARK,
                            );
                        }
                    } else {
                        let disp = expand_tabs_for_display(line);
                        let galley =
                            editor_painter.layout_no_wrap(disp, font_id.clone(), TEXT_WHITE);
                        let end_x = text_origin_x + galley.size().x;
                        editor_painter.galley(Pos2::new(text_origin_x, text_y), galley, TEXT_WHITE);

                        if line_idx + 1 < total_lines {
                            editor_painter.text(
                                Pos2::new(end_x + 2.0, y + self.line_height * 0.5),
                                Align2::LEFT_CENTER,
                                "↵",
                                FontId::monospace(marker_font_size),
                                TEXT_CR_MARK,
                            );
                        }
                    }
                }

                // [EOF] marker
                let eof_y = editor_rect.min.y
                    + top_pad
                    + total_lines as f32 * self.line_height
                    - scroll_y;
                if eof_y >= editor_rect.min.y - self.line_height && eof_y <= editor_rect.max.y {
                    editor_painter.text(
                        Pos2::new(
                            editor_rect.min.x + left_pad - scroll_x,
                            eof_y + self.line_height * 0.5,
                        ),
                        Align2::LEFT_CENTER,
                        "[EOF]",
                        FontId::monospace(marker_font_size),
                        TEXT_EOF,
                    );
                }

                // Cursor rendering (dynamically scaled to current font_size & line_height)
                let caret_x = editor_rect.min.x + left_pad + cursor_px_x + preedit_px_w - scroll_x;
                let caret_y = editor_rect.min.y + top_pad + cursor_px_y - scroll_y;
                let caret_h = (self.line_height - 2.0).max(4.0);
                let caret_bar_w = (self.font_size / 8.0).clamp(1.5, 4.0);
                let cursor_rect = Rect::from_min_size(
                    Pos2::new(caret_x, caret_y + 1.0),
                    Vec2::new(cursor_w, caret_h),
                );

                if editor_rect.intersects(cursor_rect) && self.inline_prompt.is_none() {
                    let t = ctx.input(|i| i.time);
                    let blink_on = !self.ime_preedit.is_empty() || ((t * 1.8) as u64 % 2 == 0);
                    if blink_on {
                        editor_painter.rect_filled(cursor_rect, CornerRadius::ZERO, CURSOR_FILL);
                        editor_painter.line_segment(
                            [
                                Pos2::new(caret_x, caret_y + 1.0),
                                Pos2::new(caret_x, caret_y + self.line_height - 1.0),
                            ],
                            Stroke::new(caret_bar_w, CURSOR_BAR),
                        );
                    }
                    ctx.request_repaint_after(std::time::Duration::from_millis(450));
                }

                // Report exact IME cursor area for Fcitx5 (tracks font_size & line_height accurately)
                if self.inline_prompt.is_none()
                    && self.filer_modal.is_none()
                    && self.open_menu.is_none()
                    && !self.show_about
                {
                    let preedit_start_x =
                        editor_rect.min.x + left_pad + cursor_px_x - scroll_x;
                    let max_ime_x = (editor_rect.max.x - 4.0).max(editor_rect.min.x);
                    let max_ime_y = (editor_rect.max.y - self.line_height).max(editor_rect.min.y);
                    let ime_x = preedit_start_x.clamp(editor_rect.min.x, max_ime_x);
                    let ime_y = caret_y.clamp(editor_rect.min.y, max_ime_y);
                    let ime_rect = Rect::from_min_size(
                        Pos2::new(ime_x, ime_y),
                        Vec2::new(preedit_px_w.max(cursor_w), self.line_height),
                    );
                    ctx.output_mut(|o| {
                        o.ime = Some(egui::output::IMEOutput {
                            rect: ime_rect,
                            cursor_rect: ime_rect,
                        });
                    });
                }

                // Vertical scrollbar
                let sb_rect = Rect::from_min_max(
                    Pos2::new(avail_rect.max.x - scrollbar_w, avail_rect.min.y),
                    avail_rect.max,
                );
                window_style::bg_painter(&painter).rect_filled(sb_rect, CornerRadius::ZERO, bg(BG_GUTTER));
                if max_scroll_y > 0.0 {
                    let ratio = (viewport_h / total_content_h).clamp(0.06, 1.0);
                    let thumb_h = (sb_rect.height() * ratio).max(24.0);
                    let track_h = (sb_rect.height() - thumb_h).max(1.0);
                    let thumb_y = sb_rect.min.y + (scroll_y / max_scroll_y) * track_h;
                    let thumb_rect = Rect::from_min_size(
                        Pos2::new(sb_rect.min.x + 2.0, thumb_y),
                        Vec2::new(scrollbar_w - 4.0, thumb_h),
                    );
                    painter.rect_filled(thumb_rect, CornerRadius::same(3), BORDER_BLUE);

                    let sb_resp =
                        ui.interact(sb_rect, ui.id().with("vscroll"), Sense::click_and_drag());
                    if sb_resp.dragged() || sb_resp.clicked() {
                        if let Some(pos) = sb_resp.interact_pointer_pos() {
                            let norm =
                                ((pos.y - sb_rect.min.y - thumb_h * 0.5) / track_h).clamp(0.0, 1.0);
                            scroll_y = norm * max_scroll_y;
                        }
                    }
                }

                let slot = self.cur_slot_mut();
                slot.scroll_x = scroll_x;
                slot.scroll_y = scroll_y;
            });
    }

    fn render_filer_canvas(&mut self, ctx: &egui::Context) {
        // When browsing in Filer (no modal open), disable IME so Space/Tab/ESC/VZ keys work cleanly.
        if self.filer_modal.is_none() {
            ctx.output_mut(|o| {
                o.ime = None;
            });
        }

        let mut open_double_clicked = false;

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(bg(BG_NAVY))
                    .inner_margin(egui::Margin::same(6)),
            )
            .show(ctx, |ui| {
                ui.multiply_opacity(window_style::text_alpha());
                let avail = ui.available_rect_before_wrap();
                let gap = 6.0_f32;
                let half_w = ((avail.width() - gap) * 0.5).max(200.0);

                let left_rect =
                    Rect::from_min_size(avail.min, Vec2::new(half_w, avail.height()));
                let right_rect = Rect::from_min_size(
                    Pos2::new(avail.min.x + half_w + gap, avail.min.y),
                    Vec2::new(avail.width() - half_w - gap, avail.height()),
                );

                if self.paint_filer_pane(ui, ctx, ActivePane::Left, left_rect) {
                    open_double_clicked = true;
                }
                if self.paint_filer_pane(ui, ctx, ActivePane::Right, right_rect) {
                    open_double_clicked = true;
                }
            });

        if open_double_clicked {
            self.filer_open_selected();
        }
    }

    /// Paint one side (Left or Right) of the 2-pane VZ Filer. Returns true if an item was double-clicked.
    fn paint_filer_pane(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        side: ActivePane,
        rect: Rect,
    ) -> bool {
        let is_active = self.filer.active == side;
        let pane_id = ui.id().with(match side {
            ActivePane::Left => "filer_pane_left",
            ActivePane::Right => "filer_pane_right",
        });

        // Scale all filer typography & row geometry proportionally from self.font_size
        let scale = self.font_size / DEFAULT_FONT_SIZE;
        let name_font_size = (14.0 * scale).clamp(9.0, 34.0);
        let hdr_font_size = (13.0 * scale).clamp(9.0, 28.0);
        let size_font_size = (12.5 * scale).clamp(8.5, 26.0);
        let date_font_size = (12.0 * scale).clamp(8.5, 24.0);
        let col_font_size = (12.0 * scale).clamp(8.5, 24.0);

        let header_h = (hdr_font_size * 2.0).round().max(24.0);
        let col_hdr_h = (col_font_size * 1.85).round().max(20.0);
        let row_h = (name_font_size * 1.55).round().max(name_font_size + 6.0);
        let sb_w = 10.0_f32;

        // Measure monospace glyph widths at the active font scale for accurate column layout & clipping
        let (hdr_char_w, name_char_w, size_col_w, date_col_w) = ui.fonts(|f| {
            let hc = f
                .layout_no_wrap("M".into(), FontId::monospace(hdr_font_size), TEXT_WHITE)
                .size()
                .x
                .max(hdr_font_size * 0.55);
            let nc = f
                .layout_no_wrap("M".into(), FontId::monospace(name_font_size), TEXT_WHITE)
                .size()
                .x
                .max(name_font_size * 0.55);
            let sc = f
                .layout_no_wrap("999.9 M".into(), FontId::monospace(size_font_size), TEXT_WHITE)
                .size()
                .x
                + 10.0;
            let dc = f
                .layout_no_wrap(
                    "2026-11-23 00:00".into(),
                    FontId::monospace(date_font_size),
                    TEXT_DIM_CYAN,
                )
                .size()
                .x
                + 10.0;
            (hc, nc, sc, dc)
        });

        let border_color = if is_active {
            BORDER_ACTIVE_PANE
        } else {
            BORDER_BLUE
        };
        let hdr_bg = if is_active {
            FILER_HDR_ACTIVE
        } else {
            FILER_HDR_INACTIVE
        };

        let painter = ui.painter_at(rect);
        painter.rect(
            rect,
            CornerRadius::same(2),
            Color32::TRANSPARENT,
            Stroke::new(if is_active { 1.5_f32 } else { 1.0_f32 }, border_color),
            StrokeKind::Inside,
        );

        // Directory Header Bar
        let hdr_rect = Rect::from_min_size(
            Pos2::new(rect.min.x + 1.0, rect.min.y + 1.0),
            Vec2::new(rect.width() - 2.0, header_h),
        );
        painter.rect_filled(hdr_rect, CornerRadius::ZERO, hdr_bg);

        let pane = match side {
            ActivePane::Left => &mut self.filer.left,
            ActivePane::Right => &mut self.filer.right,
        };

        let side_tag = match side {
            ActivePane::Left => "[L]",
            ActivePane::Right => "[R]",
        };
        let entry_count = pane.entries.iter().filter(|e| !e.is_parent).count();
        let marked_count = pane.marked.len();
        let right_info = if marked_count > 0 {
            format!("*{marked_count}/{entry_count}")
        } else {
            format!("{entry_count}件")
        };
        let right_info_w = (right_info.chars().count() as f32 * hdr_char_w * 1.2).max(56.0);
        let max_dir_cols = ((hdr_rect.width() - right_info_w - hdr_char_w * 6.0) / hdr_char_w)
            .floor()
            .max(8.0) as usize;
        let dir_str = truncate_path_left(&pane.current_dir.display().to_string(), max_dir_cols);

        painter.text(
            Pos2::new(hdr_rect.min.x + 8.0, hdr_rect.center().y),
            Align2::LEFT_CENTER,
            format!("{side_tag} {dir_str}"),
            FontId::monospace(hdr_font_size),
            if is_active { TEXT_CYAN } else { TEXT_WHITE },
        );

        painter.text(
            Pos2::new(hdr_rect.max.x - 8.0, hdr_rect.center().y),
            Align2::RIGHT_CENTER,
            right_info,
            FontId::monospace(size_font_size),
            if marked_count > 0 {
                TEXT_AMBER
            } else {
                TEXT_DIM_CYAN
            },
        );

        // Column Header Row
        let col_rect = Rect::from_min_size(
            Pos2::new(rect.min.x + 1.0, hdr_rect.max.y),
            Vec2::new(rect.width() - 2.0, col_hdr_h),
        );
        painter.rect_filled(col_rect, CornerRadius::ZERO, FILER_COL_HDR);
        painter.line_segment(
            [
                Pos2::new(col_rect.min.x, col_rect.max.y),
                Pos2::new(col_rect.max.x, col_rect.max.y),
            ],
            Stroke::new(1.0_f32, GUTTER_SEP),
        );

        let list_rect = Rect::from_min_max(
            Pos2::new(rect.min.x + 1.0, col_rect.max.y + 1.0),
            Pos2::new(rect.max.x - sb_w - 1.0, rect.max.y - 1.0),
        );

        let mark_x = list_rect.min.x + (8.0 * scale).clamp(6.0, 14.0);
        let name_x = list_rect.min.x + (22.0 * scale).clamp(16.0, 38.0);
        let min_name_w = (name_char_w * 12.0).max(96.0);
        let show_date = list_rect.width() >= (name_x - list_rect.min.x) + min_name_w + size_col_w + date_col_w + 24.0;
        let show_size = list_rect.width() >= (name_x - list_rect.min.x) + min_name_w + size_col_w + 16.0;

        let date_x = list_rect.max.x - 8.0;
        let size_x = if show_date {
            date_x - date_col_w - 8.0
        } else {
            list_rect.max.x - 8.0
        };
        let name_right_limit = if show_size {
            size_x - size_col_w - 6.0
        } else {
            list_rect.max.x - 8.0
        };
        let name_max_w = (name_right_limit - name_x).max(60.0);

        painter.text(
            Pos2::new(name_x, col_rect.center().y),
            Align2::LEFT_CENTER,
            "名前",
            FontId::monospace(col_font_size),
            TEXT_DIM_CYAN,
        );
        if show_size {
            painter.text(
                Pos2::new(size_x, col_rect.center().y),
                Align2::RIGHT_CENTER,
                "サイズ",
                FontId::monospace(col_font_size),
                TEXT_DIM_CYAN,
            );
        }
        if show_date {
            painter.text(
                Pos2::new(date_x, col_rect.center().y),
                Align2::RIGHT_CENTER,
                "更新日時",
                FontId::monospace(col_font_size),
                TEXT_DIM_CYAN,
            );
        }

        // Interaction area for the pane
        let resp = ui.interact(list_rect, pane_id, Sense::click());
        let visible_rows = (list_rect.height() / row_h).floor().max(1.0) as usize;
        if is_active {
            self.last_filer_visible_rows = visible_rows;
        }

        // Scroll handling
        let pane = match side {
            ActivePane::Left => &mut self.filer.left,
            ActivePane::Right => &mut self.filer.right,
        };
        if resp.hovered() {
            let delta = ctx.input(|i| i.smooth_scroll_delta);
            if delta.y.abs() > 0.0 {
                pane.scroll_y -= delta.y;
            }
        }

        if pane.need_scroll_to_cursor {
            let cy = pane.cursor as f32 * row_h;
            if cy < pane.scroll_y {
                pane.scroll_y = cy;
            } else if cy + row_h > pane.scroll_y + list_rect.height() {
                pane.scroll_y = cy + row_h - list_rect.height();
            }
            pane.need_scroll_to_cursor = false;
        }

        let total_h = pane.entries.len() as f32 * row_h;
        let max_scroll = (total_h - list_rect.height()).max(0.0);
        pane.scroll_y = pane.scroll_y.clamp(0.0, max_scroll);

        // Mouse click / double-click on rows
        let mut double_clicked = false;
        // ヘルプ・About 表示中のクリックはそれを閉じるだけにする
        let overlay_open = self.show_help || self.show_about;
        if !overlay_open && (resp.clicked() || resp.double_clicked()) {
            if let Some(pos) = resp.interact_pointer_pos() {
                let clicked_row =
                    ((pos.y - list_rect.min.y + pane.scroll_y) / row_h).floor().max(0.0) as usize;
                if clicked_row < pane.entries.len() {
                    pane.cursor = clicked_row;
                    if resp.double_clicked() {
                        double_clicked = true;
                    }
                }
            }
            self.filer.active = side;
        }

        // Re-borrow pane immutably for painting
        let pane = match side {
            ActivePane::Left => &self.filer.left,
            ActivePane::Right => &self.filer.right,
        };
        let is_active_now = self.filer.active == side;
        let list_painter = ui.painter_at(list_rect);

        let first_row = (pane.scroll_y / row_h).floor().max(0.0) as usize;
        let last_row = ((pane.scroll_y + list_rect.height()) / row_h).ceil() as usize;
        let end_row = (last_row + 1).min(pane.entries.len());

        let cursor_bar_w = (3.0 * scale).clamp(2.5, 5.0);

        for idx in first_row..end_row {
            let entry = &pane.entries[idx];
            let y = list_rect.min.y + idx as f32 * row_h - pane.scroll_y;
            let row_rect = Rect::from_min_size(
                Pos2::new(list_rect.min.x, y),
                Vec2::new(list_rect.width(), row_h),
            );

            let is_cursor = idx == pane.cursor;
            let is_marked = !entry.is_parent && pane.marked.contains(&entry.path);

            let row_bg = match (is_cursor, is_marked, is_active_now) {
                (true, true, true) => Some(FILER_ROW_MARKED_CURSOR),
                (true, false, true) => Some(FILER_ROW_CURSOR_ACTIVE),
                (true, _, false) => Some(FILER_ROW_CURSOR_INACTIVE),
                (false, true, _) => Some(FILER_ROW_MARKED),
                (false, false, _) => None,
            };
            if let Some(bg) = row_bg {
                list_painter.rect_filled(row_rect, CornerRadius::ZERO, bg);
            }
            if is_cursor && is_active_now {
                list_painter.rect_filled(
                    Rect::from_min_size(row_rect.min, Vec2::new(cursor_bar_w, row_h)),
                    CornerRadius::ZERO,
                    CURSOR_BAR,
                );
            }

            // Mark asterisk
            if is_marked {
                list_painter.text(
                    Pos2::new(mark_x, row_rect.center().y),
                    Align2::LEFT_CENTER,
                    "*",
                    FontId::monospace(name_font_size),
                    TEXT_AMBER,
                );
            }

            // Name display
            let display_name = if entry.is_parent {
                "..".to_string()
            } else if entry.is_dir {
                format!("{}/", entry.name)
            } else {
                entry.name.clone()
            };
            let max_chars = (name_max_w / name_char_w).floor().max(8.0) as usize;
            let clipped_name = truncate_display_right(&display_name, max_chars);

            let name_color = if is_marked {
                TEXT_AMBER
            } else if entry.is_dir {
                TEXT_CYAN
            } else {
                TEXT_WHITE
            };
            list_painter.text(
                Pos2::new(name_x, row_rect.center().y),
                Align2::LEFT_CENTER,
                clipped_name,
                FontId::monospace(name_font_size),
                name_color,
            );

            // Size column
            if show_size {
                let size_text = if entry.is_parent {
                    "<UP>".to_string()
                } else if entry.is_dir {
                    "<DIR>".to_string()
                } else {
                    format_size(entry.size)
                };
                list_painter.text(
                    Pos2::new(size_x, row_rect.center().y),
                    Align2::RIGHT_CENTER,
                    size_text,
                    FontId::monospace(size_font_size),
                    if entry.is_dir {
                        TEXT_DIM_CYAN
                    } else {
                        TEXT_WHITE
                    },
                );
            }

            // Modified timestamp
            if show_date && !entry.modified_str.is_empty() {
                list_painter.text(
                    Pos2::new(date_x, row_rect.center().y),
                    Align2::RIGHT_CENTER,
                    &entry.modified_str,
                    FontId::monospace(date_font_size),
                    TEXT_DIM_CYAN,
                );
            }
        }

        // Scrollbar for pane
        let sb_rect = Rect::from_min_max(
            Pos2::new(rect.max.x - sb_w - 1.0, col_rect.max.y + 1.0),
            Pos2::new(rect.max.x - 1.0, rect.max.y - 1.0),
        );
        window_style::bg_painter(&painter).rect_filled(sb_rect, CornerRadius::ZERO, bg(BG_GUTTER));
        if max_scroll > 0.0 {
            let ratio = (list_rect.height() / total_h).clamp(0.08, 1.0);
            let thumb_h = (sb_rect.height() * ratio).max(20.0);
            let track_h = (sb_rect.height() - thumb_h).max(1.0);
            let thumb_y = sb_rect.min.y + (pane.scroll_y / max_scroll) * track_h;
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(sb_rect.min.x + 2.0, thumb_y),
                    Vec2::new(sb_w - 4.0, thumb_h),
                ),
                CornerRadius::same(2),
                if is_active_now {
                    BORDER_ACTIVE_PANE
                } else {
                    BORDER_BLUE
                },
            );
        }

        double_clicked
    }

    // --- Overlays & Modals ---

    fn render_help_overlay(&mut self, ctx: &egui::Context) {
        if !self.show_help {
            self.help_click_armed = false;
            return;
        }
        let close_by_click = self.help_click_armed && ctx.input(|i| i.pointer.any_click());
        self.help_click_armed = true;
        let mut open = self.show_help;
        let title = match self.mode {
            AppMode::Editor => "VZ Editor - エディタ キーバインド (F1 / ESC / クリックで閉じる)",
            AppMode::Filer => "VZ Filer - 2画面ファイラー操作 (F1 / ESC / クリックで閉じる)",
        };
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(STATUS_BG)
                    .stroke(Stroke::new(1.5_f32, TEXT_CYAN)),
            )
            .show(ctx, |ui| {
                let rows: &[(&str, &str)] = match self.mode {
                    AppMode::Editor if self.key_scheme == KeyScheme::Sakura => &[
                        ("Ctrl+N / Ctrl+O", "新規作成 / 開く (F2 / F3)"),
                        ("F4 / F5", "行頭 / 行末 (ビュー時は先頭 / 末尾。Shift で範囲選択)"),
                        ("Ctrl+S / Ctrl+Shift+S", "上書き保存 / 名前を付けて保存"),
                        ("Ctrl+F4", "ファイル（現在のバッファ）を閉じる"),
                        ("Ctrl+Z / Ctrl+Y", "元に戻す / やり直し"),
                        ("Ctrl+X / Ctrl+C / Ctrl+V", "切り取り / コピー / 貼り付け (F7 / F8 / F9)"),
                        ("Shift+移動キー / F6", "範囲選択 / 範囲選択開始"),
                        ("Ctrl+A / Ctrl+W", "すべて選択 / 単語選択 (ダブル・トリプルクリック)"),
                        ("Ctrl+← → / Ctrl+Home End", "単語移動 / ファイル先頭・末尾"),
                        ("Ctrl+E / Ctrl+Shift+E", "行切り取り / 行削除"),
                        ("Ctrl+K / Ctrl+U / Ctrl+D", "行末まで / 行頭まで / 単語を切り取り (Shift で削除)"),
                        ("Ctrl+BkSp / Ctrl+Del / Ctrl+I", "単語の左・右を削除 / 行の二重化"),
                        ("Ctrl+Tab / Ctrl+Shift+Tab", "バッファ切替"),
                        ("Ctrl+Shift+V / Ctrl+P", "Markdown 編集 / ビュー切替"),
                        ("Ctrl++ / Ctrl+- / Ctrl+0", "フォントサイズ (Ctrl+ホイールも可)"),
                        ("Esc", "選択解除 / 2画面ファイラーへ切替"),
                        ("Alt+T / Alt+↑↓0", "最前面固定 / 背景の濃さ"),
                        ("Alt+Shift+「+」「-」「0」", "文字の濃さ"),
                        ("Alt+C → C / Alt+C → X", "バッファ全体を Claude Code / Codex へ送信"),
                        ("ツール→キー割り当て", "VZ 互換へ切替"),
                    ],
                    AppMode::Editor => &[
                        ("Ctrl+E / S / D / X", "ダイヤモンドカーソル移動 (ビュー時は上下スクロール)"),
                        ("Ctrl+A / Ctrl+F", "単語単位の左・右移動"),
                        ("Ctrl+Q+S / Ctrl+Q+D", "行頭・行末移動 (Home / End も可)"),
                        ("Ctrl+Q+R / Ctrl+Q+C", "ファイル先頭・末尾移動"),
                        ("Ctrl+Y", "現在行の削除"),
                        ("Ctrl+G / Delete", "カーソル位置の1文字削除"),
                        ("BackSpace / Ctrl+H", "前1文字削除"),
                        ("Ctrl+W / Ctrl+Shift+W", "複数バッファ（裏画面）切替"),
                        ("Ctrl+P / Ctrl+K+V / Alt+P", "Markdown 編集 / ビュー（プレビュー）切替"),
                        ("Ctrl++ / Ctrl+- / Ctrl+0", "フォントサイズ拡大 / 縮小 / リセット"),
                        ("Ctrl+K+D / F10", "上書き保存 / 保存ダイアログ"),
                        ("Ctrl+F4", "ファイル（現在のバッファ）を閉じる"),
                        ("ESC / Ctrl+K+Q / F9", "2画面ファイラーへ切替"),
                        ("Alt+T", "最前面固定の切替"),
                        ("ファイルをドロップ", "複数ファイルをまとめてバッファで開く"),
                        ("Alt+C → C / Alt+C → X", "バッファ全体を Claude Code / Codex へ送信"),
                        ("Alt+↑ / Alt+↓ / Alt+0", "背景を濃く / 薄く / 透過解除"),
                        ("Alt+Shift+「+」「-」「0」", "文字を濃く / 薄く / 透過解除"),
                    ],
                    AppMode::Filer => &[
                        ("Ctrl+E / X (↑ / ↓)", "ファイルカーソル移動"),
                        ("Tab / Ctrl+S / D (← / →)", "左右ペイン（アクティブ窓）切替"),
                        ("Enter / ダブルクリック", "フォルダ移動・ファイルを開く (マーク時はまとめて開く)"),
                        ("BackSpace / Ctrl+H", "親ディレクトリへ移動"),
                        ("Space", "ファイルマーク（複数選択・反転表示）"),
                        ("Ctrl++ / Ctrl+- / Ctrl+0", "フォントサイズ拡大 / 縮小 / リセット"),
                        ("F2 / F3 / F7", "リネーム / 新規ファイル作成 / フォルダ作成"),
                        ("F4 / F5 / F6", "反対側窓へコピー / 移動 / 削除"),
                        ("F8", "ソート順切替（名前/更新/サイズ/拡張子）"),
                        ("ESC / F9", "エディタ画面へ戻る"),
                        ("F10", "アプリケーション終了"),
                        ("Alt+T / Alt+↑ / Alt+↓", "最前面固定 / 背景の濃さ変更"),
                        ("Alt+Shift+「+」「-」「0」", "文字の濃さ変更 / 透過解除"),
                    ],
                };
                egui::Grid::new("vz_help_grid")
                    .num_columns(2)
                    .spacing([18.0, 6.0])
                    .show(ui, |ui| {
                        for &(k, desc) in rows {
                            ui.label(
                                egui::RichText::new(k)
                                    .font(FontId::monospace(13.5))
                                    .strong()
                                    .color(TEXT_CYAN),
                            );
                            ui.label(
                                egui::RichText::new(desc)
                                    .font(FontId::monospace(13.5))
                                    .color(TEXT_WHITE),
                            );
                            ui.end_row();
                        }
                    });
            });
        self.show_help = open && !close_by_click;
    }

    fn render_about_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            self.about_click_armed = false;
            return;
        }
        ctx.output_mut(|o| {
            o.ime = None;
        });

        let mut open = self.show_about;
        // どこをクリックしても閉じる（リンクのクリックは除く）
        let mut close_clicked = self.about_click_armed && ctx.input(|i| i.pointer.any_click());
        self.about_click_armed = true;
        let mut link_clicked = false;

        egui::Window::new("VZ Editor について (バージョン情報)")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(STATUS_BG)
                    .stroke(Stroke::new(1.5_f32, TEXT_CYAN))
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                ui.set_min_width(430.0);

                // Logo & version header
                ui.horizontal(|ui| {
                    egui::Frame::NONE
                        .fill(STATUS_BADGE_BG)
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(12, 6))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("VZ")
                                    .font(FontId::monospace(22.0))
                                    .strong()
                                    .color(STATUS_BADGE_FG),
                            );
                        });
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("VZ Editor")
                                .font(FontId::monospace(20.0))
                                .strong()
                                .color(TEXT_CYAN),
                        );
                        ui.label(
                            egui::RichText::new(format!("Version {APP_VERSION}  Build At {BUILD_AT}"))
                                .font(FontId::monospace(13.5))
                                .strong()
                                .color(TEXT_AMBER),
                        );
                    });
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(
                    egui::RichText::new(
                        "Rust + egui 製の高速ネイティブ GUI VZ 風エディター & 2画面ファイラー",
                    )
                    .font(FontId::monospace(13.0))
                    .color(TEXT_WHITE),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "背面透過・最前面固定・ダイヤモンドカーソル・ESC 2画面ファイラー・IME・Markdown ビュー対応",
                    )
                    .font(FontId::monospace(12.0))
                    .color(TEXT_DIM_CYAN),
                );

                ui.add_space(10.0);
                egui::Grid::new("about_meta_grid")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("License:")
                                .font(FontId::monospace(12.5))
                                .color(TEXT_DIM_CYAN),
                        );
                        ui.label(
                            egui::RichText::new("MIT License")
                                .font(FontId::monospace(12.5))
                                .color(TEXT_WHITE),
                        );
                        ui.end_row();

                        ui.label(
                            egui::RichText::new("Copyright:")
                                .font(FontId::monospace(12.5))
                                .color(TEXT_DIM_CYAN),
                        );
                        ui.label(
                            egui::RichText::new(
                                "(c) 2026 Touryu Tamane\n(c) 2026 Sekiya Sato",
                            )
                            .font(FontId::monospace(12.5))
                            .color(TEXT_WHITE),
                        );
                        ui.end_row();

                        ui.label(
                            egui::RichText::new("Based on:")
                                .font(FontId::monospace(12.5))
                                .color(TEXT_DIM_CYAN),
                        );
                        link_clicked |= ui
                            .hyperlink_to(
                                egui::RichText::new(format!("LZ Editor  {BASE_REPO_URL}"))
                                    .font(FontId::monospace(12.5))
                                    .color(TEXT_CYAN),
                                BASE_REPO_URL,
                            )
                            .clicked();
                        ui.end_row();

                        ui.label(
                            egui::RichText::new("Repository:")
                                .font(FontId::monospace(12.5))
                                .color(TEXT_DIM_CYAN),
                        );
                        link_clicked |= ui
                            .hyperlink_to(
                                egui::RichText::new(format!("VZ Editor  {FORK_REPO_URL}"))
                                    .font(FontId::monospace(12.5))
                                    .color(TEXT_CYAN),
                                FORK_REPO_URL,
                            )
                            .clicked();
                        ui.end_row();
                    });

                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("閉じる (ESC / Enter / クリック)").clicked()
                        || ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::Enter))
                    {
                        close_clicked = true;
                    }
                });
            });

        self.show_about = open && (link_clicked || !close_clicked);
    }

    fn render_filer_modal(&mut self, ctx: &egui::Context) {
        let Some(mut modal) = self.filer_modal.take() else {
            return;
        };

        let mut keep_modal = true;

        match &mut modal {
            FilerModal::Rename {
                src,
                input,
                just_opened,
            } => {
                let mut do_submit = false;
                let mut do_cancel = false;
                egui::Window::new("リネーム (F2)")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                    .frame(
                        egui::Frame::window(&ctx.style())
                            .fill(STATUS_BG)
                            .stroke(Stroke::new(1.5_f32, TEXT_CYAN)),
                    )
                    .show(ctx, |ui| {
                        ui.set_min_width(420.0);
                        ui.label(
                            egui::RichText::new(format!("対象: {}", src.display()))
                                .font(FontId::monospace(12.5))
                                .color(TEXT_DIM_CYAN),
                        );
                        ui.add_space(4.0);
                        let resp = ui.add(
                            egui::TextEdit::singleline(input)
                                .font(FontId::monospace(14.0))
                                .desired_width(400.0),
                        );
                        if *just_opened {
                            resp.request_focus();
                            *just_opened = false;
                        }
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            do_submit = true;
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("変更 (Enter)").clicked() {
                                do_submit = true;
                            }
                            if ui.button("キャンセル (ESC)").clicked()
                                || ui.input(|i| i.key_pressed(Key::Escape))
                            {
                                do_cancel = true;
                            }
                        });
                    });

                if do_submit {
                    keep_modal = false;
                    match rename_entry(src, input) {
                        Ok(new_path) => {
                            self.update_open_buffer_path(src, &new_path);
                            self.filer.refresh_both();
                            self.status_message =
                                format!("リネームしました: {}", new_path.display());
                        }
                        Err(e) => {
                            self.status_message = format!("リネームエラー: {e}");
                        }
                    }
                } else if do_cancel {
                    keep_modal = false;
                    self.status_message = "リネームをキャンセルしました".to_string();
                }
            }

            FilerModal::NewFile {
                dir,
                input,
                just_opened,
            } => {
                let mut do_submit = false;
                let mut do_cancel = false;
                egui::Window::new("新規ファイル作成 (F3)")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                    .frame(
                        egui::Frame::window(&ctx.style())
                            .fill(STATUS_BG)
                            .stroke(Stroke::new(1.5_f32, TEXT_CYAN)),
                    )
                    .show(ctx, |ui| {
                        ui.set_min_width(420.0);
                        ui.label(
                            egui::RichText::new(format!("作成先: {}", dir.display()))
                                .font(FontId::monospace(12.5))
                                .color(TEXT_DIM_CYAN),
                        );
                        ui.add_space(4.0);
                        let resp = ui.add(
                            egui::TextEdit::singleline(input)
                                .hint_text("新しいファイル名を入力 (例: memo.txt)")
                                .font(FontId::monospace(14.0))
                                .desired_width(400.0),
                        );
                        if *just_opened {
                            resp.request_focus();
                            *just_opened = false;
                        }
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            do_submit = true;
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("作成して開く (Enter)").clicked() {
                                do_submit = true;
                            }
                            if ui.button("キャンセル (ESC)").clicked()
                                || ui.input(|i| i.key_pressed(Key::Escape))
                            {
                                do_cancel = true;
                            }
                        });
                    });

                if do_submit {
                    keep_modal = false;
                    match create_file(dir, input) {
                        Ok(created_path) => {
                            self.filer.refresh_both();
                            self.open_file_in_buffer(created_path);
                        }
                        Err(e) => {
                            self.status_message = format!("新規作成エラー: {e}");
                        }
                    }
                } else if do_cancel {
                    keep_modal = false;
                    self.status_message = "新規作成をキャンセルしました".to_string();
                }
            }

            FilerModal::NewDir {
                dir,
                input,
                just_opened,
            } => {
                let mut do_submit = false;
                let mut do_cancel = false;
                egui::Window::new("新規フォルダ作成 (F7)")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                    .frame(
                        egui::Frame::window(&ctx.style())
                            .fill(STATUS_BG)
                            .stroke(Stroke::new(1.5_f32, TEXT_CYAN)),
                    )
                    .show(ctx, |ui| {
                        ui.set_min_width(420.0);
                        ui.label(
                            egui::RichText::new(format!("作成先: {}", dir.display()))
                                .font(FontId::monospace(12.5))
                                .color(TEXT_DIM_CYAN),
                        );
                        ui.add_space(4.0);
                        let resp = ui.add(
                            egui::TextEdit::singleline(input)
                                .hint_text("新しいフォルダ名を入力")
                                .font(FontId::monospace(14.0))
                                .desired_width(400.0),
                        );
                        if *just_opened {
                            resp.request_focus();
                            *just_opened = false;
                        }
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            do_submit = true;
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("作成 (Enter)").clicked() {
                                do_submit = true;
                            }
                            if ui.button("キャンセル (ESC)").clicked()
                                || ui.input(|i| i.key_pressed(Key::Escape))
                            {
                                do_cancel = true;
                            }
                        });
                    });

                if do_submit {
                    keep_modal = false;
                    match create_dir(dir, input) {
                        Ok(created_dir) => {
                            self.filer.refresh_both();
                            self.status_message =
                                format!("フォルダを作成しました: {}", created_dir.display());
                        }
                        Err(e) => {
                            self.status_message = format!("フォルダ作成エラー: {e}");
                        }
                    }
                } else if do_cancel {
                    keep_modal = false;
                    self.status_message = "フォルダ作成をキャンセルしました".to_string();
                }
            }

            FilerModal::ConfirmCopyOrMove {
                is_move,
                targets,
                dst_dir,
            } => {
                let op_name = if *is_move { "移動" } else { "コピー" };
                let mut do_exec = false;
                let mut do_cancel = false;

                egui::Window::new(format!("ファイル{op_name}確認"))
                    .collapsible(false)
                    .resizable(false)
                    .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                    .frame(
                        egui::Frame::window(&ctx.style())
                            .fill(STATUS_BG)
                            .stroke(Stroke::new(1.5_f32, TEXT_CYAN)),
                    )
                    .show(ctx, |ui| {
                        ui.set_min_width(440.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "{}件の項目を反対側ペインへ{op_name}しますか？",
                                targets.len()
                            ))
                            .font(FontId::monospace(14.0))
                            .color(TEXT_WHITE),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(format!("宛先: {}", dst_dir.display()))
                                .font(FontId::monospace(13.0))
                                .color(TEXT_CYAN),
                        );
                        ui.add_space(4.0);
                        for entry in targets.iter().take(6) {
                            ui.label(
                                egui::RichText::new(format!("  - {}", entry.name))
                                    .font(FontId::monospace(12.5))
                                    .color(TEXT_AMBER),
                            );
                        }
                        if targets.len() > 6 {
                            ui.label(
                                egui::RichText::new(format!("  ... 他 {}件", targets.len() - 6))
                                    .font(FontId::monospace(12.5))
                                    .color(TEXT_DIM_CYAN),
                            );
                        }
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button(format!("実行 (Enter / Y)")).clicked()
                                || ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Y))
                            {
                                do_exec = true;
                            }
                            if ui.button("キャンセル (ESC / N)").clicked()
                                || ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::N))
                            {
                                do_cancel = true;
                            }
                        });
                    });

                if do_exec {
                    keep_modal = false;
                    let mut ok_count = 0usize;
                    let mut last_err: Option<String> = None;
                    for entry in targets.iter() {
                        let res = if *is_move {
                            move_entry(&entry.path, dst_dir)
                        } else {
                            copy_entry(&entry.path, dst_dir)
                        };
                        match res {
                            Ok(new_path) => {
                                if *is_move {
                                    self.update_open_buffer_path(&entry.path, &new_path);
                                }
                                ok_count += 1;
                            }
                            Err(e) => {
                                last_err = Some(format!("{}: {e}", entry.name));
                            }
                        }
                    }
                    self.filer.active_pane_mut().marked.clear();
                    self.filer.refresh_both();
                    if let Some(err) = last_err {
                        self.status_message =
                            format!("{op_name}: {ok_count}件完了 / エラー({err})");
                    } else {
                        self.status_message = format!("{ok_count}件を{op_name}しました");
                    }
                } else if do_cancel {
                    keep_modal = false;
                    self.status_message = format!("{op_name}をキャンセルしました");
                }
            }

            FilerModal::ConfirmDelete { targets } => {
                let mut do_exec = false;
                let mut do_cancel = false;

                egui::Window::new("削除確認 (F6)")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                    .frame(
                        egui::Frame::window(&ctx.style())
                            .fill(STATUS_BG)
                            .stroke(Stroke::new(1.5_f32, TEXT_AMBER)),
                    )
                    .show(ctx, |ui| {
                        ui.set_min_width(420.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "以下の {}件 を完全に削除します。よろしいですか？",
                                targets.len()
                            ))
                            .font(FontId::monospace(14.0))
                            .color(TEXT_AMBER),
                        );
                        ui.add_space(4.0);
                        for entry in targets.iter().take(6) {
                            let kind = if entry.is_dir { "[DIR] " } else { "" };
                            ui.label(
                                egui::RichText::new(format!("  - {kind}{}", entry.name))
                                    .font(FontId::monospace(13.0))
                                    .color(TEXT_WHITE),
                            );
                        }
                        if targets.len() > 6 {
                            ui.label(
                                egui::RichText::new(format!("  ... 他 {}件", targets.len() - 6))
                                    .font(FontId::monospace(12.5))
                                    .color(TEXT_DIM_CYAN),
                            );
                        }
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("削除実行 (Enter / Y)").clicked()
                                || ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Y))
                            {
                                do_exec = true;
                            }
                            if ui.button("キャンセル (ESC / N)").clicked()
                                || ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::N))
                            {
                                do_cancel = true;
                            }
                        });
                    });

                if do_exec {
                    keep_modal = false;
                    let mut ok_count = 0usize;
                    let mut last_err: Option<String> = None;
                    for entry in targets.iter() {
                        match delete_entry(&entry.path) {
                            Ok(()) => ok_count += 1,
                            Err(e) => last_err = Some(format!("{}: {e}", entry.name)),
                        }
                    }
                    self.filer.active_pane_mut().marked.clear();
                    self.filer.refresh_both();
                    if let Some(err) = last_err {
                        self.status_message = format!("削除: {ok_count}件完了 / エラー({err})");
                    } else {
                        self.status_message = format!("{ok_count}件を削除しました");
                    }
                } else if do_cancel {
                    keep_modal = false;
                    self.status_message = "削除をキャンセルしました".to_string();
                }
            }

            FilerModal::ConfirmCloseBuffer { buf_idx, name } => {
                let mut do_close = false;
                let mut do_cancel = false;
                let target_idx = *buf_idx;
                egui::Window::new("バッファを閉じる確認")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                    .frame(
                        egui::Frame::window(&ctx.style())
                            .fill(STATUS_BG)
                            .stroke(Stroke::new(1.5_f32, TEXT_AMBER)),
                    )
                    .show(ctx, |ui| {
                        ui.set_min_width(380.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "「{name}」には未保存の変更があります。保存せず閉じますか？"
                            ))
                            .font(FontId::monospace(13.5))
                            .color(TEXT_AMBER),
                        );
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("保存せず閉じる (Enter / Y)").clicked()
                                || ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Y))
                            {
                                do_close = true;
                            }
                            if ui.button("キャンセル (ESC / N)").clicked()
                                || ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::N))
                            {
                                do_cancel = true;
                            }
                        });
                    });

                if do_close {
                    keep_modal = false;
                    self.force_close_buffer(target_idx);
                } else if do_cancel {
                    keep_modal = false;
                    self.status_message = "バッファを閉じるのをキャンセルしました".to_string();
                }
            }

            FilerModal::ConfirmQuit => {
                let mut do_save_quit = false;
                let mut do_quit = false;
                let mut do_cancel = false;
                let dirty_names: Vec<String> = self
                    .buffers
                    .iter()
                    .filter(|s| s.buffer.dirty)
                    .map(|s| s.buffer.display_name())
                    .collect();
                egui::Window::new("終了確認 - 未保存の変更")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                    .frame(
                        egui::Frame::window(&ctx.style())
                            .fill(STATUS_BG)
                            .stroke(Stroke::new(1.5_f32, TEXT_AMBER)),
                    )
                    .show(ctx, |ui| {
                        ui.set_min_width(380.0);
                        ui.label(
                            egui::RichText::new("次のバッファに未保存の変更があります。保存しますか？")
                                .font(FontId::monospace(13.5))
                                .color(TEXT_AMBER),
                        );
                        ui.add_space(6.0);
                        for name in &dirty_names {
                            ui.label(
                                egui::RichText::new(format!("  * {name}"))
                                    .font(FontId::monospace(13.0))
                                    .color(TEXT_WHITE),
                            );
                        }
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("保存して終了 (Enter / Y)").clicked()
                                || ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Y))
                            {
                                do_save_quit = true;
                            }
                            if ui.button("保存せず終了 (N)").clicked()
                                || ui.input(|i| i.key_pressed(Key::N))
                            {
                                do_quit = true;
                            }
                            if ui.button("キャンセル (ESC)").clicked()
                                || ui.input(|i| i.key_pressed(Key::Escape))
                            {
                                do_cancel = true;
                            }
                        });
                    });

                if do_save_quit {
                    keep_modal = false;
                    self.quit_after_save = true;
                    self.continue_save_and_quit(ctx);
                } else if do_quit {
                    keep_modal = false;
                    self.close_now(ctx);
                } else if do_cancel {
                    keep_modal = false;
                    self.status_message = "終了をキャンセルしました".to_string();
                }
            }
        }

        if keep_modal {
            self.filer_modal = Some(modal);
        }
    }

    fn render_inline_path_prompt(&mut self, ctx: &egui::Context) {
        let mut submit_path: Option<(bool, PathBuf)> = None;
        let mut cancel = false;

        if let Some(prompt) = &mut self.inline_prompt {
            let title = if prompt.is_save {
                "名前を付けて保存 (ファイルパスを入力)"
            } else {
                "ファイルを開く (ファイルパスを入力)"
            };
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .frame(
                    egui::Frame::window(&ctx.style())
                        .fill(STATUS_BG)
                        .stroke(Stroke::new(1.5_f32, TEXT_CYAN)),
                )
                .show(ctx, |ui| {
                    ui.set_min_width(440.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut prompt.path_input)
                            .font(FontId::monospace(14.0))
                            .desired_width(420.0),
                    );
                    if prompt.just_opened {
                        resp.request_focus();
                        prompt.just_opened = false;
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("実行 (Enter)").clicked()
                            || (resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)))
                        {
                            let trimmed = prompt.path_input.trim();
                            if !trimmed.is_empty() {
                                submit_path = Some((prompt.is_save, PathBuf::from(trimmed)));
                            }
                        }
                        if ui.button("キャンセル (ESC)").clicked()
                            || ui.input(|i| i.key_pressed(Key::Escape))
                        {
                            cancel = true;
                        }
                    });
                });
        }

        if let Some((is_save, path)) = submit_path {
            self.inline_prompt = None;
            if is_save {
                self.perform_save(path);
            } else {
                self.open_file_in_buffer(path);
            }
        } else if cancel {
            self.inline_prompt = None;
            self.quit_after_save = false;
            self.status_message = "キャンセルしました".to_string();
        }
    }
}

/// ウィンドウスタイル用ショートカットの判定。該当しなければ None。
fn window_style_key_action(
    key: Key,
    physical_key: Option<Key>,
    modifiers: egui::Modifiers,
) -> Option<MenuAction> {
    if !modifiers.alt || modifiers.ctrl || modifiers.command {
        return None;
    }
    if modifiers.shift {
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
        Key::ArrowUp => Some(MenuAction::OpacityUp),
        Key::ArrowDown => Some(MenuAction::OpacityDown),
        Key::Num0 => Some(MenuAction::OpacityReset),
        _ => None,
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn read_system_clipboard() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// Attempt to read UTF-8 text from the Linux system clipboard (Wayland `wl-paste`, X11 `xclip`/`xsel`).
#[cfg(not(any(windows, target_os = "macos")))]
fn read_system_clipboard() -> Option<String> {
    use std::process::Command;

    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if let Ok(out) = Command::new("wl-paste").arg("--no-newline").output() {
            if out.status.success() {
                if let Ok(s) = String::from_utf8(out.stdout) {
                    return Some(s);
                }
            }
        }
    }

    for (bin, args) in [
        ("xclip", &["-o", "-selection", "clipboard"][..]),
        ("xsel", &["--clipboard", "--output"][..]),
    ] {
        if let Ok(out) = Command::new(bin).args(args).output() {
            if out.status.success() {
                if let Ok(s) = String::from_utf8(out.stdout) {
                    return Some(s);
                }
            }
        }
    }

    None
}

fn truncate_path_left(s: &str, max_cols: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let total_w: usize = s.chars().map(|c| c.width().unwrap_or(1)).sum();
    if total_w <= max_cols {
        return s.to_string();
    }
    let budget = max_cols.saturating_sub(3);
    let mut kept: Vec<char> = Vec::new();
    let mut acc = 0usize;
    for c in s.chars().rev() {
        let cw = c.width().unwrap_or(1);
        if acc + cw > budget {
            break;
        }
        acc += cw;
        kept.push(c);
    }
    kept.reverse();
    let tail: String = kept.into_iter().collect();
    format!("...{tail}")
}

fn truncate_display_right(s: &str, max_cols: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let total_w: usize = s.chars().map(|c| c.width().unwrap_or(1)).sum();
    if total_w <= max_cols {
        return s.to_string();
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
    out
}

impl eframe::App for VzEditorApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // 背面透過: ウィンドウ自体は透明にし、背景色のアルファで濃さを決める
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        window_style::set_opacity_percent(self.bg_opacity);
        window_style::set_text_opacity_percent(self.text_opacity);
        self.handle_close_request(ctx);
        self.handle_dropped_files(ctx);
        self.poll_dialog();
        self.poll_send(ctx);
        self.continue_save_and_quit(ctx);
        self.handle_events(ctx);

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
        }

        self.render_help_overlay(ctx);
        self.render_about_dialog(ctx);
        self.render_filer_modal(ctx);
        self.render_inline_path_prompt(ctx);
        self.render_drop_overlay(ctx);

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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_app() -> VzEditorApp {
        let mut buf = TextBuffer::new();
        buf.file_path = Some(PathBuf::from("notes.md"));
        buf.insert_text("# Title\n\n**Bold** and `code`\n");
        VzEditorApp {
            mode: AppMode::Editor,
            buffers: vec![BufferSlot::new(buf)],
            active_buf: 0,
            filer: FilerState::new(std::env::temp_dir()),
            prefix: PrefixKey::None,
            status_message: String::new(),
            ime_preedit: String::new(),
            ime_active: false,
            last_window_title: String::new(),
            internal_clipboard: String::new(),
            font_size: DEFAULT_FONT_SIZE,
            line_height: calc_line_height(DEFAULT_FONT_SIZE),
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
            bg_opacity: 100,
            text_opacity: 100,
            always_on_top: false,
            persist_settings: false,
            quit_after_save: false,
            allow_close: false,
            send_prefix: false,
            pending_send: None,
            key_scheme: KeyScheme::Vz,
            select_mode: false,
            clipboard_out: None,
        }
    }

    #[test]
    fn test_font_size_scaling_clamp_and_reset() {
        let mut app = make_test_app();
        assert_eq!(app.font_size, 16.0);
        assert_eq!(app.line_height, 23.0);

        app.zoom_in_font();
        assert_eq!(app.font_size, 18.0);
        assert!(app.line_height > 23.0);
        assert!(app.need_scroll_to_cursor);
        assert!(app.filer.left.need_scroll_to_cursor);
        assert!(app.filer.right.need_scroll_to_cursor);

        app.set_font_size(100.0);
        assert_eq!(app.font_size, MAX_FONT_SIZE);
        assert!(app.line_height >= MAX_FONT_SIZE + 4.0);

        app.set_font_size(2.0);
        assert_eq!(app.font_size, MIN_FONT_SIZE);
        assert!(app.line_height >= MIN_FONT_SIZE + 4.0);

        app.reset_font_size();
        assert_eq!(app.font_size, DEFAULT_FONT_SIZE);
        assert_eq!(app.line_height, calc_line_height(DEFAULT_FONT_SIZE));
    }

    #[test]
    fn test_font_zoom_keys() {
        let mut app = make_test_app();
        let ctrl = egui::Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let ctrl_shift = egui::Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };

        assert!(app.handle_font_zoom_key(Key::Plus, ctrl));
        assert_eq!(app.font_size, 18.0);
        assert!(app.handle_font_zoom_key(Key::Equals, ctrl));
        assert_eq!(app.font_size, 20.0);
        assert!(app.handle_font_zoom_key(Key::Semicolon, ctrl_shift));
        assert_eq!(app.font_size, 22.0);
        assert!(app.handle_font_zoom_key(Key::Minus, ctrl));
        assert_eq!(app.font_size, 20.0);
        assert!(app.handle_font_zoom_key(Key::Num0, ctrl));
        assert_eq!(app.font_size, DEFAULT_FONT_SIZE);
    }

    #[test]
    fn test_markdown_view_toggle_and_cache() {
        let mut app = make_test_app();
        assert!(!app.is_markdown_view());

        app.toggle_markdown_view();
        assert!(app.is_markdown_view());
        assert!(app.cur_slot().md_scroll_dirty);

        app.cur_slot_mut().ensure_md_cache();
        assert_eq!(app.cur_slot().md_blocks.len(), 2);

        // Scroll clamping
        app.cur_slot_mut().md_max_scroll_y = 200.0;
        app.scroll_markdown_by(80.0);
        assert_eq!(app.cur_slot().md_scroll_y, 80.0);
        app.scroll_markdown_to_bottom();
        assert_eq!(app.cur_slot().md_scroll_y, 200.0);
        app.scroll_markdown_to_top();
        assert_eq!(app.cur_slot().md_scroll_y, 0.0);

        // Toggle back to Edit
        app.toggle_markdown_view();
        assert!(!app.is_markdown_view());
        assert!(app.need_scroll_to_cursor);
    }

    #[test]
    fn test_dynamic_window_title_formats() {
        let mut app = make_test_app();
        // Dirty named file
        assert_eq!(app.compute_window_title(), "*notes.md - VZ Editor");

        // Clean named file
        app.cur_buf_mut().dirty = false;
        assert_eq!(app.compute_window_title(), "notes.md - VZ Editor");

        // Untitled new buffer
        app.new_empty_buffer();
        assert_eq!(app.compute_window_title(), "[無題] - VZ Editor");

        // Dirty untitled buffer
        app.cur_buf_mut().insert_text("draft");
        assert_eq!(app.compute_window_title(), "*[無題] - VZ Editor");

        // Buffer switch updates title immediately
        app.switch_buffer_step(1);
        assert_eq!(app.compute_window_title(), "notes.md - VZ Editor");

        // 2-pane filer title reflects active pane directory
        app.enter_filer();
        let expected_dir = app.filer.active_pane().current_dir.display().to_string();
        assert_eq!(
            app.compute_window_title(),
            format!("[2画面ファイラー] {expected_dir} - VZ Editor")
        );
    }

    #[test]
    fn test_menu_navigation_and_vz_keybinding_protection() {
        let mut app = make_test_app();
        let ctx = egui::Context::default();

        // 1. When menu is closed, VZ Ctrl+E / Ctrl+X and ESC must work normally
        app.cur_buf_mut().set_cursor(1, 0);
        let raw_ctrl_e = egui::RawInput {
            events: vec![Event::Key {
                key: Key::E,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        };
        let _ = ctx.run(raw_ctrl_e, |ctx| app.handle_events(ctx));
        assert_eq!(app.cur_buf().cursor_line, 0);

        // 2. When menu is open, ESC closes the menu WITHOUT switching to Filer!
        app.open_top_menu(TopMenu::File);
        assert_eq!(app.open_menu, Some(TopMenu::File));
        assert_eq!(app.mode, AppMode::Editor);

        // ArrowRight moves to Edit menu
        let raw_right = egui::RawInput {
            events: vec![Event::Key {
                key: Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(raw_right, |ctx| app.handle_events(ctx));
        assert_eq!(app.open_menu, Some(TopMenu::Edit));

        // ESC closes menu and stays in Editor mode
        let raw_esc = egui::RawInput {
            events: vec![Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(raw_esc, |ctx| app.handle_events(ctx));
        assert_eq!(app.open_menu, None);
        assert_eq!(app.mode, AppMode::Editor);

        // 3. Now that menu is closed, ESC switches immediately to 2-pane Filer!
        let raw_esc2 = egui::RawInput {
            events: vec![Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(raw_esc2, |ctx| app.handle_events(ctx));
        assert_eq!(app.mode, AppMode::Filer);
    }

    #[test]
    fn test_close_buffer_clean_and_dirty() {
        let mut app = make_test_app();
        // Dirty buffer prompts for confirmation
        app.close_current_buffer();
        assert!(matches!(
            app.filer_modal,
            Some(FilerModal::ConfirmCloseBuffer { .. })
        ));
        app.filer_modal = None;

        // Clean buffer closes immediately; last buffer resets to fresh [無題]
        app.cur_buf_mut().dirty = false;
        app.close_current_buffer();
        assert_eq!(app.buffers.len(), 1);
        assert_eq!(app.cur_buf().display_name(), "[無題]");
        assert!(!app.cur_buf().dirty);
    }

    #[test]
    fn test_ctrl_f4_closes_buffer_in_both_schemes() {
        for sakura in [false, true] {
            let ctx = egui::Context::default();
            let mut app = if sakura { make_sakura_app("a") } else { make_test_app() };
            let mut second = TextBuffer::new();
            second.insert_text("b");
            second.dirty = false;
            app.buffers.push(BufferSlot::new(second));
            app.active_buf = 1;
            let ctrl = mods(true, false);
            run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::F4, ctrl)]);
            assert_eq!(app.buffers.len(), 1, "sakura={sakura}");
            assert!(app.filer_modal.is_none());

            // 未保存なら確認ダイアログを出す
            app.cur_buf_mut().dirty = true;
            run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::F4, ctrl)]);
            assert!(matches!(
                app.filer_modal,
                Some(FilerModal::ConfirmCloseBuffer { .. })
            ));
        }
    }

    #[test]
    fn test_help_and_about_close_on_click() {
        let ctx = egui::Context::default();
        let mut app = make_test_app();
        let click = |pos: Pos2| -> egui::RawInput {
            let ev = |pressed| Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 680.0))),
                events: vec![Event::PointerMoved(pos), ev(true), ev(false)],
                ..Default::default()
            }
        };
        let pos = Pos2::new(20.0, 20.0);
        for about in [false, true] {
            if about {
                app.show_about = true;
            } else {
                app.show_help = true;
            }
            let frame = |app: &mut VzEditorApp| {
                let _ = ctx.run(click(pos), |ctx| {
                    app.render_help_overlay(ctx);
                    app.render_about_dialog(ctx);
                });
            };
            // 開いたフレームのクリック（メニュー操作など）では閉じない
            frame(&mut app);
            assert!(app.show_help || app.show_about, "about={about}");
            // 次のクリックで閉じる
            frame(&mut app);
            assert!(!app.show_help && !app.show_about, "about={about}");
        }
    }

    #[test]
    fn test_headless_render_all_modes_and_font_sizes() {
        let mut app = make_test_app();
        // Load real README.md content into the buffer
        let readme = include_str!("../tests/fixtures/markdown_sample.md");
        app.cur_buf_mut().lines = readme.lines().map(String::from).collect();
        app.cur_buf_mut().revision += 1;
        app.ime_preedit = "にほんご".to_string();
        app.ime_active = true;

        let ctx = egui::Context::default();
        let raw_input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 680.0))),
            ..Default::default()
        };

        for &size in &[MIN_FONT_SIZE, DEFAULT_FONT_SIZE, MAX_FONT_SIZE] {
            app.set_font_size(size);

            // 1. Editor Edit Mode (including menu bar, active IME preedit & IME rect output)
            app.mode = AppMode::Editor;
            app.cur_slot_mut().markdown_view = false;
            app.open_menu = None;
            app.show_about = false;
            let out_edit = ctx.run(raw_input.clone(), |ctx| {
                app.render_menu_bar(ctx);
                app.render_status_bar(ctx);
                app.render_buffer_strip(ctx);
                app.render_fkey_bar(ctx);
                app.render_editor_canvas(ctx);
                app.sync_window_title(ctx);
            });
            let ime = out_edit
                .platform_output
                .ime
                .expect("IME output must be set in Edit mode");
            assert!(
                (ime.cursor_rect.height() - app.line_height).abs() < 0.01,
                "IME cursor rect height must track line_height at {size}pt"
            );

            // 2. Markdown View Mode (IME must be disabled, content scrollable)
            app.cur_slot_mut().markdown_view = true;
            app.cur_slot_mut().md_scroll_dirty = true;
            let out_md = ctx.run(raw_input.clone(), |ctx| {
                app.render_menu_bar(ctx);
                app.render_status_bar(ctx);
                app.render_buffer_strip(ctx);
                app.render_fkey_bar(ctx);
                app.render_markdown_canvas(ctx);
                app.sync_window_title(ctx);
            });
            assert!(
                out_md.platform_output.ime.is_none(),
                "IME should be disabled in Markdown View mode"
            );
            assert!(
                app.cur_slot().md_max_scroll_y > 0.0,
                "README.md should be scrollable in Markdown View mode at {size}pt"
            );

            // 3. 2-Pane Filer Mode
            app.mode = AppMode::Filer;
            let out_filer = ctx.run(raw_input.clone(), |ctx| {
                app.render_menu_bar(ctx);
                app.render_status_bar(ctx);
                app.render_fkey_bar(ctx);
                app.render_filer_canvas(ctx);
                app.sync_window_title(ctx);
            });
            assert!(out_filer.platform_output.ime.is_none());
        }

        // 4. Open each TopMenu dropdown + About dialog headlessly
        app.mode = AppMode::Editor;
        app.cur_slot_mut().markdown_view = false;
        for &menu in &TopMenu::ALL {
            app.open_top_menu(menu);
            let _ = ctx.run(raw_input.clone(), |ctx| {
                app.render_menu_bar(ctx);
                app.render_status_bar(ctx);
                app.render_fkey_bar(ctx);
                app.render_editor_canvas(ctx);
            });
        }
        app.open_menu = None;
        app.show_about = true;
        let _ = ctx.run(raw_input, |ctx| {
            app.render_menu_bar(ctx);
            app.render_status_bar(ctx);
            app.render_fkey_bar(ctx);
            app.render_editor_canvas(ctx);
            app.render_about_dialog(ctx);
        });
    }

    #[test]
    fn test_window_style_opacity_and_menu() {
        let mut app = make_test_app();
        assert_eq!(app.bg_opacity, 100);
        app.change_bg_opacity(-10);
        assert_eq!(app.bg_opacity, 90);
        for _ in 0..20 {
            app.change_bg_opacity(-10);
        }
        assert_eq!(app.bg_opacity, window_style::MIN_OPACITY);
        app.set_bg_opacity(window_style::MAX_OPACITY);
        assert_eq!(app.bg_opacity, 100);

        let has = |app: &VzEditorApp, label: &str| {
            app.build_menu_entries(TopMenu::Tools)
                .iter()
                .any(|e| matches!(e, MenuEntry::Item { label: l, .. } if *l == label))
        };
        assert!(has(&app, "最前面に固定"));
        app.always_on_top = true;
        assert!(has(&app, "最前面固定を解除"));
    }

    #[test]
    fn test_window_style_keys_do_not_overlap() {
        let m = |alt: bool, shift: bool, ctrl: bool| egui::Modifiers {
            alt,
            shift,
            ctrl,
            command: ctrl,
            ..Default::default()
        };
        let alt = m(true, false, false);
        let alt_shift = m(true, true, false);
        let ctrl = m(false, false, true);

        // Alt: 背景・最前面
        assert_eq!(
            window_style_key_action(Key::T, Some(Key::T), alt),
            Some(MenuAction::ToggleAlwaysOnTop)
        );
        assert_eq!(
            window_style_key_action(Key::ArrowUp, None, alt),
            Some(MenuAction::OpacityUp)
        );
        assert_eq!(
            window_style_key_action(Key::Num0, Some(Key::Num0), alt),
            Some(MenuAction::OpacityReset)
        );
        // Alt+P（Markdown 切替）と Alt+±は奪わない
        assert_eq!(window_style_key_action(Key::P, Some(Key::P), alt), None);
        assert_eq!(
            window_style_key_action(Key::Minus, Some(Key::Minus), alt),
            None
        );

        // Alt+Shift: 文字（US 配列 / JIS 配列）
        assert_eq!(
            window_style_key_action(Key::Plus, Some(Key::Equals), alt_shift),
            Some(MenuAction::TextOpacityUp)
        );
        assert_eq!(
            window_style_key_action(Key::Plus, Some(Key::Semicolon), alt_shift),
            Some(MenuAction::TextOpacityUp)
        );
        assert_eq!(
            window_style_key_action(Key::Minus, Some(Key::Minus), alt_shift),
            Some(MenuAction::TextOpacityDown)
        );
        // JIS: Shift+「-」は論理キー '=' になるが物理キーで「-」と判定
        assert_eq!(
            window_style_key_action(Key::Equals, Some(Key::Minus), alt_shift),
            Some(MenuAction::TextOpacityDown)
        );
        assert_eq!(
            window_style_key_action(Key::Num0, Some(Key::Num0), alt_shift),
            Some(MenuAction::TextOpacityReset)
        );
        // Alt+Shift+↑/T は背景・最前面に流れない
        assert_eq!(window_style_key_action(Key::ArrowUp, None, alt_shift), None);
        assert_eq!(
            window_style_key_action(Key::T, Some(Key::T), alt_shift),
            None
        );

        // Ctrl 系（VZ キー・フォントサイズ）は対象外、フォントズームは Alt 併用で反応しない
        assert_eq!(
            window_style_key_action(Key::Plus, Some(Key::Equals), ctrl),
            None
        );
        assert_eq!(
            window_style_key_action(Key::Num0, Some(Key::Num0), m(true, true, true)),
            None
        );
        let mut app = make_test_app();
        assert!(!app.handle_font_zoom_key(Key::Plus, alt_shift));
        assert!(!app.handle_font_zoom_key(Key::Minus, alt_shift));

        app.change_text_opacity(-10);
        assert_eq!(app.text_opacity, 90);
        app.set_text_opacity(0);
        assert_eq!(app.text_opacity, window_style::MIN_OPACITY);
    }

    #[test]
    fn test_quit_confirm_for_new_buffer_input() {
        // 新規バッファは入力前なら確認なし
        let mut app = make_test_app();
        app.buffers = vec![BufferSlot::new(TextBuffer::new())];
        assert!(!app.has_any_dirty_buffer());

        // 新規バッファに文字を入力すると終了確認が出る
        app.cur_buf_mut().insert_text("あ");
        assert!(app.has_any_dirty_buffer());
        app.open_quit_confirm();
        assert!(matches!(app.filer_modal, Some(FilerModal::ConfirmQuit)));

        // 無題のバッファは保存先を尋ねる段階になる
        assert_eq!(app.save_dirty_buffers_for_quit(), QuitSaveStep::NeedSaveAs(0));
    }

    #[test]
    fn test_save_dirty_buffers_for_quit_saves_files() {
        let dir = std::env::temp_dir().join(format!("vz_quit_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("edited.txt");
        std::fs::write(&path, "old
").unwrap();

        let mut app = make_test_app();
        let mut buf = TextBuffer::from_file(&path).unwrap();
        buf.insert_text("new ");
        app.buffers = vec![BufferSlot::new(buf)];
        assert!(app.has_any_dirty_buffer());

        assert_eq!(app.save_dirty_buffers_for_quit(), QuitSaveStep::Done);
        assert!(!app.has_any_dirty_buffer());
        assert!(std::fs::read_to_string(&path).unwrap().starts_with("new old"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_open_paths_multiple_files_and_dir() {
        let dir = std::env::temp_dir().join(format!("vz_open_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.txt");
        let b = dir.join("b.md");
        std::fs::write(&a, "aaa\n").unwrap();
        std::fs::write(&b, "# b\n").unwrap();

        let mut app = make_test_app();
        app.buffers = vec![BufferSlot::new(TextBuffer::new())];

        // 複数ファイル: 未編集の [無題] を置き換えて2つ開き、最初のファイルを表示
        app.open_paths(vec![a.clone(), b.clone(), dir.join("missing.txt")], false);
        assert_eq!(app.buffers.len(), 2);
        assert_eq!(app.active_buf, 0);
        assert_eq!(app.cur_buf().display_name(), "a.txt");
        assert!(app.status_message.contains("2 個のファイルを開きました"));
        assert!(app.status_message.contains("missing.txt"));

        // 同じファイルを再ドロップしても重複しない
        app.open_paths(vec![b.clone()], false);
        assert_eq!(app.buffers.len(), 2);
        assert_eq!(app.active_buf, 1);

        // 存在しないパスは create_missing で新規ファイル
        app.open_paths(vec![dir.join("new.txt")], true);
        assert_eq!(app.buffers.len(), 3);
        assert!(!app.cur_buf().dirty);

        // フォルダだけならファイラーで移動
        app.open_paths(vec![dir.clone()], false);
        assert_eq!(app.mode, AppMode::Filer);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_filer_enter_opens_marked_files() {
        let dir = std::env::temp_dir().join(format!("vz_mark_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["m1.txt", "m2.txt", "m3.txt"] {
            std::fs::write(dir.join(name), name).unwrap();
        }
        let mut app = make_test_app();
        app.buffers = vec![BufferSlot::new(TextBuffer::new())];
        app.filer = FilerState::new(dir.clone());
        {
            // ファイラーはフォルダを正規化するため、一覧の項目のパスでマークする
            let pane = app.filer.active_pane_mut();
            let targets: Vec<PathBuf> = pane
                .entries
                .iter()
                .filter(|e| e.name == "m1.txt" || e.name == "m3.txt")
                .map(|e| e.path.clone())
                .collect();
            assert_eq!(targets.len(), 2);
            pane.marked.extend(targets);
        }
        app.filer_open_selected();
        assert_eq!(app.buffers.len(), 2);
        assert_eq!(app.mode, AppMode::Editor);
        assert!(app.filer.active_pane().marked.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn key_event(key: Key, alt: bool) -> Event {
        Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                alt,
                ..Default::default()
            },
        }
    }

    #[test]
    fn test_send_prefix_keys() {
        let ctx = egui::Context::default();
        let mut app = make_test_app();
        // Alt+C は透過・最前面のキーと重ならない
        assert_eq!(
            window_style_key_action(Key::C, Some(Key::C), egui::Modifiers { alt: true, ..Default::default() }),
            None
        );

        // Alt+C → 2打目待ち。C / X 以外は取り消し、文字も入力されない
        let run = |app: &mut VzEditorApp, events: Vec<Event>| {
            let input = egui::RawInput { events, ..Default::default() };
            let _ = ctx.run(input, |ctx| app.handle_send_keys(ctx));
        };
        run(&mut app, vec![key_event(Key::C, true), Event::Text("c".into())]);
        assert!(app.send_prefix);
        run(&mut app, vec![key_event(Key::D, false), Event::Text("d".into())]);
        assert!(!app.send_prefix);
        assert_eq!(app.status_message, "送信を取り消しました");
        assert!(app.pending_send.is_none());

        // 空のバッファでは送らない（2打目は Alt を押したままでもよい）
        app.buffers = vec![BufferSlot::new(TextBuffer::new())];
        run(&mut app, vec![key_event(Key::C, true)]);
        run(&mut app, vec![key_event(Key::X, true)]);
        assert!(!app.send_prefix);
        assert!(app.pending_send.is_none());
        assert!(app.status_message.contains("Codex へ送る文字がありません"));
    }

    fn mods(ctrl: bool, shift: bool) -> egui::Modifiers {
        egui::Modifiers {
            ctrl,
            command: ctrl,
            shift,
            ..Default::default()
        }
    }

    fn key_with(key: Key, m: egui::Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: m,
        }
    }

    /// エディター画面の入力処理にイベントを流す（RawInput の modifiers も設定する）。
    fn run_editor(ctx: &egui::Context, app: &mut VzEditorApp, m: egui::Modifiers, events: Vec<Event>) {
        let input = egui::RawInput {
            events,
            modifiers: m,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.handle_editor_events(ctx));
    }

    fn make_sakura_app(text: &str) -> VzEditorApp {
        let mut app = make_test_app();
        app.key_scheme = KeyScheme::Sakura;
        let mut buf = TextBuffer::new();
        buf.insert_text(text);
        buf.set_cursor(0, 0);
        app.buffers = vec![BufferSlot::new(buf)];
        app
    }

    #[test]
    fn test_sakura_shift_select_and_replace() {
        let ctx = egui::Context::default();
        let mut app = make_sakura_app("abcdef\nghi");
        let shift = mods(false, true);
        run_editor(&ctx, &mut app, shift, vec![
            key_with(Key::ArrowRight, shift),
            key_with(Key::ArrowRight, shift),
            key_with(Key::ArrowRight, shift),
        ]);
        assert_eq!(app.cur_buf().selected_text().as_deref(), Some("abc"));

        // 文字入力で選択範囲を置き換える
        run_editor(&ctx, &mut app, mods(false, false), vec![Event::Text("X".into())]);
        assert_eq!(app.cur_buf().lines, vec!["Xdef", "ghi"]);

        // Shift なしの移動で選択は解除される
        run_editor(&ctx, &mut app, shift, vec![key_with(Key::End, shift)]);
        assert!(app.cur_buf().has_selection());
        run_editor(&ctx, &mut app, mods(false, false), vec![key_with(Key::ArrowLeft, mods(false, false))]);
        assert!(!app.cur_buf().has_selection());

        // Ctrl+Shift+End でファイル末尾まで選択
        let cs = mods(true, true);
        run_editor(&ctx, &mut app, cs, vec![key_with(Key::Home, mods(false, false)), key_with(Key::End, cs)]);
        assert_eq!(app.cur_buf().selected_text().as_deref(), Some("Xdef\nghi"));
    }

    #[test]
    fn test_sakura_f2_to_f5_same_as_vz() {
        let ctx = egui::Context::default();
        let mut app = make_sakura_app("abcdef\nghi");
        let none = mods(false, false);
        let shift = mods(false, true);

        // F5 / F4: 行末 / 行頭（選択なし）
        run_editor(&ctx, &mut app, none, vec![key_with(Key::F5, none)]);
        assert_eq!(app.cur_buf().cursor_pos(), (0, 6));
        assert!(!app.cur_buf().has_selection());
        run_editor(&ctx, &mut app, none, vec![key_with(Key::F4, none)]);
        assert_eq!(app.cur_buf().cursor_pos(), (0, 0));

        // Shift+F5 は範囲選択
        run_editor(&ctx, &mut app, shift, vec![key_with(Key::F5, shift)]);
        assert_eq!(app.cur_buf().selected_text().as_deref(), Some("abcdef"));
        run_editor(&ctx, &mut app, none, vec![key_with(Key::F4, none)]);
        assert!(!app.cur_buf().has_selection());

        // F2: 新規バッファ
        run_editor(&ctx, &mut app, none, vec![key_with(Key::F2, none)]);
        assert_eq!(app.buffers.len(), 2);
        assert_eq!(app.active_buf, 1);
    }

    #[test]
    fn test_sakura_select_all_cut_undo_and_escape() {
        let ctx = egui::Context::default();
        let mut app = make_sakura_app("one\ntwo");
        let ctrl = mods(true, false);
        run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::A, ctrl), Event::Cut]);
        assert_eq!(app.cur_buf().lines, vec![""]);
        assert_eq!(app.clipboard_out.as_deref(), Some("one\ntwo"));

        run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::Z, ctrl)]);
        assert_eq!(app.cur_buf().lines, vec!["one", "two"]);
        run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::Y, ctrl)]);
        assert_eq!(app.cur_buf().lines, vec![""]);
        run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::Z, ctrl)]);

        // 選択が無いときの Ctrl+C は現在行（改行付き）をコピー
        app.clipboard_out = None;
        app.cur_buf_mut().set_cursor(0, 1);
        run_editor(&ctx, &mut app, ctrl, vec![Event::Copy]);
        assert_eq!(app.clipboard_out.as_deref(), Some("one\n"));

        // Esc: 選択中は解除、選択が無ければファイラーへ
        let none = mods(false, false);
        run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::W, ctrl)]);
        assert!(app.cur_buf().has_selection());
        run_editor(&ctx, &mut app, none, vec![key_with(Key::Escape, none)]);
        assert!(!app.cur_buf().has_selection());
        assert_eq!(app.mode, AppMode::Editor);
        run_editor(&ctx, &mut app, none, vec![key_with(Key::Escape, none)]);
        assert_eq!(app.mode, AppMode::Filer);
    }

    #[test]
    fn test_sakura_line_and_word_cut_keys() {
        let ctx = egui::Context::default();
        let mut app = make_sakura_app("first line\nsecond\nthird");
        let ctrl = mods(true, false);
        let cs = mods(true, true);
        // Ctrl+E: 行切り取り（改行込み）
        run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::E, ctrl)]);
        assert_eq!(app.cur_buf().lines, vec!["second", "third"]);
        assert_eq!(app.clipboard_out.as_deref(), Some("first line\n"));
        // Ctrl+Shift+E: 行削除（クリップボードは変えない）
        app.clipboard_out = None;
        run_editor(&ctx, &mut app, cs, vec![key_with(Key::E, cs)]);
        assert_eq!(app.cur_buf().lines, vec!["third"]);
        assert!(app.clipboard_out.is_none());
        // Ctrl+K: 行末まで切り取り / Ctrl+U: 行頭まで
        app.cur_buf_mut().set_cursor(0, 2);
        run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::K, ctrl)]);
        assert_eq!(app.cur_buf().lines, vec!["th"]);
        run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::U, ctrl)]);
        assert_eq!(app.cur_buf().lines, vec![""]);
        // Ctrl+I: 行の二重化
        app.cur_buf_mut().insert_text("dup");
        run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::I, ctrl)]);
        assert_eq!(app.cur_buf().lines, vec!["dup", "dup"]);
        // VZ のキーは効かない（Ctrl+Y は Redo、Ctrl+G は未対応で文字も入らない）
        run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::G, ctrl)]);
        assert_eq!(app.cur_buf().lines, vec!["dup", "dup"]);
    }

    #[test]
    fn test_sakura_paste_markdown_toggle_and_scheme_switch() {
        let ctx = egui::Context::default();
        let mut app = make_test_app(); // notes.md
        app.key_scheme = KeyScheme::Sakura;
        // Ctrl+Shift+V は egui では貼り付けとして届く → Markdown 切替
        let cs = mods(true, true);
        run_editor(&ctx, &mut app, cs, vec![Event::Paste("ignored".into())]);
        assert!(app.cur_slot().markdown_view);
        run_editor(&ctx, &mut app, cs, vec![Event::Paste("ignored".into())]);
        assert!(!app.cur_slot().markdown_view);

        // Ctrl+V は選択範囲を置き換える
        let ctrl = mods(true, false);
        run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::A, ctrl), Event::Paste("new".into())]);
        assert_eq!(app.cur_buf().lines, vec!["new"]);

        // VZ 互換へ切り替えると選択は解除され、Ctrl+E は上移動に戻る
        app.cur_buf_mut().select_all();
        app.toggle_key_scheme();
        assert_eq!(app.key_scheme, KeyScheme::Vz);
        assert!(!app.cur_buf().has_selection());
    }
}
