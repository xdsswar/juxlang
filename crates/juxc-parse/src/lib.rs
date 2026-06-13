//! Phase 2 — parser.
//!
//! Hand-written recursive-descent parser. Conforms to the syntactic grammar
//! in `JUX-GRAMMAR-ADDENDUM.md` §A.2.
//!
//! ## Approach
//!
//! Per the grammar addendum's *Implementation Notes*: "The grammar is suitable
//! for either a hand-written recursive-descent parser (the recommended Phase 1
//! approach…) or an LALR(1) generator." We pick the hand-written form —
//! easier diagnostics, easier error recovery, no generator dependency.
//!
//! ## Coverage in this revision
//!
//! Milestone-1 subset. Enough to parse `examples/hello.jux` into a real AST:
//!
//! - `compilation-unit` with `top-level-decl*` (skips package/import for now).
//! - `function-decl` with visibility, `void` return, empty parameter list,
//!   block body.
//! - Statements: `expression-stmt`, `return` stmt.
//! - Expressions: literals (string, int, bool, null), identifier paths, and
//!   postfix call (`callee(args…)`).
//!
//! Productions not yet implemented (`class-decl`, `if-stmt`, binary ops, etc.)
//! cause the parser to emit `E0200_UnexpectedToken` and resume at the next
//! statement or top-level boundary. They're added incrementally as the
//! milestones grow.
//!
//! ## Error recovery
//!
//! Per the grammar addendum: on a parse error inside a declaration or
//! statement, recover at the next `;`, `}`, or top-level keyword. This
//! produces a small number of cascading errors per genuine fault instead
//! of fail-fast.
//!
//! ## Module layout
//!
//! The parser is split across action-focused module files. `lib.rs` holds
//! the `Parser` struct definition, its constructor, the cursor/matching
//! primitives that everything else builds on, and the public `parse`
//! entry point. Each sibling module owns an `impl Parser<'a>` block
//! containing the methods for one grammar area.

use juxc_ast::{CompilationUnit, Ident};
use juxc_diagnostics::{code, Diagnostic};
use juxc_lex::{Keyword, Token, TokenKind};
use juxc_source::Span;

mod compilation;
mod decls;
mod exprs;
mod generics;
mod interpolation;
mod literals;
mod patterns;
mod stmts;
mod types;

#[cfg(test)]
mod tests;

/// Output of [`parse`].
///
/// `ast` is always populated — even on errors, the parser returns a
/// possibly-degraded tree so downstream phases can keep running and produce
/// additional diagnostics. `diagnostics` lists everything that went wrong.
pub struct ParseResult {
    /// Parsed compilation unit. Possibly partial if `diagnostics` is non-empty.
    pub ast: CompilationUnit,
    /// Syntax diagnostics emitted during parsing (E0200_… codes).
    pub diagnostics: Vec<Diagnostic>,
}

/// Parse a token stream into an AST. The token slice must end with an
/// [`TokenKind::Eof`] token (the lexer guarantees this).
pub fn parse(tokens: &[Token]) -> ParseResult {
    let mut p = Parser::new(tokens);
    let mut ast = p.parse_compilation_unit();
    // Desugar C#-style properties (JUX-MISSING-DEFS §M.7) into backing
    // fields + getter / setter methods so every downstream phase
    // (resolve, tycheck, backend) reuses the existing field / method
    // machinery. The original `PropertyDecl` list stays on each class
    // for tycheck access-control and backend setter routing.
    juxc_ast::desugar_properties(&mut ast);
    ParseResult { ast, diagnostics: p.diagnostics }
}

// ============================================================================
// Parser state
// ============================================================================

