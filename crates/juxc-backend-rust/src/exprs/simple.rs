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
    /// The bare class / interface name a downcast or type-test **source**
    /// expression evaluates to, robust to two failure modes the raw span-keyed
    /// `expr_types` lookup has: a `T?` nullable wrapper (peeled here) and a
    /// span collision from interpolated `${…}` re-parsing (falls back to the
    /// name-keyed `receiver_class_bare` / `local_types` path). Returns `None`
    /// when the source isn't a user type.
    pub(crate) fn cast_source_bare(&self, expr: &Expr) -> Option<String> {
        // For a bare place (`Path`/`this`/field/index), the NAME-keyed lookup
        // is immune to the span collisions that interpolated `${…}` re-parsing
        // causes — prefer it so `${d => …} ${a => …}` doesn't alias `d`'s type
        // to `a`'s.
        if matches!(
            expr,
            Expr::Path(_) | Expr::This(_) | Expr::Field(_) | Expr::Index(_)
        ) {
            if let Some(b) = self.receiver_class_bare(expr) {
                return Some(b);
            }
        }
        // Otherwise the span-keyed inferred type, peeling any `T?` wrapper.
        if let Some(ty) = self.expr_types.get(&crate::exprs::expr_span_of(expr)) {
            let mut t = ty;
            while let juxc_tycheck::Ty::Nullable(inner) = t {
                t = inner;
            }
            if let juxc_tycheck::Ty::User { name, .. } = t {
                return Some(name.rsplit('.').next().unwrap_or(name).to_string());
            }
        }
        self.receiver_class_bare(expr)
    }

    /// True when a value statically typed `bare` is a **trait object**
    /// (a polymorphic base or an interface) — i.e. it carries the
    /// `__jux_as_<T>` runtime-type hooks. Concrete classes don't.
    pub(crate) fn source_is_dyn(&self, bare: &str) -> bool {
        self.poly_base_classes.contains(bare)
            || self.lookup_interface_by_bare_or_fqn(bare).is_some()
    }

    pub(crate) fn emit_cast(&mut self, c: &CastExpr) {
        // **Reference cast between user types** (class / interface): an upcast
        // coerces into the target trait object, a downcast goes through the
        // runtime-type `__jux_as_<T>` hook (panicking `ClassCastException` on
        // failure), and a same-type cast is the identity. Only the numeric
        // `value as Type` path falls through to the bottom.
        let target_plain = c.ty.array_shape.is_none()
            && !c.ty.nullable
            && c.ty.ptr_depth == 0
            && c.ty.fn_shape.is_none();
        let target_bare = c.ty.name.segments.last().map(|s| s.text.clone());
        let target_is_user = target_plain
            && target_bare.as_deref().is_some_and(|t| {
                self.lookup_class_by_bare_or_fqn(t).is_some()
                    || self.lookup_interface_by_bare_or_fqn(t).is_some()
            });
        if target_is_user {
            let t = target_bare.unwrap();
            let src_bare = self.cast_source_bare(&c.value);
            if let Some(s) = src_bare {
                if s == t {
                    // Identity cast — emit the value (share-clone a place).
                    self.emit_expr(&c.value);
                    if self.wrapper_value_needs_clone(&c.value) {
                        self.w.push_str(".clone()");
                    }
                    return;
                }
                if self.class_is_a(&s, &t) {
                    // Upcast (S IS-A T) — coerce into T's trait-object slot.
                    self.emit_expr_coerced_to_iface(&c.ty, &c.value);
                    return;
                }
                if self.source_is_dyn(&s) {
                    // Downcast (T IS-A S) or interface sidecast (a common
                    // subtype exists) — runtime-type hook; panics
                    // `ClassCastException` on a miss. The hook lives on a single
                    // trait per supertrait chain (topmost base), so the
                    // unqualified call is unambiguous.
                    self.w.push('(');
                    self.emit_expr(&c.value);
                    self.w.push_str(".__jux_as_");
                    self.w.push_str(&t);
                    self.w.push_str("().unwrap_or_else(|| panic!(\"ClassCastException: value is not a ");
                    self.w.push_str(&t);
                    self.w.push_str("\")))");
                    return;
                }
                // Concrete source that's neither identity nor an upcast — a
                // leaf can't narrow further, and tycheck (E0442) rejected
                // unrelated casts, so this is unreachable for valid programs.
                // Emit the value defensively rather than a broken hook call.
                self.emit_expr(&c.value);
                return;
            }
        }
        // Numeric / primitive cast — Rust `value as type`.
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

    /// Lower the **bare boolean** type-test `x => T` (the binder form in an
    /// `if` condition is handled in `emit_if`). For a value statically typed as
    /// a trait object (a polymorphic base or interface) the test is the runtime
    /// hook `x.__jux_as_T().is_some()`. For a concrete value the runtime type
    /// is known at compile time, so the result is a constant `true`/`false` —
    /// emitted inside a block that still evaluates the operand for its side
    /// effects.
    pub(crate) fn emit_type_test(&mut self, t: &juxc_ast::TypeTestExpr) {
        let target = t
            .ty
            .name
            .segments
            .last()
            .map(|s| s.text.clone())
            .unwrap_or_default();
        let src = self.cast_source_bare(&t.value);
        // An identity test (`z => Animal` where `z` is already `Animal`) is
        // always true — there is no `__jux_as_<Self>` hook (subtypes only), so
        // it must NOT take the dyn-hook path; the concrete branch below folds it
        // to a constant `true` via `class_is_a`.
        let src_is_dyn = src.as_deref().is_some_and(|s| self.source_is_dyn(s))
            && src.as_deref() != Some(target.as_str());
        if src_is_dyn {
            self.w.push('(');
            self.emit_expr(&t.value);
            self.w.push_str(".__jux_as_");
            self.w.push_str(&target);
            self.w.push_str("().is_some())");
        } else {
            // Concrete (or unknown) source — runtime type is statically known.
            // The block is **parenthesized** so it's an expression in every
            // position (binary operand, `return`, …), not a statement.
            let result = src
                .as_deref()
                .map(|s| self.class_is_a(s, &target))
                .unwrap_or(false);
            self.w.push_str("({ let _ = &(");
            self.emit_expr(&t.value);
            self.w.push_str("); ");
            self.w.push_str(if result { "true" } else { "false" });
            self.w.push_str(" })");
        }
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
        // Unary `-` overload (§O.2.4): `-obj` on a type declaring
        // `operator-()` dispatches to its `__op_neg` method.
        if matches!(u.op, juxc_ast::UnaryOp::Neg)
            && self.expr_declares_operator(&u.operand, juxc_ast::OperatorKind::Neg)
        {
            self.emit_expr_with_parent_prec(&u.operand, u8::MAX, false);
            self.w.push_str(".__op_neg()");
            return;
        }
        self.w.push_str(u.op.as_rust_str());
        // Unary precedence is higher than any binary; reusing
        // emit_expr_with_parent_prec at UNARY_PREC gives the right
        // wrapping for free.
        self.emit_expr_with_parent_prec(&u.operand, UNARY_PREC, /*right=*/ false);
    }
}
