//! Expression AST nodes — values produced by evaluating an expression in
//! the source language. Covers literals, paths, calls, binary/unary ops,
//! casts, ranges, array constructors, member/index access, interpolated
//! strings, `this`, object construction, and `switch` expressions.
//!
//! References:
//! - [`crate::Literal`] for literal payloads.
//! - [`crate::Ident`] / [`crate::QualifiedName`] for paths and names.
//! - [`crate::TypeRef`] for casts, generic args, and array element types.
//! - [`crate::Pattern`] / [`crate::SwitchExpr`] etc. — mutually recursive
//!   with [`Expr`] through [`Expr::Switch`].

use juxc_source::Span;

use crate::common::{Ident, QualifiedName};
use crate::literals::Literal;
use crate::patterns::SwitchExpr;
use crate::types::TypeRef;

/// An expression. Per §A.2.9 this is a deep precedence-layered grammar;
/// we add layers as features need them.
#[derive(Debug, Clone)]
pub enum Expr {
    /// A literal — `42`, `"hi"`, `true`, `null`.
    Literal(Literal),
    /// A path — `foo` or `foo.bar.baz`. The resolver binds these to
    /// concrete definitions.
    Path(QualifiedName),
    /// A call expression — `callee(args…)`.
    Call(CallExpr),
    /// A binary expression — `left op right`. Precedence is encoded by
    /// the parser; the AST just stores the operator and its operands.
    Binary(BinaryExpr),
    /// A prefix unary expression — `op operand`. See [`UnaryExpr`].
    Unary(UnaryExpr),
    /// A range expression — `start..end` or `start..=end`. See [`RangeExpr`].
    Range(RangeExpr),
    /// A cast expression — `value as Type`. See [`CastExpr`].
    Cast(CastExpr),
    /// A `sizeof(...)` compile-time type query. See [`SizeOfExpr`].
    SizeOf(SizeOfExpr),
    /// `new T[N]` — fixed-size array creation. See [`NewArrayExpr`].
    NewArray(NewArrayExpr),
    /// `new T[]{a, b, c}` — array literal with explicit element type
    /// and inferred size. See [`NewArrayLitExpr`].
    NewArrayLit(NewArrayLitExpr),
    /// `array[index]` — element access. See [`IndexExpr`].
    Index(IndexExpr),
    /// `object.field` — member access (e.g. `arr.length`). See [`FieldExpr`].
    Field(FieldExpr),
    /// `$"…$name…${expr}…"` — interpolated string per §3.4. See [`InterpStringExpr`].
    InterpString(InterpStringExpr),
    /// `this` — the implicit receiver inside a class constructor or
    /// instance method per §7.3. Lowers to Rust `self` (in a method) or
    /// `__self` (in a constructor's struct-builder pattern).
    This(Span),
    /// `x => Type` / `x => Type binder` — the type-test operator (§T.1.4).
    /// Evaluates to `bool` (true when `x`'s runtime type is `Type` or a
    /// subtype). The bound form introduces `binder: Type` as a smart-cast in
    /// the enclosing `if`'s then-branch (`if (x => Dog d) { d.bark(); }`).
    TypeTest(TypeTestExpr),
    /// `super` — a reference to the superclass slice, valid only as the
    /// receiver of a method call (`super.method(args)`, §6.9.4). It resolves
    /// **statically** to the nearest concrete ancestor's version of the named
    /// method (bypassing virtual dispatch for that one call). A bare `super`
    /// with no `.method(...)` is rejected.
    Super(Span),
    /// `new ClassName(args)` — class instantiation per §7.3.1. See [`NewObjectExpr`].
    NewObject(NewObjectExpr),
    /// `switch (scrutinee) { case PATTERN -> body; … }` per §A.2.8.
    /// See [`SwitchExpr`]. The same node serves both statement-form and
    /// expression-form switch — context decides whether the resulting
    /// value is used.
    Switch(SwitchExpr),
    /// `(args) -> body` or `x -> body` per grammar §A.2.9. See
    /// [`LambdaExpr`]. Lowers to a Rust closure (`|args| body`).
    Lambda(LambdaExpr),
    /// `a ?: b` — null-coalescing per §7.10 / §A.4 level 3. `a` is
    /// a nullable expression (`T?`); when non-null its inner `T`
    /// is the result, else `b` (also of type `T`) provides the
    /// fallback. Backend lowers to `a.unwrap_or(b)`.
    Elvis(ElvisExpr),
    /// `Type::method` — method reference per §A.4 level 20. Produces
    /// a function-typed value bound to the named member: instance
    /// methods lower to `|x| x.method()`, static methods to
    /// `Type::method` directly. Common shape for higher-order
    /// callbacks: `users.forEach(User::greet)`.
    MethodRef(MethodRefExpr),
    /// `cond ? then : else` — ternary expression per §A.4 level 2.
    /// Right-associative; the two branches must produce types
    /// that unify. Backend lowers to Rust's `if cond { then }
    /// else { else }` expression form.
    Ternary(TernaryExpr),
    /// `await expr` — suspend the current async function until the
    /// future produced by `expr` resolves, then yield its value.
    /// Per JUX-ASYNC-ADDENDUM v2 §A.2 this is a *prefix unary form*
    /// (the keyword sits at expression level, not statement level)
    /// that may only appear inside an `async` function body. The
    /// backend lowers it to Rust's postfix `.await`. The boxed
    /// `Expr` is the awaited operand; the `Span` covers
    /// `await expr` in source.
    Await(Box<Expr>, Span),
    /// `(a, b, ...)` — tuple literal (§5.3). Two or more elements
    /// (`()` is reserved unit syntax with no v1 meaning; one element
    /// is plain grouping). Lowers verbatim to a Rust tuple — value
    /// semantics, stack-allocated, no boxing.
    TupleLit(Vec<Expr>, Span),
    /// `try { … } catch (E e) { … }` — **try-expression** (§X.3.1/
    /// §X.3.3): evaluates to the try block's trailing expression, or
    /// the matching catch block's trailing expression. No `finally`
    /// (the grammar reserves the value form for pure
    /// produce-or-recover shapes). Reuses [`crate::TryStmt`] for the
    /// clause structure; the parser guarantees `finally` is `None`
    /// and at least one catch exists.
    TryExpr(Box<crate::stmts::TryStmt>),
    /// `expr?` — **error propagation** (§X.4.1, postfix level 19).
    /// On a `Result<T, E>` operand: `Ok(v)` yields `v`, `Err(e)`
    /// returns `Err(e)` from the enclosing function. On a `T?`
    /// operand: a value yields it, `null` returns `null`. The
    /// enclosing return type must match the propagation shape
    /// (E0730/E0731).
    ErrorProp(Box<Expr>, Span),
    /// `expr!!` — **non-null assertion** (grammar §A.4 level 19,
    /// postfix; conversion table `T?` → `T!!`). Asserts the nullable
    /// operand holds a value and unwraps it; a `null` operand throws
    /// `NullPointerException` at run time. Chains like any postfix
    /// (`a.peer!!.id`). The boxed `Expr` is the asserted operand; the
    /// `Span` covers `expr!!`.
    NotNullAssert(Box<Expr>, Span),
}

