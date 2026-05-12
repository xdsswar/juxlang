//! `sizeof(...)` lowering — picks between the type form
//! (`std::mem::size_of::<T>()`) and the value form
//! (`std::mem::size_of_val(&expr)`) per §5.9.3.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{Expr, QualifiedName, SizeOfExpr};

use crate::analysis::{starts_with_uppercase, try_flatten_dotted_path};
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
    ///    here before applying rule 4.
    /// 5. Anything else (compound expression) → value.
    pub(crate) fn emit_sizeof(&mut self, s: &SizeOfExpr) {
        // Single-segment Path uses the existing rules 1-3 dispatch.
        if let Expr::Path(qn) = &*s.operand {
            self.emit_sizeof_path(qn);
            return;
        }
        // Field-chain rooted in a Path collapses into a multi-segment
        // path for §5.9.3 rule 4 (type form, joined with `::`).
        if let Some(segs) = try_flatten_dotted_path(&s.operand) {
            self.w.push_str("std::mem::size_of::<");
            self.w.push_str(&segs.join("::"));
            self.w.push_str(">()");
            return;
        }
        // Everything else is a compound expression → value form.
        self.emit_sizeof_value(&s.operand);
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
