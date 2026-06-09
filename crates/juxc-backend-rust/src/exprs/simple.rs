//! Small leaf-expression emitters — cast, range, prefix-unary.
//! Each is short enough on its own that grouping them keeps related
//! "operator-shaped but not arithmetic" lowerings near each other.

use juxc_ast::{CastExpr, Expr, RangeExpr, UnaryExpr};

use crate::exprs::UNARY_PREC;
use crate::RustEmitter;

impl RustEmitter {
    /// Lower `value as Type` to Rust `value as type`.
    ///
    /// **Paren rules.** Rust's `as` binds tighter than every binary
    /// operator and looser than unary. So a binary operand of `as` must
    /// be parenthesized — `(a + b) as i64` not `a + b as i64`, since
    /// the latter would parse in Rust as `a + (b as i64)`. A range
    /// operand needs parens too (range is the loosest expression form
    /// in Rust). Everything else — unary, postfix, path, literal,
    /// another cast — emits naked.
    pub(crate) fn emit_cast(&mut self, c: &CastExpr) {
        let needs_paren = matches!(&*c.value, Expr::Binary(_) | Expr::Range(_));
        if needs_paren {
            self.w.push('(');
        }
        self.emit_expr(&c.value);
        if needs_paren {
            self.w.push(')');
        }
        self.w.push_str(" as ");
        self.emit_type_as_rust(&c.ty);
    }

    /// Lower a range expression. Jux and Rust use the same tokens with
    /// the same meanings: `a..b` is half-open, `a..=b` is inclusive.
    /// No parens around operands — they're already at additive
    /// precedence (tighter than range), so they emit naked.
    pub(crate) fn emit_range(&mut self, r: &RangeExpr) {
        self.emit_expr(&r.start);
        if r.inclusive {
            self.w.push_str("..=");
        } else {
            self.w.push_str("..");
        }
        self.emit_expr(&r.end);
    }

    /// Lower a prefix unary expression. The operator text comes from
    /// [`UnaryOp::as_rust_str`]; the operand is wrapped in parens only
    /// if its precedence would otherwise change the grouping.
    ///
    /// Concretely, since every binary operator we model binds **looser**
    /// than unary (§A.4 levels 6–11 vs level 18), any `Binary` operand
    /// needs parens (`-(x + y)` rather than `-x + y`). Atomic and postfix
    /// operands don't.
    pub(crate) fn emit_unary(&mut self, u: &UnaryExpr) {
        // `&x` (address-of) lowers to a raw-pointer macro, not a prefix
        // token: `core::ptr::addr_of_mut!(x)` yields a `*mut T` (a Rust
        // reference `&x` is a different type). The operand is a place, so
        // it's emitted verbatim inside the macro call.
        if matches!(u.op, juxc_ast::UnaryOp::AddrOf) {
            self.w.push_str("core::ptr::addr_of_mut!(");
            self.emit_expr(&u.operand);
            self.w.push(')');
            return;
        }
        self.w.push_str(u.op.as_rust_str());
        // Unary precedence is higher than any binary; reusing
        // emit_expr_with_parent_prec at UNARY_PREC gives the right
        // wrapping for free.
        self.emit_expr_with_parent_prec(&u.operand, UNARY_PREC, /*right=*/ false);
    }
}