/// Internal parser state. Holds the immutable token slice, a moving cursor,
/// and an accumulator for diagnostics. Not exposed publicly.
///
/// Fields are `pub(crate)` so the sibling action modules (`compilation`,
/// `decls`, `stmts`, `exprs`, …) can read and write the cursor and
/// emit diagnostics. The struct itself stays private — outside the
/// crate the only entry point is the free [`parse`] fn.
pub(crate) struct Parser<'a> {
    /// Token stream from the lexer, EOF-terminated.
    pub(crate) tokens: &'a [Token],
    /// Index of the *next* token to be consumed. Indexes into `tokens`.
    pub(crate) pos: usize,
    /// Leftover closing `>`s from a split `>>` token. The lexer glues adjacent
    /// `>` into a single `GtGt`, but in **type position** a `>>` closes two
    /// nested generic lists (`List<List<int>>`). When a generic-close consumes a
    /// `GtGt`, it advances past the token and parks the *second* `>` here for the
    /// enclosing generic list to consume. Always drained back to 0 within a
    /// balanced generic close; only ever touched by the generic-close helpers
    /// ([`Self::close_generic_angle`]), so ordinary `>>`-shift parsing is
    /// unaffected. See `parse_generic_args` / `parse_generic_params`.
    pub(crate) pending_gt: u8,
    /// Diagnostics emitted along the way, in source order.
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Extra statements queued by a single `parse_stmt` call that
    /// desugars one source statement into several — currently only
    /// tuple destructuring (`var (q, r) = e;` → temp decl + one
    /// `var` per element). `parse_block` drains this after every
    /// statement so the extras land in source order at the same
    /// scope level.
    pub(crate) pending_stmts: Vec<juxc_ast::Stmt>,
    /// Monotonic counter for `__jux_tup{N}` destructuring temps —
    /// unique per compilation unit so nested/sibling destructures
    /// never collide.
    pub(crate) tuple_tmp_counter: u32,
}

