//! Editor positions: line + column, with the column counted in whichever unit
//! the editor and the language server agreed on.
//!
//! A [`crate::Span`] is a pair of UTF-8 **byte** offsets. Editors address text
//! as a zero-based line plus a column, and the LSP lets the two ends negotiate
//! what a column counts (JUX-LSP-SERVER-ADDENDUM §L.8): UTF-8 bytes, or the
//! protocol's default of UTF-16 code units. Both conversions live here, in the
//! crate every other one already depends on, so the language server does not
//! grow its own copy and a future tool (a formatter, a playground) reuses the
//! same arithmetic.
//!
//! Two layers:
//!
//! - **Per line** ([`byte_to_col`], [`col_to_byte`]): the column of a byte
//!   offset within one line's text, and back. Callers that already hold the
//!   line (a rope slice, say) use these directly and never materialise the
//!   whole file.
//! - **Whole text** ([`LineIndex`]): a line-start table over one string, for
//!   callers holding the full text.
//!
//! Both clamp rather than panic: a column past the end of a line lands on the
//! line end, and a column that falls inside a character (half of a UTF-16
//! surrogate pair, or the middle of a multi-byte UTF-8 sequence) lands on that
//! character's start. An editor can send a stale position after the text
//! changed under it, and a language server must survive that.

/// What an editor column counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PositionEncoding {
    /// UTF-8 code units, which are bytes: the column is the byte distance from
    /// the line start. What a [`crate::Span`] already stores, so conversion is
    /// a subtraction.
    Utf8,
    /// UTF-16 code units: the LSP default, and what editors that have not
    /// adopted the negotiation assume. A character outside the Basic
    /// Multilingual Plane (an emoji) is two units.
    #[default]
    Utf16,
}

impl PositionEncoding {
    /// How many columns `ch` occupies in this encoding.
    pub fn width(self, ch: char) -> u32 {
        match self {
            PositionEncoding::Utf8 => ch.len_utf8() as u32,
            PositionEncoding::Utf16 => ch.len_utf16() as u32,
        }
    }
}

/// The column of byte offset `byte` within `line` (the line's own text, with
/// or without its trailing newline). `byte` counts from the line start and is
/// clamped to the line's length; an offset inside a character counts the
/// characters before it only.
pub fn byte_to_col(line: impl IntoIterator<Item = char>, byte: usize, enc: PositionEncoding) -> u32 {
    let mut seen_bytes = 0usize;
    let mut col = 0u32;
    for ch in line {
        let next = seen_bytes + ch.len_utf8();
        if next > byte {
            break;
        }
        seen_bytes = next;
        col += enc.width(ch);
    }
    col
}

/// The byte offset, from the line start, of column `col` within `line`.
///
/// Stops at a newline, so a column past the end of the line lands on the line
/// end rather than on the next line. A column inside a character lands on
/// that character's first byte.
pub fn col_to_byte(line: impl IntoIterator<Item = char>, col: u32, enc: PositionEncoding) -> usize {
    let mut remaining = col;
    let mut byte = 0usize;
    for ch in line {
        if remaining == 0 || ch == '\n' || ch == '\r' {
            break;
        }
        let w = enc.width(ch);
        if w > remaining {
            break;
        }
        remaining -= w;
        byte += ch.len_utf8();
    }
    byte
}

/// A line-start table over one text, for callers that hold the whole string.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of each line's first byte. `starts[0]` is always `0`.
    starts: Vec<usize>,
    /// Total length in bytes, the clamp for every offset.
    len: usize,
}

impl LineIndex {
    /// Index `text`'s lines. A `\n` ends a line; a `\r` before it stays part of
    /// the line's text, which is what editors expect of a CRLF file.
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0usize];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        LineIndex { starts, len: text.len() }
    }

    /// Number of lines (a text ending in `\n` has an empty last line).
    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    /// Zero-based `(line, column)` of byte `offset`, clamped to the text.
    pub fn line_col(&self, text: &str, offset: usize, enc: PositionEncoding) -> (u32, u32) {
        let offset = offset.min(self.len);
        let line = match self.starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let start = self.starts[line];
        let col = byte_to_col(text[start..].chars(), offset - start, enc);
        (line as u32, col)
    }

    /// Byte offset of zero-based `(line, col)`, clamped: a line past the end
    /// is the text's end, a column past the line's end is that line's end.
    pub fn offset(&self, text: &str, line: u32, col: u32, enc: PositionEncoding) -> usize {
        let Some(&start) = self.starts.get(line as usize) else {
            return self.len;
        };
        start + col_to_byte(text[start..].chars(), col, enc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character boundary survives a round trip in both encodings.
    fn round_trips(text: &str) {
        let index = LineIndex::new(text);
        for enc in [PositionEncoding::Utf8, PositionEncoding::Utf16] {
            for offset in 0..=text.len() {
                if !text.is_char_boundary(offset) {
                    continue;
                }
                // Between the `\r` and `\n` of a CRLF is no editor column: a
                // column there clamps to the line end, by design.
                if offset > 0 && text.as_bytes()[offset - 1] == b'\r' {
                    continue;
                }
                let (line, col) = index.line_col(text, offset, enc);
                assert_eq!(
                    index.offset(text, line, col, enc),
                    offset,
                    "{enc:?}: offset {offset} of {text:?} did not round-trip via ({line}, {col})",
                );
            }
        }
    }

    #[test]
    fn ascii_multibyte_and_astral_text_round_trips() {
        round_trips("public void main() {\n    print(\"hi\");\n}\n");
        round_trips("var s = \"caf\u{e9} \u{6f22}\u{5b57}\";\nvar t = 1;\n");
        round_trips("var s = \"a\u{1F600}b\";\r\nvar t = 2;\r\n");
        round_trips("");
        round_trips("\n");
    }

    /// The two encodings disagree exactly on non-ASCII text: an emoji is four
    /// bytes and two UTF-16 units.
    #[test]
    fn utf8_counts_bytes_and_utf16_counts_units() {
        let text = "\u{1F600}x";
        let index = LineIndex::new(text);
        assert_eq!(index.line_col(text, 4, PositionEncoding::Utf8), (0, 4));
        assert_eq!(index.line_col(text, 4, PositionEncoding::Utf16), (0, 2));
        assert_eq!(index.offset(text, 0, 4, PositionEncoding::Utf8), 4);
        assert_eq!(index.offset(text, 0, 2, PositionEncoding::Utf16), 4);
    }

    /// A column inside a character lands on its start; never mid-character.
    #[test]
    fn a_column_inside_a_character_clamps_to_its_start() {
        let text = "\u{1F600}x";
        let index = LineIndex::new(text);
        assert_eq!(index.offset(text, 0, 1, PositionEncoding::Utf16), 0);
        assert_eq!(index.offset(text, 0, 2, PositionEncoding::Utf8), 0);
    }

    /// Stale positions clamp instead of panicking or bleeding onto the next line.
    #[test]
    fn out_of_range_positions_clamp() {
        let text = "abc\ndef";
        let index = LineIndex::new(text);
        assert_eq!(index.offset(text, 0, 99, PositionEncoding::Utf16), 3);
        assert_eq!(index.offset(text, 9, 0, PositionEncoding::Utf16), text.len());
        assert_eq!(index.line_col(text, 999, PositionEncoding::Utf8), (1, 3));
    }

    /// A CRLF line's column never counts the `\r` as a place to stop past.
    #[test]
    fn crlf_line_end_is_not_crossed() {
        let text = "ab\r\ncd";
        let index = LineIndex::new(text);
        assert_eq!(index.offset(text, 0, 50, PositionEncoding::Utf8), 2);
    }
}
