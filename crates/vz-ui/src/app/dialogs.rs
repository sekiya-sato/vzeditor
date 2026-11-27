//! ヘルプ、About、ファイラーの確認画面、パス入力欄。

use super::*;

impl VzEditorApp {
    pub(super) fn render_help_overlay(&mut self, ctx: &egui::Context) {
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
            AppMode::Terminal => "VZ Terminal - ターミナル操作 (クリックで閉じる)",
            AppMode::Browser => "VZ Browser - 簡易ブラウザ操作 (クリックで閉じる)",
        };
        modal_window(ctx, title, TEXT_CYAN)
            .open(&mut open)
            .show(ctx, |ui| {
                let rows: &[(&str, &str)] = match self.mode {
                    AppMode::Editor if self.key_scheme == KeyScheme::Sakura => &[
                        ("Ctrl+N・F2 / Ctrl+O", "新規作成 / 開く"),
                        ("Ctrl+S / Ctrl+Shift+S", "上書き保存 / 名前を付けて保存"),
                        ("Ctrl+F4", "ファイル（現在のバッファ）を閉じる"),
                        ("Ins", "挿入 / 上書きの切替 (上書き時はカーソルが下半分)"),
                        ("Enter / Shift+Enter", "改行 (行頭の空白を引き継ぐ) / インデントなしで改行"),
                        ("Ctrl+Z / Ctrl+Y", "元に戻す / やり直し"),
                        ("Ctrl+X / Ctrl+C / Ctrl+V", "切り取り / コピー / 貼り付け (F7 / F8 / F9)"),
                        ("Shift+移動キー / F6", "範囲選択 / 範囲選択開始"),
                        ("Ctrl+A / Ctrl+W", "すべて選択 / 単語選択 (ダブル・トリプルクリック)"),
                        ("Ctrl+← → / Ctrl+Home End", "単語移動 / ファイル先頭・末尾"),
                        ("Ctrl+E / Ctrl+Shift+E", "行切り取り / 行削除"),
                        ("Ctrl+K / Ctrl+U / Ctrl+D", "行末まで / 行頭まで / 単語を切り取り (Shift で削除)"),
                        ("Ctrl+BkSp / Ctrl+Del / Ctrl+I", "単語の左・右を削除 / 行の二重化"),
                        ("Ctrl+F / R / G / J", "検索 / 置換 / Grep / 行ジャンプ"),
                        ("F3 / Shift+F3", "次を検索 / 前を検索 (Alt+N / Alt+Shift+N も可)"),
                        ("Ctrl+F3", "検索ハイライトのオン / オフ"),
                        ("Ctrl+Tab / Ctrl+Shift+Tab", "バッファ切替"),
                        ("Ctrl+Shift+V / Ctrl+P", "Markdown 編集 / ビュー切替"),
                        ("Ctrl++ / Ctrl+- / Ctrl+0", "フォントサイズ (Ctrl+ホイールも可)"),
                        ("Esc", "選択解除 / 2画面ファイラーへ切替"),
                        ("Alt+T / Alt+↑↓0", "最前面固定 / 背景の濃さ"),
                        ("Alt+W", "長い行の折り返し表示の切替"),
                        ("Alt+Shift+「+」「-」「0」", "文字の濃さ"),
                        ("Alt+Shift+F4", "状態を記録して終了 (Shift を押して起動で復元)"),
                        ("Alt+C → C / Alt+C → X", "バッファ全体を Claude Code / Codex へ送信"),
                        ("Alt+C → N / Alt+C → M", "Claude Code / Codex で新規セッション (Ctrl+N)"),
                        ("Alt+C → P", "GitHub Desktop のカレントリポジトリを Push"),
                        ("Alt+K / Alt+B", "ターミナル / ブラウザ (編集中の HTML を表示) の切替"),
                        ("ファイル→文字コードを指定して保存", "UTF-8 / UTF-8 BOM / Shift_JIS で保存"),
                        ("128MB 以上のファイル", "巨大ファイルモード (Markdown ビュー・行マークなし)"),
                        ("Esc (巨大ファイルの検索・置換中)", "検索・すべて置換を中止"),
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
                        ("Enter / Ctrl+M / Shift+Enter", "改行 (行頭の空白を引き継ぐ) / インデントなしで改行"),
                        ("Ins", "挿入 / 上書きの切替 (上書き時はカーソルが下半分)"),
                        ("Alt+F / R / G / J", "検索 / 置換 / Grep / 行ジャンプ"),
                        ("F3 / Shift+F3", "次を検索 / 前を検索 (Alt+N / Alt+Shift+N も可)"),
                        ("Ctrl+F3", "検索ハイライトのオン / オフ"),
                        ("Ctrl+W / Ctrl+Shift+W", "複数バッファ（裏画面）切替"),
                        ("Ctrl+P / Ctrl+K+V / Alt+P", "Markdown 編集 / ビュー（プレビュー）切替"),
                        ("Ctrl++ / Ctrl+- / Ctrl+0", "フォントサイズ拡大 / 縮小 / リセット"),
                        ("Ctrl+K+D / F10", "上書き保存 / 保存ダイアログ"),
                        ("Ctrl+F4", "ファイル（現在のバッファ）を閉じる"),
                        ("ESC / Ctrl+K+Q / F9", "2画面ファイラーへ切替"),
                        ("Alt+T", "最前面固定の切替"),
                        ("Alt+W", "長い行の折り返し表示の切替"),
                        ("ファイルをドロップ", "複数ファイルをまとめてバッファで開く"),
                        ("Alt+C → C / Alt+C → X", "バッファ全体を Claude Code / Codex へ送信"),
                        ("Alt+C → N / Alt+C → M", "Claude Code / Codex で新規セッション (Ctrl+N)"),
                        ("Alt+C → P", "GitHub Desktop のカレントリポジトリを Push"),
                        ("Alt+K / Alt+B", "ターミナル / ブラウザ (編集中の HTML を表示) の切替"),
                        ("Alt+↑ / Alt+↓ / Alt+0", "背景を濃く / 薄く / 透過解除"),
                        ("Alt+Shift+「+」「-」「0」", "文字を濃く / 薄く / 透過解除"),
                        ("Alt+Shift+F4", "状態を記録して終了 (Shift を押して起動で復元)"),
                        ("ファイル→文字コードを指定して保存", "UTF-8 / UTF-8 BOM / Shift_JIS で保存"),
                        ("128MB 以上のファイル", "巨大ファイルモード (Markdown ビュー・行マークなし)"),
                        ("Esc (巨大ファイルの検索・置換中)", "検索・すべて置換を中止"),
                    ],
                    AppMode::Filer => &[
                        ("Ctrl+E / X (↑ / ↓)", "ファイルカーソル移動"),
                        ("Tab / Ctrl+S / D (← / →)", "左右ペイン（アクティブ窓）切替"),
                        ("Enter / ダブルクリック", "フォルダ移動・ファイルを開く (マーク時はまとめて開く)"),
                        ("Shift+Enter", "カーソル位置のファイルを OS の関連付けで実行 (フォルダは OS で開く)"),
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
                        ("Alt+Shift+F4", "状態を記録して終了 (Shift を押して起動で復元)"),
                        ("Alt+K / Alt+B", "ターミナル / ブラウザの切替"),
                    ],
                    AppMode::Terminal => &[
                        ("Alt+K", "元の画面 (エディター / ファイラー) へ戻る"),
                        ("Alt+B", "ブラウザへ切替"),
                        ("その他のキー", "シェルへ送る (Ctrl+C は中断、F キーも送る)"),
                        ("Ctrl+V / Shift+Ins", "貼り付け"),
                        ("マウスでドラッグ", "離した時点で選択範囲をコピー"),
                        ("Ctrl+Shift+C", "選択範囲（無ければ画面）の文字をコピー"),
                        ("ホイール", "履歴 (スクロールバック) を表示。キー入力で最新へ"),
                        ("exit", "シェルを終了するとターミナルを閉じる (Alt+K で新しく起動)"),
                        ("Alt+T / Alt+↑↓0 / Alt+W", "透過・最前面などは共通 (シェルへは送らない)"),
                    ],
                    AppMode::Browser => &[
                        ("Alt+B", "元の画面へ戻る (エディターで HTML 編集中なら Alt+B で表示・再読込、無ければ新しいタブ)"),
                        ("Alt+K", "ターミナルへ切替"),
                        ("Ctrl+T / Ctrl+W", "新しいタブ / このタブを閉じる (最後のタブで元の画面へ)"),
                        ("Ctrl+Tab / Ctrl+Shift+Tab", "次 / 前のタブ (バッファ帯のタブのクリックでも切替)"),
                        ("URL 欄 + Enter", "URL・ファイルのパス・localhost:ポートを開く"),
                        ("F5 / Alt+← / Alt+→", "再読み込み / 戻る / 進む"),
                        ("外部ブラウザ", "表示中のページを既定のブラウザで開く"),
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

    pub(super) fn render_about_dialog(&mut self, ctx: &egui::Context) {
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
                // 2つの表の1列目の幅をそろえる（License / Copyright と Based on / Repository）
                const LABEL_WIDTH: f32 = 90.0;
                egui::Grid::new("about_meta_grid")
                    .num_columns(2)
                    .min_col_width(LABEL_WIDTH)
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
                    });

                ui.add_space(10.0);
                egui::Grid::new("about_link_grid")
                    .num_columns(2)
                    .min_col_width(LABEL_WIDTH)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
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
                // 行の高さをボタン1つ分にする（with_layout は残りの高さを全部使い、ダイアログが縦に伸びる）
                let row = Vec2::new(ui.available_width(), ui.spacing().interact_size.y);
                ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("閉じる (ESC / Enter / クリック)").clicked()
                        || ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::Enter))
                    {
                        close_clicked = true;
                    }
                });
            });

        self.show_about = open && (link_clicked || !close_clicked);
    }

    /// ファイラーのコピー・移動・削除の後始末（マーク解除、両ペイン更新、結果の表示）。
    pub(super) fn finish_filer_op(
        &mut self,
        op_name: &str,
        ok_count: usize,
        last_err: Option<String>,
    ) {
        self.filer.active_pane_mut().marked.clear();
        self.filer.refresh_both();
        self.status_message = match last_err {
            Some(err) => format!("{op_name}: {ok_count}件完了 / エラー({err})"),
            None => format!("{ok_count}件を{op_name}しました"),
        };
    }

    pub(super) fn render_filer_modal(&mut self, ctx: &egui::Context) {
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
                modal_window(ctx, "リネーム (F2)", TEXT_CYAN).show(ctx, |ui| {
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
                modal_window(ctx, "新規ファイル作成 (F3)", TEXT_CYAN).show(ctx, |ui| {
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
                modal_window(ctx, "新規フォルダ作成 (F7)", TEXT_CYAN).show(ctx, |ui| {
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

                modal_window(ctx, format!("ファイル{op_name}確認"), TEXT_CYAN).show(ctx, |ui| {
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
                    more_items_label(ui, targets.len());
                    ui.add_space(10.0);
                    (do_exec, do_cancel) = yes_no_row(ui, "実行 (Enter / Y)");
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
                    self.finish_filer_op(op_name, ok_count, last_err);
                } else if do_cancel {
                    keep_modal = false;
                    self.status_message = format!("{op_name}をキャンセルしました");
                }
            }

            FilerModal::ConfirmDelete { targets } => {
                let mut do_exec = false;
                let mut do_cancel = false;

                modal_window(ctx, "削除確認 (F6)", TEXT_AMBER).show(ctx, |ui| {
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
                    more_items_label(ui, targets.len());
                    ui.add_space(10.0);
                    (do_exec, do_cancel) = yes_no_row(ui, "削除実行 (Enter / Y)");
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
                    self.finish_filer_op("削除", ok_count, last_err);
                } else if do_cancel {
                    keep_modal = false;
                    self.status_message = "削除をキャンセルしました".to_string();
                }
            }

            FilerModal::ConfirmCloseBuffer { buf_idx, name } => {
                let mut do_close = false;
                let mut do_cancel = false;
                let target_idx = *buf_idx;
                modal_window(ctx, "バッファを閉じる確認", TEXT_AMBER).show(ctx, |ui| {
                    ui.set_min_width(380.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "「{name}」には未保存の変更があります。保存せず閉じますか？"
                        ))
                        .font(FontId::monospace(13.5))
                        .color(TEXT_AMBER),
                    );
                    ui.add_space(10.0);
                    (do_close, do_cancel) = yes_no_row(ui, "保存せず閉じる (Enter / Y)");
                });

                if do_close {
                    keep_modal = false;
                    self.force_close_buffer(target_idx);
                } else if do_cancel {
                    keep_modal = false;
                    self.status_message = "バッファを閉じるのをキャンセルしました".to_string();
                }
            }

            FilerModal::ChooseEncoding { selected, message } => {
                let mut do_save = false;
                let mut do_cancel = false;
                modal_window(ctx, "文字コードを指定して保存", TEXT_CYAN).show(ctx, |ui| {
                    ui.set_min_width(380.0);
                    ui.label(
                        egui::RichText::new(self.cur_buf().display_name())
                            .font(FontId::monospace(12.5))
                            .color(TEXT_DIM_CYAN),
                    );
                    ui.add_space(4.0);
                    for enc in TextEncoding::ALL {
                        ui.radio_value(
                            selected,
                            enc,
                            egui::RichText::new(enc.label()).font(FontId::monospace(14.0)),
                        );
                    }
                    // ↑↓ で選ぶ
                    let pos = TextEncoding::ALL
                        .iter()
                        .position(|e| e == selected)
                        .unwrap_or(0);
                    let n = TextEncoding::ALL.len();
                    if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                        *selected = TextEncoding::ALL[(pos + 1) % n];
                    }
                    if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
                        *selected = TextEncoding::ALL[(pos + n - 1) % n];
                    }
                    if !message.is_empty() {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(message.as_str())
                                .font(FontId::monospace(12.5))
                                .color(TEXT_AMBER),
                        );
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("保存 (Enter)").clicked()
                            || ui.input(|i| i.key_pressed(Key::Enter))
                        {
                            do_save = true;
                        }
                        if ui.button("キャンセル (ESC)").clicked()
                            || ui.input(|i| i.key_pressed(Key::Escape))
                        {
                            do_cancel = true;
                        }
                    });
                });
                if do_save {
                    match self.save_with_encoding(*selected) {
                        Ok(()) => keep_modal = false,
                        Err(e) => *message = e,
                    }
                } else if do_cancel {
                    keep_modal = false;
                }
            }
            FilerModal::ConfirmHardLinkSave { path, encoding } => {
                let mut answer = None;
                modal_window(ctx, "保存の確認 - ハードリンク", TEXT_AMBER).show(ctx, |ui| {
                    ui.set_min_width(420.0);
                    ui.label(
                        egui::RichText::new(path.display().to_string())
                            .font(FontId::monospace(12.5))
                            .color(TEXT_DIM_CYAN),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "このファイルにはハードリンクがあります。巨大ファイルは置き換えて保存するため、\nリンクが切れ、このファイルだけが新しい内容になります。保存しますか？",
                        )
                        .font(FontId::monospace(13.0))
                        .color(TEXT_AMBER),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("保存 (Enter / Y)").clicked()
                            || ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Y))
                        {
                            answer = Some(true);
                        }
                        if ui.button("キャンセル (ESC / N)").clicked()
                            || ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::N))
                        {
                            answer = Some(false);
                        }
                    });
                });
                match answer {
                    Some(true) => {
                        keep_modal = false;
                        let (path, encoding) = (path.clone(), *encoding);
                        self.start_huge_save(path, encoding);
                    }
                    Some(false) => {
                        keep_modal = false;
                        self.quit_after_save = false;
                        self.status_message = "保存をキャンセルしました".into();
                    }
                    None => {}
                }
            }
            FilerModal::ConfirmQuit => {
                let mut do_save_quit = false;
                let mut do_quit = false;
                let mut do_cancel = false;
                modal_window(ctx, "終了確認 - 未保存の変更", TEXT_AMBER).show(ctx, |ui| {
                    ui.set_min_width(380.0);
                    ui.label(
                        egui::RichText::new("次のバッファに未保存の変更があります。保存しますか？")
                            .font(FontId::monospace(13.5))
                            .color(TEXT_AMBER),
                    );
                    ui.add_space(6.0);
                    for slot in self.buffers.iter().filter(|s| s.buffer.dirty) {
                        ui.label(
                            egui::RichText::new(format!("  * {}", slot.buffer.display_name()))
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

            FilerModal::ConfirmReload {
                buf_idx,
                stamp,
                shown,
            } => {
                let (idx, stamp) = (*buf_idx, *stamp);
                if self.mode != AppMode::Editor || idx != self.active_buf {
                    return;
                }
                // 表示直後の打鍵（入力中の Enter や Y）では答えない
                let keys_ok = shown.elapsed() >= std::time::Duration::from_millis(400);
                let mut do_reload = false;
                let mut do_keep = false;
                let (name, path, dirty) = self
                    .buffers
                    .get(idx)
                    .map(|s| {
                        (
                            s.buffer.display_name(),
                            s.buffer
                                .file_path
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                            s.buffer.dirty,
                        )
                    })
                    .unwrap_or_default();
                modal_window(ctx, "外部での変更", TEXT_AMBER)
                    .show(ctx, |ui| {
                        ui.set_min_width(420.0);
                        ui.label(
                            egui::RichText::new(format!("「{name}」が外部で変更されました。再読み込みしますか？"))
                                .font(FontId::monospace(13.5))
                                .color(TEXT_AMBER),
                        );
                        ui.label(
                            egui::RichText::new(path)
                                .font(FontId::monospace(12.5))
                                .color(TEXT_DIM_CYAN),
                        );
                        if dirty {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(
                                    "vz での未保存の変更は破棄されます（再読み込み後に「元に戻す」で戻せます）",
                                )
                                .font(FontId::monospace(13.0))
                                .color(TEXT_WHITE),
                            );
                        }
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("再読み込み (Y)").clicked()
                                || (keys_ok && ui.input(|i| i.key_pressed(Key::Y)))
                            {
                                do_reload = true;
                            }
                            if ui.button("このまま (ESC / N)").clicked()
                                || (keys_ok && ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::N)))
                            {
                                do_keep = true;
                            }
                        });
                    });

                if do_reload {
                    keep_modal = false;
                    self.reload_buffer(idx);
                } else if do_keep {
                    keep_modal = false;
                    if let Some(slot) = self.buffers.get_mut(idx) {
                        slot.ignored_stamp = Some(stamp);
                    }
                    self.status_message = format!("外部での変更を読み込みませんでした: {name}");
                } else if idx >= self.buffers.len() {
                    keep_modal = false;
                }
            }

            FilerModal::ResolveConflict { path, diff, shown } => {
                let keys_ok = shown.elapsed() >= std::time::Duration::from_millis(400);
                let mut choice: Option<Option<bool>> = None;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                modal_window(ctx, "外部の変更との衝突", TEXT_AMBER)
                    .show(ctx, |ui| {
                        ui.set_min_width(640.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "「{name}」は前回の保存して終了のとき外部で変更されていたため、上書きしていません。"
                            ))
                            .font(FontId::monospace(13.5))
                            .color(TEXT_AMBER),
                        );
                        ui.label(
                            egui::RichText::new(
                                "vz の編集内容は [無題] バッファに残しています。どちらを採用しますか？",
                            )
                            .font(FontId::monospace(13.5))
                            .color(TEXT_AMBER),
                        );
                        ui.label(
                            egui::RichText::new(path.display().to_string())
                                .font(FontId::monospace(12.5))
                                .color(TEXT_DIM_CYAN),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("差分:  - ファイル（外部の変更）だけの行   + vz の編集だけの行")
                                .font(FontId::monospace(12.5))
                                .color(TEXT_WHITE),
                        );
                        egui::ScrollArea::both()
                            .max_height(360.0)
                            .max_width(760.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                const MAX_LINES: usize = 3000;
                                for (kind, text) in diff.iter().take(MAX_LINES) {
                                    let color = match kind {
                                        DiffKind::Removed => Color32::from_rgb(255, 128, 128),
                                        DiffKind::Added => Color32::from_rgb(128, 232, 144),
                                        DiffKind::Skip => TEXT_DIM_CYAN,
                                        DiffKind::Same => TEXT_WHITE,
                                    };
                                    ui.label(
                                        egui::RichText::new(text)
                                            .font(FontId::monospace(12.5))
                                            .color(color),
                                    );
                                }
                                if diff.len() > MAX_LINES {
                                    ui.label(
                                        egui::RichText::new(format!("... 他 {} 行", diff.len() - MAX_LINES))
                                            .font(FontId::monospace(12.5))
                                            .color(TEXT_DIM_CYAN),
                                    );
                                }
                            });
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("vz の内容で上書き保存 (V)").clicked()
                                || (keys_ok && ui.input(|i| i.key_pressed(Key::V)))
                            {
                                choice = Some(Some(true));
                            }
                            if ui.button("ファイルの内容を採用 (F)").clicked()
                                || (keys_ok && ui.input(|i| i.key_pressed(Key::F)))
                            {
                                choice = Some(Some(false));
                            }
                            if ui.button("あとで決める (ESC)").clicked()
                                || (keys_ok && ui.input(|i| i.key_pressed(Key::Escape)))
                            {
                                choice = Some(None);
                            }
                        });
                    });

                if let Some(choice) = choice {
                    keep_modal = false;
                    self.resolve_conflict(path, choice);
                }
            }
        }

        if keep_modal {
            self.filer_modal = Some(modal);
        }
    }

    pub(super) fn render_inline_path_prompt(&mut self, ctx: &egui::Context) {
        let mut submit_path: Option<(bool, PathBuf)> = None;
        let mut cancel = false;

        if let Some(prompt) = &mut self.inline_prompt {
            let title = if prompt.is_save {
                "名前を付けて保存 (ファイルパスを入力)"
            } else {
                "ファイルを開く (ファイルパスを入力)"
            };
            modal_window(ctx, title, TEXT_CYAN).show(ctx, |ui| {
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
