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
    /// A top-level type alias — `type Name<...>? = TypeRef;`. Per
    /// grammar §A.2.4. Resolved transparently by tycheck (name
    /// looks like an alias on use, expands to its target type) and
    /// emitted as a Rust `pub type Name<...>? = ...;`.
    TypeAlias(TypeAliasDecl),
    /// A top-level constant — `const Type NAME = expr;` (or the
    /// `final` synonym per grammar §A.2.2). Resolves to a Rust
    /// `pub const NAME: T = …;`. Evaluated at compile time
    /// — the initializer must be a const-expression today,
    /// which Phase 1 broadly approximates as "any literal /
    /// arithmetic on literals."
    Const(ConstDecl),
}

/// `const-decl` per grammar §A.2.2:
/// ```text
/// const-decl = ('const' | 'final') type identifier '=' expression ';'
/// ```
///
/// `const` and `final` are synonyms in this position — the AST
/// records which spelling the user wrote so error messages can
/// echo it, but downstream semantics treat them identically.
#[derive(Debug, Clone)]
pub struct ConstDecl {
    /// Annotations attached to this constant.
    pub annotations: Vec<Annotation>,
    /// Source visibility.
    pub visibility: Visibility,
    /// `true` if the user wrote `final`; `false` if they wrote
    /// `const`. The two are synonymous everywhere else.
    pub used_final_keyword: bool,
    /// Declared type, or `None` when **inferred** from the initializer
    /// (`const PI = 3.14;` → `double`). Tycheck resolves the inferred type.
    pub ty: Option<TypeRef>,
    /// Constant's identifier — UPPER_SNAKE_CASE conventionally,
    /// not enforced by the parser.
    pub name: Ident,
    /// Initializer expression. Today any expression parses; a
    /// future const-expr pass tightens this.
    pub value: Expr,
    /// Span of the whole declaration.
    pub span: Span,
}

/// A single annotation occurrence — `@Name`, `@Name(args)`, or
/// `@Pkg.Name(args)`. Per grammar §A.2.3.
///
/// Stored on the declarations the spec says it applies to. The
/// resolved interpretation lives elsewhere:
/// - `@Override` is verified at tycheck time.
/// - `@Deprecated` lowers to Rust `#[deprecated]`.
/// - `@Cfg(...)` lowers to Rust `#[cfg(...)]` for conditional
///   compilation.
/// - User-defined annotations (`annotation Foo { … }` declarations)
///   are not yet wired — they parse but produce no semantic effect.
#[derive(Debug, Clone)]
pub struct Annotation {
    /// Dotted name, e.g. `Override`, `Deprecated`, `foo.bar.Trace`.
    pub name: QualifiedName,
    /// Optional argument list. Empty for the bare `@Name` form
    /// AND for `@Name()` — the two are indistinguishable here
    /// since arglists are sugar over positional defaults.
    pub args: Vec<AnnotationArg>,
    /// Span of the whole `@…` form including the `@`.
    pub span: Span,
}

/// One entry in an annotation's argument list. Either a positional
/// expression (`@Cfg(linux)`) or a named binding
/// (`@Extern(lib = "m")`).
#[derive(Debug, Clone)]
pub enum AnnotationArg {
    /// `expr` — positional argument.
    Positional(Expr),
    /// `name = expr` — named argument.
    Named { name: Ident, value: Expr },
}