/// Ternary expression: `condition ? then_branch : else_branch`.
/// Right-associative — `a ? b : c ? d : e` parses as
/// `a ? b : (c ? d : e)`. The condition must be a `bool`; the
/// branches' types unify under the surrounding context (tycheck
/// rules in §T.5).
#[derive(Debug, Clone)]
pub struct TernaryExpr {
    /// Bool-valued condition expression.
    pub condition: Box<Expr>,
    /// Value when the condition is `true`.
    pub then_branch: Box<Expr>,
    /// Value when the condition is `false`.
    pub else_branch: Box<Expr>,
    /// Span covering `condition ? then : else`.
    pub span: Span,
}

/// Method-reference expression: `ReceiverType::memberName`.
/// `receiver` is the qualified-name on the LHS of `::`, `member`
/// the bare identifier after it. Result is a function value whose
/// shape mirrors the referenced member: an instance method
/// `(T) -> R`, a static method `(args…) -> R`.
#[derive(Debug, Clone)]
pub struct MethodRefExpr {
    /// Type/class/path on the LHS of `::`.
    pub receiver: QualifiedName,
    /// Member name on the RHS of `::`.
    pub member: Ident,
    /// Span covering the whole `Receiver::member` form.
    pub span: Span,
}

/// Elvis (null-coalescing) operator: `value ?: fallback`. Returns
/// the unwrapped `T` if `value` is non-null, else `fallback`.
/// Right-associative per the grammar table; the parser builds the
/// chain so `a ?: b ?: c` is `a ?: (b ?: c)`.
#[derive(Debug, Clone)]
pub struct ElvisExpr {
    /// Nullable left-hand value being unwrapped.
    pub value: Box<Expr>,
    /// Non-nullable fallback used when `value` is null.
    pub fallback: Box<Expr>,
    /// Span covering `value ?: fallback`.
    pub span: Span,
}

