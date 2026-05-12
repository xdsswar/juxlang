//! Interpolated-string (`$"…"`) segmentation and inline-expression parsing.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{Expr, Ident, InterpSegment};
use juxc_diagnostics::{code, Diagnostic};
use juxc_source::Span;

use crate::literals::process_string_escapes;
use crate::Parser;

impl<'a> Parser<'a> {
    /// Split the raw body of an interpolated-string token into segments
    /// per §3.4. The raw body is whatever sits between the `$"` and the
    /// closing `"` — the lexer captured it verbatim, including the
    /// `$name` and `${...}` markers.
    ///
    /// Walk byte-by-byte:
    /// - Accumulate plain text into the current `Literal` buffer.
    /// - On `$` followed by an identifier-start char, flush the literal
    ///   buffer, then read the identifier and push a `Bare` segment.
    /// - On `${`, flush the literal buffer, scan to the matching `}`
    ///   tracking brace depth, then **recursively lex+parse** the
    ///   captured inner text as a Jux expression and push an `Expr`
    ///   segment.
    /// - Anything else is just literal text.
    ///
    /// Inner-expression diagnostics get re-emitted into our own
    /// diagnostics vector. Their spans point at offsets inside the
    /// inner substring — column-imprecise for now, but the user still
    /// sees the message. A future pass can rebase spans to the outer
    /// source by adding the interp's content-start offset.
    pub(crate) fn parse_interp_segments(&mut self, raw: &str) -> Vec<InterpSegment> {
        let bytes = raw.as_bytes();
        let mut segments: Vec<InterpSegment> = Vec::new();
        let mut lit_buf = String::new();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            // `$name` — bare-identifier interpolation.
            if b == b'$' && i + 1 < bytes.len() && is_ident_start(bytes[i + 1]) {
                if !lit_buf.is_empty() {
                    segments.push(InterpSegment::Literal(std::mem::take(&mut lit_buf)));
                }
                let id_start = i + 1;
                let mut j = id_start + 1;
                while j < bytes.len() && is_ident_cont(bytes[j]) {
                    j += 1;
                }
                let name = String::from_utf8_lossy(&bytes[id_start..j]).into_owned();
                // Ident spans get DUMMY here — interp inner span fidelity
                // is a known polish item. The outer InterpString span
                // already points at the literal.
                segments.push(InterpSegment::Bare(Ident {
                    text: name,
                    span: Span::DUMMY,
                }));
                i = j;
                continue;
            }
            // `${expression}` — expression interpolation.
            if b == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                if !lit_buf.is_empty() {
                    segments.push(InterpSegment::Literal(std::mem::take(&mut lit_buf)));
                }
                // Scan to the matching `}` with brace-depth tracking.
                // The opening `{` counts as depth 1.
                let expr_start = i + 2;
                let mut j = expr_start;
                let mut depth: u32 = 1;
                while j < bytes.len() && depth > 0 {
                    match bytes[j] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                if depth != 0 {
                    // Unterminated `${…` — emit a diagnostic and treat
                    // the rest as literal text so parsing keeps going.
                    self.diagnostics.push(
                        Diagnostic::error(
                            code::Code::E0200_UnexpectedToken,
                            "unterminated `${...}` in interpolated string",
                        ),
                    );
                    lit_buf.push_str(&raw[i..]);
                    i = bytes.len();
                    continue;
                }
                let inner = &raw[expr_start..j];
                // Recursively lex + parse the inner expression.
                if let Some(inner_expr) = self.parse_inline_expr(inner) {
                    segments.push(InterpSegment::Expr(Box::new(inner_expr)));
                }
                // Skip past the closing `}`.
                i = j + 1;
                continue;
            }
            // Backslash escape — decode through the shared escape
            // table per §A.1.5. We process one escape sequence at a
            // time so that subsequent `$name`/`${…}` markers retain
            // their normal meaning inside the same literal run.
            if b == b'\\' && i + 1 < bytes.len() {
                // Find the end of this single escape (it may span
                // multiple bytes for `\u{…}` and `\xHH`).
                let esc_end = escape_byte_end(bytes, i);
                let (decoded, errs) = process_string_escapes(&raw[i..esc_end]);
                lit_buf.push_str(&decoded);
                for msg in errs {
                    self.diagnostics.push(
                        Diagnostic::error(code::Code::E0200_UnexpectedToken, msg),
                    );
                }
                i = esc_end;
                continue;
            }
            // Plain literal byte — append as UTF-8 char.
            if let Some(ch) = raw[i..].chars().next() {
                lit_buf.push(ch);
                i += ch.len_utf8();
            } else {
                // Defensive: malformed UTF-8 shouldn't happen because the
                // source went through the lexer, which preserves bytes.
                i += 1;
            }
        }
        if !lit_buf.is_empty() {
            segments.push(InterpSegment::Literal(lit_buf));
        }
        segments
    }

    /// Lex + parse a substring as a single Jux expression — used for
    /// `${…}` interpolation chunks per §3.4.
    ///
    /// Wraps the inner text in a synthetic `SourceFile` and runs a
    /// fresh lexer + parser over it. Diagnostics from the inner parse
    /// merge back into this parser's diagnostics so the user sees them
    /// at the call site. Inner-expression spans live in a different
    /// SourceFile — column fidelity in nested diagnostics is a known
    /// polish item.
    pub(crate) fn parse_inline_expr(&mut self, source: &str) -> Option<Expr> {
        let synthetic = juxc_source::SourceFile::new("<interp>", source.to_string());
        let lex_out = juxc_lex::lex(&synthetic);
        // Propagate any lexer diagnostics to the outer parser so the
        // user sees them.
        self.diagnostics.extend(lex_out.diagnostics);
        let mut inner = Parser::new(&lex_out.tokens);
        let expr = inner.parse_expr();
        // Anything left over is a parse error: the inner text should
        // tokenize to one expression then EOF.
        if !inner.at_eof() {
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    "unexpected trailing tokens inside `${...}` interpolation",
                ),
            );
        }
        self.diagnostics.extend(inner.diagnostics);
        expr
    }
}