/// `type-alias` per grammar §A.2.4:
/// ```text
/// type-alias = 'type' identifier generic-params? '=' type ';'
/// ```
///
/// A type alias introduces a new name for an existing type. Phase-1
/// semantics mirror Rust's `type X = Y;` — transparent at use sites
/// (tycheck rewrites a reference to `X` into the underlying `Y`
/// before further inference). Generic aliases (`type Pair<A, B> =
/// Tuple<A, B>;`) are supported syntactically; expansion threads
/// the alias's params through the substituted target.
#[derive(Debug, Clone)]
pub struct TypeAliasDecl {
    /// Annotations attached to this alias.
    pub annotations: Vec<Annotation>,
    /// Source visibility — `public` / `internal` / etc.
    pub visibility: Visibility,
    /// Alias name (PascalCase by convention, not enforced).
    pub name: Ident,
    /// Generic parameters in declaration order. Empty for a bare
    /// alias `type StringList = List<String>;`.
    pub generic_params: Vec<TypeParam>,
    /// The target type the alias resolves to.
    pub target: TypeRef,
    /// Span of the whole `type … ;` declaration.
    pub span: Span,
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
    /// Annotations attached to this interface.
    pub annotations: Vec<Annotation>,
    /// `public` / `private` / `internal` / `protected` / package-private.
    pub visibility: Visibility,
    /// Interface name.
    pub name: Ident,
    /// Type parameters in declaration order, e.g. `<T>` in
    /// `interface Comparable<T>`. Empty for non-generic interfaces.
    pub generic_params: Vec<TypeParam>,
    /// Parent interfaces this one extends. Mirrors Java's
    /// `interface Foo extends A, B { … }` form. Each entry is a
    /// `TypeRef` so generic parents (`extends Comparable<Foo>`)
    /// carry their type arguments through. Empty when no
    /// `extends` clause is present.
    pub extends: Vec<TypeRef>,
    /// Method signatures. Each `FnDecl` here has `body: None`. The
    /// parser enforces signature-only form for Turn 1.
    pub methods: Vec<FnDecl>,
    /// Interface fields — implicitly `public static final` per
    /// `classes-rules.md` §3.3. The parser treats `int X = 10;` as
    /// a constant declaration; an initializer is required because
    /// the field can't be assigned elsewhere.
    pub fields: Vec<FieldDecl>,
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
    /// Annotations attached to this record.
    pub annotations: Vec<Annotation>,
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
    /// Static-field declarations inside the record body, in source
    /// order. Java records permit `public static [final] T x = …;`
    /// (JEP 395 §3) — instance fields are still forbidden. The
    /// parser sets `is_static = true` on every entry and rejects
    /// non-static fields with E0200 to keep that rule explicit.
    pub static_fields: Vec<FieldDecl>,
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
    /// Annotations attached to this enum.
    pub annotations: Vec<Annotation>,
    /// Enum visibility.
    pub visibility: Visibility,
    /// The enum's name (used as the type and as the variant qualifier).
    pub name: Ident,
    /// Type parameters in declaration order — the `B` in `enum Cow<B>` or the
    /// `K, V` in `enum Entry<K, V>`. Empty for a non-generic enum. Payload types
    /// of the variants may reference these parameters (`Borrowed(B)`).
    pub generic_params: Vec<TypeParam>,
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
    /// Methods declared in the enum body after the `;` terminator
    /// (§A.2.5). Lowered as inherent methods on the Rust enum
    /// (`this` ≡ the enum value, dispatched by `switch (this)`).
    pub methods: Vec<FnDecl>,
    /// `const` fields in the enum body — implicitly static, like
    /// interface constants.
    pub constants: Vec<FieldDecl>,
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
    /// Source-order annotation list — `@Deprecated`, `@Cfg(...)`,
    /// user-defined, etc. Empty for un-annotated classes.
    pub annotations: Vec<Annotation>,
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
    /// True when this node came from a `struct` declaration rather than a
    /// `class` (grammar §A.2.5 `struct-decl`). A Jux `struct` is a value-type
    /// aggregate with no inheritance; in Phase 1 it shares the `ClassDecl`
    /// representation (parsed identically — fields + methods, implicitly
    /// `final`, never `extends`) so it flows through resolve / tycheck / the
    /// symbol table unchanged. The flag is retained so later turns can give
    /// structs their distinct value semantics (and so the backend can lower a
    /// `struct` to a Rust `struct` rather than the class handle representation)
    /// without re-deriving the origin. External `.jux.d` stubs (Rust structs
    /// surfaced by bindgen, §G.6.3) are the first consumers.
    pub is_struct: bool,
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
    /// Static nested type declarations inside this class body —
    /// `static class Inner { … }`, `static record Pair(int x, int y)`,
    /// etc. Per spec §1379 only `static` nested forms are
    /// allowed (no inner classes, no anonymous classes here —
    /// the latter live as expression forms via `new Iface() { … }`).
    /// The parser stores them here; the backend lifts each to the
    /// enclosing module scope with a name-prefixed identifier so
    /// the Java-style `Outer.Inner` access path round-trips
    /// through the FQN-resolver.
    pub nested_types: Vec<crate::TopLevelDecl>,
    /// C#-style property declarations per JUX-MISSING-DEFS §M.7.
    ///
    /// Parsed losslessly into [`PropertyDecl`] nodes, then *desugared*
    /// (by [`crate::desugar_properties`], run at the end of parsing)
    /// into a private backing field plus a getter / setter [`FnDecl`]
    /// that land in [`Self::fields`] / [`Self::methods`]. This list is
    /// retained so tycheck and the backend can still see the property
    /// shape — which accessors exist, their per-accessor visibility,
    /// and whether the property is read-only / init-only — to enforce
    /// §M.7.2 access control and route `obj.Prop = v` writes through
    /// the synthesized setter. Empty for classes with no properties.
    pub properties: Vec<PropertyDecl>,
    /// Instance initializer blocks (`init { … }`, JUX-MISSING-DEFS §M.1), in
    /// source order. Every constructor runs every `init` block at the end of
    /// its body (construction sequence step 5, §S.4.4), before the reference is
    /// returned. Empty for classes with no `init` blocks.
    pub init_blocks: Vec<Block>,
    /// Static initializer blocks (`static { … }`, JUX-SEMANTICS §S.4.1), in
    /// source order. They run once, on first observable use of the class.
    /// Empty for classes with no `static` blocks.
    pub static_init_blocks: Vec<Block>,
    /// Destructor blocks (`drop { … }`, §6.6 / §S.5). The spec allows
    /// at most ONE per class — parsed as a list so tycheck can
    /// diagnose duplicates with a span instead of silently dropping
    /// them. Runs when the last strong reference is released.
    pub drop_blocks: Vec<Block>,
    /// Span covering the whole `class Name { … }` declaration.
    pub span: Span,
}

