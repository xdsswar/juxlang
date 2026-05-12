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
    /// Generic args inside `<…>`, empty when not present.
    pub generic_args: Vec<TypeRef>,
    /// Whether the type carries a trailing `?` (nullable).
    pub nullable: bool,
    /// Array shape — `Some` for array types (`T[N]` or `T[]`), `None`
    /// for plain (scalar) types. Multi-dimensional support is deferred.
    pub array_shape: Option<ArrayShape>,
    /// Span of the whole reference.
    pub span: Span,
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
