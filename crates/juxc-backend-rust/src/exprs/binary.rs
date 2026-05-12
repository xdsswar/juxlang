//! Binary-expression emission — the `+`/`-`/`*`/`/`/`%`/bitwise/shift/
//! comparison family, plus the two special-case lowerings:
//! string-concatenation (`&str + &str` → `format!`) and the
//! clone-injection rewrite for operator overloads on user types.

use juxc_ast::{BinaryExpr, BinaryOp, Expr, OperatorKind};
use juxc_tycheck::Ty;

use crate::analysis::is_string_literal;
use crate::decls::synthetic_op_method_name;
use crate::exprs::{binary_prec, expr_span_of};
use crate::RustEmitter;

/// Recursively flatten a string-concat `Add` chain into a list of
/// operands in left-to-right order. An operand is "concat-shaped"
/// when it's a `Binary(Add, lhs, rhs)` with at least one string-
/// literal child — exactly the condition `emit_binary` uses to
/// route into `emit_string_concat`. Any other operand contributes
/// itself as a single element.
fn collect_string_concat_operands<'a>(b: &'a BinaryExpr, out: &mut Vec<&'a Expr>) {
    push_concat_operand(&b.left, out);
    push_concat_operand(&b.right, out);
}

fn push_concat_operand<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::Binary(inner) = e {
        if inner.op == BinaryOp::Add
            && (is_string_literal(&inner.left) || is_string_literal(&inner.right))
        {
            collect_string_concat_operands(inner, out);
            return;
        }
    }
    out.push(e);
}

/// Fold each operand of a flattened string-concat into either part
/// of the `format!` template string (for `Literal::String`
/// operands) or into the runtime arg list (everything else).
///
/// The returned tuple is `(format_template, runtime_args)`. The
/// template is ready to drop straight inside the macro's `"..."`
/// quotes — each literal's bytes are re-escaped for Rust string
/// literal context, and each `{` / `}` inside a literal is doubled
/// so `format!`'s own parser keeps its hands off them.
///
/// Mirrors the brace-doubling that
/// `RustEmitter::emit_interp_literal_chunk` does for interpolation
/// segments, but for arbitrary `Literal::String` text rather than
/// lexer-segmented interp chunks.
fn fold_concat_into_format<'a>(
    operands: &[&'a Expr],
) -> (String, Vec<&'a Expr>) {
    let mut template = String::new();
    let mut runtime: Vec<&'a Expr> = Vec::new();
    for op in operands {
        if let Expr::Literal(juxc_ast::Literal::String(s)) = op {
            for ch in s.chars() {
                match ch {
                    // Brace-double for format!() parser safety.
                    '{' => template.push_str("{{"),
                    '}' => template.push_str("}}"),
                    // Re-escape Rust string-literal chars.
                    '"' => template.push_str("\\\""),
                    '\\' => template.push_str("\\\\"),
                    '\n' => template.push_str("\\n"),
                    '\r' => template.push_str("\\r"),
                    '\t' => template.push_str("\\t"),
                    c => template.push(c),
                }
            }
        } else {
            template.push_str("{}");
            runtime.push(op);
        }
    }
    (template, runtime)
}

/// Match `expr op null` / `null op expr` for `==` / `!=`. Returns
/// `Some((target_expr, is_equality))` when the binary is a null
/// comparison; `is_equality` is true for `==`, false for `!=`.
/// Returns `None` for every other shape — including `null == null`
/// (degenerate but harmless: caller falls through to the generic
/// binary path which emits `None == None`, valid Rust).
fn match_null_comparison<'a>(b: &'a BinaryExpr) -> Option<(&'a Expr, bool)> {
    let is_eq = match b.op {
        BinaryOp::Eq => true,
        BinaryOp::NotEq => false,
        _ => return None,
    };
    let left_null = matches!(*b.left, Expr::Literal(juxc_ast::Literal::Null));
    let right_null = matches!(*b.right, Expr::Literal(juxc_ast::Literal::Null));
    match (left_null, right_null) {
        (false, true) => Some((&b.left, is_eq)),
        (true, false) => Some((&b.right, is_eq)),
        _ => None,
    }
}

