//! Top-level declaration AST nodes — classes, records, enums, interfaces,
//! functions, and their pieces (fields, constructors, parameters, etc.).
//!
//! References:
//! - [`crate::Block`] for function/constructor bodies.
//! - [`crate::TypeRef`] for declared types.
//! - [`crate::Expr`] for default initializers on fields/parameters.
//! - [`crate::Ident`] / [`crate::QualifiedName`] / [`crate::Visibility`]
//!   from `common`.

use juxc_source::Span;

use crate::common::{Ident, QualifiedName, Visibility};
use crate::exprs::Expr;
use crate::stmts::Block;
use crate::types::TypeRef;

/// Per §A.2.2:
/// ```text
/// top-level-decl    = annotation* visibility? top-level-decl-body
///                   | annotation* top-level-statement   -- entry file only
/// ```
///
/// We start with just the function variant; class/interface/struct/record/
/// enum/const/type-alias/annotation get added as we implement them.
#[derive(Debug, Clone)]
pub enum TopLevelDecl {
    /// A top-level function declaration.
    Function(FnDecl),
    /// A top-level class declaration. See [`ClassDecl`].
    Class(ClassDecl),
    /// A top-level enum declaration. See [`EnumDecl`].
    Enum(EnumDecl),
    /// A top-level record declaration. See [`RecordDecl`].
    Record(RecordDecl),
    /// A top-level interface declaration. See [`InterfaceDecl`].
    Interface(InterfaceDecl),
}

/// `interface-decl` per grammar §A.2.4.
///
/// **Turn-1 scope** (this revision):
/// - Method signatures only — no default-method bodies, no static
///   methods, no constants. The methods list reuses [`FnDecl`] with
///   `body: None`.
/// - Optional generic parameters: `interface Comparable<T> { … }`.
/// - No `extends` between interfaces (`interface B extends A`).
#[derive(Debug, Clone)]
pub struct InterfaceDecl {
    /// `public` / `private` / `internal` / `protected` / package-private.
    pub visibility: Visibility,
    /// Interface name.
    pub name: Ident,
    /// Type parameters in declaration order, e.g. `<T>` in
    /// `interface Comparable<T>`. Empty for non-generic interfaces.
    pub generic_params: Vec<TypeParam>,
    /// Method signatures. Each `FnDecl` here has `body: None`. The
    /// parser enforces signature-only form for Turn 1.
    pub methods: Vec<FnDecl>,
    /// Span covering the whole `interface Name { … }` declaration.
    pub span: Span,
}

/// `record-decl` per grammar §A.2.4.
///
/// **Turn-1 scope** (this revision):
/// - Header form `record Name<T>(Type f1, Type f2)` only — no body
///   methods, no compact constructor, no `this(...)` secondary
///   constructors, no `implements` clause.
/// - Auto-canonical constructor synthesized from the header.
/// - Auto-derived `Debug` + `Clone` + `PartialEq` on the emitted Rust
///   struct (Java's record-equality semantics for free). `Hash` and
///   `Eq` are deferred because `f32`/`f64` payloads break them.
#[derive(Debug, Clone)]
pub struct RecordDecl {
    /// `public` / `private` / `internal` / `protected` / package-private.
    pub visibility: Visibility,
    /// Record name — used as the type and as the constructor target.
    pub name: Ident,
    /// Generic type parameters, e.g. `<A, B>` in
    /// `record Pair<A, B>(A first, B second)`. Empty for non-generic
    /// records.
    pub generic_params: Vec<TypeParam>,
    /// Header components in source order — each becomes a struct
    /// field and a canonical-constructor parameter.
    pub components: Vec<RecordComponent>,
    /// Operator-override declarations inside the record body, in
    /// source order. Each entry can be a real override (custom body)
    /// or a `= delete;` suppression per §O.3.4 — `is_deleted` on the
    /// [`OperatorDecl`] distinguishes the two. Empty when the record
    /// body has no operator overrides.
    pub operators: Vec<OperatorDecl>,
    /// Method declarations inside the record body, in source order.
    /// Per grammar §A.2.4 records may contain function declarations
    /// (Java-style record methods) but NOT additional instance fields
    /// or extra constructors — the header components are the only
    /// fields, and the canonical constructor is synthesized. Empty
    /// when the body has no methods.
    pub methods: Vec<FnDecl>,
    /// Span covering the whole `record … { … }` declaration.
    pub span: Span,
}

/// One header component of a record per §A.2.4 `record-component`.
///
/// Syntactically a `type identifier` pair. The same component drives
/// both the auto-generated field and the canonical constructor's
/// parameter, so they share the type and name.
#[derive(Debug, Clone)]
pub struct RecordComponent {
    /// Declared type of the component.
    pub ty: TypeRef,
    /// Component name.
    pub name: Ident,
    /// Span of the whole `type identifier` clause.
    pub span: Span,
}

