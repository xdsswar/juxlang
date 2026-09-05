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
    /// This file's position in the compilation's source list. Stamped onto
    /// every token span so spans from different files can never collide as map
    /// keys (see [`Span::file`]). `0` unless the driver sets it.
    index: u32,
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
        Self { path: path.into(), contents, line_starts, index: 0 }
    }

    /// This file's index in the compilation's source list — the value stamped
    /// into [`Span::file`] for every token lexed from it.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Set the file's index. The driver calls this once, in the same order it
    /// reports diagnostics with, so a span's `file` and a diagnostic's `file`
    /// agree.
    pub fn set_index(&mut self, index: u32) {
        self.index = index;
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
/// **Why the file tag.** A span is the key of every analysis map the compiler
/// hands to the backend — inferred types, overload picks, argument coercions.
/// A workspace compiles many files at once (a program always includes the
/// embedded `jux.std` sources) and offsets restart at zero in each one, so two
/// unrelated expressions at the same byte range in different files collided as
/// keys: the last one written won and the other was miscompiled. The symptom
/// was suitably spooky — adding a comment line to YOUR file changed the code
/// generated for a std file.
///
/// The tag participates in identity ([`PartialEq`], [`Hash`]) and nothing else.
/// [`Span::start`] and [`Span::end`] remain offsets into their own file, so
/// every consumer that slices source text or computes a line/column is
/// unaffected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    /// Inclusive start, byte offset into the source file.
    pub start: u32,
    /// Exclusive end, byte offset into the source file.
    pub end: u32,
    /// Which file this span indexes into — the [`SourceFile`]'s position in
    /// the compilation's source list, the same index diagnostics carry. `0`
    /// for a single-file compilation and for synthesized spans.
    pub file: u32,
}

impl Span {
    /// A span that points nowhere. Useful for synthesized nodes that don't
    /// correspond to any source text (auto-derived methods, implicit
    /// returns, etc.). The diagnostics renderer treats `DUMMY` as a
    /// signal to omit the carets / location pointer.
    pub const DUMMY: Span = Span { start: 0, end: 0, file: 0 };

    /// Construct a span in file `0` — the single-file case, and the default
    /// for synthesized nodes. Panics in debug builds if `start > end`.
    pub fn new(start: u32, end: u32) -> Self {
        debug_assert!(start <= end, "Span start must be <= end");
        Self { start, end, file: 0 }
    }

    /// Construct a span in a specific file. The lexer stamps every token this
    /// way; the parser propagates it through [`Self::join`].
    pub fn in_file(start: u32, end: u32, file: u32) -> Self {
        debug_assert!(start <= end, "Span start must be <= end");
        Self { start, end, file }
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
        // Keeps a real file over a synthesized one: the parser always joins
        // spans from the file it is parsing, and a `DUMMY` operand (file 0)
        // must not drag the result back to file 0.
        let file = if self.file != 0 { self.file } else { other.file };
        Span { start: self.start.min(other.start), end: self.end.max(other.end), file }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two files' spans at the SAME byte range are distinct keys. Every
    /// analysis map the compiler hands the backend is keyed by span, and a
    /// workspace always compiles several files at once, so without this two
    /// unrelated expressions collided and one of them was miscompiled.
    #[test]
    fn spans_from_different_files_are_distinct() {
        use std::collections::HashMap;
        let a = Span::in_file(10, 20, 0);
        let b = Span::in_file(10, 20, 1);
        assert_ne!(a, b, "same offsets in different files must not be equal");
        let mut map: HashMap<Span, &str> = HashMap::new();
        map.insert(a, "file 0");
        map.insert(b, "file 1");
        assert_eq!(map.len(), 2, "one span must not evict the other");
        assert_eq!(map[&a], "file 0");
        assert_eq!(map[&b], "file 1");
    }

    /// The file tag is identity only — offsets stay relative to their own
    /// file, so line/column and snippet extraction are untouched.
    #[test]
    fn the_file_tag_does_not_disturb_offsets() {
        let s = Span::in_file(4, 9, 7);
        assert_eq!(s.start, 4);
        assert_eq!(s.end, 9);
        assert_eq!(s.len(), 5);
    }

    /// Joining keeps the real file rather than falling back to a synthesized
    /// operand's file 0.
    #[test]
    fn join_keeps_the_real_file() {
        let real = Span::in_file(10, 20, 3);
        assert_eq!(real.join(Span::DUMMY).file, 3);
        assert_eq!(Span::DUMMY.join(real).file, 3);
        assert_eq!(real.join(Span::in_file(30, 40, 3)), Span::in_file(10, 40, 3));
    }

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