/// A C#-style property declaration per JUX-MISSING-DEFS §M.7.
///
/// Captures every accessor form losslessly:
/// - auto (`get;` / `set;` / `init;`) → synthesized body over a
///   private backing field,
/// - expression-bodied (`get => e` / `set => e`),
/// - full block (`get { … }` / `set { … }`),
/// - expression-bodied read-only property (`T Name => e;`) — modeled
///   as a getter whose body is the expression, with no setter.
///
/// Desugaring ([`crate::desugar_properties`]) turns this into a
/// backing field + getter / setter [`FnDecl`]s so the rest of the
/// pipeline reuses the existing field / method machinery; the
/// `PropertyDecl` itself is kept on [`ClassDecl::properties`] for
/// tycheck access-control and backend setter routing.
#[derive(Debug, Clone)]
pub struct PropertyDecl {
    /// Annotations attached to the property.
    pub annotations: Vec<Annotation>,
    /// The property's outer visibility.
    pub visibility: Visibility,
    /// True when the property is declared `static` (class-scoped).
    pub is_static: bool,
    /// Declared property type.
    pub ty: TypeRef,
    /// Property name — the user-visible member accessed as `obj.Name`.
    pub name: Ident,
    /// The getter accessor. Always present in practice (a property
    /// with only a setter is rejected by the parser), but `Option`
    /// keeps the shape uniform with [`Self::setter`].
    pub getter: Option<PropertyAccessor>,
    /// The setter / init accessor, when the property is writable.
    /// `None` for read-only properties (`{ get; }`, `T Name => e;`).
    pub setter: Option<PropertySetter>,
    /// Optional `= expr` initializer (auto-property default). Lowered
    /// into the backing field's default so it runs during construction.
    pub initializer: Option<Expr>,
    /// True when this property was synthesized from a backing field
    /// the user can't name (auto-property `get;`/`set;`/`init;`). The
    /// desugarer emits the backing field only in this case; computed
    /// properties (expression / block bodies with no auto accessor)
    /// read existing fields and need no backing storage.
    pub has_backing_field: bool,
    /// Span covering the whole property declaration.
    pub span: Span,
}

