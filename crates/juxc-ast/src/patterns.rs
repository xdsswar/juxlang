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
    /// Optional `when <cond>` guard (§A.2.8): the arm matches only
    /// when the pattern matches AND the guard evaluates true. Pattern
    /// bindings are in scope inside the guard. Guarded arms don't
    /// count toward exhaustiveness (§T.5.6).
    pub guard: Option<Expr>,
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
/// - [`Pattern::Wildcard`] — `_` or `default`.
/// - [`Pattern::Literal`] — `42`, `"hi"`, `true`, `null`.
/// - [`Pattern::Bind`] — `var name`. Binds the scrutinee.
/// - [`Pattern::EnumVariant`] — every `Name(sub, …)` / `Name` shape: an enum
///   variant (`Color.Red`, `Token.Word(var s)`), a record (`Point(var x, 0)`),
///   or a sealed subclass. The grammar gives record and enum patterns one
///   shape, so which it is follows from what the path resolves to, not from
///   the syntax.
/// - [`Pattern::Tuple`] — `(p, q, …)`.
/// - [`Pattern::Range`], [`Pattern::Or`], [`Pattern::TypeBind`].
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
    /// Range pattern (M.6.4): `case 0..10 ->`, `case 'a'..='z' ->`, and the
    /// open forms `case 100.. ->` (100 or more), `case ..0 ->` (below 0) and
    /// `case ..=0 ->` (0 or less). Maps to Rust's range patterns. Endpoints
    /// are literals (a variable is not a pattern), possibly negative.
    Range {
        /// Lower bound, always inclusive; `None` for `..x` / `..=x`.
        start: Option<Literal>,
        /// Upper bound, inclusive when `inclusive` (`..=`) and exclusive
        /// otherwise (`..`); `None` for the open `x..`.
        end: Option<Literal>,
        /// `..=` (true) or `..` (false).
        inclusive: bool,
        /// Span covering the whole range form.
        span: Span,
    },
    /// Type-test bind pattern — `case Sub ident ->`. A bare
    /// identifier name followed by another identifier, with no
    /// parens. Equivalent to `Sub(var ident)` for sealed-class
    /// hierarchies (the `ident` binds to the matched variant's
    /// underlying struct). Matches Java 21's record/type-pattern
    /// shape: `case Box b -> ...`.
    /// Tuple pattern — `(p, q, …)` with at least two elements (§A.3
    /// `tuple-pattern`). Matches a tuple value element by element.
    Tuple(Vec<Pattern>, Span),
    /// Or-pattern — `case A | B | C ->` (§A.3). Matches when ANY
    /// alternative matches. Alternatives may bind names only when every
    /// alternative binds the same names with the same types
    /// (`case Num(var n) | Neg(var n) ->`), so the arm body sees one `n`
    /// whichever alternative matched; tycheck raises E0447 otherwise.
    Or(Vec<Pattern>, Span),
    TypeBind {
        /// The class name being matched on.
        type_name: Ident,
        /// The identifier binding the matched value.
        binder: Ident,
        /// Span covering `Type ident`.
        span: Span,
    },
}

impl Pattern {
    /// The source span the whole pattern covers.
    pub fn span(&self) -> Span {
        match self {
            Pattern::Wildcard(span)
            | Pattern::Literal(_, span)
            | Pattern::Tuple(_, span)
            | Pattern::Or(_, span)
            | Pattern::EnumVariant { span, .. }
            | Pattern::Range { span, .. }
            | Pattern::TypeBind { span, .. } => *span,
            Pattern::Bind(ident) => ident.span,
        }
    }

    /// Every name this pattern binds, in source order and at any depth:
    /// each `var x`, and the binder of a type pattern `Type t`. An
    /// or-pattern contributes every alternative's binders, so the same
    /// name can appear once per alternative; a caller that wants one copy
    /// of each dedupes by text.
    pub fn binders(&self) -> Vec<&Ident> {
        let mut out = Vec::new();
        self.collect_binders(&mut out);
        out
    }

    /// The walk behind [`Pattern::binders`].
    fn collect_binders<'a>(&'a self, out: &mut Vec<&'a Ident>) {
        match self {
            Pattern::Bind(name) => out.push(name),
            Pattern::TypeBind { binder, .. } => out.push(binder),
            Pattern::EnumVariant { args: parts, .. } | Pattern::Tuple(parts, _) | Pattern::Or(parts, _) => {
                for part in parts {
                    part.collect_binders(out);
                }
            }
            Pattern::Wildcard(_) | Pattern::Literal(..) | Pattern::Range { .. } => {}
        }
    }
}
