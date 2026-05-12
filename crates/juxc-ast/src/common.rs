//! Shared identifier and visibility primitives used across the AST.
//!
//! These types are referenced from nearly every other module — declarations
//! name their owner via [`Ident`], qualified imports/paths use
//! [`QualifiedName`], and members of a type (fields, methods, ctors) carry
//! a [`Visibility`]. Keeping them here breaks any otherwise-cyclic module
//! dependency.

use juxc_source::Span;

/// A dot-separated path — `foo`, `com.example.Foo`, `Map.Entry`.
#[derive(Debug, Clone)]
pub struct QualifiedName {
    /// Path segments in order, e.g. `["com", "example", "Foo"]`.
    pub segments: Vec<Ident>,
    /// Span covering all segments.
    pub span: Span,
}

/// A single identifier token, after lexing.
#[derive(Debug, Clone)]
pub struct Ident {
    /// The identifier text.
    pub text: String,
    /// Source span of the identifier.
    pub span: Span,
}

/// Visibility modifier on a top-level decl or class member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// `public` — visible everywhere.
    Public,
    /// `internal` — visible inside the declaring module.
    Internal,
    /// `protected` — visible to subclasses.
    Protected,
    /// `private` — visible only inside the declaring type/file.
    Private,
    /// No modifier written — package-private.
    Package,
}
