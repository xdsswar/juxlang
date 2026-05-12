//! Binary-expression emission — the `+`/`-`/`*`/`/`/`%`/bitwise/shift/
//! comparison family, plus the two special-case lowerings:
//! string-concatenation (`&str + &str` → `format!`) and the
//! clone-injection rewrite for operator overloads on user types.

use juxc_ast::{BinaryExpr, BinaryOp, OperatorKind};
use juxc_tycheck::Ty;

use crate::analysis::is_string_literal;
use crate::decls::synthetic_op_method_name;
use crate::exprs::{binary_prec, expr_span_of};
use crate::RustEmitter;

impl RustEmitter {
    /// Lower a binary expression. Every operator in [`BinaryOp`] maps
    /// onto a Rust operator with identical spelling, so the lowering is
    /// mostly textual.
    ///
    /// **String concatenation special case.** Rust has no `&str + &str`
    /// operator, but Jux's `+` is overloaded for string concatenation.
    /// When we see `Add` with at least one **string-literal** operand,
    /// we lower to `format!("{}{}", lhs, rhs)` — that produces an owned
    /// `String` that any `Display` operand can feed into. This covers
    /// the common `"hello " + name` / `name + " world"` shapes; once
    /// type-checking carries real type info, we can extend the rule to
    /// any pair of string-typed operands.
    ///
    /// **Parens:** we add them only when an operand's precedence is
    /// *lower* than this operator's (or equal-precedence on the right
    /// side of a left-associative parent, where missing parens would
    /// silently change grouping). This matches what a human would write
    /// and keeps the output rustfmt-shaped.
    pub(crate) fn emit_binary(&mut self, b: &BinaryExpr) {
        if b.op == BinaryOp::Add && (is_string_literal(&b.left) || is_string_literal(&b.right)) {
            self.emit_string_concat(b);
            return;
        }
        // Operator-overload clone-injection: when the LHS is a user
        // class with an `operator+` (etc.) declared, rewrite from the
        // trait form (`a + b` — consumes both) into a direct inherent
        // method call (`a.__op_add(b.clone())`). Rust's method-call
        // autoref preserves the LHS; the explicit RHS clone preserves
        // the RHS. The trait impl still exists so call sites that DO
        // want consumption (rare) can be rewritten to use it later.
        if let Some(synth) = self.class_op_method_for_binary(b) {
            self.emit_class_op_method_call(b, synth);
            return;
        }
        let prec = binary_prec(b.op);
        // Left side of a left-associative op: equal precedence is OK,
        // because emission order already preserves grouping.
        self.emit_expr_with_parent_prec(&b.left, prec, /*right=*/ false);
        self.w.push(' ');
        self.w.push_str(b.op.as_rust_str());
        self.w.push(' ');
        // Right side: equal precedence would *change* grouping
        // (`1 + (2 + 3)` vs `1 + 2 + 3`), so parens are required.
        self.emit_expr_with_parent_prec(&b.right, prec, /*right=*/ true);
    }

    /// If `b`'s LHS is a known user class that defines the matching
    /// operator overload, return the synthetic inherent method name
    /// (`__op_add`, `__op_sub`, …) we should dispatch through. Returns
    /// `None` for primitives, unknown types, comparison/logical ops
    /// (which don't consume operands), and class types that don't
    /// declare the relevant operator.
    fn class_op_method_for_binary(&self, b: &BinaryExpr) -> Option<&'static str> {
        let kind = match b.op {
            BinaryOp::Add => OperatorKind::Plus,
            BinaryOp::Sub => OperatorKind::Minus,
            BinaryOp::Mul => OperatorKind::Mul,
            BinaryOp::Div => OperatorKind::Div,
            BinaryOp::Rem => OperatorKind::Rem,
            BinaryOp::BitAnd => OperatorKind::BitAnd,
            BinaryOp::BitOr => OperatorKind::BitOr,
            BinaryOp::BitXor => OperatorKind::BitXor,
            BinaryOp::Shl => OperatorKind::Shl,
            BinaryOp::Shr => OperatorKind::Shr,
            // Equality and comparison use trait methods that take
            // references — no consumption, so no rewrite needed.
            // Logical && / || aren't overloadable per spec §O.2.5.
            _ => return None,
        };
        let left_ty = self.expr_types.get(&expr_span_of(&b.left))?;
        let Ty::User { name, .. } = left_ty else {
            return None;
        };
        let class = self.symbols.classes.get(name)?;
        if class.operators.contains_key(&kind) {
            Some(synthetic_op_method_name(kind))
        } else {
            None
        }
    }

    /// Emit `b` as a direct inherent method call:
    /// `<LHS>.<synth>(<RHS>.clone())`. The LHS is emitted at maximum
    /// precedence so any composite expression gets parens (a method
    /// call binds tighter than every binary op). The RHS is cloned
    /// before being passed by value.
    fn emit_class_op_method_call(&mut self, b: &BinaryExpr, synth: &str) {
        // Use the maximum precedence value so any non-atomic LHS
        // (binary, range, etc.) gets wrapped in parens — method-call
        // dot binds tighter than every binary op.
        self.emit_expr_with_parent_prec(&b.left, u8::MAX, /*right=*/ false);
        self.w.push('.');
        self.w.push_str(synth);
        self.w.push('(');
        self.emit_expr(&b.right);
        self.w.push_str(".clone())");
    }

    /// Emit a string-concatenation `Add` as a Rust `format!` call.
    ///
    /// Each operand is plugged into a `{}` placeholder — Rust's
    /// `Display` impl handles strings, integers, floats, bools, and
    /// most user types. Chains like `"a" + b + "c"` lower naturally:
    /// the inner `Add(... + "c")` recurses into another `format!` if
    /// either side is a literal, else into the regular binary path.
    pub(crate) fn emit_string_concat(&mut self, b: &BinaryExpr) {
        self.w.push_str("format!(\"{}{}\", ");
        self.emit_expr(&b.left);
        self.w.push_str(", ");
        self.emit_expr(&b.right);
        self.w.push(')');
    }
}