/// One accessor (getter or setter) of a [`PropertyDecl`].
#[derive(Debug, Clone)]
pub struct PropertyAccessor {
    /// Per-accessor visibility, when the user wrote one (e.g. the
    /// `private` in `{ get; private set; }`). `None` means the
    /// accessor inherits the property's outer visibility.
    pub visibility: Option<Visibility>,
    /// The accessor's body form.
    pub body: AccessorBody,
    /// Span of the accessor.
    pub span: Span,
}

/// The setter accessor of a [`PropertyDecl`], carrying whether it's a
/// plain `set` or an `init`-only setter (settable during construction
/// only, per §M.7.2).
#[derive(Debug, Clone)]
pub struct PropertySetter {
    /// Per-accessor visibility, when written (`{ get; private set; }`).
    pub visibility: Option<Visibility>,
    /// `true` when this is an `init` accessor (write only during
    /// construction), `false` for a plain `set`.
    pub is_init: bool,
    /// The setter's body form. The implicit parameter is named `value`.
    pub body: AccessorBody,
    /// Span of the accessor.
    pub span: Span,
}

/// The body of a property accessor per §M.7.1's `accessor-body`.
#[derive(Debug, Clone)]
pub enum AccessorBody {
    /// `;` — auto accessor; the desugarer synthesizes the body over a
    /// private backing field.
    Auto,
    /// `=> expr ;` — expression-bodied accessor.
    Expr(Expr),
    /// `{ … }` — full block body.
    Block(Block),
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
    /// Unary `-` (`operator-()` with NO parameters, §O.2.4). The
    /// parser re-kinds a zero-param `operator-` from Minus to this
    /// so binary subtraction and unary negation coexist in the
    /// per-kind operator maps. Unary `+` is the identity and isn't
    /// overloadable in Phase 1.
    Neg,
    /// `..` — exclusive range (§O.2.4).
    Range,
    /// `..=` — inclusive range (§O.2.4).
    RangeInclusive,
    /// `in` — containment (§O.2.4), declared on the CONTAINER type:
    /// `bool operator in(T element)`.
    In,
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
    /// `Some(value_type)` makes this a **const generic** parameter —
    /// the `<int N>` in `class RingBuffer<T, int N>` per grammar
    /// §A.2.6 (`generic-param = 'int' identifier | type identifier`)
    /// and type-system §T.11.3. The `TypeRef` is the *value* type of
    /// the parameter (`int`, `bool`, …), and the argument at a use
    /// site is a compile-time-constant value (`new RingBuffer<float,
    /// 256>()`), not a type. `None` for ordinary type parameters.
    /// Const params never carry `bounds`.
    pub const_ty: Option<TypeRef>,
    /// Span of the parameter declaration.
    pub span: Span,
}

impl TypeParam {
    /// True iff this is a const-generic parameter (`<int N>`) rather
    /// than an ordinary type parameter (`<T>`).
    pub fn is_const(&self) -> bool {
        self.const_ty.is_some()
    }
}

