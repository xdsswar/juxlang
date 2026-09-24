//! Type-reference AST nodes — the syntactic form of a type as written in
//! a Jux source file.
//!
//! References:
//! - [`crate::Ident`] / [`crate::QualifiedName`] for type names.
//! - [`crate::Expr`] for the size expression on fixed-size arrays
//!   (`T[N]` where `N` is any const-expr).

use juxc_source::Span;

use crate::common::{Ident, QualifiedName};
use crate::exprs::Expr;

/// The sentinel type-name used to encode **tuple types** in
/// [`TypeRef`] / the checker's `Ty::User` without a dedicated
/// variant: `(int, String)` parses to `name == "__tuple"` with the
/// element types as ordinary generic args. The leading `__` keeps it
/// out of user namespace; helpers below construct/recognize it.
pub const TUPLE_SENTINEL: &str = "__tuple";

/// The sentinel type-name for the handle `spawn(f)` gives back (§18.1.4), the
/// same encoding trick: `Task<T>` is `Ty::User { name: "__jux_task" }` with `T`
/// as its one generic arg, and `await` on one peels it.
///
/// The sentinel, rather than the bare name `Task`, because `Task` is a name a
/// program may use: `examples/apps/todo` declares a `todo.model.Task` record,
/// and a spawn handle typed plainly `Task` collided with it, so every call on
/// one of its records was reported as a method a task does not have.
pub const TASK_SENTINEL: &str = "__jux_task";

/// **Record destructuring** (`var Pt(a, b) = p;`, JUX-LANG-V1 §5.4) is
/// desugared by the parser into a typed temporary and one read per binder:
///
/// ```text
/// Pt __jux_rec0_2 = p;
/// var a = __jux_rec0_2.__jux_component_0;
/// var b = __jux_rec0_2.__jux_component_1;
/// ```
///
/// The parser cannot know the component NAMES (the record may live in another
/// file), so the reads name components by position. The checker validates the
/// temporary (a record of that arity, a value that is always one: E0439 /
/// E0271), resolves each position to its component, and the driver rewrites
/// the AST to the real names before the backend runs. The temporary's name
/// carries the pattern's arity, since `_` binders produce no read.
pub fn record_destructure_temp(counter: u32, arity: usize) -> String {
    format!("__jux_rec{counter}_{arity}")
}

/// The pattern arity encoded in a [`record_destructure_temp`] name, or `None`
/// for any other name.
pub fn record_destructure_arity(name: &str) -> Option<usize> {
    let rest = name.strip_prefix("__jux_rec")?;
    let (counter, arity) = rest.split_once('_')?;
    counter.parse::<u32>().ok()?;
    arity.parse().ok()
}

/// The positional field name the destructuring desugar reads component
/// `index` through (see [`record_destructure_temp`]).
pub fn record_component_marker(index: usize) -> String {
    format!("__jux_component_{index}")
}

/// The component position a [`record_component_marker`] names, or `None`.
pub fn record_component_index(field: &str) -> Option<usize> {
    field.strip_prefix("__jux_component_")?.parse().ok()
}