/// Lambda expression per grammar §A.2.9:
/// ```text
/// lambda       = 'async'? '(' lambda-params? ')' '->' lambda-body
///              | 'async'? identifier '->' lambda-body  -- single-param
/// lambda-body  = expression | block
/// lambda-param = type? identifier
/// ```
///
/// **Phase-1 scope.** `async` lambdas parse but are not yet wired
/// to a runtime — the marker is stored for forward-compat. Closure
/// capture semantics are left to Rust's borrow-checker (`move`
/// vs borrow is inferred at the emit site).
#[derive(Debug, Clone)]
pub struct LambdaExpr {
    /// Whether the user wrote `async`. Currently informational —
    /// `async` lambdas need a runtime decision that's still ahead.
    pub is_async: bool,
    /// Formal parameters in declaration order. Each may carry an
    /// explicit type (`int x`) or be untyped (`x`); untyped params
    /// fall through to Rust's closure-parameter inference.
    pub params: Vec<LambdaParam>,
    /// Body — either a single expression (`x -> x * 2`) or a block
    /// (`(x) -> { … }`).
    pub body: LambdaBody,
    /// Span of the whole lambda.
    pub span: Span,
}

/// One lambda parameter — optional type, mandatory name.
#[derive(Debug, Clone)]
pub struct LambdaParam {
    /// Declared type, if the user wrote `(int x) -> …`. Absent in
    /// the untyped form `x -> …`.
    pub ty: Option<TypeRef>,
    /// Parameter name.
    pub name: Ident,
    /// Span covering the param.
    pub span: Span,
}

/// Lambda body — single expression or block.
#[derive(Debug, Clone)]
pub enum LambdaBody {
    /// `x -> x + 1`.
    Expr(Box<Expr>),
    /// `(x) -> { … return x + 1; }`.
    Block(Box<crate::stmts::Block>),
}

/// `new ClassName(args)` per §7.3.1 — invokes a class's constructor and
/// produces a fresh instance. Lowers to Rust `ClassName::new(args)`,
/// or `ClassName::<T1, …>::new(args)` when `generic_args` is non-empty.
#[derive(Debug, Clone)]
pub struct NewObjectExpr {
    /// The class being constructed.
    pub class_name: QualifiedName,
    /// Explicit generic-args list — `<int, String>` in
    /// `new Map<int, String>(…)`. Empty when the user lets Rust
    /// infer (`new Box(42)` ≈ `Box::new(42)`).
    pub generic_args: Vec<TypeRef>,
    /// Constructor arguments in source order.
    pub args: Vec<Expr>,
    /// Per-argument label, parallel to [`Self::args`] — same named
    /// argument / default-filling story as [`CallExpr::arg_names`].
    pub arg_names: Vec<Option<Ident>>,
    /// Anonymous-class body — `Some(_)` when the user wrote
    /// `new Iface() { method overrides }` per spec §1379's
    /// anonymous-class form. Holds method declarations PLUS the
    /// instance-initializer-block statements (Java's bare
    /// `{ … }` at class-body level — the only constructor-like
    /// setup hook anonymous classes have). Fields, named
    /// constructors, and static members are rejected by the
    /// parser. `None` for the regular `new T(args)` form.
    pub anonymous_body: Option<AnonymousBody>,
    /// Span of the whole `new T(args) [ { body } ]` form.
    pub span: Span,
}