/// A class field per §7.3 + grammar §A.2.4 `field-decl`.
///
/// **Scope**: visibility + `static` / `final` / `weak` modifiers + type +
/// name + optional default value. (`volatile` is reserved but not yet wired.)
#[derive(Debug, Clone)]
pub struct FieldDecl {
    /// Annotations attached to this field.
    pub annotations: Vec<Annotation>,
    /// Field visibility.
    pub visibility: Visibility,
    /// True if the field is declared `static`. Static fields live on
    /// the class, not on instances — `Foo.X` reads a static, no
    /// receiver involved. Backend emits as `pub const` (when also
    /// `final`/`const`) or `pub static` (otherwise).
    pub is_static: bool,
    /// True if the field is declared `final` (or `const` — synonyms
    /// per spec §A.2.2). For instance fields this marks
    /// non-reassignability (informational today). For static fields
    /// it picks the `pub const` over `pub static` shape in the
    /// emitted Rust.
    pub is_final: bool,
    /// True if the field is declared `weak` (§6.5). A weak field does **not**
    /// contribute to the owning class's refcount, so it breaks reference
    /// cycles (the classic `Child` holding a back-reference to `Parent`).
    /// Its storage lowers to `std::rc::Weak<RefCell<Target_Inner>>`; it is
    /// read only via `.get()` (yielding `Target?`, since the target may be
    /// gone), defaults to an empty `Weak::new()`, and is exempt from
    /// definite-assignment (§S.4.5). Only valid on (non-generic, in Phase 1)
    /// class-typed fields — see `E0455`.
    pub is_weak: bool,
    /// Declared type, or `None` when the type is **inferred** from the
    /// initializer (`const I = 2;` → `int`). Inference requires an
    /// initializer; a type-less field with no initializer is an error.
    /// Tycheck resolves the inferred type and records it in the symbol
    /// table's `FieldSig`; the backend reads the resolved type from there
    /// when this is `None`.
    pub ty: Option<TypeRef>,
    /// Field name.
    pub name: Ident,
    /// Optional default initializer (`= expr`). When absent, the backend
    /// zero/empty-initializes per the type's natural default. Required when
    /// [`ty`](Self::ty) is `None` (inference needs a value).
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
    /// Annotations attached to this constructor.
    pub annotations: Vec<Annotation>,
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
    /// Source-order annotation list. `@Override` is checked at
    /// tycheck time; `@Deprecated` lowers to Rust `#[deprecated]`.
    pub annotations: Vec<Annotation>,
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
    /// `where` constraints (§O.5) — empty for non-generic functions
    /// and unconstrained generics.
    pub wheres: Vec<WhereConstraint>,
    /// Body block, or `None` for `abstract`/`native` declarations.
    pub body: Option<Block>,
    /// True when this `FnDecl` was synthesized from an expression-
    /// bodied property declaration (`T name => expr;` per
    /// JUX-MISSING-DEFS §M.7.4). The method is callable like any
    /// other method, but the field-access site (`obj.name`)
    /// recognizes the flag and emits `obj.name()` so the user
    /// sees Java-style property-read syntax. Plain methods keep
    /// `is_property = false`.
    pub is_property: bool,
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
    /// `true` when the parameter carries the `final` (or its synonym `const`)
    /// binding mode (grammar §A.2.4 `param-mode`): the parameter cannot be
    /// reassigned inside the body. Allowed on method / function parameters;
    /// **rejected on constructor parameters** (a constructor parameter is
    /// typically forwarded straight into a field, where the binding mode is the
    /// field's, not the parameter's).
    pub is_final: bool,
    /// `true` when the parameter was a Rust borrow (`&T`) in a bindgen-generated
    /// `.jux.d` stub — the leading `&` marker (§G.9.2). The declared type drops
    /// the `&` (§G.3.4); this flag tells codegen to re-attach the call-site
    /// borrow when invoking the foreign method (`contains_key(&arg)`). Always
    /// `false` for ordinary user-written parameters.
    pub is_ref: bool,
    /// Default value, if any.
    pub default: Option<Expr>,
    /// `true` for a variadic parameter — `T... name` (§7.2). The
    /// parser desugars the declared type to the array form (`T[]`),
    /// so bodies and emission see an ordinary dynamic array; this
    /// flag drives the CALL-SITE rules (trailing args packed into a
    /// synthesized array literal, §E.1.2.1 / §S.1.4) and the
    /// last-parameter-only check (E0212).
    pub is_varargs: bool,
    /// Span of the entire parameter.
    pub span: Span,
}

/// One `where T has operator OP(params) -> R` constraint (§O.5) on
/// a generic function — a STRUCTURAL capability requirement on a
/// type parameter. Phase 1 records the full shape but enforces
/// operator PRESENCE at instantiation sites (E0941); the param/return
/// shapes inform emission bounds.
#[derive(Debug, Clone)]
pub struct WhereConstraint {
    /// The constrained type parameter (`T`).
    pub param: Ident,
    /// Required operator.
    pub kind: OperatorKind,
    /// Declared operand types of the operator shape (often `[T]`).
    pub param_tys: Vec<TypeRef>,
    /// Declared return type of the operator shape.
    pub ret: Option<TypeRef>,
    /// Span of the whole constraint.
    pub span: Span,
}
