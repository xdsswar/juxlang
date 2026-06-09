//! Compilation-unit structure — root of the AST plus the package and
//! import preamble nodes.
//!
//! References:
//! - [`crate::Ident`] / [`crate::QualifiedName`] for names.
//! - [`crate::TopLevelDecl`] for the body of the compilation unit.

use juxc_source::Span;

use crate::common::{Ident, QualifiedName};
use crate::decls::TopLevelDecl;

/// Root of the AST — one parsed `.jux` file.
///
/// Per §A.2.1:
/// ```text
/// compilation-unit  = package-decl? import-decl* top-level-decl*
/// ```
#[derive(Debug, Clone)]
pub struct CompilationUnit {
    /// Optional `package com.example.foo;` at the top of the file.
    pub package: Option<PackageDecl>,
    /// `import …;` statements, in source order.
    pub imports: Vec<ImportDecl>,
    /// Top-level declarations (types, functions, constants, type aliases).
    pub items: Vec<TopLevelDecl>,
    /// True when this unit was loaded from a `.jux.d` **declaration stub**
    /// (JUX-BINDGEN-ADDENDUM.md §G.9) rather than an ordinary `.jux`
    /// source. An external unit is *opaque*: its declarations contribute
    /// names / types / signatures to name-resolution and type-checking, but
    /// they have no bodies to lower, so the backend **skips** them entirely
    /// (the real Rust crate / C shim provides the implementation at link
    /// time — §G.9.1/§G.9.2). The parser never sets this — it's a property
    /// of *where the source came from*, so the driver flips it on after
    /// parsing based on the file's `.jux.d` extension.
    pub is_external: bool,
    /// Span covering the whole file.
    pub span: Span,
}

/// `package com.example.foo;` declaration.
#[derive(Debug, Clone)]
pub struct PackageDecl {
    /// Dot-separated package path.
    pub name: QualifiedName,
    /// Span of the entire declaration including the trailing `;`.
    pub span: Span,
}

/// A single `import …;` statement.
#[derive(Debug, Clone)]
pub struct ImportDecl {
    /// What's being imported and (optionally) under what alias.
    pub spec: ImportSpec,
    /// Span of the entire declaration including the trailing `;`.
    pub span: Span,
}

/// The body of an import — per §A.2.1:
/// ```text
/// import-spec       = qualified-name ( '.' '*' )? ( 'as' identifier )?
///                   | qualified-name '.' '{' import-item ( ',' import-item )* '}'
/// ```
#[derive(Debug, Clone)]
pub enum ImportSpec {
    /// `import com.example.Foo;`, `import com.example.*;`, or
    /// `import com.example.Foo as Bar;`.
    Path {
        /// The path being imported.
        name: QualifiedName,
        /// True if the import ends in `.*`.
        wildcard: bool,
        /// Optional `as Alias` rename. Mutually exclusive with `wildcard`.
        alias: Option<Ident>,
    },
    /// `import com.example.{ Foo, Bar as Baz };`.
    Items {
        /// The prefix path before `{`.
        prefix: QualifiedName,
        /// The items inside the braces.
        items: Vec<ImportItem>,
    },
}

/// One entry inside a grouped import.
#[derive(Debug, Clone)]
pub struct ImportItem {
    /// The item's original name.
    pub name: Ident,
    /// Optional `as Alias` rename for this specific item.
    pub alias: Option<Ident>,
}
