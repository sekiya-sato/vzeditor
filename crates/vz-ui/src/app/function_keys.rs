//! ファンクションキーの割り当てとボタンのラベル。

use super::{egui, EditorAction, FilerAction, Key, VzEditorApp};

/// F キー欄に表示する修飾キーの段（Shift・Ctrl・Alt のどれか 1 つだけを押している間）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FKeyLayer {
    Normal,
    Shift,
    Ctrl,
    Alt,
}

impl FKeyLayer {
    /// 修飾キーから段を決める。2 つ以上の組み合わせ（Ctrl+Shift など）は通常の段。
    pub(super) fn from_modifiers(m: egui::Modifiers) -> Self {
        match (m.shift, m.ctrl || m.command, m.alt) {
            (true, false, false) => Self::Shift,
            (false, true, false) => Self::Ctrl,
            (false, false, true) => Self::Alt,
            _ => Self::Normal,
        }
    }
}

impl VzEditorApp {
    /// エディターの F キー欄（F1〜F12 の順のラベルと動作）。`layer` は押している修飾キー。
    pub(super) fn editor_fkeys(
        sakura: bool,
        in_md_view: bool,
        layer: FKeyLayer,
    ) -> [(&'static str, EditorAction); 12] {
        use EditorAction::*;
        const NONE: (&str, EditorAction) = ("", Nop);
        if sakura {
            // サクラ互換の割り当て。未対応のキーは空欄。
            let mut keys = [NONE; 12];
            match layer {
                FKeyLayer::Normal => {
                    keys[..9].copy_from_slice(&[
                        ("ヘルプ", ToggleHelp),
                        ("新規", NewBuffer),
                        ("次検索", RepeatSearch(false)),
                        ("分割", ToggleSplit),
                        NONE,
                        ("選択", BeginSelect),
                        ("切取", Cut),
                        ("コピー", Copy),
                        ("貼付", Paste),
                    ]);
                    keys[10] = ("特殊", CopySpecialLine);
                }
                FKeyLayer::Shift => {
                    keys[2] = ("前検索", RepeatSearch(true));
                    keys[7] = ("コピー", Copy);
                    keys[9] = ("行複製", DuplicateLine);
                }
                FKeyLayer::Ctrl => {
                    keys[2] = ("強調切替", ToggleSearchHighlight);
                    keys[3] = ("閉じる", CloseBuffer);
                }
                FKeyLayer::Alt => keys[3] = ("終了", Quit),
            }
            return keys;
        }
        let (f5, f7, f8) = if in_md_view {
            ("末尾", "半頁上", "半頁下")
        } else {
            ("行末", "単語左", "単語右")
        };
        let mut keys = [
            ("ヘルプ", ToggleHelp),
            ("新規", NewBuffer),
            ("次検索", RepeatSearch(false)),
            ("分割", ToggleSplit),
            (f5, MoveLineEnd),
            ("行削除", DeleteLine),
            (f7, MoveWordLeft),
            (f8, MoveWordRight),
            ("ファイラ", SwitchToFiler),
            ("保存", SaveFile),
            NONE,
            NONE,
        ];
        match layer {
            FKeyLayer::Shift => {
                keys[2] = ("前検索", RepeatSearch(true));
                // F4 は修飾キーなしのときだけ分割する
                keys[3] = NONE;
            }
            FKeyLayer::Ctrl => {
                keys[2] = ("強調切替", ToggleSearchHighlight);
                keys[3] = ("閉じる", CloseBuffer);
            }
            FKeyLayer::Alt => {
                keys[2] = NONE;
                keys[3] = ("終了", Quit);
            }
            FKeyLayer::Normal => keys[10] = ("特殊", CopySpecialLine),
        }
        keys
    }

    /// ファイラーの F キー欄。修飾キーは区別しない（OS の Alt+F4 の終了を除く）。
    pub(super) fn filer_fkeys(layer: FKeyLayer) -> [(&'static str, FilerAction); 12] {
        use FilerAction::*;
        let mut keys = [
            ("ヘルプ", ToggleHelp),
            ("名前変", Rename),
            ("新規作成", NewFile),
            ("コピー", Copy),
            ("移動", Move),
            ("削除", Delete),
            ("フォルダ", NewDir),
            ("ソート", CycleSort),
            ("編集戻", BackToEditor),
            ("終了", Quit),
            ("", Nop),
            ("", Nop),
        ];
        if layer == FKeyLayer::Alt {
            keys[3] = ("終了", Quit);
        }
        keys
    }

    pub(super) fn editor_fkey_action(
        key: Key,
        sakura: bool,
        in_md_view: bool,
        layer: FKeyLayer,
    ) -> Option<EditorAction> {
        let idx = fkey_index(key)?;
        let action = Self::editor_fkeys(sakura, in_md_view, layer)[idx].1;
        (action != EditorAction::Nop).then_some(action)
    }

    pub(super) fn filer_fkey_action(key: Key) -> Option<FilerAction> {
        let idx = fkey_index(key)?;
        let action = Self::filer_fkeys(FKeyLayer::Normal)[idx].1;
        (action != FilerAction::Nop).then_some(action)
    }
}

fn fkey_index(key: Key) -> Option<usize> {
    const KEYS: [Key; 12] = [
        Key::F1,
        Key::F2,
        Key::F3,
        Key::F4,
        Key::F5,
        Key::F6,
        Key::F7,
        Key::F8,
        Key::F9,
        Key::F10,
        Key::F11,
        Key::F12,
    ];
    KEYS.iter().position(|&function_key| function_key == key)
}
