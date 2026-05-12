//! Type-reference AST nodes — the syntactic form of a type as written in
//! a Jux source file.
//!
//! References:
//! - [`crate::Ident`] / [`crate::QualifiedName`] for type names.
//! - [`crate::Expr`] for the size expression on fixed-size arrays
//!   (`T[N]` where `N` is any const-expr).

use juxc_source::Span;

use crate::common::QualifiedName;
use crate::exprs::Expr;

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
    /// Span of the whole reference.
    pub span: Span,
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
