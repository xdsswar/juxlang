//! Small leaf-expression emitters — cast, range, prefix-unary.
//! Each is short enough on its own that grouping them keeps related
//! "operator-shaped but not arithmetic" lowerings near each other.

use juxc_ast::{CastExpr, Expr, RangeExpr, UnaryExpr};

use crate::exprs::UNARY_PREC;
use crate::RustEmitter;

/// True when emitting `expr.as_ptr()` for a class `&obj` (§L.6.5) would need
/// `expr` wrapped in parens. Atoms and postfix shapes (a bare path, `this`, a
/// field access, a call, an index) bind tightly enough to take `.as_ptr()`
/// directly; composite shapes (a cast, a binary, etc.) need wrapping. Same
/// shape as `binary::receiver_needs_parens`, kept local per the established
/// per-module-copy convention for this tiny helper.
fn addr_receiver_needs_parens(e: &Expr) -> bool {
    !matches!(
        e,
        Expr::Path(_)
            | Expr::This(_)
            | Expr::Field(_)
            | Expr::Call(_)
            | Expr::Index(_)
    )
}

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
        // `(char) intVal` — Rust only permits `u8 as char`, so a wider integer
        // (`int` is `isize`) would hit E0604. Route it through `char::from_u32`,
        // lossily mapping an out-of-range code unit to U+FFFD (a tolerant
        // narrowing). A `char`-typed source needs no conversion and falls
        // through to the identity-ish `as` path below.
        if target_plain
            && c.ty.name.segments.last().map(|s| s.text.as_str()) == Some("char")
        {
            let src_is_char = matches!(
                self.expr_types.get(&crate::exprs::expr_span_of(&c.value)),
                Some(juxc_tycheck::Ty::Primitive(juxc_tycheck::Primitive::Char))
            );
            if !src_is_char {
                self.w.push_str("char::from_u32((");
                self.emit_expr(&c.value);
                self.w.push_str(") as u32).unwrap_or('\u{FFFD}')");
                return;
            }
        }
        // Numeric / primitive cast — Rust `value as type`. The operand must be
        // parenthesized when it isn't a simple atom: a binary/range expression,
        // or a method call that hoists into a `{ … }` block (a bare block before
        // `as` is a parse error — see `call_emits_block`).
        let needs_paren = match &*c.value {
            Expr::Binary(_) | Expr::Range(_) => true,
            Expr::Call(call) => self.call_emits_block(call),
            _ => false,
        };
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
        if matches!(u.op, juxc_ast::UnaryOp::AddrOf) {
            // `&obj` on a CLASS object (§L.6.5). A class lowers to
            // `Rc<RefCell<C>>`, so the address we want is the INNER value's,
            // not the handle's. `RefCell::as_ptr` (reached through `Rc`'s
            // `Deref`) returns that `*mut C` without taking a runtime borrow
            // or touching the refcount — a borrowing, non-owning pointer,
            // which is exactly the spec semantics. A value-typed `&local`
            // falls through to the place-pointer macro below.
            if self.expr_is_class_instance(&u.operand) {
                // The class newtype is `C(Rc<RefCell<C_Inner>>)`, so the
                // handle's `Rc` is field `.0`; `.as_ptr()` on the `RefCell`
                // it derefs to yields `*mut C_Inner` — a pointer to the inner
                // data, matching the `C*` → `*mut C_Inner` type lowering.
                let needs_parens = addr_receiver_needs_parens(&u.operand);
                if needs_parens {
                    self.w.push('(');
                }
                self.emit_expr(&u.operand);
                if needs_parens {
                    self.w.push(')');
                }
                self.w.push_str(".0.as_ptr()");
                return;
            }
            // `&x` (address-of) on a value place lowers to a raw-pointer
            // macro, not a prefix token: `core::ptr::addr_of_mut!(x)` yields
            // a `*mut T` (a Rust reference `&x` is a different type). The
            // operand is a place, so it's emitted verbatim inside the macro.
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

    /// Lower an expression-position `++place` / `place++` (§A `incdec`,
    /// value form) to a value-returning Rust block.
    ///
    /// Rust has no `++`/`--`, so we synthesize a block that:
    ///   1. hoists any **side-effecting** sub-part of the place (an
    ///      index expression, or a non-trivial index/field receiver)
    ///      into a `let` temp so the place is evaluated EXACTLY ONCE
    ///      (`arr[next()]++` runs `next()` a single time);
    ///   2. performs the `+= 1` / `-= 1` mutation by REUSING the full
    ///      statement-level assignment machinery ([`Self::emit_assign`])
    ///      against the rewritten (already-hoisted) place — so wrapper-
    ///      class `.0.borrow_mut()`, `ref` cells, mutable statics, and
    ///      `operator[]=` places all stay correct without re-deriving
    ///      that logic here;
    ///   3. yields the right value: the **postfix** form caches the OLD
    ///      value in `let __jux_t` before the mutation and returns it;
    ///      the **prefix** form mutates first, then re-reads the place
    ///      (the NEW value).
    ///
    /// Shapes produced (decrement is identical with `- 1`):
    /// ```text
    /// x++       -> { let __jux_t = x; x += 1; __jux_t }
    /// ++x       -> { x += 1; x }
    /// a[i]++    -> { let __jux_i = i; let __jux_t = a[__jux_i]; a[__jux_i] += 1; __jux_t }
    /// ++a[i]    -> { let __jux_i = i; a[__jux_i] += 1; a[__jux_i] }
    /// o.f++     -> { let __jux_t = o.f; o.f += 1; __jux_t }   (o hoisted if non-trivial)
    /// ```
    ///
    /// The whole thing is a single Rust block expression — already a
    /// primary, so it needs no extra parens even in a format argument
    /// (`$"${x++}"`) or as a call argument.
    pub(crate) fn emit_incdec_value(&mut self, i: &juxc_ast::IncDecExpr) {
        // Build the rewritten place (with side-effecting parts replaced
        // by references to hoisted temps) and the list of `let` bindings
        // those temps need. `hoists` is emitted first inside the block.
        let mut hoists: Vec<(String, juxc_ast::Expr)> = Vec::new();
        let target = self.hoist_incdec_place(&i.target, &mut hoists);

        // The step operator + amount: `+= 1` / `-= 1`. We reuse
        // `emit_assign` with a compound op so every place shape is
        // handled by the existing, battle-tested store path.
        let one = juxc_ast::Expr::Literal(juxc_ast::Literal::Int(juxc_ast::IntLit {
            value: 1,
            kind: None,
            radix: juxc_ast::IntRadix::Decimal,
            digit_width: 1,
        }));
        let step = juxc_ast::AssignStmt {
            target: target.clone(),
            op: Some(if i.is_inc {
                juxc_ast::BinaryOp::Add
            } else {
                juxc_ast::BinaryOp::Sub
            }),
            value: one,
            span: i.span,
        };

        // The block reads a place value verbatim — never as a format
        // argument (`&` / borrow context would be wrong for the numeric
        // copy we want), so clear the flag while emitting the body and
        // restore it after.
        let prev_fmt = self.emitting_format_arg;
        self.emitting_format_arg = false;

        self.w.push_str("{ ");
        // 1. Hoist side-effecting sub-parts into temps (single-eval).
        for (name, expr) in &hoists {
            self.w.push_str("let ");
            self.w.push_str(name);
            self.w.push_str(" = ");
            self.emit_expr(expr);
            self.w.push_str("; ");
        }
        if i.is_prefix {
            // Prefix: mutate first, then yield the NEW value.
            self.emit_assign(&step); // emits `<place> += 1;\n`
            self.w.push(' ');
            self.emit_expr(&target);
        } else {
            // Postfix: cache the OLD value, mutate, then yield the cache.
            self.w.push_str("let __jux_t = ");
            self.emit_expr(&target);
            self.w.push_str("; ");
            self.emit_assign(&step);
            self.w.push_str(" __jux_t");
        }
        self.w.push_str(" }");

        self.emitting_format_arg = prev_fmt;
    }

    /// Rewrite an inc/dec place so it can be evaluated more than once
    /// (read THEN write) without re-running any side effects, by hoisting
    /// the side-effecting sub-parts into `let` temps.
    ///
    /// - **Name** (`x`) — no sub-parts, returned unchanged.
    /// - **Index** (`a[idx]`) — the index `idx` is ALWAYS hoisted to
    ///   `__jux_i` (it's read on both the load and the store), and a
    ///   non-trivial array receiver is hoisted too.
    /// - **Field** (`o.f`) — a non-trivial receiver `o` is hoisted.
    ///
    /// "Trivial" = a bare name or a `this`/`super` receiver: re-emitting
    /// it is free and side-effect-free, so it stays inline for readable
    /// output. Each hoisted sub-part is pushed onto `hoists` as
    /// `(temp_name, original_expr)` for the caller to emit as a `let`.
    fn hoist_incdec_place(
        &self,
        place: &juxc_ast::Expr,
        hoists: &mut Vec<(String, juxc_ast::Expr)>,
    ) -> juxc_ast::Expr {
        match place {
            // Index place: hoist the index (read+written), plus a
            // non-trivial receiver. `__jux_i` shadows safely inside this
            // block; nested inc/dec each get their own block scope.
            juxc_ast::Expr::Index(ix) => {
                let array = if Self::incdec_trivial_receiver(&ix.array) {
                    (*ix.array).clone()
                } else {
                    let name = "__jux_recv".to_string();
                    hoists.push((name.clone(), (*ix.array).clone()));
                    Self::incdec_temp_path(&name, ix.span)
                };
                let idx_name = "__jux_i".to_string();
                hoists.push((idx_name.clone(), (*ix.index).clone()));
                let index = Self::incdec_temp_path(&idx_name, ix.span);
                juxc_ast::Expr::Index(juxc_ast::IndexExpr {
                    array: Box::new(array),
                    index: Box::new(index),
                    span: ix.span,
                })
            }
            // Field place: hoist a non-trivial receiver.
            juxc_ast::Expr::Field(f) => {
                if Self::incdec_trivial_receiver(&f.object) {
                    place.clone()
                } else {
                    let name = "__jux_recv".to_string();
                    hoists.push((name.clone(), (*f.object).clone()));
                    juxc_ast::Expr::Field(juxc_ast::FieldExpr {
                        object: Box::new(Self::incdec_temp_path(&name, f.span)),
                        field: f.field.clone(),
                        safe: f.safe,
                        span: f.span,
                    })
                }
            }
            // Bare name (or any other place the parser admitted) — no
            // sub-parts to hoist; emit it in place.
            other => other.clone(),
        }
    }

    /// True when a place receiver is side-effect-free AND free to
    /// re-emit: a single-segment name, `this`, or `super`. Anything else
    /// (a call, an index, a chained field) gets hoisted.
    fn incdec_trivial_receiver(e: &juxc_ast::Expr) -> bool {
        matches!(e, juxc_ast::Expr::Path(_) | juxc_ast::Expr::This(_) | juxc_ast::Expr::Super(_))
    }

    /// Build a synthetic single-segment [`juxc_ast::Expr::Path`] naming a
    /// hoist temp (e.g. `__jux_i`). The span is cosmetic — these nodes
    /// never reach a diagnostic.
    fn incdec_temp_path(name: &str, span: juxc_source::Span) -> juxc_ast::Expr {
        juxc_ast::Expr::Path(juxc_ast::QualifiedName {
            segments: vec![juxc_ast::Ident {
                text: name.to_string(),
                span,
            }],
            span,
        })
    }
}
