//! Statement AST nodes — control-flow constructs, local-variable decls,
//! and assignment.
//!
//! References:
//! - [`crate::Expr`] for the value-carrying parts of every statement.
//! - [`crate::Ident`] for local-variable names and loop-binding names.
//! - [`crate::TypeRef`] for optional declared types on locals and loop
//!   variables.

use juxc_source::Span;

use crate::common::Ident;
use crate::exprs::{BinaryOp, Expr};
use crate::types::TypeRef;

/// A brace-delimited block of statements.
///
/// Per §A.2.4: `block = '{' statement* '}'`.
#[derive(Debug, Clone)]
pub struct Block {
    /// Statements in source order.
    pub statements: Vec<Stmt>,
    /// Span covering both braces and everything between.
    pub span: Span,
}

/// A single statement. Per §A.2.8 this is a large sum type; coverage
/// grows as the milestones do.
#[derive(Debug, Clone)]
pub enum Stmt {
    /// `expr ;` — an expression evaluated for its side effects.
    Expr(Expr),
    /// `return [expr] ;`. The [`Span`] covers the `return` keyword (and the
    /// value when present) so diagnostics on the statement — notably a bare
    /// `return;` in a non-`void` function — carry a source location for the IDE.
    Return(Option<Expr>, Span),
    /// `var name = expr ;` — a local variable declaration with type
    /// inference. See [`VarDecl`].
    VarDecl(VarDecl),
    /// `if (cond) block (else …)?` — see [`IfStmt`].
    If(IfStmt),
    /// `while (cond) block` — see [`WhileStmt`].
    While(WhileStmt),
    /// `do block while (cond);` per §A.2.8 — the body runs at least
    /// once; the condition is evaluated AFTER each iteration. See
    /// [`DoWhileStmt`].
    DoWhile(DoWhileStmt),
    /// `for (var name : iter) block` (or `for (Type name : iter) block`).
    /// See [`ForEachStmt`]. Java-style "enhanced for" / Kotlin-style
    /// for-each; this is the only `for` form currently parsed.
    /// C-style `for (init; cond; update)` lands later.
    ForEach(ForEachStmt),
    /// `name = expr ;` — assignment to a previously-declared `var`. See
    /// [`AssignStmt`]. Per §A.2.9 assignment is technically an expression,
    /// but we model it as a statement here so the backend lowers it
    /// directly to a Rust statement and not a value-bearing expression.
    Assign(AssignStmt),
    /// `break [label] ;` — exit the innermost enclosing loop, or the
    /// loop named by `label` (§A.2.8 `break-stmt = 'break' identifier? ';'`).
    Break(Option<Ident>, Span),
    /// `continue [label] ;` — skip to the next iteration of the
    /// innermost enclosing loop, or of the loop named by `label`.
    Continue(Option<Ident>, Span),
    /// `label: <loop>` — a labeled loop statement (§A.2.8). The inner
    /// statement is always one of the loop forms (the parser enforces
    /// it); `break label;` / `continue label;` target it. Lowers to a
    /// Rust loop label (`'label: while …`).
    Labeled {
        /// The label name (no trailing `:`).
        label: Ident,
        /// The labeled loop statement (While / DoWhile / ForEach / ForC).
        stmt: Box<Stmt>,
    },
    /// `super(args) ;` — parent-constructor delegation per §7.3.1.
    /// Must be the first statement of a constructor body (the parser
    /// enforces this when more validation lands). Lowered by the
    /// backend into the `__parent: Parent::new(args)` slot of the
    /// child's struct literal.
    SuperCall(Vec<Expr>, Span),
    /// `throw <expr> ;` — raise an exception per
    /// `JUX-EXCEPTIONS-ADDENDUM.md` §X.2. Phase-1 lowering panics
    /// the thread with the expression's `Display` rendering; full
    /// typed-exception semantics land with the Result-mode pass.
    Throw(Expr, Span),
    /// `try { B } catch (T name) { B } ... [finally { B }]`. Per
    /// §X.3. Phase-1 lowering wraps the try block in
    /// `std::panic::catch_unwind` and binds the caught name to
    /// the panic payload's `Display` string regardless of `T`.
    Try(TryStmt),
    /// `unsafe { B }` — an unsafe block per grammar §A.2.8
    /// (`unsafe-stmt = 'unsafe' block`). Inside it, calls to `unsafe`
    /// functions and the raw-pointer operators (`*p`, `&x`) are permitted;
    /// the body lowers verbatim to a Rust `unsafe { … }` block.
    Unsafe(Block),
    /// `for (init; cond; update) block` — the C-style counted loop per
    /// §A.2.8. Distinct from the enhanced [`Self::ForEach`] form. See
    /// [`ForCStmt`].
    ForC(ForCStmt),
    // Switch, …
}

