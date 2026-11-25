//! 配色の定数（PC-98 / VZ 風）。

use eframe::egui::Color32;

// --- PC-98 / VZ Editor Color Palette ---
pub(super) const BG_NAVY: Color32 = Color32::from_rgb(8, 24, 56); // #081838
pub(super) const BG_ACTIVE_LINE: Color32 = Color32::from_rgb(14, 38, 84);
pub(super) const BG_GUTTER: Color32 = Color32::from_rgb(6, 19, 46);
pub(super) const BG_TAB_BAR: Color32 = Color32::from_rgb(9, 28, 66);
pub(super) const BG_TAB_ACTIVE: Color32 = Color32::from_rgb(24, 66, 134);
pub(super) const BG_TAB_INACTIVE: Color32 = Color32::from_rgb(12, 34, 78);

pub(super) const BORDER_BLUE: Color32 = Color32::from_rgb(42, 98, 168);
pub(super) const BORDER_ACTIVE_PANE: Color32 = Color32::from_rgb(90, 212, 255);
pub(super) const GUTTER_SEP: Color32 = Color32::from_rgb(26, 62, 118);

pub(super) const STATUS_BG: Color32 = Color32::from_rgb(15, 42, 92);
pub(super) const STATUS_BADGE_BG: Color32 = Color32::from_rgb(64, 210, 255);
pub(super) const STATUS_BADGE_FILER_BG: Color32 = Color32::from_rgb(255, 206, 86);
pub(super) const STATUS_BADGE_VIEW_BG: Color32 = Color32::from_rgb(56, 204, 152);
pub(super) const STATUS_BADGE_FG: Color32 = Color32::from_rgb(6, 20, 48);

pub(super) const FKEY_BAR_BG: Color32 = Color32::from_rgb(10, 28, 64);
pub(super) const FKEY_BTN_BG: Color32 = Color32::from_rgb(18, 48, 102);
pub(super) const FKEY_BTN_HOVER: Color32 = Color32::from_rgb(30, 74, 148);
pub(super) const FKEY_NUM_BG: Color32 = Color32::from_rgb(46, 140, 208);
pub(super) const FKEY_NUM_FILER_BG: Color32 = Color32::from_rgb(36, 138, 168);
pub(super) const FKEY_NUM_FG: Color32 = Color32::from_rgb(240, 250, 255);

pub(super) const FILER_HDR_ACTIVE: Color32 = Color32::from_rgb(18, 56, 114);
pub(super) const FILER_HDR_INACTIVE: Color32 = Color32::from_rgb(11, 32, 72);
pub(super) const FILER_COL_HDR: Color32 = Color32::from_rgb(9, 26, 60);
pub(super) const FILER_ROW_CURSOR_ACTIVE: Color32 = Color32::from_rgb(28, 82, 156);
pub(super) const FILER_ROW_CURSOR_INACTIVE: Color32 = Color32::from_rgb(16, 44, 92);
pub(super) const FILER_ROW_MARKED: Color32 = Color32::from_rgb(62, 54, 18);
pub(super) const FILER_ROW_MARKED_CURSOR: Color32 = Color32::from_rgb(88, 76, 24);

pub(super) const TEXT_WHITE: Color32 = Color32::from_rgb(234, 244, 255);
pub(super) const TEXT_CYAN: Color32 = Color32::from_rgb(112, 232, 255);
pub(super) const TEXT_DIM_CYAN: Color32 = Color32::from_rgb(88, 146, 204);
pub(super) const TEXT_CR_MARK: Color32 = Color32::from_rgb(56, 110, 168);
pub(super) const TEXT_EOF: Color32 = Color32::from_rgb(76, 178, 224);
pub(super) const TEXT_AMBER: Color32 = Color32::from_rgb(255, 220, 96);

pub(super) const CURSOR_FILL: Color32 = Color32::from_rgba_premultiplied(80, 220, 255, 110);
pub(super) const CURSOR_BAR: Color32 = Color32::from_rgb(120, 244, 255);
/// 範囲選択の背景色
pub(super) const SELECTION_BG: Color32 = Color32::from_rgb(38, 108, 190);
pub(super) const PREEDIT_BG: Color32 = Color32::from_rgb(28, 72, 134);
pub(super) const PREEDIT_FG: Color32 = Color32::from_rgb(255, 246, 160);
pub(super) const PREEDIT_UNDERLINE: Color32 = Color32::from_rgb(112, 236, 255);

pub(super) const MENU_BAR_BG: Color32 = Color32::from_rgb(7, 21, 50);
pub(super) const MENU_BAR_HOVER: Color32 = Color32::from_rgb(20, 56, 114);
pub(super) const MENU_POPUP_BG: Color32 = Color32::from_rgb(10, 30, 70);
pub(super) const MENU_ITEM_HOVER: Color32 = Color32::from_rgb(26, 74, 146);
pub(super) const MENU_TEXT_DISABLED: Color32 = Color32::from_rgb(74, 108, 152);