/// `enum-decl` per §7.7 + grammar §A.2.4.
///
/// **Turn-1 scope** (this revision):
/// - Visibility modifier only — no `sealed` / `@layout(c, ...)`.
/// - No generic parameters.
/// - Variants are unit (`North`) or tuple-payload (`Number(int, String)`).
///   Payload positions accept Jux primitives and `String`.
/// - No methods inside the enum body yet — pattern matching first.
/// - Auto-derived helpers (`name()`, `ordinal()`, `values()`, …) deferred.
#[derive(Debug, Clone)]
pub struct EnumDecl {
    /// Enum visibility.
    pub visibility: Visibility,
    /// The enum's name (used as the type and as the variant qualifier).
    pub name: Ident,
    /// Variant declarations in source order. Order determines auto-
    /// derived ordinal values when those land.
    pub variants: Vec<EnumVariant>,
    /// Operator-override declarations on the enum body, in source
    /// order. Like records (§O.3.4), each entry can be a real override
    /// or a `= delete;` suppression. Empty when the user wrote no
    /// operator section after the variant list. Enums rarely need
    /// custom operators (the natural variant-order semantics cover
    /// most cases) but `= delete;` for `operator string` is the same
    /// security-sensitive use case records have.
    pub operators: Vec<OperatorDecl>,
    /// Span covering the whole `enum Name { … }` declaration.
    pub span: Span,
}

/// One variant inside an enum body per §7.7.1.
///
/// Unit variants carry an empty `payload`; tuple-payload variants list
/// their slot types in source order. Payload slots may carry an
/// optional name (`Ok(int status, String body)`); the name is captured
/// for future record-style access but ignored by the Turn-1 backend.
#[derive(Debug, Clone)]
pub struct EnumVariant {
    /// Variant name (e.g. `North`, `Number`, `Ok`).
    pub name: Ident,
    /// Payload slots — empty for unit variants.
    pub payload: Vec<EnumPayload>,
    /// Span covering the variant declaration.
    pub span: Span,
}

/// One payload slot of a tuple-style enum variant.
#[derive(Debug, Clone)]
pub struct EnumPayload {
    /// Declared payload type.
    pub ty: TypeRef,
    /// Optional field name (`Ok(int status, …)` → `status`). Captured
    /// so a future record-style pattern matching pass can reference
    /// it; the Turn-1 backend emits tuple variants and ignores names.
    pub name: Option<Ident>,
    /// Span of the payload slot.
    pub span: Span,
}

/// `class-decl` per grammar §A.2.4.
///
/// **Turn-1 scope** (this revision):
/// - Visibility modifier only — no `abstract`/`sealed`/`final`.
/// - Generic parameters supported as plain type variables (no bounds,
///   no wildcards, no variance annotations — those land in follow-up
///   turns).
/// - No `extends` / `implements`.
/// - Members: fields and constructors and methods only.
/// - At most one constructor (no overloading yet).
///
/// Everything else from §7.3 lands in later turns.
#[derive(Debug, Clone)]
pub struct ClassDecl {
    /// `public` / `private` / `internal` / `protected` / package-private.
    pub visibility: Visibility,
    /// True when the class is declared with the `abstract` modifier.
    /// Abstract classes can't be instantiated directly; their abstract
    /// methods are concretized by subclasses. Phase-1 abstract-method
    /// bodies lower to `unimplemented!()` stubs.
    pub is_abstract: bool,
    /// True when the class is declared `final` — no class may extend
    /// it. Tycheck enforces with `E0420_FinalClassExtended`.
    pub is_final: bool,
    /// True when the class is declared `sealed`. A sealed class
    /// restricts its subclasses to the explicit `permits` list. Any
    /// extender outside the list fires `E0422_SealedClassNotPermitted`.
    pub is_sealed: bool,
    /// Names of the classes that may extend this class — populated
    /// only when `is_sealed` is true. Each entry is the bare class
    /// name from the `permits` clause.
    pub permits: Vec<Ident>,
    /// The class name (used as the type and as the constructor's name).
    pub name: Ident,
    /// Type parameters in declaration order, e.g. the `T, K, V` in
    /// `class Map<T, K, V> { … }`. Empty when the class isn't generic.
    pub generic_params: Vec<TypeParam>,
    /// Parent class this one extends, or `None` for a root class.
    /// Phase 1: single inheritance only — Jux follows Java in not
    /// allowing multiple class parents.
    pub extends: Option<TypeRef>,
    /// Interfaces this class implements, in source order. Each entry
    /// is a `TypeRef` so generic interfaces (`Comparable<Box>`) carry
    /// their type arguments through. Empty when the class implements
    /// no interfaces.
    pub implements: Vec<TypeRef>,
    /// Field declarations in source order.
    pub fields: Vec<FieldDecl>,
    /// Constructor(s). At most one in Turn 1; the parser enforces this.
    pub constructors: Vec<ConstructorDecl>,
    /// Instance methods. Static methods land in a later turn.
    pub methods: Vec<FnDecl>,
    /// Operator overload declarations per `JUX-OPERATORS-ADDENDUM.md`
    /// §O.2 — e.g. `public bool operator==(Other o) { … }`. Empty for
    /// classes that don't override any operator (the default — class
    /// identity equality, identity hash, type-and-address `string`).
    ///
    /// These live in their own list rather than under `methods` so the
    /// compiler can route them through the dispatch rules in §O.2.6
    /// without having to filter by name shape.
    pub operators: Vec<OperatorDecl>,
    /// Span covering the whole `class Name { … }` declaration.
    pub span: Span,
}