/// True if `b` can start an ASCII identifier (`A-Z`, `a-z`, `_`).
/// Mirrors the lexical rule in §A.1.3 — Jux identifiers are ASCII-only.
pub(crate) fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

/// True if `b` can continue an ASCII identifier (`A-Z`, `a-z`, `0-9`, `_`).
pub(crate) fn is_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Given that `bytes[i]` is a `\` opening an escape inside an interp
/// string body, return the exclusive end byte index of that one
/// escape sequence. Used by the interp segment walker so we can hand
/// exactly the escape's bytes to [`process_string_escapes`] without
/// accidentally consuming a following `$name` marker as part of the
/// escape's lookahead.
///
/// Rules (mirroring §A.1.5):
/// - `\x` + 2 chars → 4 bytes total (the `\`, the `x`, two hex digits).
/// - `\u{...}` → from `\` through the closing `}`. If `{` is missing
///   or the form is unterminated, we consume what's there and let
///   [`process_string_escapes`] surface the diagnostic.
/// - any other escape (`\n`, `\\`, `\"`, …) → 2 bytes total.
pub(crate) fn escape_byte_end(bytes: &[u8], i: usize) -> usize {
    if i + 1 >= bytes.len() {
        return bytes.len();
    }
    match bytes[i + 1] {
        b'x' => (i + 4).min(bytes.len()),
        b'u' => {
            // Scan to the matching `}`. Tolerate malformed shapes by
            // stopping at end-of-buffer; the decoder reports them.
            let mut j = i + 2;
            if j < bytes.len() && bytes[j] == b'{' {
                j += 1;
                while j < bytes.len() && bytes[j] != b'}' {
                    j += 1;
                }
                if j < bytes.len() {
                    return j + 1;
                }
                return bytes.len();
            }
            j.min(bytes.len())
        }
        _ => i + 2,
    }
}
