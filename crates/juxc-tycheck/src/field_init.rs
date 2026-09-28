//! Instance field initializers that use the object being built (ERRATA
//! E139).
//!
//! `int y = twice(3);`, `Worker w = new Worker(this);` and an anonymous class
//! reaching the object's members all need the object itself, which does not
//! exist yet while the fields are being put together. Such an initializer
//! runs against the finished object instead: the object is built with the
//! field at a stand-in value, and the initializer then assigns it, before any
//! `init` block or constructor body (JUX-LANG-V1 §7.3.1). Every initializer
//! after the first such one is run the same way, so they still run in the
//! order they are written; and all of a class's are when an ancestor's are,
//! so the order holds across the hierarchy.
//!
//! The backend decides the lowering from [`deferred_indices`]; the driver's
//! [`crate::late_fields`] pass reads the same answer to give a field with no
//! stand-in value a nullable slot.

use juxc_ast::{Expr, FieldDecl};

/// Whether an instance field's initializer uses the object being built:
/// `this`, `super`, or a bare name that is one of its instance members (a
/// field read, a method call).
pub fn uses_object(init: &Expr, is_member: &dyn Fn(&str) -> bool) -> bool {
    let mut found = false;
    juxc_ast::visit::for_each_expr_in(init, &mut |e| match e {
        Expr::This(_) | Expr::Super(_) => found = true,
        Expr::Path(qn) if qn.segments.len() == 1 && is_member(&qn.segments[0].text) => found = true,
        _ => {}
    });
    found
}

/// The instance fields (indices into `fields`) whose initializers run
/// against the finished object: every one from the first that uses the
/// object on, or all of them when `ancestor_defers`.
pub fn deferred_indices(fields: &[FieldDecl], ancestor_defers: bool, is_member: &dyn Fn(&str) -> bool) -> Vec<usize> {
    let with_init: Vec<usize> = fields
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.is_static && f.default.is_some())
        .map(|(i, _)| i)
        .collect();
    if ancestor_defers {
        return with_init;
    }
    let first = with_init
        .iter()
        .position(|&i| fields[i].default.as_ref().is_some_and(|d| uses_object(d, is_member)));
    match first {
        Some(k) => with_init[k..].to_vec(),
        None => Vec::new(),
    }
}