/// `operator-decl` per `JUX-OPERATORS-ADDENDUM.md` §O.2 — an operator
/// override on a class or record (records use it primarily to suppress
/// auto-derived behavior via the `= delete;` form per §O.3.4).
///
/// Shape: `[visibility] [returnType] operator <op>(params) { body }`,
/// or `[visibility] [returnType] operator <op>(params) = delete;` for
/// the suppression form. Return type is parsed and stored as the user
/// wrote it, even though the spec fixes it for many operators (`bool`
/// for `==`, `int` for `<=>` and `hash`, `String` for `string`). A
/// future tycheck pass will validate the return type matches the
/// operator.
#[derive(Debug, Clone)]
pub struct OperatorDecl {
    /// Member visibility — defaults to package-private when the user
    /// writes no modifier (consistent with [`FnDecl`]).
    pub visibility: Visibility,
    /// Which operator this overrides.
    pub kind: OperatorKind,
    /// Formal parameters in declaration order. Arity is operator-fixed
    /// (zero for unary `~`, `string`, `hash`, `()`; one for everything
    /// else; two for `[]=`) — enforcement lands in tycheck.
    pub params: Vec<Param>,
    /// Declared return type. Stored exactly as written.
    pub return_type: ReturnType,
    /// Method body. `None` when `is_deleted` is true (§O.3.4 form).
    pub body: Option<Block>,
    /// True when this declaration is a `= delete;` suppression rather
    /// than a real override. Per §O.3.4, `= delete;` on a record/struct/
    /// enum's operator turns off the auto-derivation for that operator
    /// — useful for security-sensitive types where the default would
    /// be misleading. Always `false` for class operators in practice;
    /// the parser doesn't restrict it but classes don't have auto-
    /// derives to suppress.
    pub is_deleted: bool,
    /// Span covering the whole declaration.
    pub span: Span,
}

/// Which operator an [`OperatorDecl`] overrides. Mirrors the table in
/// `JUX-OPERATORS-ADDENDUM.md` §O.2.1–§O.2.4.
///
/// Some operators look the same lexically — `operator+` with one param
/// is binary plus, with zero params is unary plus. The arity decides;
/// the kind tag here records the **symbol**, not the arity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperatorKind {
    /// `==` — structural equality (§O.2.1). Auto-derives `!=`.
    Eq,
    /// `<=>` — three-way comparison (§O.2.1). Auto-derives `<`, `<=`,
    /// `>`, `>=` from sign.
    Cmp,
    /// `<` — less than (§O.2.1, four-operator set).
    Lt,
    /// `<=` — less or equal.
    Le,
    /// `>` — greater than.
    Gt,
    /// `>=` — greater or equal.
    Ge,
    /// `hash` — hash value for use as a `Map`/`Set` key (§O.2.2).
    Hash,
    /// `string` — string representation for `$"…"` and `print(x)`
    /// (§O.2.2).
    ToString,
    /// `+` — binary or unary (§O.2.3). Arity decides.
    Plus,
    /// `-` — binary or unary.
    Minus,
    /// `*` — multiplication.
    Mul,
    /// `/` — division.
    Div,
    /// `%` — remainder.
    Rem,
    /// `&` — bitwise AND.
    BitAnd,
    /// `|` — bitwise OR.
    BitOr,
    /// `^` — bitwise XOR.
    BitXor,
    /// `~` — unary bitwise NOT.
    BitNot,
    /// `<<` — left shift.
    Shl,
    /// `>>` — right shift.
    Shr,
    /// `[]` — indexed read (§O.2.4).
    Index,
    /// `[]=` — indexed write (§O.2.4).
    IndexSet,
    /// `()` — call (§O.2.4) — makes the type callable.
    Call,
    /// `..` — exclusive range (§O.2.4).
    Range,
    /// `..=` — inclusive range (§O.2.4).
    RangeInclusive,
}

