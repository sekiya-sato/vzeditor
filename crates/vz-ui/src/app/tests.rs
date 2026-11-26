use super::wrap::wrap_breaks;
use super::*;

pub(super) fn make_test_app() -> VzEditorApp {
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
        search: SearchState::default(),
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
        overwrite_mode: false,
        clipboard_out: None,
        quit_request: None,
        last_disk_check: std::time::Instant::now(),
        conflict_queue: Vec::new(),
        word_wrap: false,
        wrap_cols: 80,
        window_geometry: None,
        pending_window_restore: None,
        terminal: None,
        term_selection: None,
        pending_terminal_cwd: None,
        return_mode: AppMode::Editor,
        browsers: Vec::new(),
        active_browser: 0,
        browser_view_rect: None,
        browser_url_editing: false,
    }
}

fn viewport_info(outer: [f32; 4], ppp: f32, maximized: bool) -> egui::ViewportInfo {
    let rect = egui::Rect::from_min_max(
        egui::pos2(outer[0], outer[1]),
        egui::pos2(outer[2], outer[3]),
    );
    egui::ViewportInfo {
        native_pixels_per_point: Some(ppp),
        outer_rect: Some(rect),
        inner_rect: Some(rect.shrink2(egui::vec2(8.0, 16.0))),
        maximized: Some(maximized),
        minimized: Some(false),
        ..Default::default()
    }
}

#[test]
fn test_next_window_geometry() {
    // 通常: 外枠の左上を物理ピクセル、内側サイズを論理ピクセルで覚える（2台目 150%）
    let info = viewport_info([-1000.0, 100.0, -200.0, 700.0], 1.5, false);
    let g = next_window_geometry(None, &info, 1.0).unwrap();
    assert_eq!((g.x, g.y, g.scale, g.maximized), (-1500, 150, 1.5, false));
    assert_eq!((g.width, g.height), (784.0, 568.0));

    // 最大化: 元のサイズと、最大化したモニター上の元の位置を残す
    let max = viewport_info([-1285.0, -5.0, 5.0, 715.0], 1.5, true);
    let m = next_window_geometry(Some(g), &max, 1.0).unwrap();
    assert_eq!(
        (m.x, m.y, m.width, m.height, m.maximized),
        (-1500, 150, 784.0, 568.0, true)
    );

    // 別のモニターで最大化: そのモニターの中央に置く
    let other = viewport_info([0.0, 0.0, 1920.0, 1040.0], 1.0, true);
    let o = next_window_geometry(Some(g), &other, 1.0).unwrap();
    assert_eq!((o.x, o.y, o.maximized), (568, 236, true));

    // 最小化中・情報なしは前の値を残す（None）
    let mut min = info.clone();
    min.minimized = Some(true);
    assert_eq!(next_window_geometry(Some(g), &min, 1.0), None);
    assert_eq!(
        next_window_geometry(Some(g), &egui::ViewportInfo::default(), 1.0),
        None
    );
}

#[test]
fn test_truncate_helpers() {
    assert!(matches!(truncate_path_left("abc", 5), Cow::Borrowed("abc")));
    assert_eq!(
        truncate_path_left("C:/work/日本語/file.txt", 12),
        ".../file.txt"
    );
    // 幅 2 の文字は予算（7 - 3 = 4）に 2 文字まで
    assert_eq!(truncate_path_left("ab日本語", 7), "...本語");
    assert!(matches!(
        truncate_display_right("abc", 3),
        Cow::Borrowed("abc")
    ));
    assert_eq!(truncate_display_right("日本語テキスト", 8), "日本語…");
}

#[test]
fn test_wrap_breaks_and_layout() {
    // 半角 1・全角 2 セル。全角が端に収まらなければ次の表示行へ送る
    assert_eq!(wrap_breaks("abcdefghij", 4), vec![4, 8]);
    assert_eq!(wrap_breaks("abc", 4), Vec::<usize>::new());
    assert_eq!(wrap_breaks("abcあい", 4), vec![3]);
    assert_eq!(wrap_breaks("abcあいう", 4), vec![3, 5]);
    assert_eq!(wrap_breaks("", 4), Vec::<usize>::new());

    let lines: Vec<String> = vec!["abcdefghij".into(), "".into(), "xy".into()];
    let w = WrapLayout::build(&lines, 4, 0);
    assert_eq!(w.total_rows(), 5);
    assert_eq!(w.line_of_row(0), (0, 0));
    assert_eq!(w.line_of_row(2), (0, 2));
    assert_eq!(w.line_of_row(3), (1, 0));
    assert_eq!(w.line_of_row(4), (2, 0));
    assert_eq!(w.seg_range(0, 1, 10), (4, 8));
    assert_eq!(w.seg_range(0, 2, 10), (8, 10));
    // 折り返し位置ちょうどは次の表示行、行末は最後の表示行
    assert_eq!(w.row_of(0, 4), 1);
    assert_eq!(w.row_of(0, 10), 2);
    assert!(w.is_last_seg(0, 2) && !w.is_last_seg(0, 1));
}

#[test]
fn test_word_wrap_cursor_moves_by_display_row() {
    let mut app = make_test_app();
    let mut buf = TextBuffer::new();
    buf.insert_text("abcdefghij\nxy");
    buf.set_cursor(0, 1);
    app.buffers = vec![BufferSlot::new(buf)];
    app.word_wrap = true;
    app.wrap_cols = 4;

    app.cursor_down();
    assert_eq!(app.cur_buf().cursor_pos(), (0, 5));
    app.cursor_down();
    assert_eq!(app.cur_buf().cursor_pos(), (0, 9));
    app.cursor_down();
    assert_eq!(app.cur_buf().cursor_pos(), (1, 1));
    app.cursor_up();
    app.cursor_up();
    assert_eq!(app.cur_buf().cursor_pos(), (0, 5));
    // 次の表示行の先頭に移らないよう、途中の表示行の終端は1文字手前
    app.cur_buf_mut().set_cursor(1, 2);
    app.cursor_up();
    app.cursor_up();
    assert_eq!(app.cur_buf().cursor_pos(), (0, 6));
    app.cur_buf_mut().set_cursor(0, 7);
    app.cursor_up();
    assert_eq!(app.cur_buf().cursor_pos(), (0, 3));

    // オフなら論理行単位
    app.toggle_word_wrap();
    assert!(!app.word_wrap);
    app.cur_buf_mut().set_cursor(0, 1);
    app.cursor_down();
    assert_eq!(app.cur_buf().cursor_pos(), (1, 1));

    // サクラ互換の ↓ も表示行単位
    let ctx = egui::Context::default();
    let mut app = make_sakura_app("abcdefghij\nxy");
    app.word_wrap = true;
    app.wrap_cols = 4;
    let none = mods(false, false);
    run_editor(&ctx, &mut app, none, vec![key_with(Key::ArrowDown, none)]);
    assert_eq!(app.cur_buf().cursor_pos(), (0, 4));
}

