//! Indent-aware text writer.
//!
//! Owns the emitted Rust source buffer and tracks the current indent
//! depth so emitters can stop hand-emitting `"    "` runs at line
//! starts. The migration path is gradual: existing emitters still call
//! [`Writer::push_str`] and [`Writer::push`] for explicit appends —
//! exactly the [`String`]-shaped API they used before — and newer code
//! moves to [`Writer::line`] / [`Writer::indent_inc`] /
//! [`Writer::indent_dec`] / [`Writer::emit_indent`] for indent-aware
//! emission.
//!
//! This is intentionally a tiny abstraction — not a Wadler-style
//! pretty-printer. Group/Break/Nest combinators land in a follow-up
//! turn once enough emitters are migrated for the layout machinery to
//! pay off.

/// Output buffer + indent state for the backend.
///
/// Construct with [`Writer::new`], emit text through the various
/// `push_*` / `line` / `newline` methods, then consume via
/// [`Writer::into_string`] at the end of compilation.
pub(crate) struct Writer {
    /// The accumulated Rust source.
    buf: String,
    /// Current indent depth (in *levels*, not spaces). One level renders
    /// as four spaces — Rust's canonical indent.
    indent_level: usize,
}

impl Writer {
    /// Construct an empty writer at indent depth 0.
    pub(crate) fn new() -> Self {
        Self { buf: String::new(), indent_level: 0 }
    }

    // ------------------------------------------------------------------
    // Raw append (no auto-indent) — for the legacy emit_* style.
    // ------------------------------------------------------------------

    /// Append a string slice verbatim. No indent insertion — the caller
    /// is responsible for emitting the right indent at line starts.
    /// This is the bridge for the bulk of existing emitters that
    /// already construct their output character-by-character.
    pub(crate) fn push_str(&mut self, s: &str) {
        self.buf.push_str(s);
    }

    /// Append a single character verbatim.
    pub(crate) fn push(&mut self, ch: char) {
        self.buf.push(ch);
    }

    /// Append a newline only. Newer code that uses the indent helpers
    /// usually prefers [`Writer::line`], which combines indent + line +
    /// newline.
    pub(crate) fn newline(&mut self) {
        self.buf.push('\n');
    }

    // ------------------------------------------------------------------
    // Indent-aware helpers — the target API for migrated emitters.
    // ------------------------------------------------------------------

    /// Increase the indent depth by one level. Subsequent
    /// [`Writer::emit_indent`] / [`Writer::line`] calls will emit one
    /// more `"    "` chunk at line starts.
    pub(crate) fn indent_inc(&mut self) {
        self.indent_level += 1;
    }

    /// Decrease the indent depth by one level (saturating at zero so
    /// over-dedenting is harmless rather than a panic).
    pub(crate) fn indent_dec(&mut self) {
        self.indent_level = self.indent_level.saturating_sub(1);
    }

    /// Emit `indent_level × 4` spaces. Use at a line start where you
    /// want the current depth's indent prefix before more content goes
    /// onto the same line.
    pub(crate) fn emit_indent(&mut self) {
        for _ in 0..self.indent_level {
            self.buf.push_str("    ");
        }
    }

    /// Emit `current indent`, then `s`, then a newline. The convenience
    /// shortcut for single-line statements/declarations.
    pub(crate) fn line(&mut self, s: &str) {
        self.emit_indent();
        self.buf.push_str(s);
        self.buf.push('\n');
    }

    // ------------------------------------------------------------------
    // Consumption / inspection.
    // ------------------------------------------------------------------

    /// Consume the writer and return the accumulated source.
    pub(crate) fn into_string(self) -> String {
        self.buf
    }

    /// Current indent depth — exposed for emitters that need to record
    /// and restore depth (e.g. when emitting a `match` arm body that
    /// runs at one deeper level than the arm header).
    #[allow(dead_code)]
    pub(crate) fn level(&self) -> usize {
        self.indent_level
    }

    /// Replace the first occurrence of `needle` in the buffer with
    /// `replacement`. Used by the file-header patcher to fill in the
    /// real source path once a `SourceFile` is attached to the emitter
    /// (the placeholder line is written up-front in [`Writer::new`] so
    /// that crate-wide `#![allow(...)]` attributes stay at the file
    /// top, but the source path isn't known until the lower entry
    /// point decides which file we're compiling).
    pub(crate) fn replace_first(&mut self, needle: &str, replacement: &str) {
        if let Some(pos) = self.buf.find(needle) {
            self.buf.replace_range(pos..pos + needle.len(), replacement);
        }
    }
}