/// `for ( init? ; cond? ; update? ) block` — the C-style three-clause loop.
///
/// All three header clauses are optional (`for (;;)` is an infinite loop).
/// `init` and `update` are modeled as statements (a `var`/typed local decl or
/// an assignment / expression); `cond` is a boolean expression.
#[derive(Debug, Clone)]
pub struct ForCStmt {
    /// Initializer — typically a local declaration (`int i = 0`) or an
    /// assignment. `None` for an empty init clause.
    pub init: Option<Box<Stmt>>,
    /// Loop condition, re-checked before each iteration. `None` (empty
    /// clause) means "always true".
    pub cond: Option<Expr>,
    /// Update step, run after each iteration (and on `continue`). Usually an
    /// assignment (`i = i + 1`). `None` for an empty update clause.
    pub update: Option<Box<Stmt>>,
    /// The loop body.
    pub body: Block,
    /// Span of the whole `for (…) { … }`.
    pub span: Span,
}

/// `try B0 catch (T1 e1) B1 catch (T2 e2) B2 ... finally Bf` —
/// the statement form of try/catch per spec §X.3.1.
#[derive(Debug, Clone)]
pub struct TryStmt {
    /// `B0` — the body that may throw.
    pub body: Block,
    /// `catch (T eᵢ) Bᵢ` clauses in source order.
    pub catches: Vec<CatchClause>,
    /// Optional `finally { Bf }` block. `None` when omitted.
    pub finally: Option<Block>,
    /// Span covering `try { … } …`.
    pub span: Span,
}

/// One `catch (T name) { ... }` clause. The declared type drives
/// the diagnostic (and, in the future, the runtime type filter);
/// Phase-1 lowering catches any panic and binds `name` to the
/// panic message as a String, ignoring the declared `T`.
#[derive(Debug, Clone)]
pub struct CatchClause {
    /// Declared exception type — `IOException`, `MyError`, …
    /// For a multi-catch (`catch (E1 | E2 e)`, §X.3.6) this is the
    /// FIRST listed type; the rest live in [`Self::alt_tys`].
    pub ty: TypeRef,
    /// Additional `|`-separated types of a multi-catch clause —
    /// empty for the ordinary single-type form. The listed types
    /// must be pairwise unrelated (E0721); the bound name's static
    /// type is their most specific common supertype.
    pub alt_tys: Vec<TypeRef>,
    /// Bound name inside the catch block.
    pub name: Ident,
    /// Body executed when the catch matches.
    pub body: Block,
    /// Span of the whole clause.
    pub span: Span,
}

/// A `var` local-variable declaration. Per §A.2.8:
/// ```text
/// local-decl = ( 'var' | binding-immut 'var' | type | binding-immut type ) identifier
///              ( '=' expression )? ';'
/// ```
///
/// Both inferred (`var name = expr`) and typed (`Type name [= expr]`)
/// forms are modeled by this single shape; the `ty` field distinguishes
/// them. A leading `final` or `const` modifier sets [`Self::is_final`].
#[derive(Debug, Clone)]
pub struct VarDecl {
    /// Variable name.
    pub name: Ident,
    /// Optional declared type. `None` means the type is inferred from
    /// the initializer (Java/Kotlin/C# `var`).
    pub ty: Option<TypeRef>,
    /// Initializer expression. `None` is permitted by the grammar but
    /// not yet by us; once we support it, definite-assignment analysis
    /// in a later phase enforces use-before-assign.
    pub init: Option<Expr>,
    /// `true` when the declaration carried a leading `final` or `const`
    /// modifier (per `JUX-LANG-V1.md` §549–565). Reassignment of a
    /// `final` local should be a tycheck error; enforcement lands in
    /// tycheck once this bit is consumed there. `const` currently
    /// parses identically to `final` — the compile-time-constant
    /// distinction is deferred until we need it.
    pub is_final: bool,
    /// `ref` binding mode (§M.13) — the local holds a SHARED reference
    /// to a value-typed object (`Rc<RefCell<T>>`): aliases see each
    /// other's writes, assignment stores through.
    pub is_ref: bool,
    /// Span covering `[modifier] (type | 'var') name [= init] ;`.
    pub span: Span,
}