#[test]
fn test_save_session_and_restore() {
    let dir = std::env::temp_dir().join(format!("vz_session_app_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let saved_path = dir.join("a.txt");
    let dirty_path = dir.join("b.md");
    fs::write(&saved_path, "one\ntwo\nthree").unwrap();
    fs::write(&dirty_path, "# old").unwrap();
    let session_dir = dir.join("session");

    let mut app = make_test_app();
    app.buffers.clear();
    app.buffers
        .push(BufferSlot::new(TextBuffer::from_file(&saved_path).unwrap()));
    app.buffers[0].buffer.set_cursor(2, 3);
    let mut b = TextBuffer::from_file(&dirty_path).unwrap();
    b.set_cursor(0, 5);
    b.insert_text(" new");
    app.buffers.push(BufferSlot::new(b));
    app.buffers[1].markdown_view = true;
    let mut u = TextBuffer::new();
    u.insert_text("無題の内容\n2行目");
    app.buffers.push(BufferSlot::new(u));
    app.buffers.push(BufferSlot::new(TextBuffer::new())); // 未編集の無題は残さない
    app.active_buf = 2;

    app.save_session_to(&session_dir).unwrap();
    // パスのある未保存バッファは保存される
    assert_eq!(fs::read_to_string(&dirty_path).unwrap(), "# old new");
    assert!(!app.buffers[1].buffer.dirty);

    let mut app2 = make_test_app();
    app2.buffers = vec![BufferSlot::new(TextBuffer::new())];
    app2.active_buf = 0;
    app2.restore_session_from(&session_dir);
    assert_eq!(app2.buffers.len(), 3);
    assert_eq!(
        app2.buffers[0].buffer.file_path.as_deref(),
        Some(saved_path.as_path())
    );
    assert_eq!(app2.buffers[0].buffer.cursor_pos(), (2, 3));
    assert!(app2.buffers[1].markdown_view);
    assert_eq!(app2.buffers[1].buffer.text_lines(), vec!["# old new"]);
    let u2 = &app2.buffers[2].buffer;
    assert!(u2.file_path.is_none() && u2.dirty);
    assert_eq!(u2.text_lines(), vec!["無題の内容", "2行目"]);
    assert_eq!(app2.active_buf, 2);
    // 状態ファイルと一時保存ファイルは消える
    assert_eq!(fs::read_dir(&session_dir).unwrap().count(), 0);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_save_session_keeps_huge_edits_when_save_fails() {
    let dir = std::env::temp_dir().join(format!("vz_session_huge_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.log");
    let session_dir = dir.join("session");
    fs::write(&path, "line0\nline1\nline2\n").unwrap();

    let mut app = make_test_app();
    let mut b = TextBuffer::open_huge(&path).unwrap();
    b.huge_mut().unwrap().wait_index();
    b.set_cursor(1, 5);
    b.insert_text("-vz");
    app.buffers = vec![BufferSlot::new(b)];
    app.active_buf = 0;
    // 一時ファイルと同名のディレクトリで、原子的保存を失敗させる。
    fs::create_dir(dir.join(".big.log.vzsave.tmp")).unwrap();
    app.save_session_to(&session_dir).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "line0\nline1\nline2\n");
    drop(app);

    let mut app2 = make_test_app();
    app2.buffers = vec![BufferSlot::new(TextBuffer::new())];
    app2.active_buf = 0;
    app2.restore_session_from(&session_dir);
    let buf = &mut app2.cur_slot_mut().buffer;
    assert!(buf.is_huge(), "{}", app2.status_message);
    let buf = &mut app2.cur_slot_mut().buffer;
    buf.huge_mut().unwrap().wait_index();
    assert_eq!(buf.line(1), "line1-vz");
    assert!(buf.dirty);
    // 一時保存した編集内容は当て直した後に消す
    assert!(fs::read_dir(&session_dir)
        .map(|d| d
            .flatten()
            .all(|e| !e.file_name().to_string_lossy().ends_with(".vzedit")))
        .unwrap_or(true));
    drop(app2);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_save_session_reports_failure_when_huge_edits_cannot_be_kept() {
    let dir = std::env::temp_dir().join(format!("vz_session_huge_failed_{}", std::process::id()));
    let session_dir = dir.join("session");
    fs::create_dir_all(session::huge_edits_path(&session_dir, 0)).unwrap();
    fs::create_dir(dir.join(".big.log.vzsave.tmp")).unwrap();
    let path = dir.join("big.log");
    fs::write(&path, "original\n").unwrap();

    let mut app = make_test_app();
    let mut buf = TextBuffer::open_huge(&path).unwrap();
    buf.huge_mut().unwrap().wait_index();
    buf.insert_text("edit ");
    app.buffers = vec![BufferSlot::new(buf)];

    let err = app.save_session_to(&session_dir).unwrap_err();
    assert!(err.to_string().contains("編集内容の一時保存"));
    assert!(!session_dir.join("session.ini").exists());
    assert_eq!(fs::read_to_string(&path).unwrap(), "original\n");
    assert_eq!(app.cur_buf().line(0), "edit original");
    assert!(app.cur_buf().dirty);
    drop(app);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_huge_hard_links_in_both_quit_paths() {
    let dir = std::env::temp_dir().join(format!("vz_quit_huge_links_{}", std::process::id()));
    let session_dir = dir.join("session");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.log");
    let link = dir.join("link.log");
    fs::write(&path, "original\n").unwrap();
    fs::hard_link(&path, &link).unwrap();

    let mut app = make_test_app();
    let mut buf = TextBuffer::open_huge(&path).unwrap();
    buf.huge_mut().unwrap().wait_index();
    buf.insert_text("edit ");
    app.buffers = vec![BufferSlot::new(buf)];

    // 外部コマンドの経路では、リンクを保ったまま編集内容を復元する。
    app.save_session_to(&session_dir).unwrap();
    assert_eq!(vz_platform::file_info::hard_link_count(&path), Some(2));
    assert_eq!(fs::read_to_string(&link).unwrap(), "original\n");
    drop(app);

    let mut app = make_test_app();
    app.restore_session_from(&session_dir);
    app.cur_buf_mut().huge_mut().unwrap().wait_index();
    assert_eq!(app.cur_buf().line(0), "edit original");
    assert!(app.cur_buf().dirty);

    // 画面の「保存して終了」では、確認を待っている間は上書きしない。
    let ctx = egui::Context::default();
    app.quit_after_save = true;
    app.continue_save_and_quit(&ctx);
    app.continue_save_and_quit(&ctx);
    assert!(matches!(
        app.filer_modal,
        Some(FilerModal::ConfirmHardLinkSave { .. })
    ));
    assert!(app.cur_slot().huge_save.is_none());
    assert!(!app.allow_close);
    assert_eq!(vz_platform::file_info::hard_link_count(&path), Some(2));

    let input = |key| egui::RawInput {
        events: vec![key_event(key, false)],
        ..Default::default()
    };
    let _ = ctx.run(input(Key::Escape), |ctx| app.render_filer_modal(ctx));
    assert!(!app.quit_after_save);
    assert!(app.cur_buf().dirty);
    assert_eq!(vz_platform::file_info::hard_link_count(&path), Some(2));

    app.quit_after_save = true;
    app.continue_save_and_quit(&ctx);
    let _ = ctx.run(input(Key::Enter), |ctx| app.render_filer_modal(ctx));
    assert!(app.cur_slot().huge_save.is_some());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.cur_slot().huge_save.is_some() {
        assert!(std::time::Instant::now() < deadline);
        app.continue_save_and_quit(&ctx);
        app.poll_huge_buffers(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    app.continue_save_and_quit(&ctx);
    assert!(app.allow_close);
    assert_eq!(fs::read_to_string(&path).unwrap(), "edit original\n");
    assert_eq!(fs::read_to_string(&link).unwrap(), "original\n");
    assert_eq!(vz_platform::file_info::hard_link_count(&path), Some(1));
    drop(app);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_save_session_conflict_keeps_external_change() {
    let dir = std::env::temp_dir().join(format!("vz_session_conflict_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("README.md");
    let session_dir = dir.join("session");
    fs::write(&path, "a\nb\nc").unwrap();

    let mut app = make_test_app();
    let mut b = TextBuffer::from_file(&path).unwrap();
    b.set_cursor(0, 1);
    b.insert_text("-vz");
    app.buffers = vec![BufferSlot::new(b)];
    app.active_buf = 0;
    // vz で編集中に外部でファイルが変更された（サイズを変えて確実に検知させる）
    fs::write(&path, "a\nb\nc\nexternal line").unwrap();

    app.save_session_to(&session_dir).unwrap();
    // 外部の変更は上書きしない
    assert_eq!(fs::read_to_string(&path).unwrap(), "a\nb\nc\nexternal line");

    let mut app2 = make_test_app();
    app2.buffers = vec![BufferSlot::new(TextBuffer::new())];
    app2.active_buf = 0;
    app2.restore_session_from(&session_dir);
    // ファイルは外部の内容で開き、vz の編集内容は無題バッファに残る
    assert_eq!(app2.buffers.len(), 2);
    assert_eq!(
        app2.buffers[0].buffer.text_lines(),
        vec!["a", "b", "c", "external line"]
    );
    assert!(!app2.buffers[0].buffer.dirty);
    let u = &app2.buffers[1];
    assert!(u.buffer.file_path.is_none() && u.buffer.dirty);
    assert_eq!(u.buffer.text_lines(), vec!["a-vz", "b", "c"]);
    assert_eq!(u.conflict_of.as_deref(), Some(path.as_path()));
    assert_eq!(app2.conflict_queue, vec![path.clone()]);

    // 確認ダイアログには差分を出す
    let queued = app2.conflict_queue.remove(0);
    app2.open_conflict_dialog(queued);
    match &app2.filer_modal {
        Some(FilerModal::ResolveConflict { diff, .. }) => {
            assert!(diff
                .iter()
                .any(|(k, t)| *k == DiffKind::Removed && t.ends_with("- external line")));
            assert!(diff
                .iter()
                .any(|(k, t)| *k == DiffKind::Added && t.ends_with("+ a-vz")));
        }
        _ => panic!("衝突の確認ダイアログが開いていない"),
    }
    app2.filer_modal = None;

    // あとで決める: 次の保存して終了でも確認を引き継ぐ
    app2.resolve_conflict(&path, None);
    assert_eq!(app2.buffers.len(), 2);
    app2.save_session_to(&session_dir).unwrap();
    let mut app3 = make_test_app();
    app3.buffers = vec![BufferSlot::new(TextBuffer::new())];
    app3.restore_session_from(&session_dir);
    assert_eq!(app3.conflict_queue, vec![path.clone()]);
    assert_eq!(app3.buffers.len(), 2);

    // vz の内容を採用: ファイルを上書きし、無題バッファを閉じる
    app3.resolve_conflict(&path, Some(true));
    assert_eq!(fs::read_to_string(&path).unwrap(), "a-vz\nb\nc");
    assert_eq!(app3.buffers.len(), 1);
    assert!(!app3.cur_buf().dirty);
    assert_eq!(app3.cur_buf().text_lines(), vec!["a-vz", "b", "c"]);

    // ファイルの内容を採用: 無題バッファを閉じるだけ
    app2.resolve_conflict(&path, Some(false));
    assert_eq!(app2.buffers.len(), 1);
    assert_eq!(fs::read_to_string(&path).unwrap(), "a-vz\nb\nc");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_external_change_detect_and_reload() {
    let dir = std::env::temp_dir().join(format!("vz_external_change_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.txt");
    fs::write(&path, "one\ntwo").unwrap();

    let mut app = make_test_app();
    app.buffers = vec![BufferSlot::new(TextBuffer::from_file(&path).unwrap())];
    app.active_buf = 0;
    assert!(app.active_external_change().is_none());

    // 自分で保存した変更は外部の変更として扱わない
    app.cur_buf_mut().insert_text("x");
    let p = path.clone();
    app.cur_buf_mut().save_to_path(&p).unwrap();
    assert!(app.active_external_change().is_none());

    // 外部で変更されると検知する。「このまま」と答えた同じ変更は再度尋ねない
    fs::write(&path, "one\ntwo\nthree external").unwrap();
    let stamp = app
        .active_external_change()
        .expect("外部の変更を検知していない");
    app.buffers[0].ignored_stamp = Some(stamp);
    assert!(app.active_external_change().is_none());

    // 再読み込み（未保存の変更は破棄。元に戻すで戻せる）
    app.cur_buf_mut().insert_text("y");
    app.reload_buffer(0);
    assert_eq!(
        app.cur_buf().text_lines(),
        vec!["one", "two", "three external"]
    );
    assert!(!app.cur_buf().dirty);
    assert!(app.buffers[0].ignored_stamp.is_none());
    assert!(app.active_external_change().is_none());
    assert!(app.cur_buf_mut().undo());
    assert_eq!(app.cur_buf().text_lines(), vec!["xyone", "two"]);
    assert!(app.cur_buf().dirty);

    // 削除されたファイルは尋ねない
    fs::remove_file(&path).unwrap();
    assert!(app.active_external_change().is_none());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_external_change_prompts_only_for_the_active_editor_tab() {
    let dir = std::env::temp_dir().join(format!("vz_external_active_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.txt");
    let second = dir.join("second.txt");
    fs::write(&first, "first").unwrap();
    fs::write(&second, "second").unwrap();
    let mut app = make_test_app();
    app.buffers = vec![
        BufferSlot::new(TextBuffer::from_file(&first).unwrap()),
        BufferSlot::new(TextBuffer::from_file(&second).unwrap()),
    ];
    fs::write(&second, "second edited outside").unwrap();

    let ctx = egui::Context::default();
    let check = |app: &mut VzEditorApp| {
        app.last_disk_check = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            app.check_external_changes(ctx)
        });
    };
    check(&mut app);
    assert_eq!(app.active_buf, 0);
    assert!(app.filer_modal.is_none());

    app.switch_buffer_to(1);
    app.enter_filer();
    check(&mut app);
    assert!(app.filer_modal.is_none());

    app.enter_editor();
    check(&mut app);
    assert!(matches!(
        app.filer_modal,
        Some(FilerModal::ConfirmReload { buf_idx: 1, .. })
    ));
    assert_eq!(app.cur_buf().line(0), "second");

    app.switch_buffer_to(0);
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.render_filer_modal(ctx));
    assert!(app.filer_modal.is_none());
    app.switch_buffer_to(1);
    check(&mut app);
    assert!(matches!(
        app.filer_modal,
        Some(FilerModal::ConfirmReload { buf_idx: 1, .. })
    ));
    fs::remove_dir_all(dir).unwrap();
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
        let mut app = if sakura {
            make_sakura_app("a")
        } else {
            make_test_app()
        };
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
    let readme = include_str!("../../../../tests/fixtures/markdown_sample.md");
    *app.cur_buf_mut() = TextBuffer::from_lines(readme.lines().map(String::from).collect());
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
    assert_eq!(
        window_style_key_action(Key::W, Some(Key::W), alt),
        Some(MenuAction::ToggleWordWrap)
    );
    assert_eq!(
        window_style_key_action(Key::W, Some(Key::W), alt_shift),
        None
    );
    // Alt+K / Alt+B: ターミナル / ブラウザ（Shift・Ctrl 付きは対象外）
    assert_eq!(
        window_style_key_action(Key::K, Some(Key::K), alt),
        Some(MenuAction::ToggleTerminal)
    );
    assert_eq!(
        window_style_key_action(Key::B, Some(Key::B), alt),
        Some(MenuAction::ToggleBrowser)
    );
    assert_eq!(
        window_style_key_action(Key::K, Some(Key::K), alt_shift),
        None
    );
    assert_eq!(window_style_key_action(Key::B, Some(Key::B), ctrl), None);
    for (key, dialog) in [
        (Key::F, SearchDialog::Find),
        (Key::R, SearchDialog::Replace),
        (Key::G, SearchDialog::Grep),
        (Key::J, SearchDialog::Jump),
    ] {
        assert_eq!(
            window_style_key_action(key, Some(key), alt),
            Some(MenuAction::OpenSearch(dialog))
        );
        assert_eq!(window_style_key_action(key, Some(key), ctrl), None);
        assert_eq!(window_style_key_action(key, Some(key), alt_shift), None);
    }
    assert_eq!(
        window_style_key_action(Key::N, None, alt),
        Some(MenuAction::RepeatSearch(false))
    );
    assert_eq!(
        window_style_key_action(Key::N, None, alt_shift),
        Some(MenuAction::RepeatSearch(true))
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
    assert_eq!(
        app.save_dirty_buffers_for_quit(),
        QuitSaveStep::NeedSaveAs(0)
    );
}

#[test]
fn test_save_dirty_buffers_for_quit_saves_files() {
    let dir = std::env::temp_dir().join(format!("vz_quit_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("edited.txt");
    std::fs::write(
        &path, "old
",
    )
    .unwrap();

    let mut app = make_test_app();
    let mut buf = TextBuffer::from_file(&path).unwrap();
    buf.insert_text("new ");
    app.buffers = vec![BufferSlot::new(buf)];
    assert!(app.has_any_dirty_buffer());

    assert_eq!(app.save_dirty_buffers_for_quit(), QuitSaveStep::Done);
    assert!(!app.has_any_dirty_buffer());
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .starts_with("new old"));
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

#[test]
fn test_filer_shift_enter_does_not_open_in_editor() {
    let dir = std::env::temp_dir().join(format!("vz_run_test_{}", std::process::id()));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let mut app = make_test_app();
    app.filer = FilerState::new(dir.join("sub"));
    app.mode = AppMode::Filer;
    let pane = app.filer.active_pane_mut();
    pane.cursor = pane.entries.iter().position(|e| e.is_parent).unwrap();
    let before = pane.current_dir.clone();
    let shift_enter = Event::Key {
        key: Key::Enter,
        physical_key: Some(Key::Enter),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::SHIFT,
    };
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        events: vec![shift_enter],
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| app.handle_filer_events(ctx));
    // Shift+Enter は実行で、Enter（親フォルダへ移動・エディターで開く）にはならない。
    // 実際に OS で開く処理は副作用があるので、ここでは .. を対象にして呼び出し経路だけ確かめる。
    assert_eq!(app.mode, AppMode::Filer);
    assert_eq!(app.filer.active_pane().current_dir, before);
    assert!(app.status_message.contains("実行できません"));
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
        window_style_key_action(
            Key::C,
            Some(Key::C),
            egui::Modifiers {
                alt: true,
                ..Default::default()
            }
        ),
        None
    );

    // Alt+C → 2打目待ち。C / X 以外は取り消し、文字も入力されない
    let run = |app: &mut VzEditorApp, events: Vec<Event>| {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.handle_send_keys(ctx));
    };
    run(
        &mut app,
        vec![key_event(Key::C, true), Event::Text("c".into())],
    );
    assert!(app.send_prefix);
    run(
        &mut app,
        vec![key_event(Key::D, false), Event::Text("d".into())],
    );
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

    // 2打目の割り当て（P は GitHub Desktop の Push。実際の送信はテストで起こさない）
    assert_eq!(
        send_target_for_key(Key::C, false),
        Some(SendTarget::ClaudeCode)
    );
    assert_eq!(send_target_for_key(Key::X, false), Some(SendTarget::Codex));
    assert_eq!(
        send_target_for_key(Key::P, false),
        Some(SendTarget::GitHubDesktop)
    );
    assert_eq!(
        send_target_for_key(Key::N, false),
        Some(SendTarget::ClaudeCodeNewSession)
    );
    assert_eq!(
        send_target_for_key(Key::M, false),
        Some(SendTarget::CodexNewSession)
    );
    assert_eq!(send_target_for_key(Key::N, true), None);
    assert_eq!(send_target_for_key(Key::P, true), None);
    assert_eq!(send_target_for_key(Key::D, false), None);
    // Alt+P（VZ 互換の Markdown 切替）は Alt+C の後でなければ奪わない
    assert_eq!(
        window_style_key_action(
            Key::P,
            Some(Key::P),
            egui::Modifiers {
                alt: true,
                ..Default::default()
            }
        ),
        None
    );
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
fn test_huge_wrap_cache_matches_full_wrap() {
    let dir = std::env::temp_dir().join(format!("vz_app_hugewrap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.txt");
    // 長い行（表示行が 1,024 を大きく超える）・全角・タブ・空行を混ぜる
    let long: String = (0..30_000)
        .map(|i| {
            if i % 7 == 0 {
                'あ'
            } else if i % 101 == 0 {
                '\t'
            } else {
                'x'
            }
        })
        .collect();
    let text = format!("short\n{long}\n\nあいうえおかきくけこ\nend");
    fs::write(&path, &text).unwrap();
    let mut buf = TextBuffer::open_huge(&path).unwrap();
    buf.huge_mut().unwrap().wait_index();
    let cols = 10;
    let mut cache = wrap::HugeWrapCache::default();
    // 全部の表示行を先頭から取ると、通常の折り返し（wrap_breaks）と同じになる
    let mut expected = Vec::new();
    for (line, s) in text.split('\n').enumerate() {
        let br = wrap::wrap_breaks(s, cols);
        let len = s.chars().count();
        let mut starts = vec![0];
        starts.extend(br.iter().copied());
        for (seg, &st) in starts.iter().enumerate() {
            let last = seg + 1 == starts.len();
            let end = if last { len } else { starts[seg + 1] };
            expected.push(wrap::HugeRow {
                line,
                seg,
                start: st,
                end,
                last,
            });
        }
    }
    let all = cache.rows_from(&buf, cols, (0, 0), expected.len() + 5);
    assert_eq!(all, expected);
    // 途中の表示行から取っても同じ。前後への移動は互いに戻る
    for (i, row) in expected.iter().enumerate().step_by(997) {
        let got = cache.rows_from(&buf, cols, (row.line, row.seg), 3);
        assert_eq!(got[..], expected[i..(i + 3).min(expected.len())]);
        assert_eq!(
            cache.seg_of_col(&buf, cols, row.line, row.start),
            (row.seg, row.start)
        );
        let fwd = cache.forward(&buf, cols, (row.line, row.seg), 1500);
        let back_to = cache.back(&buf, cols, fwd, 1500);
        if i + 1500 < expected.len() {
            assert_eq!(back_to, (row.line, row.seg));
        }
    }
    // アプリ: 折り返しオンで下へ動かすと、長い行の中を表示行単位で進む
    let ctx = egui::Context::default();
    let mut app = make_sakura_app("");
    app.word_wrap = true;
    app.wrap_cols = cols;
    app.buffers = vec![BufferSlot::new(buf)];
    let none = mods(false, false);
    run_editor(
        &ctx,
        &mut app,
        none,
        vec![
            key_with(Key::ArrowDown, none),
            key_with(Key::ArrowDown, none),
        ],
    );
    assert_eq!(app.cur_buf().cursor_pos(), (1, expected[2].start));
    drop(app);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_huge_buffer_keys_with_word_wrap_and_save() {
    let dir = std::env::temp_dir().join(format!("vz_app_huge_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.txt");
    fs::write(&path, "line0\nline1\nline2\n").unwrap();
    let ctx = egui::Context::default();
    let mut app = make_sakura_app("");
    // 折り返し表示がオンでも、巨大ファイルは論理行で上下に動く
    app.word_wrap = true;
    let mut buf = TextBuffer::open_huge(&path).unwrap();
    buf.huge_mut().unwrap().wait_index();
    app.buffers = vec![BufferSlot::new(buf)];
    let none = mods(false, false);
    run_editor(
        &ctx,
        &mut app,
        none,
        vec![
            key_with(Key::ArrowDown, none),
            key_with(Key::ArrowDown, none),
            key_with(Key::End, none),
        ],
    );
    run_editor(&ctx, &mut app, none, vec![Event::Text("X".into())]);
    assert_eq!(app.cur_buf().cursor_pos(), (2, 6));
    assert_eq!(app.cur_buf().line(2), "line2X");
    assert!(app.cur_buf().dirty);
    // Markdown ビュー・行マークは使えない（メッセージだけ）
    app.toggle_markdown_view();
    assert!(!app.cur_slot().markdown_view);

    // 保存は裏のスレッドで行い、終わったら未保存の印が消える
    app.trigger_save();
    assert!(app.cur_slot().huge_save.is_some());
    // 保存中は編集できない
    run_editor(&ctx, &mut app, none, vec![Event::Text("Y".into())]);
    assert_eq!(app.cur_buf().line(2), "line2X");
    for _ in 0..200 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.poll_huge_buffers(ctx));
        if app.cur_slot().huge_save.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!app.cur_buf().dirty, "{}", app.status_message);
    assert_eq!(fs::read_to_string(&path).unwrap(), "line0\nline1\nline2X\n");
    // 保存後も Undo できる
    assert!(app.cur_buf_mut().undo());
    assert_eq!(app.cur_buf().line(2), "line2");
    drop(app);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_sakura_shift_select_and_replace() {
    let ctx = egui::Context::default();
    let mut app = make_sakura_app("abcdef\nghi");
    let shift = mods(false, true);
    run_editor(
        &ctx,
        &mut app,
        shift,
        vec![
            key_with(Key::ArrowRight, shift),
            key_with(Key::ArrowRight, shift),
            key_with(Key::ArrowRight, shift),
        ],
    );
    assert_eq!(app.cur_buf().selected_text().as_deref(), Some("abc"));

    // 文字入力で選択範囲を置き換える
    run_editor(
        &ctx,
        &mut app,
        mods(false, false),
        vec![Event::Text("X".into())],
    );
    assert_eq!(app.cur_buf().text_lines(), vec!["Xdef", "ghi"]);

    // Shift なしの移動で選択は解除される
    run_editor(&ctx, &mut app, shift, vec![key_with(Key::End, shift)]);
    assert!(app.cur_buf().has_selection());
    run_editor(
        &ctx,
        &mut app,
        mods(false, false),
        vec![key_with(Key::ArrowLeft, mods(false, false))],
    );
    assert!(!app.cur_buf().has_selection());

    // Ctrl+Shift+End でファイル末尾まで選択
    let cs = mods(true, true);
    run_editor(
        &ctx,
        &mut app,
        cs,
        vec![
            key_with(Key::Home, mods(false, false)),
            key_with(Key::End, cs),
        ],
    );
    assert_eq!(app.cur_buf().selected_text().as_deref(), Some("Xdef\nghi"));
}

/// Enter は行頭の空白を引き継ぎ、Shift+Enter は行頭から始める（両方式共通）。
#[test]
fn test_enter_auto_indent_both_schemes() {
    let ctx = egui::Context::default();
    let none = mods(false, false);
    let shift = mods(false, true);
    for sakura in [false, true] {
        let mut app = if sakura {
            make_sakura_app("\t\tこれはテスト")
        } else {
            let mut app = make_test_app();
            let mut buf = TextBuffer::new();
            buf.insert_text("\t\tこれはテスト");
            app.buffers = vec![BufferSlot::new(buf)];
            app
        };
        app.cur_buf_mut().move_line_end();
        run_editor(&ctx, &mut app, none, vec![key_with(Key::Enter, none)]);
        assert_eq!(
            app.cur_buf().text_lines(),
            vec!["\t\tこれはテスト", "\t\t"],
            "sakura={sakura}"
        );
        assert_eq!(app.cur_buf().cursor_pos(), (1, 2));
        run_editor(&ctx, &mut app, none, vec![Event::Text("次".into())]);
        assert_eq!(app.cur_buf().text_lines()[1], "\t\t次");

        run_editor(&ctx, &mut app, shift, vec![key_with(Key::Enter, shift)]);
        assert_eq!(
            app.cur_buf().text_lines(),
            vec!["\t\tこれはテスト", "\t\t次", ""],
            "sakura={sakura}"
        );
        assert_eq!(app.cur_buf().cursor_pos(), (2, 0));

        // 1回の Undo で Enter の改行とインデントがまとめて戻る
        app.cur_buf_mut().undo();
        app.cur_buf_mut().undo();
        app.cur_buf_mut().undo();
        assert_eq!(
            app.cur_buf().text_lines(),
            vec!["\t\tこれはテスト"],
            "sakura={sakura}"
        );
    }
}

#[test]
fn test_sakura_enter_replaces_selection_with_indent() {
    let ctx = egui::Context::default();
    let mut app = make_sakura_app("\tabc\ndef");
    app.cur_buf_mut().select_range((0, 2), (1, 1));
    let none = mods(false, false);
    run_editor(&ctx, &mut app, none, vec![key_with(Key::Enter, none)]);
    assert_eq!(app.cur_buf().text_lines(), vec!["\ta", "\tef"]);
    assert!(!app.cur_buf().has_selection());
}

#[test]
fn test_sakura_new_close_and_unassigned_f5() {
    let ctx = egui::Context::default();
    let mut app = make_sakura_app("abcdef\nghi");
    app.cur_buf_mut().dirty = false;
    let none = mods(false, false);

    // F5: 割り当てなし（カーソルも内容も変わらない）
    run_editor(&ctx, &mut app, none, vec![key_with(Key::F5, none)]);
    assert_eq!(app.cur_buf().cursor_pos(), (0, 0));
    assert_eq!(app.cur_buf().text_lines(), vec!["abcdef", "ghi"]);

    // F2: 新規バッファ
    run_editor(&ctx, &mut app, none, vec![key_with(Key::F2, none)]);
    assert_eq!(app.buffers.len(), 2);
    assert_eq!(app.active_buf, 1);

    // Ctrl+F4: 現在のバッファを閉じる
    let ctrl = mods(true, false);
    run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::F4, ctrl)]);
    assert_eq!(app.buffers.len(), 1);
    assert_eq!(app.cur_buf().text_lines(), vec!["abcdef", "ghi"]);
}

#[test]
fn test_insert_key_toggles_overwrite_in_both_schemes() {
    for sakura in [false, true] {
        let ctx = egui::Context::default();
        let mut app = make_sakura_app("abcdef\nghi");
        if !sakura {
            app.key_scheme = KeyScheme::Vz;
        }
        let none = mods(false, false);
        run_editor(&ctx, &mut app, none, vec![key_with(Key::Insert, none)]);
        assert!(app.overwrite_mode, "sakura={sakura}");

        // 上書き: カーソル位置の文字を置き換え、行末を越えたら追加する（改行は消さない）
        run_editor(&ctx, &mut app, none, vec![Event::Text("XY".into())]);
        assert_eq!(app.cur_buf().text_lines(), vec!["XYcdef", "ghi"]);
        app.cur_buf_mut().set_cursor(0, 5);
        run_editor(&ctx, &mut app, none, vec![Event::Text("123".into())]);
        assert_eq!(app.cur_buf().text_lines(), vec!["XYcde123", "ghi"]);

        // 貼り付けは挿入
        app.cur_buf_mut().set_cursor(1, 0);
        run_editor(&ctx, &mut app, none, vec![Event::Paste("P".into())]);
        assert_eq!(app.cur_buf().text_lines()[1], "Pghi");

        // もう一度 Ins で挿入に戻る
        run_editor(&ctx, &mut app, none, vec![key_with(Key::Insert, none)]);
        assert!(!app.overwrite_mode);
        run_editor(&ctx, &mut app, none, vec![Event::Text("Z".into())]);
        assert_eq!(app.cur_buf().text_lines()[1], "PZghi");
    }
}

#[test]
fn test_sakura_select_all_cut_undo_and_escape() {
    let ctx = egui::Context::default();
    let mut app = make_sakura_app("one\ntwo");
    let ctrl = mods(true, false);
    run_editor(
        &ctx,
        &mut app,
        ctrl,
        vec![key_with(Key::A, ctrl), Event::Cut],
    );
    assert_eq!(app.cur_buf().text_lines(), vec![""]);
    assert_eq!(app.clipboard_out.as_deref(), Some("one\ntwo"));

    run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::Z, ctrl)]);
    assert_eq!(app.cur_buf().text_lines(), vec!["one", "two"]);
    run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::Y, ctrl)]);
    assert_eq!(app.cur_buf().text_lines(), vec![""]);
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
    assert_eq!(app.cur_buf().text_lines(), vec!["second", "third"]);
    assert_eq!(app.clipboard_out.as_deref(), Some("first line\n"));
    // Ctrl+Shift+E: 行削除（クリップボードは変えない）
    app.clipboard_out = None;
    run_editor(&ctx, &mut app, cs, vec![key_with(Key::E, cs)]);
    assert_eq!(app.cur_buf().text_lines(), vec!["third"]);
    assert!(app.clipboard_out.is_none());
    // Ctrl+K: 行末まで切り取り / Ctrl+U: 行頭まで
    app.cur_buf_mut().set_cursor(0, 2);
    run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::K, ctrl)]);
    assert_eq!(app.cur_buf().text_lines(), vec!["th"]);
    run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::U, ctrl)]);
    assert_eq!(app.cur_buf().text_lines(), vec![""]);
    // Ctrl+I: 行の二重化
    app.cur_buf_mut().insert_text("dup");
    run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::I, ctrl)]);
    assert_eq!(app.cur_buf().text_lines(), vec!["dup", "dup"]);
    // サクラ互換の Ctrl+G は Grep を開き、VZ の1文字削除は実行しない。
    run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::G, ctrl)]);
    assert_eq!(app.cur_buf().text_lines(), vec!["dup", "dup"]);
    assert!(app.search.is_open());
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
    run_editor(
        &ctx,
        &mut app,
        ctrl,
        vec![key_with(Key::A, ctrl), Event::Paste("new".into())],
    );
    assert_eq!(app.cur_buf().text_lines(), vec!["new"]);

    // VZ 互換へ切り替えると選択は解除され、Ctrl+E は上移動に戻る
    app.cur_buf_mut().select_all();
    app.toggle_key_scheme();
    assert_eq!(app.key_scheme, KeyScheme::Vz);
    assert!(!app.cur_buf().has_selection());
}

