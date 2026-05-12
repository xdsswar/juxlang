//! Source-file storage, byte positions, and spans.
//!
//! Every diagnostic, token, and AST node carries a [`Span`] that points back
//! into a [`SourceFile`]. Keeping this crate dependency-free is deliberate:
//! the whole workspace pulls it in, and bloating it would cascade.
//!
//! ## Byte offsets, not character indices
//!
//! Spans index into the **byte** stream of the source. UTF-8 multi-byte
//! sequences span multiple offsets. Rendering to terminal columns (which
//! cares about character widths, tabs, ANSI, etc.) is the diagnostic
//! renderer's job, not this crate's.

use std::path::{Path, PathBuf};

/// A loaded `.jux` source file, with its path and UTF-8 contents.
///
/// Construct with [`SourceFile::new`]. Once built, a `SourceFile` is
/// immutable; clones are cheap because the heavy fields (path, contents,
/// line-start index) live inside `Arc`-friendly types you can wrap if you
/// need cheap sharing later.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Filesystem path or virtual name used in diagnostics.
    path: PathBuf,
    /// The full source text, UTF-8.
    contents: String,
    /// Byte offsets of the start of each line. `line_starts[0] == 0` always;
    /// `line_starts[i]` is the byte offset just past the `i`th `\n`. Built
    /// once in [`SourceFile::new`] so [`SourceFile::line_col`] is O(log n).
    line_starts: Vec<usize>,
}

impl SourceFile {
    /// Build a `SourceFile` from a path and its UTF-8 contents.
    ///
    /// The path is purely informational — it's used in rendered
    /// diagnostics. The contents are scanned once to populate the
    /// line-start index.
    pub fn new(path: impl Into<PathBuf>, contents: impl Into<String>) -> Self {
        let contents = contents.into();
        let line_starts = compute_line_starts(&contents);
        Self { path: path.into(), contents, line_starts }
    }

    /// Path used for diagnostic rendering.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The full source text. Lexers and pretty-printers consume this.
    pub fn contents(&self) -> &str {
        &self.contents
    }

    /// Map a byte offset to a 1-based `(line, column)` pair.
    ///
    /// Column is measured in **bytes** from the start of the line, not in
    /// Unicode code points or display columns. The diagnostics renderer is
    /// responsible for the byte→display-column mapping when emitting to a
    /// terminal (it cares about combining marks, wide CJK glyphs, tabs, …).
    ///
    /// `offset` is clamped: an offset past EOF returns the last line and a
    /// column past its end.
    pub fn line_col(&self, offset: usize) -> (u32, u32) {
        // Binary search the line_starts vector. If the offset lands exactly
        // on a line start (Ok), we found the line. Otherwise binary_search
        // returns the insertion index, and the line is the one before it.
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx];
        ((line_idx + 1) as u32, (offset - line_start + 1) as u32)
    }
}

/// Pre-compute the byte offset of the start of every line.
///
/// Convention: `line_starts[0] = 0` (line 1 begins at byte 0). Each `\n`
/// pushes the byte index just *past* the newline, which is the first byte
/// of the next line. Files not ending in `\n` thus have one fewer entry
/// than `<lines>` — that's fine; the last line is implicitly bounded by
/// `contents.len()`.
fn compute_line_starts(s: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in s.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// A byte-offset range within a single source file: half-open `[start, end)`.
///
/// `start <= end` is a debug-mode invariant. `start == end` is legal and
/// denotes an empty span (used for "this token is missing" diagnostics
/// that need to point at a specific byte without highlighting any).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    /// Inclusive start, byte offset into the source file.
    pub start: u32,
    /// Exclusive end, byte offset into the source file.
    pub end: u32,
}

impl Span {
    /// A span that points nowhere. Useful for synthesized nodes that don't
    /// correspond to any source text (auto-derived methods, implicit
    /// returns, etc.). The diagnostics renderer treats `DUMMY` as a
    /// signal to omit the carets / location pointer.
    pub const DUMMY: Span = Span { start: 0, end: 0 };

    /// Construct a span. Panics in debug builds if `start > end`.
    pub fn new(start: u32, end: u32) -> Self {
        debug_assert!(start <= end, "Span start must be <= end");
        Self { start, end }
    }

    /// Byte length of the span. Zero for empty spans.
    pub fn len(self) -> u32 {
        self.end - self.start
    }

    /// True if `start == end`.
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Smallest span covering both `self` and `other`.
    ///
    /// Used by the parser to compute a node's full span from its first and
    /// last children (e.g. a function decl spans from its `public`/return
    /// type to its closing brace).
    pub fn join(self, other: Span) -> Span {
        Span { start: self.start.min(other.start), end: self.end.max(other.end) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity test: line_col matches what a human would count on a
    /// multi-line source with a blank line in the middle.
    #[test]
    fn line_col_basic() {
        let sf = SourceFile::new("test.jux", "abc\ndef\n\nghi");
        assert_eq!(sf.line_col(0), (1, 1));
        assert_eq!(sf.line_col(2), (1, 3));
        assert_eq!(sf.line_col(4), (2, 1));
        assert_eq!(sf.line_col(9), (4, 1));
    }

    /// Span::join is the parser's primary tool for synthesizing a parent
    /// span from its leftmost and rightmost child. It must work
    /// symmetrically and inclusively.
    #[test]
    fn span_join_is_inclusive() {
        let a = Span::new(2, 5);
        let b = Span::new(10, 12);
        assert_eq!(a.join(b), Span::new(2, 12));
    }
}
