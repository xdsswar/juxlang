//! Incremental text sync (§L.5): apply `didChange` edits to a document's rope.
//!
//! The server asks for `TextDocumentSyncKind::Incremental`, so each change
//! carries a range and its replacement text rather than the whole buffer. The
//! rope makes each edit `O(log n)`. A change without a range is a whole-buffer
//! replacement, which the protocol still allows mid-stream.

use ropey::Rope;
use tower_lsp::lsp_types::TextDocumentContentChangeEvent;

use crate::position::{position_to_offset, PositionEncoding};

/// Apply `changes`, in order, to `rope`. Positions are in `enc`, and each
/// change's range refers to the text as the previous change left it, which is
/// what the protocol specifies.
pub(crate) fn apply_changes(rope: &mut Rope, changes: &[TextDocumentContentChangeEvent], enc: PositionEncoding) {
    for change in changes {
        match change.range {
            None => *rope = Rope::from_str(&change.text),
            Some(range) => {
                let start = position_to_offset(rope, range.start, enc);
                let end = position_to_offset(rope, range.end, enc).max(start);
                let (cs, ce) = (rope.byte_to_char(start), rope.byte_to_char(end));
                rope.remove(cs..ce);
                rope.insert(cs, &change.text);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{Position, Range};

    fn edit(sl: u32, sc: u32, el: u32, ec: u32, text: &str) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent {
            range: Some(Range::new(Position::new(sl, sc), Position::new(el, ec))),
            range_length: None,
            text: text.to_string(),
        }
    }

    /// Insert, delete and replace, each against the text the previous edit
    /// produced.
    #[test]
    fn edits_apply_in_order() {
        let mut rope = Rope::from_str("var x = 1;\nprint(x);\n");
        apply_changes(
            &mut rope,
            &[
                edit(0, 4, 0, 5, "total"),  // x -> total
                edit(1, 6, 1, 7, "total"),  // x -> total on line 2
                edit(1, 0, 1, 0, "    "),   // indent line 2
                edit(0, 14, 0, 14, " // n"), // append a comment
            ],
            PositionEncoding::Utf16,
        );
        assert_eq!(rope.to_string(), "var total = 1; // n\n    print(total);\n");
    }

    /// A change without a range replaces the whole buffer.
    #[test]
    fn a_rangeless_change_replaces_everything() {
        let mut rope = Rope::from_str("old");
        let full = TextDocumentContentChangeEvent { range: None, range_length: None, text: "new text".into() };
        apply_changes(&mut rope, &[full], PositionEncoding::Utf16);
        assert_eq!(rope.to_string(), "new text");
    }

    /// Columns follow the negotiated encoding: after an emoji, UTF-16 counts
    /// two units where UTF-8 counts four bytes, and both land on the same
    /// character.
    #[test]
    fn edits_after_multibyte_text_respect_the_encoding() {
        let src = "s = \"\u{1F600}\"; x";
        let mut utf16 = Rope::from_str(src);
        apply_changes(&mut utf16, &[edit(0, 10, 0, 11, "y")], PositionEncoding::Utf16);
        let mut utf8 = Rope::from_str(src);
        apply_changes(&mut utf8, &[edit(0, 12, 0, 13, "y")], PositionEncoding::Utf8);
        assert_eq!(utf16.to_string(), "s = \"\u{1F600}\"; y");
        assert_eq!(utf8.to_string(), utf16.to_string());
    }

    /// A stale range past the end clamps instead of panicking.
    #[test]
    fn a_stale_range_clamps() {
        let mut rope = Rope::from_str("ab\n");
        apply_changes(&mut rope, &[edit(5, 0, 9, 9, "!")], PositionEncoding::Utf16);
        assert_eq!(rope.to_string(), "ab\n!");
    }
}
