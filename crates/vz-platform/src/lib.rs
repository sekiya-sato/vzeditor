//! OS 固有処理。OS ごとの違いはこのクレートの中で `#[cfg]` により吸収する。egui を使わない。

pub mod clipboard;
pub mod dialog;
pub mod file_info;
pub mod keyboard;
pub mod monitor;
pub mod send_to_app;
pub mod shell;
pub mod tab_drop;
pub mod virtual_desktop;
