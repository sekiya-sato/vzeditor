//! 印刷: 割付済みの文書（`vz_core::print::PrintDoc`）を OS の既定プリンターへ送る。
//!
//! - Windows: GDI（DEVMODE で A4・向きを指定し、ExtTextOutW で升目に置く）
//! - macOS / Linux: PDF を作り（日本語フォントは使った文字だけ埋め込む）、`lp`（CUPS）へ渡す

use std::sync::mpsc::{self, Receiver};
use vz_core::print::PrintDoc;

#[cfg(any(not(windows), test))]
mod pdf;

/// 成功時は結果の文言、失敗時は理由。
pub type PrintResult = Result<String, String>;

/// 既定のプリンター名。無ければ None。
pub fn default_printer() -> Option<String> {
    imp::default_printer()
}

/// 裏のスレッドで印刷する。
pub fn spawn_print(doc: PrintDoc) -> Receiver<PrintResult> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let pages = doc.pages.len();
        let result =
            imp::print(&doc).map(|printer| format!("{printer} へ {pages} ページを印刷しました"));
        let _ = tx.send(result);
    });
    rx
}

#[cfg(windows)]
mod imp {
    use std::path::Path;
    use std::ptr::{null, null_mut};
    use vz_core::print::{char_cells, PrintDoc, HEADER_FONT_PT};
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateDCW, CreateFontW, DeleteDC, DeleteObject, ExtTextOutW, GetDeviceCaps, SelectObject,
        SetTextAlign, DEFAULT_CHARSET, DEVMODEW, DMORIENT_LANDSCAPE, DMORIENT_PORTRAIT, DMPAPER_A4,
        DM_IN_BUFFER, DM_ORIENTATION, DM_OUT_BUFFER, DM_PAPERSIZE, FIXED_PITCH, FW_NORMAL, HDC,
        HFONT, HGDIOBJ, LOGPIXELSX, LOGPIXELSY, PHYSICALOFFSETX, PHYSICALOFFSETY, TA_BASELINE,
    };
    use windows_sys::Win32::Graphics::Printing::{
        ClosePrinter, DocumentPropertiesW, GetDefaultPrinterW, OpenPrinterW,
    };
    use windows_sys::Win32::Storage::Xps::{
        AbortDoc, EndDoc, EndPage, StartDocW, StartPage, DOCINFOW,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn default_printer() -> Option<String> {
        let mut len = 0u32;
        // SAFETY: 長さの問い合わせ（バッファ無し）
        unsafe { GetDefaultPrinterW(null_mut(), &mut len) };
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize];
        // SAFETY: buf は len 文字分ある
        if unsafe { GetDefaultPrinterW(buf.as_mut_ptr(), &mut len) } == 0 {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }

    pub fn print(doc: &PrintDoc) -> Result<String, String> {
        let printer = default_printer().ok_or("既定のプリンターがありません")?;
        print_to(&printer, doc, None)?;
        Ok(printer)
    }

    /// プリンターへ印刷する。output を渡すとファイルへ書き出す（Microsoft Print to PDF の確認用）。
    pub fn print_to(printer: &str, doc: &PrintDoc, output: Option<&Path>) -> Result<(), String> {
        let name = wide(printer);
        let devmode = devmode_for(&name, doc.setup.orientation)?;
        let driver = wide("WINSPOOL");
        // SAFETY: 文字列は NUL 終端、devmode は DocumentPropertiesW が埋めた DEVMODEW
        let hdc = unsafe {
            CreateDCW(
                driver.as_ptr(),
                name.as_ptr(),
                null(),
                devmode.as_ptr() as *const DEVMODEW,
            )
        };
        if hdc.is_null() {
            return Err(format!("{printer} を開けません"));
        }
        let dc = Dc(hdc);
        dc.print_doc(doc, output)
    }

    /// プリンターの DEVMODE を取り、A4 と向きを設定する（DEVMODEW の境界に合わせて u64 で持つ）。
    fn devmode_for(
        name: &[u16],
        orientation: vz_core::print::Orientation,
    ) -> Result<Vec<u64>, String> {
        let mut handle: HANDLE = null_mut();
        // SAFETY: name は NUL 終端
        if unsafe { OpenPrinterW(name.as_ptr(), &mut handle, null()) } == 0 {
            return Err("プリンターを開けません".into());
        }
        // SAFETY: handle は OpenPrinterW が返した有効なハンドル。バッファは size バイト以上ある
        let result = unsafe {
            let size =
                DocumentPropertiesW(null_mut(), handle, name.as_ptr(), null_mut(), null(), 0);
            if size <= 0 {
                Err("プリンターの設定を読めません".to_string())
            } else {
                let mut buf = vec![0u64; (size as usize).div_ceil(8)];
                let dm = buf.as_mut_ptr() as *mut DEVMODEW;
                if DocumentPropertiesW(null_mut(), handle, name.as_ptr(), dm, null(), DM_OUT_BUFFER)
                    < 0
                {
                    Err("プリンターの設定を読めません".to_string())
                } else {
                    (*dm).dmFields |= DM_ORIENTATION | DM_PAPERSIZE;
                    let dm0 = &mut (*dm).Anonymous1.Anonymous1;
                    dm0.dmOrientation = match orientation {
                        vz_core::print::Orientation::Portrait => DMORIENT_PORTRAIT,
                        vz_core::print::Orientation::Landscape => DMORIENT_LANDSCAPE,
                    } as i16;
                    dm0.dmPaperSize = DMPAPER_A4 as i16;
                    DocumentPropertiesW(
                        null_mut(),
                        handle,
                        name.as_ptr(),
                        dm,
                        dm,
                        DM_IN_BUFFER | DM_OUT_BUFFER,
                    );
                    Ok(buf)
                }
            }
        };
        // SAFETY: 開いたハンドルを閉じる
        unsafe { ClosePrinter(handle) };
        result
    }

    /// 印刷用の DC（Drop で削除する）。
    struct Dc(HDC);

    impl Drop for Dc {
        fn drop(&mut self) {
            // SAFETY: CreateDCW が返した DC
            unsafe { DeleteDC(self.0) };
        }
    }

    /// GDI のフォント（Drop で削除する）。
    struct Font(HFONT);

    impl Drop for Font {
        fn drop(&mut self) {
            // SAFETY: CreateFontW が返したフォント（選択を外してから Drop する）
            unsafe { DeleteObject(self.0) };
        }
    }

    impl Dc {
        fn caps(&self, index: u32) -> f32 {
            // SAFETY: 有効な DC
            unsafe { GetDeviceCaps(self.0, index as i32) as f32 }
        }

        fn font(&self, size_pt: f32, dpi_y: f32, face: &[u16]) -> Result<Font, String> {
            let height = -((size_pt * dpi_y / 72.0).round() as i32);
            // SAFETY: face は NUL 終端
            let font = unsafe {
                CreateFontW(
                    height,
                    0,
                    0,
                    0,
                    FW_NORMAL as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    0,
                    0,
                    0,
                    FIXED_PITCH as u32,
                    face.as_ptr(),
                )
            };
            if font.is_null() {
                return Err("印刷用のフォントを作れません".into());
            }
            Ok(Font(font))
        }

        fn print_doc(&self, doc: &PrintDoc, output: Option<&Path>) -> Result<(), String> {
            let (dpi_x, dpi_y) = (self.caps(LOGPIXELSX), self.caps(LOGPIXELSY));
            let (off_x, off_y) = (self.caps(PHYSICALOFFSETX), self.caps(PHYSICALOFFSETY));
            let to_x = |pt: f32| pt * dpi_x / 72.0 - off_x;
            let to_y = |pt: f32| (pt * dpi_y / 72.0 - off_y).round() as i32;

            let face = wide(face_name());
            let body = self.font(doc.setup.font_size_pt(), dpi_y, &face)?;
            let header = self.font(HEADER_FONT_PT, dpi_y, &face)?;

            let title = wide(&doc.title);
            let output_w = output.map(|p| wide(&p.to_string_lossy()));
            let info = DOCINFOW {
                cbSize: std::mem::size_of::<DOCINFOW>() as i32,
                lpszDocName: title.as_ptr(),
                lpszOutput: output_w.as_ref().map_or(null(), |w| w.as_ptr()),
                lpszDatatype: null(),
                fwType: 0,
            };
            // SAFETY: 有効な DC と DOCINFOW（文字列は NUL 終端で、この関数の間は生きている）
            unsafe {
                SetTextAlign(self.0, TA_BASELINE);
                if StartDocW(self.0, &info) <= 0 {
                    return Err("印刷を開始できませんでした（取り消した場合を含む）".into());
                }
            }
            let original = self.select(&header);
            let result = self.render_pages(doc, &body, &header, &to_x, &to_y);
            // 選択中のフォントは削除できないので元に戻してから Font を Drop する
            // SAFETY: 元の GDI オブジェクトを選択し直す
            unsafe { SelectObject(self.0, original) };
            match result {
                // SAFETY: StartDocW 済み
                Ok(()) if unsafe { EndDoc(self.0) } > 0 => Ok(()),
                Ok(()) => Err("印刷を完了できませんでした".into()),
                Err(e) => {
                    // SAFETY: 印刷を中止する
                    unsafe { AbortDoc(self.0) };
                    Err(e)
                }
            }
        }

        fn render_pages(
            &self,
            doc: &PrintDoc,
            body: &Font,
            header: &Font,
            to_x: &dyn Fn(f32) -> f32,
            to_y: &dyn Fn(f32) -> i32,
        ) -> Result<(), String> {
            let dpi_x = self.caps(LOGPIXELSX);
            let body_cell = doc.setup.cell_pt() * dpi_x / 72.0;
            let header_cell = HEADER_FONT_PT * 0.5 * dpi_x / 72.0;
            let (num_x, text_x) = doc.body_x();
            for (idx, page) in doc.pages.iter().enumerate() {
                // SAFETY: StartDocW 済みの DC
                if unsafe { StartPage(self.0) } <= 0 {
                    return Err("ページを開始できませんでした".into());
                }
                self.select(header);
                let hy = to_y(doc.setup.header_baseline_pt());
                for (x, text) in doc.header_items(idx) {
                    self.text_out(to_x(x), hy, &text, header_cell);
                }
                self.select(body);
                for (row, line) in page.iter().enumerate() {
                    let y = to_y(doc.setup.row_baseline_pt(row));
                    self.text_out(to_x(num_x), y, &doc.number_label(line), body_cell);
                    self.text_out(to_x(text_x), y, &line.text, body_cell);
                }
                // SAFETY: StartPage 済み
                if unsafe { EndPage(self.0) } <= 0 {
                    return Err("ページを出力できませんでした".into());
                }
            }
            Ok(())
        }

        /// フォントを選び、それまで選ばれていたオブジェクトを返す。
        fn select(&self, font: &Font) -> HGDIOBJ {
            // SAFETY: 有効な DC とフォント
            unsafe { SelectObject(self.0, font.0) }
        }

        /// 文字を升目（半角 = cell、全角 = 2 cell）に置く。
        fn text_out(&self, x: f32, y: i32, text: &str, cell: f32) {
            if text.is_empty() {
                return;
            }
            let mut units: Vec<u16> = Vec::with_capacity(text.len());
            let mut dx: Vec<i32> = Vec::with_capacity(text.len());
            let mut cells = 0usize;
            let mut prev = 0i32;
            for c in text.chars() {
                cells += char_cells(c);
                let pos = (cells as f32 * cell).round() as i32;
                let mut buf = [0u16; 2];
                let encoded = c.encode_utf16(&mut buf);
                units.extend_from_slice(encoded);
                dx.push(pos - prev);
                if encoded.len() == 2 {
                    dx.push(0);
                }
                prev = pos;
            }
            // SAFETY: units と dx は同じ長さ
            unsafe {
                ExtTextOutW(
                    self.0,
                    x.round() as i32,
                    y,
                    0,
                    null(),
                    units.as_ptr(),
                    units.len() as u32,
                    dx.as_ptr(),
                )
            };
        }
    }

    /// 画面と同じ優先順で、入っている日本語の等幅フォントの書体名を選ぶ。
    fn face_name() -> &'static str {
        for (path, _) in crate::fonts::japanese_font_candidates() {
            let file = path
                .file_name()
                .map(|f| f.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let face = match file.as_str() {
                "biz-udgothicr.ttc" => "BIZ UDGothic",
                "hackgen-regular.ttf" => "HackGen",
                "plemoljp-regular.ttf" => "PlemolJP",
                "msgothic.ttc" => "MS Gothic",
                "yugothm.ttc" => "Yu Gothic",
                "meiryo.ttc" => "Meiryo",
                _ => continue,
            };
            return face;
        }
        "MS Gothic"
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use vz_core::print::{layout, Orientation, PageSetup};

        /// 実機確認用: Microsoft Print to PDF で縦・横の PDF を書き出す。
        /// `cargo test -p vz-platform print_to_pdf_printer -- --ignored` で実行し、
        /// 出力先（一時フォルダ）の PDF を目で確かめる。
        #[test]
        #[ignore]
        fn print_to_pdf_printer() {
            let dir = std::env::temp_dir().join(format!("vz-print-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let lines: Vec<String> = (0..80)
                .map(|i| format!("{i}\t日本語とASCIIの混在 ｶﾅ abc{}", "x".repeat(i * 2)))
                .collect();
            for (o, name) in [
                (Orientation::Portrait, "portrait"),
                (Orientation::Landscape, "landscape"),
            ] {
                let setup = PageSetup {
                    orientation: o,
                    ..PageSetup::default()
                };
                let doc = layout(
                    &lines,
                    setup,
                    "C:\\work\\印刷テスト.txt",
                    "2026-11-27 12:00",
                )
                .unwrap();
                let out = dir.join(format!("{name}.pdf"));
                print_to("Microsoft Print to PDF", &doc, Some(&out)).unwrap();
                eprintln!("{}", out.display());
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::process::Command;
    use vz_core::print::PrintDoc;

    pub fn default_printer() -> Option<String> {
        // 表示言語に関係なく読めるよう C ロケールで問い合わせる
        let out = Command::new("lpstat")
            .arg("-d")
            .env("LC_ALL", "C")
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .find_map(|l| l.strip_prefix("system default destination:"))
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
    }

    pub fn print(doc: &PrintDoc) -> Result<String, String> {
        let printer = default_printer().ok_or("既定のプリンターがありません")?;
        let font = super::pdf::load_font().ok_or("印刷用の日本語フォントが見つかりません")?;
        let bytes = super::pdf::render(doc, font)?;
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("vzeditor-print-{}-{n}.pdf", std::process::id()));
        std::fs::write(&path, bytes).map_err(|e| format!("一時ファイルを書けません: {e}"))?;
        // 横向きは PDF のページ自体を横にしている。回転は CUPS に任せる
        let result = Command::new("lp")
            .args(["-d", &printer, "-o", "media=A4", "-t", &doc.title])
            .arg(&path)
            .output();
        // lp はファイルをスプールへ渡し終えてから戻る
        let _ = std::fs::remove_file(&path);
        match result {
            Ok(out) if out.status.success() => Ok(printer),
            Ok(out) => Err(format!(
                "lp が失敗しました: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )),
            Err(e) => Err(format!("lp を実行できません: {e}")),
        }
    }
}
