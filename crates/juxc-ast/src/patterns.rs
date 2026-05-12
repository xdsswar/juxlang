//! Pattern-matching AST nodes — the `switch` expression and the patterns
//! that drive its arms.
//!
//! References:
//! - [`crate::Expr`] (mutually recursive — a `switch` body is an `Expr`,
//!   and `Expr::Switch` wraps a [`SwitchExpr`]).
//! - [`crate::Literal`] for literal patterns.
//! - [`crate::Ident`] / [`crate::QualifiedName`] for binding names and
//!   variant paths.
//! - [`crate::Block`] for block-form switch arm bodies.

use juxc_source::Span;

use crate::common::{Ident, QualifiedName};
use crate::exprs::Expr;
use crate::literals::Literal;
use crate::stmts::Block;

/// `switch (scrutinee) { case PATTERN guard? -> body; … }` per §A.2.8.
///
/// **Turn-1 scope** (this revision):
/// - Patterns supported: literal, wildcard `_`, bind `var name`, and
///   enum-variant (`Color.Red`, `Token.Number(_)`, `Token.Word(var s)`).
/// - `default -> body` arms (synonym for `_`).
/// - Single-expression bodies (`-> expr ;`) and block bodies (`-> { … }`).
/// - No `when` guards yet (already a keyword; parser would extend
///   trivially). No exhaustiveness checking — Rust's `match` enforces
///   that at the lowered level.
#[derive(Debug, Clone)]
pub struct SwitchExpr {
    /// The expression being matched on.
    pub scrutinee: Box<Expr>,
    /// Arms in source order. Order matters — Rust's `match` tries
    /// arms top-to-bottom, so user-visible arm order maps directly.
    pub arms: Vec<SwitchArm>,
    /// Span of the whole `switch (…) { … }` form.
    pub span: Span,
}

/// One arm of a `switch`: `case PATTERN -> BODY` or `default -> BODY`.
#[derive(Debug, Clone)]
pub struct SwitchArm {
    /// Pattern this arm matches against the scrutinee.
    pub pattern: Pattern,
    /// What runs when the arm matches.
    pub body: SwitchBody,
    /// Span of the whole arm.
    pub span: Span,
}

/// An arm's right-hand side per §A.2.8 `switch-body` — either a
/// single expression (terminated with `;`) or a block.
#[derive(Debug, Clone)]
pub enum SwitchBody {
    /// `-> expr ;`. Evaluates to the value of the expression.
    Expr(Box<Expr>),
    /// `-> { stmts… }`. Evaluates to `()` (statement-form) or the
    /// trailing expression of the block (future expr-block extension).
    Block(Block),
}

/// One pattern shape per §A.3.
///
/// **Turn-1 scope** — only the four shapes we actively lower:
/// - [`Pattern::Wildcard`] — `_` or `default`.
/// - [`Pattern::Literal`] — `42`, `"hi"`, `true`, `null`.
/// - [`Pattern::Bind`] — `var name`. Binds the scrutinee.
/// - [`Pattern::EnumVariant`] — `Color.Red`, `Token.Number(_)`,
///   `Token.Word(var s)`. Path-qualified variant name with optional
///   nested sub-patterns.
///
/// Spec also defines tuple/record/range/or/type patterns; those land
/// when their use cases do.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// `_` — matches anything, binds nothing.
    Wildcard(Span),
    /// `42`, `"hi"`, `true`, `null` — matches only that literal value.
    Literal(Literal, Span),
    /// `var name` — matches anything, binds it to `name`. Compiles to
    /// a plain Rust irrefutable binding (`name`).
    Bind(Ident),
    /// `Path.Variant` or `Path.Variant(sub, sub, …)`.
    EnumVariant {
        /// Dotted path to the variant. Single segment for `Variant`
        /// (uncommon — bare names usually want bind/literal), or two
        /// segments for `Type.Variant`.
        path: QualifiedName,
        /// Nested sub-patterns. Empty for unit variants and for the
        /// no-parens form `Color.Red`.
        args: Vec<Pattern>,
        /// True when the source had parens — distinguishes the unit
        /// variant pattern `Color.Red` (no parens) from the tuple form
        /// `Color.Red()` (parens with no args). Backend treats both
        /// the same; the flag exists so a tycheck pass can warn.
        has_parens: bool,
        /// Span of the whole variant pattern.
        span: Span,
    },
}