/// Parsed body of an anonymous-class instantiation. Carries the
/// user's method overrides plus any instance-initializer blocks
/// (bare `{ … }` at class-body level — Java's only constructor
/// hook for anonymous classes). The two are stored separately
/// because they execute at different points: init blocks run
/// once, when the instance is created; method bodies run on
/// each call.
#[derive(Debug, Clone)]
pub struct AnonymousBody {
    /// Instance-initializer blocks in source order. Each runs
    /// once at instantiation time, before the synthetic struct
    /// value is returned.
    pub init_blocks: Vec<crate::Block>,
    /// Method-override declarations. Each becomes a `fn` on the
    /// synthetic impl block. Tycheck enforces that abstract
    /// methods of the target are covered.
    pub methods: Vec<crate::FnDecl>,
}

/// `$"...${expr}..."` per §3.4. Holds the parsed segments of an
/// interpolated string literal — alternating literal-text chunks and
/// embedded value expressions. The lexer captures the raw text and the
/// parser splits it into this segment list (recursively parsing the
/// expressions inside each `${...}`).
///
/// Lowers in the backend to a Rust `format!("…", arg, arg, …)` call.
#[derive(Debug, Clone)]
pub struct InterpStringExpr {
    /// Segments in source order. May be empty for the literal `$""`.
    pub segments: Vec<InterpSegment>,
    /// Span of the whole `$"…"` literal, including the leading `$` and
    /// the enclosing quotes.
    pub span: Span,
}

/// One segment of an interpolated string literal.
///
/// Three shapes per §3.4:
/// - **Literal** — plain text between the quotes (and between/after
///   any interpolation markers). Carries the source bytes as-is;
///   escape interpretation is the backend's job when it emits the
///   format string.
/// - **Bare** — a bare-identifier interpolation written as `$name`.
///   No braces. The identifier is captured directly.
/// - **Expr** — a `${expression}` interpolation. The expression has
///   already been parsed at AST-construction time.
#[derive(Debug, Clone)]
pub enum InterpSegment {
    /// Plain literal text. May contain backslash-escape sequences that
    /// the backend re-emits into the Rust format string.
    Literal(String),
    /// Bare-identifier interpolation: the `name` in `$name`.
    Bare(Ident),
    /// Expression interpolation: the `…` in `${…}`. Parsed eagerly so
    /// downstream phases (resolver, tycheck, backend) walk it like any
    /// other expression.
    Expr(Box<Expr>),
}

/// `object . field` per §A.2.9. Java-style member access. Today the only
/// supported field is `length` on array-typed expressions; class fields
/// land when classes do.
#[derive(Debug, Clone)]
pub struct FieldExpr {
    /// The expression whose field is being accessed.
    pub object: Box<Expr>,
    /// Name of the field. The backend special-cases known names
    /// (currently just `length`).
    pub field: Ident,
    /// True when the access used the `?.` safe-navigation form per
    /// §A.4 level 20 / §7.10. Plain `.` carries `false`; `?.`
    /// carries `true`. The backend lowers `obj?.field` to
    /// `obj.as_ref().map(|x| x.field.clone())` so the result is
    /// `Option<FieldType>` and chains propagate None through.
    pub safe: bool,
    /// Span of the whole `object.field` form.
    pub span: Span,
}

