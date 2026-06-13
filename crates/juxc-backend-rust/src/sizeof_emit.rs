//! `sizeof(...)` lowering — picks between the type form
//! (`std::mem::size_of::<T>()`) and the value form
//! (`std::mem::size_of_val(&expr)`) per §5.9.3.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{Expr, QualifiedName, SizeOfExpr};

use crate::analysis::{starts_with_uppercase, try_flatten_dotted_path};
use crate::exprs::expr_span_of;
use crate::types::jux_primitive_to_rust;
use crate::RustEmitter;

impl RustEmitter {
    /// Lower a `sizeof(...)` expression per §5.9.
    ///
    /// The operand is parsed as a generic expression; we apply the
    /// syntactic disambiguation rule from §5.9.3 to decide between the
    /// **type form** (lowers to `std::mem::size_of::<T>()`) and the
    /// **value form** (lowers to `std::mem::size_of_val(&expr)`).
    ///
    /// The disambiguation:
    /// 1. Primitive names → type.
    /// 2. Uppercase-leading bare ident → type.
    /// 3. Lowercase-leading bare ident → value.
    /// 4. Multi-segment path → type. After the parser refactor a
    ///    dotted source path like `std.io.Stream` arrives as a
    ///    `Field`-chain rooted in a `Path`, so we flatten that shape
    ///    here before applying rule 4 — but only when the chain is a
    ///    genuine *type path*. A dotted **member/property access on a
    ///    value** (`obj.Name`) is syntactically identical to a type path
    ///    yet is the value form (§5.9.2); see [`Self::dotted_sizeof_is_value`].
    /// 5. Anything else (compound expression) → value.
    pub(crate) fn emit_sizeof(&mut self, s: &SizeOfExpr) {
        // Single-segment Path uses the existing rules 1-3 dispatch.
        if let Expr::Path(qn) = &*s.operand {
            self.emit_sizeof_path(qn);
            return;
        }
        // Field-chain rooted in a Path: §5.9.3 rule 4 wants the type form
        // (joined with `::`) for a real type path like `std.io.Stream`, but
        // a member/property access on a value (`obj.Name`) wears the same
        // shape and must take the value form instead — otherwise we'd emit
        // `size_of::<obj::Name>()`, an unresolved-module error in Rust.
        if let Some(segs) = try_flatten_dotted_path(&s.operand) {
            if !self.dotted_sizeof_is_value(&segs, &s.operand) {
                self.w.push_str("std::mem::size_of::<");
                self.w.push_str(&segs.join("::"));
                self.w.push_str(">()");
                return;
            }
        }
        // Everything else — a compound expression, or a dotted member
        // access on a value — is the value form.
        self.emit_sizeof_value(&s.operand);
    }

    /// Disambiguate a dotted `sizeof` operand between a **type path**
    /// (`std.io.Stream`, §5.9.3 rule 4 → type form) and a **member/property
    /// access on a value** (`obj.Name` → value form, §5.9.2). The two
    /// spellings are syntactically indistinguishable once properties exist,
    /// so we resolve them semantically — exactly as `typeof` does (it reads
    /// the operand's recorded type). The operand is a value when either:
    ///
    /// - its leading segment names a local/parameter in scope, or
    /// - tycheck recorded a concrete (non-`Unknown`) value type for the
    ///   whole operand — true for `obj.Name`, false for a type path whose
    ///   root resolves to a package/type rather than a value.
    fn dotted_sizeof_is_value(&self, segs: &[String], operand: &Expr) -> bool {
        if let Some(root) = segs.first() {
            if self.local_types.iter().any(|scope| scope.contains_key(root)) {
                return true;
            }
        }
        self.expr_types
            .get(&expr_span_of(operand))
            .is_some_and(|t| !matches!(t, juxc_tycheck::Ty::Unknown))
    }

    /// Emit `size_of::<T>()` or `size_of_val(&name)` for a `Path`-shaped
    /// `sizeof` operand, choosing per the §5.9.3 disambiguation rule.
    pub(crate) fn emit_sizeof_path(&mut self, qn: &QualifiedName) {
        if qn.segments.is_empty() {
            // Defensive: parser recovery may leave an empty path. Emit
            // a compile-time-broken stub so the user sees a Rust error.
            self.w.push_str("std::mem::size_of::<()>()");
            return;
        }

        if qn.segments.len() == 1 {
            let name = &qn.segments[0].text;

            // Rule 1: primitive name → type form.
            let synth_ref = juxc_ast::TypeRef {
                name: qn.clone(),
                generic_args: Vec::new(),
                nullable: false,
                array_shape: None,
                fn_shape: None,
                ptr_depth: 0,
                span: qn.span,
            };
            if let Some(rust_ty) = jux_primitive_to_rust(&synth_ref) {
                self.w.push_str("std::mem::size_of::<");
                self.w.push_str(rust_ty);
                self.w.push_str(">()");
                return;
            }

            // Rules 2 + 3: case-based dispatch.
            if starts_with_uppercase(name) {
                // Uppercase → type form. Emit the name verbatim and let
                // Rust resolve it (works for user types, errors if the
                // name is actually a variable misnamed PascalCase).
                self.w.push_str("std::mem::size_of::<");
                self.w.push_str(name);
                self.w.push_str(">()");
            } else {
                // Lowercase → value form.
                self.w.push_str("std::mem::size_of_val(&");
                self.w.push_str(name);
                self.w.push(')');
            }
            return;
        }

        // Rule 4: multi-segment path → type form. Join with `::` for Rust.
        let path = qn
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("::");
        self.w.push_str("std::mem::size_of::<");
        self.w.push_str(&path);
        self.w.push_str(">()");
    }

    /// Emit `size_of_val(&(expr))` for a compound `sizeof` operand —
    /// per §5.9.3 rule 5, anything that isn't a bare path is a value.
    pub(crate) fn emit_sizeof_value(&mut self, expr: &Expr) {
        self.w.push_str("std::mem::size_of_val(&(");
        self.emit_expr(expr);
        self.w.push_str("))");
    }
}