/// Same shape as `field::receiver_needs_parens` (kept local so we
/// don't cross-module-import a tiny helper). True when emitting
/// `expr.method()` would require wrapping `expr` in parens —
/// false for atoms, true for composite shapes.
fn receiver_needs_parens(e: &Expr) -> bool {
    !matches!(
        e,
        Expr::Path(_)
            | Expr::This(_)
            | Expr::Field(_)
            | Expr::Call(_)
            | Expr::Index(_)
            | Expr::Literal(_)
            | Expr::InterpString(_)
            | Expr::NewObject(_)
            | Expr::NewArray(_)
            | Expr::NewArrayLit(_)
    )
}

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
        // Null-equality peephole: `x == null` and `x != null` lower
        // to `x.is_none()` / `x.is_some()` respectively, instead of
        // the literal `x == None` (which would require `T: PartialEq`
        // even for the nullable-only check). The match accepts the
        // null literal on either side, since Jux source allows both
        // orderings. The non-null side is emitted as a method
        // receiver, so we wrap composite expressions in parens via
        // the receiver-paren helper.
        if let Some((target, is_eq)) = match_null_comparison(b) {
            let needs_parens = receiver_needs_parens(target);
            if needs_parens {
                self.w.push('(');
            }
            self.emit_expr(target);
            if needs_parens {
                self.w.push(')');
            }
            self.w.push_str(if is_eq { ".is_none()" } else { ".is_some()" });
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
        // Comparison ops (`==`, `!=`, `<`, `<=`, `>`, `>=`) borrow
        // both operands through `PartialEq`/`PartialOrd` — String /
        // generic field reads inside don't need auto-`.clone()`.
        // We set the flag for the lifetime of both operand
        // emissions so a nested `(a == b)` inside another binary
        // also benefits.
        let is_cmp = matches!(
            b.op,
            BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge,
        );
        let prev_cmp = self.emitting_comparison_operand;
        if is_cmp {
            self.emitting_comparison_operand = true;
        }
        // Left side of a left-associative op: equal precedence is OK,
        // because emission order already preserves grouping.
        self.emit_expr_with_parent_prec(&b.left, prec, /*right=*/ false);
        self.w.push(' ');
        self.w.push_str(b.op.as_rust_str());
        self.w.push(' ');
        // Right side: equal precedence would *change* grouping
        // (`1 + (2 + 3)` vs `1 + 2 + 3`), so parens are required.
        self.emit_expr_with_parent_prec(&b.right, prec, /*right=*/ true);
        self.emitting_comparison_operand = prev_cmp;
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

    /// Emit a string-concatenation `Add` as a single Rust `format!`
    /// call — flattening any nested `+` chains AND folding any
    /// string-literal operands directly into the format string.
    ///
    /// `"hello, " + name + "!"` was already flattened by
    /// `collect_string_concat_operands` into `["hello, ", name, "!"]`.
    /// Naively this becomes `format!("{}{}{}", "hello, ", name, "!")`.
    /// We further notice that the literal operands can simply BECOME
    /// part of the format string (with `{` / `}` doubled for safety):
    /// `format!("hello, {}!", name)`. One `{}` per non-literal,
    /// every literal inlined — exactly what a human would write,
    /// and one less `format!` arg per literal at runtime.
    pub(crate) fn emit_string_concat(&mut self, b: &BinaryExpr) {
        let mut operands: Vec<&juxc_ast::Expr> = Vec::new();
        collect_string_concat_operands(b, &mut operands);
        let (fmt_string, runtime_args) = fold_concat_into_format(&operands);
        self.w.push_str("format!(\"");
        self.w.push_str(&fmt_string);
        self.w.push('"');
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = true;
        for op in &runtime_args {
            self.w.push_str(", ");
            self.emit_expr(op);
        }
        self.emitting_format_arg = prev;
        self.w.push(')');
    }
}