/// A generic type parameter per §A.2.4 `generic-params`.
///
/// **Turn-2 scope** (this revision): parameter name + an optional
/// list of bounds (`<T extends Drawable & Comparable>`). Phase 1
/// expects bounds to be interfaces — when a class is named here it
/// emits as a Rust trait reference that won't resolve. Variance
/// annotations and parameter defaults remain future work.
#[derive(Debug, Clone)]
pub struct TypeParam {
    /// Parameter name — `T`, `K`, `V`, etc. Conventionally
    /// PascalCase / single uppercase letter, but the parser doesn't
    /// enforce that today.
    pub name: Ident,
    /// Optional bounds list — the types listed after `extends`. Java
    /// uses `&` between multiple bounds; we use the same shape here.
    /// Empty for unbounded parameters.
    pub bounds: Vec<TypeRef>,
    /// Span of the parameter declaration.
    pub span: Span,
}

/// A class field per §7.3 + grammar §A.2.4 `field-decl`.
///
/// **Turn-1 scope**: visibility + type + name + optional default value.
/// No `static` / `const` / `final` / `volatile` / `weak` modifiers yet.
#[derive(Debug, Clone)]
pub struct FieldDecl {
    /// Field visibility.
    pub visibility: Visibility,
    /// Declared type.
    pub ty: TypeRef,
    /// Field name.
    pub name: Ident,
    /// Optional default initializer (`= expr`). When absent, the backend
    /// zero/empty-initializes per the type's natural default.
    pub default: Option<Expr>,
    /// Span covering the whole field declaration including the `;`.
    pub span: Span,
}

/// A class constructor per §7.3.1.
///
/// Syntactically a method with no return type whose name matches the
/// enclosing class. Constructors carry their own visibility but no
/// other modifiers in Turn 1.
#[derive(Debug, Clone)]
pub struct ConstructorDecl {
    /// Constructor visibility.
    pub visibility: Visibility,
    /// Formal parameters.
    pub params: Vec<Param>,
    /// Constructor body — runs after fields are zero-initialized into
    /// the `__self` builder (see backend `emit_constructor`).
    pub body: Block,
    /// Span covering the whole constructor declaration.
    pub span: Span,
}

/// Per §A.2.4:
/// ```text
/// function-decl     = modifier* return-type identifier
///                     generic-params? '(' param-list? ')' throws-clause?
///                     function-body
/// ```
#[derive(Debug, Clone)]
pub struct FnDecl {
    /// `public`/`internal`/`protected`/`private`/package-private.
    pub visibility: Visibility,
    /// `static`, `final`, `abstract`, `async`, `native`, `unsafe`, `override`.
    pub modifiers: Vec<FnModifier>,
    /// Return type (or `void`).
    pub return_type: ReturnType,
    /// Function name.
    pub name: Ident,
    /// Type parameters in declaration order, e.g. `<T>` in
    /// `public T identity<T>(T x)`. Empty when the function isn't
    /// generic. Turn-1 limitation: no bounds, no defaults.
    pub generic_params: Vec<TypeParam>,
    /// Formal parameters in declaration order.
    pub params: Vec<Param>,
    /// `throws` clause, listing exception types that may escape.
    pub throws: Vec<QualifiedName>,
    /// Body block, or `None` for `abstract`/`native` declarations.
    pub body: Option<Block>,
    /// Span covering the entire declaration.
    pub span: Span,
}

/// Modifiers permitted on a function declaration. Per §A.2.4:
/// ```text
/// modifier = 'static' | binding-immut | 'abstract' | 'async'
///          | 'native' | 'unsafe' | 'override'
/// ```
/// `binding-immut` is `final` or `const`; per the spec they're synonyms.
/// We canonicalise to `Final` at parse time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FnModifier {
    Static,
    Final,
    Abstract,
    Async,
    Native,
    Unsafe,
    Override,
}

/// Return type of a function. Per §A.2.4:
/// ```text
/// return-type       = 'void' | type | 'async' type
/// ```
#[derive(Debug, Clone)]
pub enum ReturnType {
    /// `void` — no return value.
    Void,
    /// A concrete return type.
    Type(TypeRef),
    /// `async T` — the function is async and returns `T` to awaiters.
    AsyncType(TypeRef),
}

/// One formal parameter.
#[derive(Debug, Clone)]
pub struct Param {
    /// Parameter name.
    pub name: Ident,
    /// Declared type.
    pub ty: TypeRef,
    /// Default value, if any.
    pub default: Option<Expr>,
    /// Span of the entire parameter.
    pub span: Span,
}
