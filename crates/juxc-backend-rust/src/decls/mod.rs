//! Top-level declaration emitters — split into action-focused
//! submodules to keep each file readable.
//!
//! - [`interfaces`] — `emit_interface_decl`
//! - [`records`]    — `emit_record_decl` + the auto-derived Display impl
//! - [`enums`]      — `emit_enum_decl`
//! - [`classes`]    — class struct + marker trait + trait impls + method
//! - [`constructors`] — `emit_constructor`, the simple-ctor fast path, and
//!   the synthetic default ctor
//! - [`operators`]  — operator overloads (inherent methods + trait wrappers)
//! - [`functions`]  — top-level function decls and the trailing-return
//!   elision machinery shared with method bodies
//!
//! Behavior is identical to the pre-split `decls.rs` — this directory
//! is a pure reorganization. Cross-module helpers like
//! [`synthetic_op_method_name`] live here at module-root so any
//! submodule (and the expression emitter, via `crate::decls::…`) can
//! reach them.

use juxc_ast::OperatorKind;

pub(crate) mod classes;
pub(crate) mod constructors;
pub(crate) mod enums;
pub(crate) mod functions;
pub(crate) mod interfaces;
pub(crate) mod observers;
pub(crate) mod operators;
pub(crate) mod records;

/// Synthetic name used for the inherent method that carries an
/// operator's body. The class's trait impls (`PartialEq`, `Display`,
/// etc.) delegate to these.
///
/// Naming convention: `__op_<rust-trait-method>`. So `operator==`
/// → `__op_eq` (matches `PartialEq::eq`), `operator string` →
/// `__op_string` (matches our wrapper around `Display::fmt`), and so
/// on. The double-underscore prefix flags these as compiler-generated;
/// the user shouldn't write methods with that prefix.
pub(crate) fn synthetic_op_method_name(kind: OperatorKind) -> &'static str {
    match kind {
        OperatorKind::Eq => "__op_eq",
        OperatorKind::In => "__op_in",
        OperatorKind::Cmp => "__op_cmp",
        OperatorKind::Lt => "__op_lt",
        OperatorKind::Le => "__op_le",
        OperatorKind::Gt => "__op_gt",
        OperatorKind::Ge => "__op_ge",
        OperatorKind::Hash => "__op_hash",
        OperatorKind::ToString => "__op_string",
        OperatorKind::Plus => "__op_add",
        OperatorKind::Minus => "__op_sub",
        OperatorKind::Mul => "__op_mul",
        OperatorKind::Div => "__op_div",
        OperatorKind::Rem => "__op_rem",
        OperatorKind::BitAnd => "__op_bitand",
        OperatorKind::BitOr => "__op_bitor",
        OperatorKind::BitXor => "__op_bitxor",
        OperatorKind::BitNot => "__op_not",
        OperatorKind::Shl => "__op_shl",
        OperatorKind::Shr => "__op_shr",
        OperatorKind::Neg => "__op_neg",
        OperatorKind::Index => "__op_index",
        OperatorKind::IndexSet => "__op_index_set",
        OperatorKind::Call => "__op_call",
        OperatorKind::Range => "__op_range",
        OperatorKind::RangeInclusive => "__op_range_inclusive",
    }
}

// Note: `derive_attribute_for_value_type` lived here before §O.3.4
// deletion landed. Records and enums each grew their own
// deletion-aware helpers (`record_derive_attribute` /
// `enum_derive_attribute`) so the spec wording can evolve
// independently per kind.
