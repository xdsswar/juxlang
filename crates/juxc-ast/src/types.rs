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
    /// Array shape — `Some` for array types (`T[N]` or `T[]`), `None`
    /// for plain (scalar) types. Multi-dimensional support is deferred.
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
        let is_literal = text == "true"
            || text == "false"
            || (!text.is_empty() && text.chars().all(|c| c.is_ascii_digit() || c == '_'));
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

/// Shape of an array type's dimension(s) per §A.2.7.
#[derive(Debug, Clone)]
pub enum ArrayShape {
    /// `T[N]` — fixed-size, size is a const-expr (typically an integer literal).
    /// Lowers to Rust `[T; N]`. Stack-allocated, no heap, no `Vec`.
    Fixed(Box<Expr>),
    /// `T[]` — dynamic-size, sized at runtime. Lowers to Rust `Vec<T>`.
    /// Not implemented in Turn 1.
    Dynamic,
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
