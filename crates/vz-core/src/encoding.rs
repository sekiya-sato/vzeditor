//! 文字コード（UTF-8・UTF-8 BOM・Shift_JIS）の判定・デコード・エンコード。

use encoding_rs::{CoderResult, Decoder, Encoder, SHIFT_JIS};
use std::borrow::Cow;
use std::fmt;

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
/// 巨大ファイルで文字コードの判定に使う先頭の大きさ
pub const DETECT_SAMPLE: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextEncoding {
    #[default]
    Utf8,
    Utf8Bom,
    ShiftJis,
}

/// エンコードできない文字（`index` は文字列内の文字位置）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeError {
    pub ch: char,
    pub index: usize,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "「{}」を保存できません", self.ch)
    }
}

impl std::error::Error for EncodeError {}

impl TextEncoding {
    pub const ALL: [Self; 3] = [Self::Utf8, Self::Utf8Bom, Self::ShiftJis];

    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 BOM",
            Self::ShiftJis => "Shift_JIS",
        }
    }

    /// 設定やセッションに保存した名前から戻す。
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.label() == label)
    }

    pub fn bom(self) -> &'static [u8] {
        match self {
            Self::Utf8Bom => UTF8_BOM,
            _ => b"",
        }
    }

    /// BOM → UTF-8 として正しい → それ以外は Shift_JIS。
    /// `truncated` なら `bytes` はファイルの先頭部分で、末尾で切れた文字は許容する。
    pub fn detect(bytes: &[u8], truncated: bool) -> Self {
        if bytes.starts_with(UTF8_BOM) {
            return Self::Utf8Bom;
        }
        match std::str::from_utf8(bytes) {
            Ok(_) => Self::Utf8,
            // error_len が None なら末尾で文字が切れているだけ
            Err(e) if truncated && e.error_len().is_none() => Self::Utf8,
            Err(_) => Self::ShiftJis,
        }
    }

    /// BOM を除いて UTF-8 の文字列にする（不正なバイトは U+FFFD）。
    pub fn decode(self, bytes: &[u8]) -> Cow<'_, str> {
        self.decode_body(bytes.strip_prefix(self.bom()).unwrap_or(bytes))
    }

    /// 本文の一部（BOM を含まない）を UTF-8 の文字列にする。
    pub fn decode_body(self, bytes: &[u8]) -> Cow<'_, str> {
        match self {
            Self::Utf8 | Self::Utf8Bom => String::from_utf8_lossy(bytes),
            Self::ShiftJis => SHIFT_JIS.decode_without_bom_handling(bytes).0,
        }
    }

    /// 本文をこの文字コードのバイト列にする（BOM は付けない）。
    pub fn encode(self, text: &str) -> Result<Cow<'_, [u8]>, EncodeError> {
        match self {
            Self::Utf8 | Self::Utf8Bom => Ok(Cow::Borrowed(text.as_bytes())),
            Self::ShiftJis => {
                let (bytes, _, had_errors) = SHIFT_JIS.encode(text);
                if had_errors {
                    return Err(first_unencodable(text).unwrap_or(EncodeError {
                        ch: '\u{FFFD}',
                        index: 0,
                    }));
                }
                Ok(bytes)
            }
        }
    }
}

impl fmt::Display for TextEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Shift_JIS で表せない最初の文字。
fn first_unencodable(text: &str) -> Option<EncodeError> {
    let mut buf = [0u8; 4];
    text.chars().enumerate().find_map(|(index, ch)| {
        let s = ch.encode_utf8(&mut buf);
        SHIFT_JIS.encode(s).2.then_some(EncodeError { ch, index })
    })
}

/// 文字コードを変えながら順に書き出す（巨大ファイルの保存で使う）。
/// 入力を細切れに渡しても、文字の途中で切れた分は次に回す。
pub struct Transcoder {
    from: TextEncoding,
    to: TextEncoding,
    decoder: Option<Decoder>,
    encoder: Option<Encoder>,
    text: String,
    out: Vec<u8>,
    /// UTF-8 → UTF-8 で、文字の途中で切れた分
    pending: Vec<u8>,
    decoded_chars: usize,
}

impl Transcoder {
    pub fn new(from: TextEncoding, to: TextEncoding) -> Self {
        let decoder =
            (from == TextEncoding::ShiftJis).then(|| SHIFT_JIS.new_decoder_without_bom_handling());
        let encoder = (to == TextEncoding::ShiftJis).then(|| SHIFT_JIS.new_encoder());
        Self {
            from,
            to,
            decoder,
            encoder,
            text: String::new(),
            out: Vec::new(),
            pending: Vec::new(),
            decoded_chars: 0,
        }
    }

    /// 同じ系統（UTF-8 同士、Shift_JIS 同士）ならバイト列をそのまま写せる。
    pub fn is_passthrough(&self) -> bool {
        (self.from == TextEncoding::ShiftJis) == (self.to == TextEncoding::ShiftJis)
    }