impl<'a> Parser<'a> {
    /// Build a parser over the given token slice.
    pub(crate) fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            pos: 0,
            pending_gt: 0,
            diagnostics: Vec::new(),
            pending_stmts: Vec::new(),
            tuple_tmp_counter: 0,
        }
    }

    // ------------------------------------------------------------------
    // Generic-angle closing — `>` token splitting
    // ------------------------------------------------------------------

    /// True when a generic-argument/parameter list is positioned at its closing
    /// `>` — either a parked split-`>` ([`Self::pending_gt`]), a real `Gt`, or
    /// the first `>` of a `GtGt`. Used to decide "close now vs. expect another
    /// comma-separated entry".
    pub(crate) fn at_generic_close(&self) -> bool {
        self.pending_gt > 0 || matches!(self.peek(), TokenKind::Gt | TokenKind::GtGt)
    }

    /// Consume one closing `>` of a generic list, transparently splitting a
    /// `>>` (`GtGt`) token: the first `>` closes the current list and the second
    /// is parked in [`Self::pending_gt`] for the enclosing list. Emits
    /// `E0200` via `expected` when no `>` is available. Returns whether a `>`
    /// was consumed.
    pub(crate) fn close_generic_angle(&mut self, expected: &str) -> bool {
        if self.pending_gt > 0 {
            self.pending_gt -= 1;
            return true;
        }
        match self.peek() {
            TokenKind::Gt => {
                self.advance();
                true
            }
            TokenKind::GtGt => {
                // Consume the glued `>>` and park its second `>`.
                self.advance();
                self.pending_gt += 1;
                true
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::error(
                        code::Code::E0200_UnexpectedToken,
                        format!("expected {expected}"),
                    )
                    .with_span(self.peek_span()),
                );
                false
            }
        }
    }

    // ------------------------------------------------------------------
    // Cursor primitives
    //
    // These are the only places that touch `self.pos` and `self.tokens`
    // directly — everything else builds on top.
    // ------------------------------------------------------------------

    /// The kind of the current token (the one about to be consumed).
    /// Always returns `&TokenKind::Eof` past end of input, never panics.
    pub(crate) fn peek(&self) -> &TokenKind {
        // The lexer guarantees an Eof at the end, but be defensive in case
        // we're handed a malformed stream.
        self.tokens.get(self.pos).map(|t| &t.kind).unwrap_or(&TokenKind::Eof)
    }

    /// Span of the current token. Useful for diagnostics anchored at the
    /// failure site.
    pub(crate) fn peek_span(&self) -> Span {
        self.tokens.get(self.pos).map(|t| t.span).unwrap_or(Span::DUMMY)
    }

    /// Span of the most recently consumed token. Useful when a parsing rule
    /// needs to anchor a closing span (e.g. a function decl's `}`).
    pub(crate) fn last_consumed_span(&self) -> Span {
        if self.pos == 0 { Span::DUMMY }
        else { self.tokens[self.pos - 1].span }
    }

    /// True once we're at end of file.
    pub(crate) fn at_eof(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    /// Advance the cursor by one token, unless already at EOF.
    pub(crate) fn advance(&mut self) {
        if !self.at_eof() {
            self.pos += 1;
        }
    }

    // ------------------------------------------------------------------
    // Matching helpers — keyword / punctuation
    // ------------------------------------------------------------------

    /// Is the current token the keyword `kw`?
    pub(crate) fn at_kw(&self, kw: Keyword) -> bool {
        matches!(self.peek(), TokenKind::Kw(k) if *k == kw)
    }

    /// Consume the keyword `kw` if it's current. Returns whether anything
    /// was consumed.
    pub(crate) fn eat_kw(&mut self, kw: Keyword) -> bool {
        if self.at_kw(kw) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Consume `kw` or emit `E0200_UnexpectedToken` with `expected` as
    /// the human description. Returns whether anything was consumed.
    pub(crate) fn expect_kw(&mut self, kw: Keyword, expected: &str) -> bool {
        if self.eat_kw(kw) {
            true
        } else {
            self.diagnostics.push(
                Diagnostic::error(code::Code::E0200_UnexpectedToken, expected)
                    .with_span(self.peek_span()),
            );
            false
        }
    }

    /// Is the current token of the same enum-discriminant as `kind`?
    /// Used for **payload-less** token kinds (punctuation, EOF). For
    /// payload-carrying kinds (`Ident`, `Str`, …) match directly on
    /// `self.peek()` rather than using this.
    pub(crate) fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(kind)
    }

    /// Consume a payload-less token if it matches. Returns whether anything
    /// was consumed.
    pub(crate) fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Consume `kind` or emit `E0200_UnexpectedToken` with `expected` as
    /// the human description ("`)` to close argument list"). Returns
    /// whether the token was actually consumed — callers may want to know
    /// so they can adjust recovery.
    pub(crate) fn expect(&mut self, kind: &TokenKind, expected: &str) -> bool {
        if self.eat(kind) {
            true
        } else {
            let span = self.peek_span();
            self.diagnostics.push(
                Diagnostic::error(
                    code::Code::E0200_UnexpectedToken,
                    format!("expected {expected}"),
                )
                .with_span(span),
            );
            false
        }
    }

    // ------------------------------------------------------------------
    // Names and identifiers
    // ------------------------------------------------------------------

    /// Consume one identifier token and produce an [`Ident`]. Emits
    /// `E0200_UnexpectedToken` and returns `None` if the current token
    /// isn't an identifier.
    pub(crate) fn parse_ident(&mut self) -> Option<Ident> {
        let span = self.peek_span();
        if let TokenKind::Ident(text) = self.peek() {
            let text = text.clone();
            self.advance();
            Some(Ident { text, span })
        } else {
            self.diagnostics.push(
                Diagnostic::error(code::Code::E0200_UnexpectedToken, "expected identifier")
                    .with_span(span),
            );
            None
        }
    }

    /// Consume a **member name** — an identifier OR a keyword used as one.
    ///
    /// After a `.` / `?.` (and in a few other member positions) the grammar is
    /// unambiguous: whatever follows names a field or method, never a statement.
    /// So a token that is a reserved keyword in *statement* position (`default`,
    /// `type`, `match`, `loop`, `box`, …) is just a plain member name here. This
    /// is what lets Jux call Rust APIs whose members collide with Jux keywords
    /// (`WindowOptions.default()`); the keyword-ness is purely contextual. Falls
    /// back to [`Self::parse_ident`] (and its diagnostic) for anything else.
    pub(crate) fn parse_member_name(&mut self) -> Option<Ident> {
        if let TokenKind::Kw(kw) = self.peek() {
            let span = self.peek_span();
            let text = kw.as_str().to_string();
            self.advance();
            return Some(Ident { text, span });
        }
        self.parse_ident()
    }
}
