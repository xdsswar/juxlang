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