/// `new T[size]` per §A.2.9. Allocates a fresh array of the given size,
/// every element initialized to the element type's default value.
#[derive(Debug, Clone)]
pub struct NewArrayExpr {
    /// Element type (the `T` in `new T[size]`). Has no array_shape of
    /// its own — the array shape is implicit in this expression.
    pub element_type: TypeRef,
    /// Array size. Must be a compile-time constant for Phase-1 lowering
    /// (Rust's `[T; N]` requires a `const` length).
    pub size: Box<Expr>,
    /// Span of the whole `new T[size]` expression.
    pub span: Span,
}

/// Array initializer literal: either `new T[]{a, b, c}` (§A.2.9
/// new-expression form) or the bare `{a, b, c}` form valid in a typed-
/// local RHS where the LHS array type drives the lowering.
///
/// The `fixed` flag selects the Rust output shape:
///
/// - `fixed: false` → `vec![a, b, c]` (or `Vec::<T>::new()` empty).
///   Used for `new T[]{…}` and for bare `{…}` whose LHS is `T[]`.
/// - `fixed: true`  → `[a, b, c]` (Rust array literal).
///   Used for bare `{…}` whose LHS is `T[N]`. The compile-time size
///   match is enforced by Rust on the assignment.
///
/// The element type carries no array shape of its own — the outer
/// shape is implicit in this expression.
#[derive(Debug, Clone)]
pub struct NewArrayLitExpr {
    /// Element type (the `T` in `new T[]{…}` or in the LHS `T[N]`/`T[]`).
    pub element_type: TypeRef,
    /// Initializer elements, in source order. May be empty for
    /// `new T[]{}` (always `fixed: false` in that case).
    pub elements: Vec<Expr>,
    /// Lowering shape — see struct-level docs.
    pub fixed: bool,
    /// Span of the whole literal.
    pub span: Span,
}

/// `array[index]` per §A.2.9. Postfix expression — `arr[i]` reads the
/// `i`th element. The assignment form `arr[i] = v` is represented at the
/// statement level as `AssignStmt { target: Expr::Index(...), value: v }`.
#[derive(Debug, Clone)]
pub struct IndexExpr {
    /// The array expression being indexed.
    pub array: Box<Expr>,
    /// The index expression. Should evaluate to a `uint`-ish integer.
    pub index: Box<Expr>,
    /// Span of the whole `array[index]` form.
    pub span: Span,
}

/// `sizeof '(' (type | expression) ')'` per §5.9.
///
/// The parser stores the operand as a general expression — disambiguation
/// between the **type form** and the **value form** is done at lowering
/// time using the purely syntactic rule in §5.9.3:
///
/// 1. Primitive names (`int`, `bool`, `f64`, …) → type.
/// 2. Uppercase-leading single identifier → type.
/// 3. Lowercase-leading single identifier → value.
/// 4. Multi-segment path → type.
/// 5. Compound expression → value.
///
/// The result is always a `uint` (platform-sized unsigned).
#[derive(Debug, Clone)]
pub struct SizeOfExpr {
    /// The operand. Could be a type (Path) or a value-expression.
    pub operand: Box<Expr>,
    /// Span covering `sizeof(...)` whole.
    pub span: Span,
}

/// `value as Type` per §A.5.
///
/// Permitted conversions are enumerated in §A.5's table. We don't enforce
/// the conversion-validity check here at the AST level; that's a tycheck
/// concern. The parser only verifies the syntactic shape.
#[derive(Debug, Clone)]
pub struct CastExpr {
    /// The expression whose value is being cast.
    pub value: Box<Expr>,
    /// The target type.
    pub ty: TypeRef,
    /// Span covering the whole `value as Type`.
    pub span: Span,
}

/// `x => Type` / `x => Type binder` — the type-test operator (§T.1.4). Yields
/// `bool`; the optional `binder` makes it a smart-cast in the enclosing `if`'s
/// then-branch.
#[derive(Debug, Clone)]
pub struct TypeTestExpr {
    /// The value being tested.
    pub value: Box<Expr>,
    /// The type tested against.
    pub ty: TypeRef,
    /// Optional smart-cast binder (`x => Dog d` binds `d`). `None` for the
    /// bare boolean test (`x => Dog`).
    pub binder: Option<Ident>,
    /// Span covering the whole `value => Type [binder]`.
    pub span: Span,
}

