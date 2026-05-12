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
    /// `return [expr] ;`.
    Return(Option<Expr>),
    /// `var name = expr ;` — a local variable declaration with type
    /// inference. See [`VarDecl`].
    VarDecl(VarDecl),
    /// `if (cond) block (else …)?` — see [`IfStmt`].
    If(IfStmt),
    /// `while (cond) block` — see [`WhileStmt`].
    While(WhileStmt),
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
    /// `break ;` — exit the innermost enclosing loop. Label-targeted
    /// `break <label>;` per §A.2.8 is a future addition.
    Break(Span),
    /// `continue ;` — skip to the next iteration of the innermost
    /// enclosing loop. Label-targeted form is a future addition.
    Continue(Span),
    /// `super(args) ;` — parent-constructor delegation per §7.3.1.
    /// Must be the first statement of a constructor body (the parser
    /// enforces this when more validation lands). Lowered by the
    /// backend into the `__parent: Parent::new(args)` slot of the
    /// child's struct literal.
    SuperCall(Vec<Expr>, Span),
    // For, ForEach, Switch, Throw, Try, Unsafe, …
}

/// A `var` local-variable declaration. Per §A.2.8:
/// ```text
/// local-decl = ( 'var' | binding-immut 'var' | type | binding-immut type ) identifier
///              ( '=' expression )? ';'
/// ```
///
/// For now we only model the `var name = expr` form. `final var`, the
/// Java-style `Type name`, and uninitialized declarations come later.
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
    /// Span covering `var … ;`.
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
