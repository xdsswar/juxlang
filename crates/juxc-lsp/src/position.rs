//! Position translation between Jux byte offsets and LSP positions.
//!
//! `juxc-source::Span` stores **UTF-8 byte offsets** (§L.8). LSP `Position`s
//! are line + **UTF-16 code unit** column by default. Every position-bearing
//! message must be translated across this boundary; these helpers are the one
//! place that conversion lives.
//!
//! The skeleton advertises the default UTF-16 encoding (broadest editor
//! compatibility). Negotiating UTF-8 to skip the per-line scan is a later
//! optimization noted in §L.8.

use juxc_source::Span;
use ropey::Rope;
use tower_lsp::lsp_types::{Position, Range};

/// Convert a UTF-8 byte offset into an LSP [`Position`] (UTF-16 columns).
///
/// Offsets past EOF clamp to the document end so a stale request can never
/// panic. The UTF-16 column is computed by summing `len_utf16()` over the
/// characters from the line start up to `offset`.
pub fn offset_to_position(rope: &Rope, offset: usize) -> Position {
    let offset = offset.min(rope.len_bytes());
    let line = rope.byte_to_line(offset);
    let line_start = rope.line_to_byte(line);

    let start_char = rope.byte_to_char(line_start);
    let end_char = rope.byte_to_char(offset);
    let mut col16: u32 = 0;
    for ch in rope.slice(start_char..end_char).chars() {
        col16 += ch.len_utf16() as u32;
    }
    Position::new(line as u32, col16)
}

/// Convert an LSP [`Position`] back into a UTF-8 byte offset.
///
/// Used by request handlers (hover, completion) to locate the cursor in the
/// byte-indexed AST/type maps. Out-of-range lines/columns clamp to the nearest
/// valid offset rather than panicking.
pub fn position_to_offset(rope: &Rope, pos: Position) -> usize {
    let last_line = rope.len_lines().saturating_sub(1);
    let line = (pos.line as usize).min(last_line);
    let line_start = rope.line_to_byte(line);
    let start_char = rope.byte_to_char(line_start);

    let mut remaining = pos.character;
    let mut byte = line_start;
    for ch in rope.slice(start_char..).chars() {
        if remaining == 0 || ch == '\n' {
            break;
        }
        let w = ch.len_utf16() as u32;
        if w > remaining {
            break;
        }
        remaining -= w;
        byte += ch.len_utf8();
    }
    byte
}

/// Convert a Jux [`Span`] into an LSP [`Range`].
pub fn span_to_range(rope: &Rope, span: Span) -> Range {
    Range::new(
        offset_to_position(rope, span.start as usize),
        offset_to_position(rope, span.end as usize),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every offset in a document must survive the round trip.
    ///
    /// This module is the single boundary between Jux's UTF-8 byte offsets and
    /// LSP's UTF-16 columns, and it had no tests at all — so every squiggle in
    /// the editor rested on code nothing checked. An off-by-one here does not
    /// crash; it silently underlines the wrong character, which is the kind of
    /// wrongness a user learns to distrust the whole tool for.
    fn round_trips(text: &str) {
        let rope = Rope::from_str(text);
        for offset in 0..=text.len() {
            if !text.is_char_boundary(offset) {
                continue;
            }
            let pos = offset_to_position(&rope, offset);
            assert_eq!(
                position_to_offset(&rope, pos),
                offset,
                "offset {offset} of {text:?} did not round-trip (via {pos:?})",
            );
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

    /// An emoji is a SURROGATE PAIR in UTF-16 — two units for one character —
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

    /// Columns are UTF-16 units, not characters and not bytes.
    #[test]
    fn columns_are_utf16_units() {
        let text = "\u{1F600}x";
        let rope = Rope::from_str(text);
        // The emoji is 4 UTF-8 bytes and 2 UTF-16 units.
        let after_emoji = offset_to_position(&rope, 4);
        assert_eq!(after_emoji, Position::new(0, 2));
        let after_x = offset_to_position(&rope, 5);
        assert_eq!(after_x, Position::new(0, 3));
    }

    /// A stale request must clamp, never panic — the editor can ask about a
    /// position from a document version the server has already replaced.
    #[test]
    fn out_of_range_clamps_instead_of_panicking() {
        let rope = Rope::from_str("abc\n");
        assert_eq!(offset_to_position(&rope, 9_999).line, 1);
        assert_eq!(position_to_offset(&rope, Position::new(99, 99)), rope.len_bytes());
        assert_eq!(position_to_offset(&rope, Position::new(0, 99)), 3);
    }

    /// A column landing INSIDE a surrogate pair cannot be represented; it must
    /// clamp to the character start rather than split the character.
    #[test]
    fn a_column_inside_a_surrogate_pair_clamps() {
        let rope = Rope::from_str("\u{1F600}x");
        // Column 1 is halfway through the emoji's two UTF-16 units.
        assert_eq!(position_to_offset(&rope, Position::new(0, 1)), 0);
    }

    #[test]
    fn span_to_range_spans_the_written_text() {
        let text = "var name = 1;";
        let rope = Rope::from_str(text);
        let start = text.find("name").unwrap();
        let span = Span::new(start as u32, (start + 4) as u32);
        let range = span_to_range(&rope, span);
        assert_eq!(range.start, Position::new(0, 4));
        assert_eq!(range.end, Position::new(0, 8));
    }
}