/// A reference to a type, e.g. `List<String>?`, `int[10]`, `byte[]`.
///
/// Structural details (generics, nullability, array shape) hang off this
/// flat struct as optional pieces. Multi-dimensional arrays will land
/// when we move `array_shape` from a single optional to a `Vec` of
/// nested shapes.
#[derive(Debug, Clone)]
pub struct TypeRef {
    /// The type's name path.
    pub name: QualifiedName,
    /// Generic args inside `<…>`, empty when not present. Each entry
    /// is either a concrete type or a bounded wildcard (`?`, `? extends T`,
    /// `? super T`) per Java's PECS rules.
    pub generic_args: Vec<GenericArg>,
    /// Whether the type carries a trailing `?` (nullable).
    pub nullable: bool,
    /// Array shape — `Some` for array types (`T[]`, `T[N]`, and
    /// multi-dimensional forms like `T[][]` / `T[3][4]`), `None` for
    /// plain (scalar) types. The [`ArrayShape`] holds one [`ArrayDim`]
    /// per dimension, outermost first.
    pub array_shape: Option<ArrayShape>,
    /// Function-type shape — `Some` when the user wrote
    /// `(A, B) -> R` (or `() async -> R`, `(A) throws E -> R`) per
    /// grammar §A.2.7. When set, `name`/`generic_args` are
    /// conventionally empty; consumers check `fn_shape` FIRST and
    /// short-circuit before treating this as a named type.
    ///
    /// Boxed to keep `TypeRef`'s memory footprint small in the
    /// common (non-function) case.
    pub fn_shape: Option<Box<FnTypeShape>>,
    /// Number of trailing `*` raw-pointer markers (§5.5 / §A.2.7). `0` for an
    /// ordinary type; `1` for `T*`, `2` for `T**`. Each level lowers to a Rust
    /// `*mut`, so `T*` → `*mut T`. Raw pointers are `unsafe`-only — declaring or
    /// dereferencing one is meaningful only in an `unsafe` context. The pointer
    /// suffix is the OUTERMOST modifier: `T[]*` is a pointer to an array, and a
    /// nullable pointer `T*?` lowers to `Option<*mut T>`.
    pub ptr_depth: u8,
    /// Span of the whole reference.
    pub span: Span,
}

impl TypeRef {
    /// True when this type carries one or more trailing `*` (a raw pointer).
    pub fn is_pointer(&self) -> bool {
        self.ptr_depth > 0
    }

    /// The shape of a closure type `(A) -> R`, or `None` for anything else,
    /// a function pointer included.
    pub fn closure_shape(&self) -> Option<&FnTypeShape> {
        self.fn_shape.as_deref().filter(|s| !s.is_pointer)
    }

    /// The shape of a function-pointer type `fn(A) -> R` (Layout-ABI §L.6.4),
    /// or `None` for anything else, a closure type included.
    pub fn fn_pointer_shape(&self) -> Option<&FnTypeShape> {
        self.fn_shape.as_deref().filter(|s| s.is_pointer)
    }

    /// Recognize a **synthetic const-generic argument** — the parser
    /// carries the literal in `new RingBuffer<float, 256>()` /
    /// `StackString<32>` as a `TypeRef` whose single name segment is
    /// the literal text verbatim ("256", "true"). Returns that text,
    /// or `None` for a real type reference. Used by tycheck to
    /// validate slot kinds (a const param must get a literal, a type
    /// param must not) and by the backend, which emits the text
    /// verbatim in turbofish / type-arg position.
    pub fn const_literal_text(&self) -> Option<&str> {
        if self.nullable
            || self.array_shape.is_some()
            || self.fn_shape.is_some()
            || self.ptr_depth > 0
            || !self.generic_args.is_empty()
            || self.name.segments.len() != 1
        {
            return None;
        }
        let text = self.name.segments[0].text.as_str();
        // A digit run, optionally negative (`-5` for a `<long N>`), `true` /
        // `false`, or a char literal (`'x'` for a `<char C>`).
        let digits = text.strip_prefix('-').unwrap_or(text);
        let is_literal = text == "true"
            || text == "false"
            || (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit() || c == '_'))
            || (text.len() >= 3 && text.starts_with('\'') && text.ends_with('\''));
        is_literal.then_some(text)
    }
}