    /// 本文の一部を変換して返す。`last` は最後の呼び出し。
    pub fn push(&mut self, input: &[u8], last: bool) -> Result<&[u8], EncodeError> {
        self.out.clear();
        if self.is_passthrough() {
            self.out.extend_from_slice(input);
            return Ok(&self.out);
        }
        self.text.clear();
        match &mut self.decoder {
            Some(decoder) => {
                let need = decoder
                    .max_utf8_buffer_length(input.len())
                    .unwrap_or(input.len() * 3 + 16);
                self.text.reserve(need);
                let (result, _, _) = decoder.decode_to_string(input, &mut self.text, last);
                debug_assert_eq!(result, CoderResult::InputEmpty);
            }
            None => {
                self.pending.extend_from_slice(input);
                let valid = match std::str::from_utf8(&self.pending) {
                    Ok(_) => self.pending.len(),
                    Err(e) if e.error_len().is_none() && !last => e.valid_up_to(),
                    Err(_) => self.pending.len(),
                };
                self.text
                    .push_str(&String::from_utf8_lossy(&self.pending[..valid]));
                self.pending.drain(..valid);
            }
        }
        match &mut self.encoder {
            Some(encoder) => {
                let need = encoder
                    .max_buffer_length_from_utf8_without_replacement(self.text.len())
                    .unwrap_or(self.text.len() * 2 + 16);
                self.out.reserve(need);
                let (result, read) = encoder.encode_from_utf8_to_vec_without_replacement(
                    &self.text,
                    &mut self.out,
                    last,
                );
                if let encoding_rs::EncoderResult::Unmappable(ch) = result {
                    let index = self.decoded_chars + self.text[..read].chars().count() - 1;
                    return Err(EncodeError { ch, index });
                }
            }
            None => self.out.extend_from_slice(self.text.as_bytes()),
        }
        self.decoded_chars += self.text.chars().count();
        Ok(&self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_decode_encode() {
        let sjis = SHIFT_JIS.encode("一般書籍 ホビー").0.into_owned();
        assert_eq!(TextEncoding::detect(&sjis, false), TextEncoding::ShiftJis);
        assert_eq!(
            TextEncoding::detect("あ".as_bytes(), false),
            TextEncoding::Utf8
        );
        assert_eq!(
            TextEncoding::detect(b"\xEF\xBB\xBFabc", false),
            TextEncoding::Utf8Bom
        );
        // 先頭部分だけで判定するとき、末尾で切れた UTF-8 は UTF-8 のまま
        let cut = &"あい".as_bytes()[..4];
        assert_eq!(TextEncoding::detect(cut, true), TextEncoding::Utf8);
        assert_eq!(TextEncoding::detect(cut, false), TextEncoding::ShiftJis);

        assert_eq!(TextEncoding::ShiftJis.decode(&sjis), "一般書籍 ホビー");
        assert_eq!(TextEncoding::Utf8Bom.decode(b"\xEF\xBB\xBFabc"), "abc");
        assert_eq!(
            TextEncoding::ShiftJis
                .encode("一般書籍 ホビー")
                .unwrap()
                .as_ref(),
            &sjis[..]
        );
        let err = TextEncoding::ShiftJis.encode("ab😀c").unwrap_err();
        assert_eq!((err.ch, err.index), ('😀', 2));
        assert_eq!(
            TextEncoding::from_label("Shift_JIS"),
            Some(TextEncoding::ShiftJis)
        );
    }

    #[test]
    fn test_transcoder_matches_whole_conversion() {
        let text = "行1 あいう\r\n行2 漢字テスト ｶﾅ\n".repeat(50);
        let sjis = SHIFT_JIS.encode(&text).0.into_owned();
        for (from, input, to, expected) in [
            (
                TextEncoding::ShiftJis,
                sjis.clone(),
                TextEncoding::Utf8,
                text.as_bytes().to_vec(),
            ),
            (
                TextEncoding::Utf8,
                text.as_bytes().to_vec(),
                TextEncoding::ShiftJis,
                sjis.clone(),
            ),
            (
                TextEncoding::Utf8,
                text.as_bytes().to_vec(),
                TextEncoding::Utf8Bom,
                text.as_bytes().to_vec(),
            ),
        ] {
            let mut t = Transcoder::new(from, to);
            let mut out = Vec::new();
            // 文字の途中で切れるように 7 バイトずつ渡す
            let chunks: Vec<&[u8]> = input.chunks(7).collect();
            for (i, chunk) in chunks.iter().enumerate() {
                out.extend_from_slice(t.push(chunk, i + 1 == chunks.len()).unwrap());
            }
            assert_eq!(out, expected, "{from} → {to}");
        }
        let mut t = Transcoder::new(TextEncoding::Utf8, TextEncoding::ShiftJis);
        let err = t.push("abc😀".as_bytes(), true).unwrap_err();
        assert_eq!((err.ch, err.index), ('😀', 3));
    }
}
