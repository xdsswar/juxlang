//! Phase 3 — AST types.
//!
//! Productions mirror `JUX-GRAMMAR-ADDENDUM.md` §A.2. We start with the
//! smallest subset that supports milestone 1 (end-to-end "Hello, world!"):
//! a compilation unit containing a single `void main()` function whose body
//! is a single call expression.
//!
//! ## Style
//!
//! - Every node carries a [`juxc_source::Span`] so diagnostics emitted in
//!   later phases can point back into the source.
//! - Sum types use `enum`. Open sets (modifiers, where new ones may be
//!   added) use `#[non_exhaustive]` where appropriate.
//! - New nodes are added as the language surface grows. Each new node's
//!   doc comment cites the production it implements.
//!
//! ## Module layout
//!
//! The AST is split across action-focused modules so each file stays
//! readable. Every public type is re-exported from this crate root, so
//! external code continues to write `use juxc_ast::{Expr, ClassDecl, …}`
//! without caring which submodule owns each type.
//!
//! - [`common`] — [`Ident`], [`QualifiedName`], [`Visibility`].
//! - [`compilation`] — [`CompilationUnit`] and its preamble nodes.
//! - [`decls`] — top-level declarations (class, record, enum, interface,
//!   function) and their building blocks (`FieldDecl`, `Param`, …).
//! - [`stmts`] — [`Stmt`], [`Block`], and the control-flow statement
//!   variants.
//! - [`exprs`] — [`Expr`] and its precedence-layered variant payloads.
//! - [`patterns`] — `switch` plus the [`Pattern`] tree.
//! - [`types`] — [`TypeRef`] and array shapes.
//! - [`literals`] — leaf literal nodes (`Literal`, `IntLit`, `FloatLit`).

mod common;
mod compilation;
mod decls;
mod desugar;
mod exprs;
mod literals;
mod patterns;
mod stmts;
mod types;

pub use common::{Ident, QualifiedName, Visibility};
pub use compilation::{CompilationUnit, ImportDecl, ImportItem, ImportSpec, PackageDecl};
pub use decls::{
    AccessorBody, Annotation, AnnotationArg, ClassDecl, ConstDecl, ConstructorDecl, EnumDecl,
    EnumPayload, EnumVariant, ExternBlockDecl, FieldDecl, FnDecl, FnModifier, InterfaceDecl, OperatorDecl,
    OperatorKind, Param, PropertyAccessor, PropertyDecl, PropertySetter, RecordComponent,
    RecordDecl, ReturnType, TopLevelDecl, TypeAliasDecl, TypeParam, WhereConstraint,
};
pub use desugar::{
    backing_field_name as desugar_backing_field_name, desugar_properties,
    setter_method_name as desugar_static_setter_name,
};
pub use exprs::{
    AnonymousBody, BinaryExpr, BinaryOp, CallExpr, CastExpr, ElvisExpr, Expr, FieldExpr,
    IncDecExpr, IndexExpr, InterpSegment, InterpStringExpr, LambdaBody, LambdaExpr, LambdaParam,
    MethodRefExpr, NewArrayExpr, NewArrayLitExpr, NewObjectExpr, RangeExpr, SizeOfExpr,
    TernaryExpr, TypeTestExpr, UnaryExpr, UnaryOp,
};
pub use literals::{FloatKind, FloatLit, IntKind, IntLit, IntRadix, Literal};
pub use patterns::{Pattern, SwitchArm, SwitchBody, SwitchExpr};
pub use stmts::{
    AssignStmt, Block, CatchClause, DoWhileStmt, ElseBranch, ForCStmt, ForEachStmt, IfStmt,
    Stmt, TryStmt, VarDecl, WhileStmt,
};
pub use types::{ArrayDim, ArrayShape, FnTypeShape, GenericArg, TypeRef, WildcardArg, WildcardBound, TUPLE_SENTINEL};