/// `if (cond) block (else (if-stmt | block))?` per §A.2.8.
///
/// Else-if chains are represented as a recursive [`ElseBranch::If`] —
/// `if a {} else if b {} else {}` becomes
/// `IfStmt { else_branch: Some(ElseBranch::If(IfStmt { else_branch: Some(ElseBranch::Block(…)) })) }`.
#[derive(Debug, Clone)]
pub struct IfStmt {
    /// The boolean condition; type checked in a later phase.
    pub condition: Expr,
    /// Block executed when `condition` is true.
    pub then_block: Block,
    /// Optional else clause, possibly another `if` for an else-if chain.
    pub else_branch: Option<Box<ElseBranch>>,
    /// Span of the entire `if` statement.
    pub span: Span,
}

/// What follows an `else`: either another `if` (for `else if` chains)
/// or a plain `{ … }` block.
#[derive(Debug, Clone)]
pub enum ElseBranch {
    /// `else if (…) { … }` — chained condition.
    If(IfStmt),
    /// `else { … }` — terminal block.
    Block(Block),
}

/// `while (condition) block` per §A.2.8.
///
/// The body is always a brace-delimited block; we don't accept
/// single-statement bodies without braces (matches `if` for now).
#[derive(Debug, Clone)]
pub struct WhileStmt {
    /// Boolean loop condition. Re-evaluated before every iteration.
    pub condition: Expr,
    /// The block executed each iteration while `condition` is true.
    pub body: Block,
    /// Span of the entire `while` statement.
    pub span: Span,
}

/// `do block while (cond);` per §A.2.8 `do-while-stmt`. Java
/// semantics: the body executes once before the first condition
/// check, then repeats while the condition holds. Lowered to a Rust
/// `loop { body; if !(cond) { break; } }`.
#[derive(Debug, Clone)]
pub struct DoWhileStmt {
    /// The block executed each iteration (at least once).
    pub body: Block,
    /// Boolean loop condition, evaluated AFTER each iteration.
    pub condition: Expr,
    /// Span of the entire `do … while (…);` statement.
    pub span: Span,
}

/// `for ( (var | Type) name : iter ) block` per §A.2.8.
///
/// The loop variable is bound to each element of `iter` in turn. When
/// `var_type` is `None`, the binding's type is inferred from the
/// iterator's element type (the `var` form). Otherwise the user has
/// written an explicit type — `int i`, `String s`, etc.
///
/// Notes:
/// - The iterator may be a [`crate::RangeExpr`] (`0..10`), a collection,
///   or anything else that implements the to-be-specified iterator
///   protocol.
/// - The body opens a fresh scope; the loop variable is visible only
///   inside `body`.
#[derive(Debug, Clone)]
pub struct ForEachStmt {
    /// True for the async form `for await (var x : stream)` (§18.6.3):
    /// the iterator is a `Stream<T>` and each element is produced by an
    /// awaited `next()` call. Only legal inside an async context
    /// (E0703); the iterable must be a stream (E0704).
    pub is_await: bool,
    /// Optional declared type of the loop variable. `None` for the
    /// inference-form `var i : …`.
    pub var_type: Option<TypeRef>,
    /// The loop variable's name.
    pub var_name: Ident,
    /// The iterator expression — evaluated once before the loop starts.
    pub iter: Expr,
    /// The body block, executed once per element.
    pub body: Block,
    /// Span of the entire `for` statement.
    pub span: Span,
}

/// `target = expr ;` per §A.2.9.
///
/// **Lvalue forms currently supported by the parser:**
/// - `name = value;` — simple variable assignment (`Expr::Path` with a
///   single segment).
/// - `arr[i] = value;` — array element assignment (`Expr::Index`).
/// - `obj.field = value;` — field assignment (`Expr::Field`).
///
/// **Compound assignment** (`x += y`, `x *= y`, …) preserves the
/// operator on the AssignStmt rather than desugaring at parse time.
/// The backend lowers `x += y` directly to Rust's `+=`, which evaluates
/// the lvalue exactly once even for side-effecting shapes like
/// `arr[next()] += 1`. Plain `x = y` carries `op = None`.
#[derive(Debug, Clone)]
pub struct AssignStmt {
    /// The lvalue being assigned to. Must be one of the parser-validated
    /// lvalue shapes listed above.
    pub target: Expr,
    /// Compound-assignment operator (`+=`, `-=`, …) or `None` for a
    /// plain `=`. Stored as a [`BinaryOp`] for type-uniformity with
    /// the regular binary path: tycheck reuses its op-typing rules
    /// and the backend reuses its op-spelling table when emitting
    /// the matching `target op= value` form.
    pub op: Option<BinaryOp>,
    /// New value (right-hand side of the operator).
    pub value: Expr,
    /// Span covering `target = value ;`.
    pub span: Span,
}