#[test]
fn test_sakura_ctrl_tab_switches_buffer_in_markdown_view() {
    let ctx = egui::Context::default();
    let mut app = make_sakura_app("one");
    app.buffers.push(BufferSlot::new(TextBuffer::new()));
    app.cur_slot_mut().markdown_view = true;
    let ctrl = mods(true, false);
    run_editor(&ctx, &mut app, ctrl, vec![key_with(Key::Tab, ctrl)]);
    assert_eq!(app.active_buf, 1);
    // 戻った先（Markdown ビュー）でも Ctrl+Shift+Tab で切り替えられる
    app.cur_slot_mut().markdown_view = true;
    let cs = mods(true, true);
    run_editor(&ctx, &mut app, cs, vec![key_with(Key::Tab, cs)]);
    assert_eq!(app.active_buf, 0);
    assert!(app.cur_slot().markdown_view);
}

#[test]
fn test_browser_toggle_returns_to_previous_mode() {
    let mut app = make_test_app();
    app.toggle_browser();
    assert_eq!(app.mode, AppMode::Browser);
    app.toggle_browser();
    assert_eq!(app.mode, AppMode::Editor);

    // ファイラーから開いたらファイラーへ戻る
    app.enter_filer();
    app.toggle_browser();
    assert_eq!(app.mode, AppMode::Browser);
    app.toggle_browser();
    assert_eq!(app.mode, AppMode::Filer);
}