/// `start..end` or `start..=end` per §A.2.9 level 13.
///
/// Open ranges (`0..`, `..10`, `..=10`) are pattern-only in v1 of the
/// spec, so we don't model them as expressions yet. The optional `step`
/// modifier is also deferred.
#[derive(Debug, Clone)]
pub struct RangeExpr {
    /// Lower bound (inclusive).
    pub start: Box<Expr>,
    /// Optional `step` expression (§M.6.3) — `0..n step 2`. Phase 1
    /// supports it on for-each headers over numeric ranges; the
    /// lowering is a sign-aware while loop (negative steps count
    /// down; a zero step panics).
    pub step: Option<Box<Expr>>,
    /// Upper bound — exclusive when `inclusive == false` (`..`),
    /// inclusive when `inclusive == true` (`..=`).
    pub end: Box<Expr>,
    /// `..=` (true) vs `..` (false). Reflected at lowering — Rust uses
    /// the same operator tokens with the same meanings.
    pub inclusive: bool,
    /// Span covering both bounds and the operator.
    pub span: Span,
}

/// `op operand` per §A.2.9 / §A.4 level 18.
///
/// Unary operators are prefix, right-associative (so `--x` parses as
/// `-(-x)`), and bind tighter than any binary operator currently
/// modeled.
#[derive(Debug, Clone)]
pub struct UnaryExpr {
    /// Which operator.
    pub op: UnaryOp,
    /// The operand expression.
    pub operand: Box<Expr>,
    /// Span covering operator + operand.
    pub span: Span,
}

/// The prefix unary operators modeled by the AST.
///
/// `+` (unary plus, no-op) and `move` from §A.4 are not modeled yet — they
/// land with their respective feature areas. `await` is modeled as its own
/// [`Expr::Await`] node rather than a `UnaryOp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// `-x` — arithmetic negation.
    Neg,
    /// `!x` — logical NOT (on bool operands).
    Not,
    /// `~x` — bitwise NOT (on integer operands).
    BitNot,
    /// `*p` — raw-pointer dereference (§A.2.9, `unsafe`-only). Lowers to
    /// Rust `*p`; reading/writing through it is only legal in an `unsafe`
    /// context.
    Deref,
    /// `&x` — address-of, producing a raw pointer (§A.2.9, `unsafe`-only).
    /// Lowers to Rust `core::ptr::addr_of_mut!(x)` (a `*mut T`), not a Rust
    /// reference. Only meaningful in an `unsafe` context.
    AddrOf,
}

impl UnaryOp {
    /// The Rust spelling of this operator for the simple prefix forms.
    /// `AddrOf` lowers to a macro call (`addr_of_mut!`) rather than a
    /// prefix token, so the backend special-cases it instead of using
    /// this string.
    ///
    /// **`BitNot → !`:** Rust spells bitwise NOT as `!` (the same token
    /// it uses for logical NOT), choosing operator by operand type.
    /// Jux distinguishes `~` and `!` syntactically; we lower both to
    /// Rust's `!` and let the operand type pick the meaning. The result
    /// is idiomatic Rust on both sides.
    pub fn as_rust_str(self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
            UnaryOp::BitNot => "!",
            UnaryOp::Deref => "*",
            UnaryOp::AddrOf => "&",
        }
    }
}

/// `left op right` per §A.2.9 / §A.4. The parser is responsible for
/// associativity and precedence; in the AST this is a flat triple.
#[derive(Debug, Clone)]
pub struct BinaryExpr {
    /// Which operator.
    pub op: BinaryOp,
    /// Left-hand operand.
    pub left: Box<Expr>,
    /// Right-hand operand.
    pub right: Box<Expr>,
    /// Span covering both operands and the operator.
    pub span: Span,
}

