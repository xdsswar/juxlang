//! Position translation between Jux byte offsets and LSP positions.
//!
//! `juxc-source::Span` stores **UTF-8 byte offsets** (§L.8). An LSP `Position`
//! is a zero-based line plus a column whose unit the client and server
//! negotiate at `initialize` (`general.positionEncodings`): UTF-8 bytes when
//! the client offers them, else the protocol default of UTF-16 code units.
//! Every position-bearing message crosses this boundary through the helpers
//! here, which take the negotiated [`PositionEncoding`] explicitly.
//!
//! The per-line arithmetic itself lives in `juxc_source::position` (the spec
//! puts it there, beside `Span`); this module only finds the line in the rope.

use juxc_source::position::{byte_to_col, col_to_byte};
pub use juxc_source::position::PositionEncoding;
use juxc_source::Span;
use ropey::Rope;
use tower_lsp::lsp_types::{Position, PositionEncodingKind, Range};

/// The LSP name of `enc`, as advertised in `ServerCapabilities`.
pub fn encoding_kind(enc: PositionEncoding) -> PositionEncodingKind {
    match enc {
        PositionEncoding::Utf8 => PositionEncodingKind::UTF8,
        PositionEncoding::Utf16 => PositionEncodingKind::UTF16,
    }
}

/// Pick the encoding from the client's offered list (§L.8): UTF-8 when the
/// client accepts it, since it is what spans already store; UTF-16 otherwise,
/// which every client understands whether or not it negotiates.
pub fn negotiate(offered: Option<&[PositionEncodingKind]>) -> PositionEncoding {
    match offered {
        Some(list) if list.contains(&PositionEncodingKind::UTF8) => PositionEncoding::Utf8,
        _ => PositionEncoding::Utf16,
    }
}

/// Convert a UTF-8 byte offset into an LSP [`Position`] in encoding `enc`.
///
/// Offsets past EOF clamp to the document end so a stale request can never
/// panic.
pub fn offset_to_position(rope: &Rope, offset: usize, enc: PositionEncoding) -> Position {
    let offset = offset.min(rope.len_bytes());
    let line = rope.byte_to_line(offset);
    let line_start = rope.line_to_byte(line);
    let col = byte_to_col(rope.line(line).chars(), offset - line_start, enc);
    Position::new(line as u32, col)
}

/// Convert an LSP [`Position`] in encoding `enc` back into a UTF-8 byte offset.
///
/// Out-of-range lines and columns clamp to the nearest valid offset; a column
/// inside a character lands on the character's start.
pub fn position_to_offset(rope: &Rope, pos: Position, enc: PositionEncoding) -> usize {
    let last_line = rope.len_lines().saturating_sub(1);
    let line = (pos.line as usize).min(last_line);
    let line_start = rope.line_to_byte(line);
    line_start + col_to_byte(rope.line(line).chars(), pos.character, enc)
}

/// Convert a Jux [`Span`] into an LSP [`Range`] in encoding `enc`.
pub fn span_to_range(rope: &Rope, span: Span, enc: PositionEncoding) -> Range {
    Range::new(
        offset_to_position(rope, span.start as usize, enc),
        offset_to_position(rope, span.end as usize, enc),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const U16: PositionEncoding = PositionEncoding::Utf16;
    const U8: PositionEncoding = PositionEncoding::Utf8;

    /// Every offset in a document must survive the round trip, in both
    /// encodings. This module is the boundary between Jux's byte offsets and
    /// the editor's columns; an off-by-one here does not crash, it silently
    /// underlines the wrong character.
    fn round_trips(text: &str) {
        let rope = Rope::from_str(text);
        for enc in [U16, U8] {
            for offset in 0..=text.len() {
                if !text.is_char_boundary(offset) {
                    continue;
                }
                let pos = offset_to_position(&rope, offset, enc);
                assert_eq!(
                    position_to_offset(&rope, pos, enc),
                    offset,
                    "{enc:?}: offset {offset} of {text:?} did not round-trip (via {pos:?})",
                );
            }
        }
    }

    #[test]
    fn ascii_round_trips() {
        round_trips("public void main() {\n    print(\"hi\");\n}\n");
    }

    /// A multi-byte character is one UTF-16 unit but several UTF-8 bytes, so
    /// the two coordinate systems diverge exactly here.
    #[test]
    fn multibyte_round_trips() {
        round_trips("var s = \"caf\u{e9} \u{6f22}\u{5b57}\";\nvar t = 1;\n");
    }

    /// An emoji is a SURROGATE PAIR in UTF-16, two units for one character,
    /// which is the case that breaks naive column arithmetic.
    #[test]
    fn astral_plane_round_trips() {
        round_trips("var s = \"a\u{1F600}b\";\nvar t = 2;\n");
    }

    #[test]
    fn empty_and_single_line_round_trip() {
        round_trips("");
        round_trips("x");
        round_trips("\n");
    }

    /// Columns are the negotiated unit: UTF-16 units by default, bytes once
    /// the client accepts UTF-8.
    #[test]
    fn columns_follow_the_negotiated_encoding() {
        let rope = Rope::from_str("\u{1F600}x");
        // The emoji is 4 UTF-8 bytes and 2 UTF-16 units.
        assert_eq!(offset_to_position(&rope, 4, U16), Position::new(0, 2));
        assert_eq!(offset_to_position(&rope, 5, U16), Position::new(0, 3));
        assert_eq!(offset_to_position(&rope, 4, U8), Position::new(0, 4));
        assert_eq!(position_to_offset(&rope, Position::new(0, 4), U8), 4);
    }

    /// A stale request must clamp, never panic: the editor can ask about a
    /// position from a document version the server has already replaced.
    #[test]
    fn out_of_range_clamps_instead_of_panicking() {
        let rope = Rope::from_str("abc\n");
        assert_eq!(offset_to_position(&rope, 9_999, U16).line, 1);
        assert_eq!(position_to_offset(&rope, Position::new(99, 99), U16), rope.len_bytes());
        assert_eq!(position_to_offset(&rope, Position::new(0, 99), U16), 3);
        assert_eq!(position_to_offset(&rope, Position::new(0, 99), U8), 3);
    }

    /// A column landing INSIDE a character cannot be represented; it clamps to
    /// the character start rather than splitting it.
    #[test]
    fn a_column_inside_a_character_clamps() {
        let rope = Rope::from_str("\u{1F600}x");
        assert_eq!(position_to_offset(&rope, Position::new(0, 1), U16), 0);
        assert_eq!(position_to_offset(&rope, Position::new(0, 3), U8), 0);
    }

    #[test]
    fn span_to_range_spans_the_written_text() {
        let text = "var name = 1;";
        let rope = Rope::from_str(text);
        let start = text.find("name").unwrap();
        let span = Span::new(start as u32, (start + 4) as u32);
        let range = span_to_range(&rope, span, U16);
        assert_eq!(range.start, Position::new(0, 4));
        assert_eq!(range.end, Position::new(0, 8));
    }

    /// UTF-8 is chosen only when the client offers it; anything else is
    /// UTF-16, the one encoding every client speaks.
    #[test]
    fn negotiation_prefers_utf8_only_when_offered() {
        assert_eq!(negotiate(None), U16);
        assert_eq!(negotiate(Some(&[PositionEncodingKind::UTF16])), U16);
        assert_eq!(
            negotiate(Some(&[PositionEncodingKind::UTF16, PositionEncodingKind::UTF8])),
            U8,
        );
        assert_eq!(negotiate(Some(&[PositionEncodingKind::UTF32])), U16);
    }
}