#[test]
fn test_browser_opens_edited_html_file() {
    let dir = std::env::temp_dir().join(format!("vz_browser_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let html = dir.join("page.html");
    std::fs::write(&html, "<p>hi</p>").unwrap();

    let mut app = make_test_app();
    app.open_file_in_buffer(html.clone());
    app.toggle_browser();
    assert_eq!(app.mode, AppMode::Browser);
    // WebView を作る前なので、作るときに開く URL として残る
    let url = app.browsers[0].pending_url.clone().expect("pending url");
    assert!(
        url.starts_with("file:///") && url.ends_with("/page.html"),
        "{url}"
    );
    assert_eq!(app.browsers[0].url_input, url);

    // 同じファイルは同じタブ、別の HTML は新しいタブで開く
    app.toggle_browser();
    app.toggle_browser();
    assert_eq!(app.browsers.len(), 1);
    let other = dir.join("other.html");
    std::fs::write(&other, "<p>2</p>").unwrap();
    app.toggle_browser();
    app.open_file_in_buffer(other);
    app.toggle_browser();
    assert_eq!(app.browsers.len(), 2);
    assert_eq!(app.active_browser, 1);
    assert!(app.browsers[1]
        .pending_url
        .as_deref()
        .unwrap()
        .ends_with("/other.html"));
    app.toggle_browser();
    let first = app.find_buffer_by_path(&html).expect("page.html");
    app.switch_buffer_to(first);
    app.toggle_browser();
    assert_eq!(app.active_browser, 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_browser_url_input_is_normalized() {
    let mut app = make_test_app();
    app.toggle_browser();
    app.browsers[0].url_input = "localhost:5173".to_string();
    app.navigate_browser_input();
    assert_eq!(
        app.browsers[0].pending_url.as_deref(),
        Some("http://localhost:5173")
    );

    app.browsers[0].url_input = "not a url".to_string();
    app.navigate_browser_input();
    assert!(app.status_message.contains("URL を解釈できません"));
}

#[test]
fn test_browser_tabs_new_close_and_switch() {
    let mut app = make_test_app();
    app.toggle_browser();
    app.browser_tab_shortcut('T');
    app.browser_tab_shortcut('T');
    assert_eq!((app.browsers.len(), app.active_browser), (3, 2));
    // Ctrl+Tab は端から反対の端へ回る
    app.browser_tab_shortcut('N');
    assert_eq!(app.active_browser, 0);
    app.browser_tab_shortcut('P');
    assert_eq!(app.active_browser, 2);
    app.browser_tab_shortcut('P');
    assert_eq!(app.active_browser, 1);
    // 選択中より前を閉じても同じタブを選んだまま
    app.close_browser_tab(0);
    assert_eq!((app.browsers.len(), app.active_browser), (2, 0));
    // 最後のタブを閉じたら閉じてから前のタブ、全部閉じたら元の画面へ
    app.browser_tab_shortcut('N');
    app.browser_tab_shortcut('W');
    assert_eq!((app.browsers.len(), app.active_browser), (1, 0));
    assert_eq!(app.mode, AppMode::Browser);
    app.browser_tab_shortcut('W');
    assert!(app.browsers.is_empty());
    assert_eq!(app.mode, AppMode::Editor);
}

#[test]
fn test_browser_ctrl_keys_handled_in_browser_mode() {
    let mut app = make_test_app();
    app.toggle_browser();
    let ctx = egui::Context::default();
    let press = |app: &mut VzEditorApp, key: Key, m: egui::Modifiers| {
        let input = egui::RawInput {
            events: vec![key_with(key, m)],
            modifiers: m,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.handle_browser_events(ctx));
    };
    let (ctrl, cs) = (mods(true, false), mods(true, true));
    press(&mut app, Key::T, ctrl);
    assert_eq!((app.browsers.len(), app.active_browser), (2, 1));
    press(&mut app, Key::Tab, ctrl);
    assert_eq!(app.active_browser, 0);
    press(&mut app, Key::Tab, cs);
    assert_eq!(app.active_browser, 1);
    press(&mut app, Key::W, ctrl);
    assert_eq!(app.browsers.len(), 1);
}

#[test]
fn test_session_restores_browser_tabs() {
    let dir = std::env::temp_dir().join(format!("vz_browser_tabs_session_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let session_dir = dir.join("session");

    let mut app = make_test_app();
    app.toggle_browser();
    app.browsers[0].url_input = "localhost:3000".to_string();
    app.navigate_browser_input();
    app.new_browser_tab(Some("https://example.com/".to_string()));
    app.new_browser_tab(None);
    app.switch_browser_tab(-1);
    // ブラウザを表示していなくても選択中のタブを残す
    app.toggle_browser();
    app.save_session_to(&session_dir).unwrap();

    let mut app2 = make_test_app();
    app2.restore_session_from(&session_dir);
    assert_eq!(app2.mode, AppMode::Editor);
    let urls: Vec<_> = app2
        .browsers
        .iter()
        .map(|t| t.pending_url.as_deref())
        .collect();
    assert_eq!(
        urls,
        [
            Some("http://localhost:3000"),
            Some("https://example.com/"),
            None
        ]
    );
    assert_eq!(app2.active_browser, 1);

    // 表示していたタブは画面ごと戻る
    app2.show_browser_tab(2);
    app2.save_session_to(&session_dir).unwrap();
    let mut app3 = make_test_app();
    app3.restore_session_from(&session_dir);
    assert_eq!(app3.mode, AppMode::Browser);
    assert_eq!((app3.browsers.len(), app3.active_browser), (3, 2));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_drag_file_tabs_in_the_buffer_strip() {
    let ctx = egui::Context::default();
    let mut app = make_test_app();
    let dir = std::env::temp_dir().join(format!("vz_file_tab_order_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    app.buffers = ["first.txt", "second.txt", "third.txt"]
        .map(|name| {
            let path = dir.join(name);
            fs::write(&path, name).unwrap();
            BufferSlot::new(TextBuffer::from_file(&path).unwrap())
        })
        .into();
    app.active_buf = 1;
    app.cur_buf_mut().cursor_col = 4;

    let frame = |app: &mut VzEditorApp, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 680.0))),
                events,
                ..Default::default()
            },
            |ctx| app.render_buffer_strip(ctx),
        )
    };
    let tab_positions = |output: egui::FullOutput| {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().ends_with(".txt") => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let press = |app: &mut VzEditorApp, pos| {
        frame(app, vec![Event::PointerMoved(pos), button(pos, true)]);
    };
    let order = |app: &VzEditorApp| {
        app.buffers
            .iter()
            .map(|slot| slot.buffer.display_name())
            .collect::<Vec<_>>()
    };

    let tabs = tab_positions(frame(&mut app, vec![]));
    let (first, third) = (tabs[0], tabs[2]);
    press(&mut app, first);
    frame(&mut app, vec![Event::PointerMoved(third)]);
    assert_eq!(order(&app), ["first.txt", "second.txt", "third.txt"]);
    frame(&mut app, vec![button(third, false)]);
    assert_eq!(order(&app), ["second.txt", "third.txt", "first.txt"]);
    assert_eq!(app.active_buf, 0);
    assert_eq!(app.cur_buf().cursor_col, 4);

    // レイアウトを更新してから、右から左にも挿入する。
    let tabs = tab_positions(frame(&mut app, vec![]));
    let (left, right) = (tabs[0], tabs[2]);
    press(&mut app, right);
    frame(&mut app, vec![Event::PointerMoved(left)]);
    frame(&mut app, vec![button(left, false)]);
    assert_eq!(order(&app), ["first.txt", "second.txt", "third.txt"]);
    assert_eq!(app.active_buf, 1);

    let tabs = tab_positions(frame(&mut app, vec![]));
    let third = tabs[2];
    let outside = Pos2::new(third.x, 100.0);
    press(&mut app, third);
    frame(&mut app, vec![Event::PointerMoved(outside)]);
    frame(&mut app, vec![button(outside, false)]);
    assert_eq!(order(&app), ["first.txt", "second.txt", "third.txt"]);
    assert_eq!(app.active_buf, 1);

    press(&mut app, third);
    frame(&mut app, vec![button(third, false)]);
    assert_eq!(app.active_buf, 2);

    frame(&mut app, vec![button(third, true)]);
    frame(&mut app, vec![Event::PointerMoved(tabs[0])]);
    frame(&mut app, vec![button(tabs[0], false)]);
    assert_eq!(order(&app), ["third.txt", "first.txt", "second.txt"]);
    assert_eq!(app.active_buf, 0);

    let session_dir = dir.join("session");
    app.save_session_to(&session_dir).unwrap();
    let mut restored = make_test_app();
    restored.buffers = vec![BufferSlot::new(TextBuffer::new())];
    restored.restore_session_from(&session_dir);
    assert_eq!(order(&restored), ["third.txt", "first.txt", "second.txt"]);
    assert_eq!(restored.active_buf, 0);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_double_click_file_tab_copies_only_the_file_name() {
    let ctx = egui::Context::default();
    let mut app = make_test_app();
    app.cur_buf_mut().file_path = None;
    let mut file = TextBuffer::new();
    let name = "議事録 final.md";
    file.file_path = Some(PathBuf::from("project").join(name));
    file.dirty = true;
    let mut slot = BufferSlot::new(file);
    slot.markdown_view = true;
    app.buffers.push(slot);

    let frame = |app: &mut VzEditorApp, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 680.0))),
                events,
                ..Default::default()
            },
            |ctx| app.render_buffer_strip(ctx),
        )
    };
    let output = frame(&mut app, vec![]);
    let tab_center = |label: &str| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().contains(label) => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap()
    };
    let click = |app: &mut VzEditorApp, pos| {
        let button = |pressed| Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(app, vec![Event::PointerMoved(pos), button(true)]);
        frame(app, vec![button(false)]);
    };

    let file_tab = tab_center("議事録 final.md*[V]");
    click(&mut app, file_tab);
    assert_eq!(app.active_buf, 1);
    assert!(app.clipboard_out.is_none());
    click(&mut app, file_tab);
    assert_eq!(app.clipboard_out.take().as_deref(), Some(name));
    assert_eq!(app.internal_clipboard, name);

    let untitled_tab = tab_center("[無題]");
    click(&mut app, untitled_tab);
    click(&mut app, untitled_tab);
    assert_eq!(app.active_buf, 0);
    assert!(app.clipboard_out.is_none());
    assert_eq!(app.internal_clipboard, name);
}