/// The binary operators currently modeled by the AST. Names map
/// directly onto the operator tokens emitted by the lexer.
///
/// Coverage so far: short-circuit logical, bitwise, shifts, equality,
/// comparison, and arithmetic. Three-way `<=>`, elvis `?:`, conditional
/// `? :`, range `..`/`..=`, type-test `=>` / `in`, and `as` come as
/// their features land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// `||` — short-circuit logical OR.
    Or,
    /// `&&` — short-circuit logical AND.
    And,
    /// `|` — bitwise OR. Per §A.4 looser than equality (Java/Python style).
    BitOr,
    /// `^` — bitwise XOR.
    BitXor,
    /// `&` — bitwise AND.
    BitAnd,
    /// `<<` — left shift.
    Shl,
    /// `>>` — right shift. Arithmetic on signed types, logical on unsigned.
    Shr,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%` — remainder.
    Rem,
    /// `==` — structural equality.
    Eq,
    /// `!=` — structural inequality.
    NotEq,
    /// `===` — reference identity (§T.1.4): true iff both operands are
    /// the SAME object (address identity), never structural. Not
    /// overridable.
    RefEq,
    /// `!==` — negated reference identity.
    RefNeq,
    /// `in` — containment test (§O.2.4): `x in xs`. Dispatches to the
    /// CONTAINER's `operator in`, with builtin lowerings for arrays,
    /// collections, ranges, and strings.
    In,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

impl BinaryOp {
    /// The Rust spelling of this operator. Useful for the Phase-1 backend
    /// since every operator in the current set maps 1-to-1 onto a Rust
    /// operator with identical semantics.
    pub fn as_rust_str(self) -> &'static str {
        match self {
            BinaryOp::Or     => "||",
            BinaryOp::And    => "&&",
            BinaryOp::BitOr  => "|",
            BinaryOp::BitXor => "^",
            BinaryOp::BitAnd => "&",
            BinaryOp::Shl    => "<<",
            BinaryOp::Shr    => ">>",
            BinaryOp::Add    => "+",
            BinaryOp::Sub    => "-",
            BinaryOp::Mul    => "*",
            BinaryOp::Div    => "/",
            BinaryOp::Rem    => "%",
            BinaryOp::Eq     => "==",
            BinaryOp::NotEq  => "!=",
            // Reference identity has no direct Rust operator token —
            // the backend special-cases it to `Rc::ptr_eq` /
            // `std::ptr::eq`; these spellings only surface in
            // diagnostics.
            BinaryOp::RefEq  => "===",
            BinaryOp::RefNeq => "!==",
            BinaryOp::In     => "in",
            BinaryOp::Lt     => "<",
            BinaryOp::Le     => "<=",
            BinaryOp::Gt     => ">",
            BinaryOp::Ge     => ">=",
        }
    }
}

/// `callee(args…)` per §A.2.9 postfix grammar.
#[derive(Debug, Clone)]
pub struct CallExpr {
    /// The callee expression (typically a path).
    pub callee: Box<Expr>,
    /// Explicit call-site type arguments — the `<int>` in
    /// `id<int>(5)` / `obj.pick<String>(x)` (spec: postfix
    /// `expr '<' type-args '>' '(' args ')'`). Empty when the
    /// caller relied on inference (`id(5)`). Concrete types only —
    /// wildcards are meaningless at a call site.
    pub explicit_generic_args: Vec<TypeRef>,
    /// Arguments in call-site source order. Named arguments
    /// (`port: 443`) record their label in the parallel
    /// [`Self::arg_names`] slot; positional ones record `None`.
    pub args: Vec<Expr>,
    /// Per-argument label, parallel to [`Self::args`] (§A.2.9
    /// `argument = identifier ':' expression`). The tycheck-level
    /// expansion pass (`juxc_tycheck::expand_call_sugar`) re-orders
    /// named args into parameter slots and fills omitted
    /// default-valued parameters, then clears the labels — so the
    /// backend only ever sees plain positional calls.
    pub arg_names: Vec<Option<Ident>>,
    /// Span covering callee and argument list.
    pub span: Span,
}