/// `(A, B) async? throws? -> R` — function-type per grammar §A.2.7.
///
/// Phase-1 caveats:
/// - `throws` clauses parse but are recorded only — tycheck doesn't
///   enforce them yet.
/// - `async` marks the function as suspending; runtime story for
///   async is still ahead, so for now it's informational.
#[derive(Debug, Clone)]
pub struct FnTypeShape {
    /// Parameter types in left-to-right order.
    pub params: Vec<TypeRef>,
    /// Return type. `void` is its own bare-named `TypeRef`.
    pub return_type: TypeRef,
    /// True if the user wrote `async` before the `->`.
    pub is_async: bool,
    /// Names listed in the `throws` clause, in source order.
    /// Empty when the user didn't write `throws`.
    pub throws: Vec<TypeRef>,
    /// True for a **function-pointer** type, `fn(A) -> R` (Layout-ABI
    /// §L.6.4): a C code address with no environment, whose signature follows
    /// the C mapping of §8.1.1. False for a closure type `(A) -> R`. The two
    /// share this shape because they share their syntax after the `fn`, and
    /// nothing else: a consumer that means "closure" checks
    /// [`TypeRef::closure_shape`], and one that means "function pointer"
    /// checks [`TypeRef::fn_pointer_shape`].
    pub is_pointer: bool,
}

/// One position inside a generic argument list — either a fully-named
/// type (`List<String>`) or a wildcard with an optional bound
/// (`List<?>`, `List<? extends Animal>`, `List<? super Dog>`).
///
/// Wildcards are a compile-time concept: tycheck enforces variance
/// rules (PECS — Producer Extends, Consumer Super) and the backend
/// lowers them in context — in parameter positions, a wildcard
/// becomes a synthetic generic on the enclosing function with the
/// matching bound; in storage positions, it lowers via `dyn`-trait
/// erasure.
#[derive(Debug, Clone)]
pub enum GenericArg {
    /// `List<String>` — concrete type in the slot.
    Type(TypeRef),
    /// `List<?>` / `List<? extends T>` / `List<? super T>`.
    Wildcard(WildcardArg),
}

/// Wildcard generic argument with its optional bound.
#[derive(Debug, Clone)]
pub struct WildcardArg {
    /// `None` for unbounded `?`; `Some` for `? extends T` / `? super T`.
    pub bound: Option<WildcardBound>,
    /// Span of the `?` or `? extends T` / `? super T` form.
    pub span: Span,
}

/// Direction of a wildcard bound: covariant `extends` or
/// contravariant `super`. PECS variance rules apply at use sites.
#[derive(Debug, Clone)]
pub enum WildcardBound {
    /// `? extends T` — accepts any subtype of T. Producer position.
    Extends(TypeRef),
    /// `? super T` — accepts any supertype of T. Consumer position.
    Super(TypeRef),
}

impl GenericArg {
    /// Convenience: source span covering the whole arg.
    pub fn span(&self) -> Span {
        match self {
            GenericArg::Type(t) => t.span,
            GenericArg::Wildcard(w) => w.span,
        }
    }

    /// Returns the concrete `TypeRef` if this arg names a type, or
    /// `None` for wildcards. Useful at the many call sites that
    /// haven't yet been taught the wildcard case — they can skip
    /// wildcards cleanly while consumers that DO understand them
    /// match exhaustively.
    pub fn as_type(&self) -> Option<&TypeRef> {
        match self {
            GenericArg::Type(t) => Some(t),
            GenericArg::Wildcard(_) => None,
        }
    }
}

/// One dimension of an array type per §A.2.7 — `[N]` (fixed) or `[]`
/// (dynamic). A multi-dimensional array type is an ordered list of these
/// (see [`ArrayShape`]).
#[derive(Debug, Clone)]
pub enum ArrayDim {
    /// `[N]` — fixed-size dimension; the size is a const-expr (typically
    /// an integer literal). Lowers to a Rust fixed array `[T; N]`:
    /// stack-allocated, no heap, no `Vec`.
    Fixed(Box<Expr>),
    /// `[]` — dynamic-size dimension, sized at runtime. Lowers to Rust
    /// `Vec<T>` (owned, heap-backed, growable).
    Dynamic,
}