#[test]
fn test_buffer_strip_and_browser_screen_render_headless() {
    let mut app = make_test_app();
    app.toggle_browser();
    let ctx = egui::Context::default();
    let raw_input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 680.0))),
        ..Default::default()
    };
    let _ = ctx.run(raw_input, |ctx| {
        app.render_status_bar(ctx);
        app.render_buffer_strip(ctx);
        app.render_browser_canvas(ctx);
    });
    // ブラウザの領域がツールバーの下に決まる
    let rect = app.browser_view_rect.expect("view rect");
    assert!(rect.min.y > 24.0 && rect.width() > 500.0, "{rect:?}");
}

#[test]
fn test_session_restores_filer_directories_and_view() {
    let dir = std::env::temp_dir().join(format!("vz_filer_session_{}", std::process::id()));
    let session_dir = dir.join("session");
    let left = dir.join("左 pane");
    let right = dir.join("右 pane");
    std::fs::create_dir_all(&left).unwrap();
    std::fs::create_dir_all(&right).unwrap();

    let mut app = make_test_app();
    app.filer.left.enter_dir(left.clone()).unwrap();
    app.filer.right.enter_dir(right.clone()).unwrap();
    app.enter_filer();
    app.save_session_to(&session_dir).unwrap();

    let mut restored = make_test_app();
    let initial_right = restored.filer.right.current_dir.clone();
    restored.restore_session_from(&session_dir);
    assert_eq!(restored.mode, AppMode::Filer);
    assert_eq!(
        restored.filer.left.current_dir,
        left.canonicalize().unwrap()
    );
    assert_eq!(
        restored.filer.right.current_dir,
        right.canonicalize().unwrap()
    );

    app.enter_editor();
    app.save_session_to(&session_dir).unwrap();
    std::fs::remove_dir(&right).unwrap();

    let mut restored = make_test_app();
    restored.restore_session_from(&session_dir);
    assert_eq!(restored.mode, AppMode::Editor);
    restored.enter_filer();
    assert_eq!(
        restored.filer.left.current_dir,
        left.canonicalize().unwrap()
    );
    assert_eq!(restored.filer.right.current_dir, initial_right);

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_session_restores_browser_and_terminal() {
    let dir = std::env::temp_dir().join(format!("vz_tool_session_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let session_dir = dir.join("session");

    // ブラウザを表示したまま保存して終了
    let mut app = make_test_app();
    app.toggle_browser();
    app.browsers[0].url_input = "localhost:5173/app".to_string();
    app.navigate_browser_input();
    app.save_session_to(&session_dir).unwrap();

    let mut app2 = make_test_app();
    app2.restore_session_from(&session_dir);
    assert_eq!(app2.mode, AppMode::Browser);
    assert_eq!(app2.browsers.len(), 1);
    assert_eq!(
        app2.browsers[0].pending_url.as_deref(),
        Some("http://localhost:5173/app")
    );
    assert_eq!(app2.return_mode, AppMode::Editor);

    // ターミナルは起動したフォルダで、最初のフレームに起動し直す（フォルダが無ければ戻さない）
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let entries = vec![
        tool_session_entry(EntryKind::Terminal, true, Some(work.clone())),
        tool_session_entry(EntryKind::Browser, false, None),
    ];
    session::append_session(&session_dir, &entries).unwrap();
    let mut app3 = make_test_app();
    app3.restore_session_from(&session_dir);
    assert_eq!(app3.mode, AppMode::Terminal);
    assert_eq!(app3.pending_terminal_cwd.as_deref(), Some(work.as_path()));
    assert_eq!(app3.browsers.len(), 1);
    assert_eq!(app3.browsers[0].pending_url, None);

    let gone = dir.join("gone");
    session::append_session(
        &session_dir,
        &[tool_session_entry(EntryKind::Terminal, true, Some(gone))],
    )
    .unwrap();
    let mut app4 = make_test_app();
    app4.restore_session_from(&session_dir);
    assert_eq!(app4.mode, AppMode::Editor);
    assert_eq!(app4.pending_terminal_cwd, None);

    let _ = std::fs::remove_dir_all(&dir);
}

fn tool_session_entry(kind: EntryKind, active: bool, path: Option<PathBuf>) -> SessionEntry {
    SessionEntry {
        kind,
        line: 0,
        col: 0,
        markdown_view: false,
        active,
        crlf: false,
        temp: None,
        path,
    }
}

#[test]
fn test_terminal_closes_when_shell_exits() {
    let shell = if cfg!(windows) {
        vz_terminal::ShellCommand {
            program: "cmd.exe".into(),
            args: vec!["/c".into(), "exit 0".into()],
        }
    } else {
        vz_terminal::ShellCommand {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "exit 0".into()],
        }
    };
    let mut app = make_test_app();
    app.terminal = Some(
        vz_terminal::Terminal::spawn(&shell, &std::env::temp_dir(), 24, 80, || {}).expect("spawn"),
    );
    app.mode = AppMode::Terminal;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.terminal.is_some() && std::time::Instant::now() < deadline {
        app.poll_terminal();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // ターミナルを閉じて（タブも消え）エディターへ戻る
    assert!(app.terminal.is_none());
    assert_eq!(app.mode, AppMode::Editor);
    assert!(app.status_message.contains("ターミナルを閉じました"));
}

#[test]
fn light_system_theme_keeps_dialog_controls_readable() {
    fn has_white_text(shape: &egui::Shape, label: &str) -> bool {
        match shape {
            egui::Shape::Text(text) => {
                text.galley.text() == label
                    && text.override_text_color.unwrap_or(text.fallback_color) == TEXT_WHITE
            }
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| has_white_text(shape, label)),
            _ => false,
        }
    }

    let mut app = make_test_app();
    let ctx = egui::Context::default();
    VzEditorApp::configure_visuals(&ctx);
    ctx.all_styles_mut(|style| style.animation_time = 0.0);
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 680.0))),
        system_theme: Some(egui::Theme::Light),
        ..Default::default()
    };

    for (dialog, labels) in [
        (SearchDialog::Find, ["検索条件", "正規表現", "下検索"]),
        (SearchDialog::Replace, ["置換後", "正規表現", "すべて置換"]),
        (
            SearchDialog::Grep,
            ["検索場所", "サブフォルダーも検索", "該当行"],
        ),
        (SearchDialog::Jump, ["行番号", "改行単位", "ジャンプ"]),
    ] {
        app.open_search_dialog(dialog);
        let _ = ctx.run(input.clone(), |ctx| app.render_search_dialog(ctx));
        let frame = ctx.run(input.clone(), |ctx| app.render_search_dialog(ctx));
        for label in labels {
            assert!(
                frame.shapes.iter().any(|s| has_white_text(&s.shape, label)),
                "{dialog:?}: {label} needs white text under the light system theme"
            );
        }
        app.search.close();
    }

    app.inline_prompt = Some(InlinePathPrompt {
        is_save: false,
        path_input: "sample.txt".into(),
        just_opened: true,
    });
    let _ = ctx.run(input.clone(), |ctx| app.render_inline_path_prompt(ctx));
    let frame = ctx.run(input, |ctx| app.render_inline_path_prompt(ctx));
    for label in ["sample.txt", "実行 (Enter)", "キャンセル (ESC)"] {
        assert!(
            frame.shapes.iter().any(|s| has_white_text(&s.shape, label)),
            "File dialog: {label} needs white text under the light system theme"
        );
    }
}