/// Shape of an array TYPE per §A.2.7 — one or more dimensions, stored
/// **OUTERMOST first** in Java reading order. The leftmost `[…]` written
/// in source (the outermost dimension) is `dims[0]`.
///
/// Examples:
/// - `int[]`     → `dims = [Dynamic]`
/// - `int[][]`   → `dims = [Dynamic, Dynamic]`
/// - `int[3][4]` → `dims = [Fixed(3), Fixed(4)]`
/// - `int[3][]`  → `dims = [Fixed(3), Dynamic]`
///
/// `TypeRef.array_shape` is `Some(ArrayShape)` for any array type and
/// `None` for a scalar — the `is_some()` / `as_ref()` access pattern is
/// unchanged from the single-dimension representation. Only sites that
/// match the individual dimensions need to walk `dims`.
#[derive(Debug, Clone)]
pub struct ArrayShape {
    /// The dimensions, OUTERMOST first. Always non-empty for a real
    /// array shape (the parser never produces a zero-dimension shape).
    pub dims: Vec<ArrayDim>,
    /// Whether the ELEMENT is nullable -- the `?` written BEFORE the
    /// brackets, `T?[]`.
    ///
    /// [`TypeRef::nullable`] keeps its plain meaning: the value this type
    /// describes may be missing, which for an array type is the `?` written
    /// AFTER the brackets (`T[]?`). The two are different types -- `int[]?`
    /// is a missing array, `int?[]` is an array of missing ints -- and with
    /// one flag between them the outer spelling silently became the inner.
    pub elem_nullable: bool,
}

impl ArrayShape {
    /// Construct a single-dimension (1-D) array shape — the common case
    /// (`T[]` / `T[N]`) and what synthetic call sites (e.g. varargs)
    /// produce.
    pub fn single(d: ArrayDim) -> Self {
        ArrayShape { dims: vec![d], elem_nullable: false }
    }

    /// Number of dimensions (the array's rank). `int[]` → 1,
    /// `int[][]` → 2, etc. Always ≥ 1 for a real shape.
    pub fn rank(&self) -> usize {
        self.dims.len()
    }

    /// The OUTERMOST dimension (`dims[0]`) — the one a single index
    /// operation peels and the one `.length` reports. Safe to call on
    /// any real shape (always non-empty).
    pub fn outer(&self) -> &ArrayDim {
        &self.dims[0]
    }

    /// Drop the outermost dimension, yielding the shape of the element
    /// produced by indexing once. Returns:
    /// - `Some(shape)` with one fewer dimension when rank was ≥ 2
    ///   (e.g. `int[][]` → `Some(int[])`);
    /// - `None` when rank was 1 — the element is then a plain scalar, so
    ///   the caller drops `array_shape` entirely.
    pub fn peeled(&self) -> Option<ArrayShape> {
        if self.dims.len() <= 1 {
            None
        } else {
            // Element nullability belongs to the innermost dimension, so it
            // rides along until the last one is peeled.
            Some(ArrayShape { dims: self.dims[1..].to_vec(), elem_nullable: self.elem_nullable })
        }
    }
}

impl TypeRef {
    /// Construct a tuple type — `(A, B, …)` (§5.3) — using the
    /// [`TUPLE_SENTINEL`] name encoding with the elements as
    /// generic args.
    pub fn tuple(elems: Vec<TypeRef>, span: Span) -> TypeRef {
        TypeRef {
            name: QualifiedName {
                segments: vec![Ident { text: TUPLE_SENTINEL.to_string(), span }],
                span,
            },
            generic_args: elems.into_iter().map(GenericArg::Type).collect(),
            nullable: false,
            array_shape: None,
            fn_shape: None,
            ptr_depth: 0,
            span,
        }
    }

    /// `Some(elements)` when this type is the tuple encoding.
    pub fn tuple_elems(&self) -> Option<Vec<&TypeRef>> {
        if self.fn_shape.is_none()
            && self.name.segments.len() == 1
            && self.name.segments[0].text == TUPLE_SENTINEL
        {
            Some(
                self.generic_args
                    .iter()
                    .filter_map(|g| match g {
                        GenericArg::Type(t) => Some(t),
                        _ => None,
                    })
                    .collect(),
            )
        } else {
            None
        }
    }
}
