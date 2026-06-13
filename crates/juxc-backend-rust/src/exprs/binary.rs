//! Binary-expression emission — the `+`/`-`/`*`/`/`/`%`/bitwise/shift/
//! comparison family, plus the two special-case lowerings:
//! string-concatenation (`&str + &str` → `format!`) and the
//! clone-injection rewrite for operator overloads on user types.

use juxc_ast::{BinaryExpr, BinaryOp, Expr, OperatorKind};
use juxc_tycheck::Ty;

use crate::analysis::is_string_literal;
use crate::decls::synthetic_op_method_name;
use crate::exprs::call::literal_numeric_ty;
use crate::exprs::{binary_prec, expr_span_of, rust_primitive_name};
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

/// True iff a [`juxc_ast::TypeRef`] is the bare `String` type (no
/// generics, array shape, nullability, or fn-shape). Used to detect a
/// `String`-typed property for the string-concat trigger.
fn type_ref_is_string(ty: &juxc_ast::TypeRef) -> bool {
    !ty.nullable
        && ty.array_shape.is_none()
        && ty.fn_shape.is_none()
        && ty.generic_args.is_empty()
        && ty.name.segments.len() == 1
        && ty.name.segments[0].text == "String"
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
    /// True iff `e` is recorded by tycheck as having type
    /// `Ty::String`. Used by `emit_binary` to recognize
    /// `a + b` as string concatenation even when neither operand
    /// is a string literal (e.g. `name + greeting` where both are
    /// `String`-typed locals or fields). Lookup uses
    /// `expr_types[span]`; expressions tycheck didn't visit fall
    /// back to `false` and route through the standard binary path
    /// — same conservative fallback as the other type-aware
    /// helpers.
    ///
    /// **Smart-cast aware**: when `e` is a path to a binding that
    /// the smart-cast pass has unwrapped from `T?` to `T`
    /// (removed from `nullable_locals`), and tycheck still records
    /// the original nullable shape, we peel the `Ty::Nullable`
    /// wrap and check the inner type. Without this, the type-
    /// based concat trigger misses inside `if (b != null)` blocks
    /// where `b` is now effectively `String`.
    fn operand_is_string_typed(&self, e: &Expr) -> bool {
        // A nested string-concat (`(a + " ") + b`) is itself a
        // `String` — recurse so the OUTER `+` is recognized as concat
        // and the whole chain folds into one `format!`. Without this,
        // `First + " " + Last` would emit `format!("{} ", ..) + Last`,
        // which is the invalid Rust `String + String`.
        if let Expr::Binary(b) = e {
            if b.op == BinaryOp::Add
                && (is_string_literal(&b.left)
                    || is_string_literal(&b.right)
                    || self.operand_is_string_typed(&b.left)
                    || self.operand_is_string_typed(&b.right))
            {
                return true;
            }
        }
        // A property getter read (`obj.Prop` / bare `Prop` desugared to
        // `this.Prop`) whose declared property type is `String`. The
        // getter call's value is owned `String`, so it participates in
        // concat. Resolved through the receiver's class properties.
        if let Expr::Field(f) = e {
            if let Some(prop) = self.property_on_receiver(&f.object, &f.field.text) {
                if prop.getter.is_some() && type_ref_is_string(&prop.ty) {
                    return true;
                }
            }
        }
        let recorded = self.expr_types.get(&crate::exprs::expr_span_of(e));
        let unwrapped = self.unwrap_for_smart_cast(e, recorded);
        matches!(unwrapped, Some(juxc_tycheck::Ty::String))
    }

    /// Apply the smart-cast unwrap to a recorded tycheck `Ty`
    /// when `e` is a path to a binding that's been smart-cast
    /// out of `nullable_locals`. Other shapes fall through.
    fn unwrap_for_smart_cast<'a>(
        &self,
        e: &Expr,
        recorded: Option<&'a juxc_tycheck::Ty>,
    ) -> Option<&'a juxc_tycheck::Ty> {
        if let (Expr::Path(qn), Some(juxc_tycheck::Ty::Nullable(inner))) = (e, recorded) {
            if qn.segments.len() == 1
                && !self.nullable_locals.contains(&qn.segments[0].text)
            {
                return Some(inner.as_ref());
            }
        }
        recorded
    }
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
    /// Resolve an operand expression to its primitive type, best-effort:
    /// local-variable map first (span-collision-proof), then the tycheck
    /// `expr_types` map, then structural typing for literal-only
    /// expressions (whose spans are DUMMY and never reach the map).
    pub(crate) fn operand_primitive(&self, e: &Expr) -> Option<juxc_tycheck::Primitive> {
        if let Expr::Path(qn) = e {
            if qn.segments.len() == 1 {
                let bare = qn.segments[0].text.as_str();
                if let Some(Ty::Primitive(p)) = self
                    .local_types
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(bare))
                {
                    return Some(*p);
                }
            }
        }
        if let Some(Ty::Primitive(p)) = self.expr_types.get(&expr_span_of(e)) {
            return Some(*p);
        }
        literal_numeric_ty(e)
    }

    pub(crate) fn emit_binary(&mut self, b: &BinaryExpr) {
        // String-concat trigger fires when either operand is
        // **typed** as `String` — covers literals (parser sets
        // their type to `Ty::String` upstream) AND identifier
        // references whose declared type is `String`. Falling
        // back to the literal-shape check stays for operands
        // whose `expr_types` entry is missing (e.g. an expression
        // tycheck didn't visit). The two predicates are
        // complementary: literal-shape always wins, type-shape
        // catches the `name + greeting` case where both sides are
        // `String`-typed identifiers without a literal in sight.
        if b.op == BinaryOp::Add
            && (is_string_literal(&b.left)
                || is_string_literal(&b.right)
                || self.operand_is_string_typed(&b.left)
                || self.operand_is_string_typed(&b.right))
        {
            self.emit_string_concat(b);
            return;
        }
        // **Wrapping arithmetic (§S.2.1).** `a +% b` lowers to
        // `wrapping_add` & co at the LEFT operand's exact width (the
        // spec forbids mixed-width operands, so left decides). Both
        // operands cast through the type name — that also resolves
        // Rust's ambiguous-`{integer}` inference on literal operands.
        // Shift counts cast to `u32`, the Rust shift-amount type.
        if matches!(
            b.op,
            BinaryOp::WrapAdd
                | BinaryOp::WrapSub
                | BinaryOp::WrapMul
                | BinaryOp::WrapShl
                | BinaryOp::WrapShr
        ) {
            let prim = self
                .operand_primitive(&b.left)
                .or_else(|| self.operand_primitive(&b.right));
            let ty_name = prim.map(rust_primitive_name).unwrap_or("isize");
            let (method, rhs_cast) = match b.op {
                BinaryOp::WrapAdd => ("wrapping_add", ty_name),
                BinaryOp::WrapSub => ("wrapping_sub", ty_name),
                BinaryOp::WrapMul => ("wrapping_mul", ty_name),
                BinaryOp::WrapShl => ("wrapping_shl", "u32"),
                BinaryOp::WrapShr => ("wrapping_shr", "u32"),
                _ => unreachable!("guarded by the matches! above"),
            };
            self.w.push_str("((");
            self.emit_expr(&b.left);
            self.w.push_str(") as ");
            self.w.push_str(ty_name);
            self.w.push_str(").");
            self.w.push_str(method);
            self.w.push_str("((");
            self.emit_expr(&b.right);
            self.w.push_str(") as ");
            self.w.push_str(rhs_cast);
            self.w.push(')');
            return;
        }
        // **Reference identity `===` / `!==` (§T.1.4).** Address
        // identity, never structural — not overridable:
        //   - `x === null` ≡ the null check (same `.is_none()` shape
        //     as `== null`);
        //   - wrapper-class operands (`Rc<RefCell<Inner>>` newtype) →
        //     `std::rc::Rc::ptr_eq(&l.0, &r.0)` — true iff both
        //     handles share the same cell;
        //   - interface / poly-base dyn handles (`Rc<dyn …>`) →
        //     `Rc::ptr_eq(&l, &r)` (no `.0`). Aliasing always forces
        //     the wrapper representation, so two handles to ONE object
        //     can never meet on the inline path.
        if matches!(b.op, BinaryOp::RefEq | BinaryOp::RefNeq) {
            let is_eq = b.op == BinaryOp::RefEq;
            if matches!(&*b.left, Expr::Literal(juxc_ast::Literal::Null))
                || matches!(&*b.right, Expr::Literal(juxc_ast::Literal::Null))
            {
                let target: &Expr = if matches!(&*b.left, Expr::Literal(juxc_ast::Literal::Null)) {
                    &b.right
                } else {
                    &b.left
                };
                let needs_parens = receiver_needs_parens(target);
                if needs_parens {
                    self.w.push('(');
                }
                self.emit_expr(target);
                if needs_parens {
                    self.w.push(')');
                }
                self.w
                    .push_str(if is_eq { ".is_none()" } else { ".is_some()" });
                return;
            }
            if !is_eq {
                self.w.push('!');
            }
            let left_wrapper = self.receiver_is_wrapper_class(&b.left);
            let right_wrapper = self.receiver_is_wrapper_class(&b.right);
            self.w.push_str("std::rc::Rc::ptr_eq(&");
            self.emit_expr(&b.left);
            if left_wrapper {
                self.w.push_str(".0");
            }
            self.w.push_str(", &");
            self.emit_expr(&b.right);
            if right_wrapper {
                self.w.push_str(".0");
            }
            self.w.push(')');
            return;
        }
        // **Containment `x in xs` (§O.2.4).** Dispatch order:
        //   1. the CONTAINER's user `operator in` → `xs.__op_in(x)`;
        //   2. a map receiver → `.contains_key(&x)`;
        //   3. everything else (arrays/Vec, sets, ranges, String) →
        //      `.contains(&x)` — `&String` implements `Pattern`, so
        //      the string case rides the same shape.
        if b.op == BinaryOp::In {
            // User-defined `operator in` on the right operand's class.
            if let Some(Ty::User { name, .. }) =
                self.expr_types.get(&expr_span_of(&b.right))
            {
                let has_user_in = self
                    .symbols
                    .classes
                    .get(name)
                    .map(|c| c.operators.contains_key(&OperatorKind::In))
                    .unwrap_or(false);
                if has_user_in {
                    let needs_parens = receiver_needs_parens(&b.right);
                    if needs_parens {
                        self.w.push('(');
                    }
                    self.emit_expr(&b.right);
                    if needs_parens {
                        self.w.push(')');
                    }
                    self.w.push_str(".__op_in(");
                    self.emit_expr(&b.left);
                    if self.wrapper_value_needs_clone(&b.left) {
                        self.w.push_str(".clone()");
                    }
                    self.w.push(')');
                    return;
                }
            }
            let is_map = matches!(
                self.expr_types.get(&expr_span_of(&b.right)),
                Some(Ty::User { name, .. })
                    if name.rsplit('.').next().unwrap_or(name).contains("Map"),
            );
            let needs_parens = receiver_needs_parens(&b.right);
            if needs_parens {
                self.w.push('(');
            }
            self.emit_expr(&b.right);
            if needs_parens {
                self.w.push(')');
            }
            // A string LITERAL is already a `&str` — pass it bare
            // (`contains_key("k")` via `Borrow<str>`, `contains("x")`
            // via `Pattern`); any other operand borrows.
            let bare_str = is_string_literal(&b.left);
            self.w.push_str(if is_map { ".contains_key(" } else { ".contains(" });
            if bare_str {
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = true; // keep the literal &str
                self.emit_expr(&b.left);
                self.emitting_format_arg = prev;
            } else {
                self.w.push_str("&(");
                self.emit_expr(&b.left);
                self.w.push(')');
            }
            self.w.push(')');
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
            // Raw pointer vs `null` (§L.6): a `*mut T` has no `is_none`/`is_some`
            // (those are `Option`'s). Use the pointer's own `is_null()` test:
            // `p == null` → `p.is_null()`, `p != null` → `!p.is_null()`. We
            // recognize a raw-pointer target by name (`pointer_locals`) because
            // the lowered `Ty` erases `ptr_depth`. Address-of `&obj` / `&x` is
            // intrinsically a pointer too, and never null.
            let target_is_ptr = self.expr_is_raw_pointer(target);
            let needs_parens = receiver_needs_parens(target);
            if target_is_ptr && !is_eq {
                self.w.push('!');
            }
            if needs_parens {
                self.w.push('(');
            }
            self.emit_expr(target);
            if needs_parens {
                self.w.push(')');
            }
            if target_is_ptr {
                self.w.push_str(".is_null()");
            } else {
                self.w.push_str(if is_eq { ".is_none()" } else { ".is_some()" });
            }
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
        // `<=>` without a user overload (§A.4 level 11): primitives
        // and String go through partial_cmp; Ordering's repr makes
        // the -1/0/+1 mapping a plain cast.
        if matches!(b.op, juxc_ast::BinaryOp::Cmp) {
            self.w.push('(');
            self.emit_expr_with_parent_prec(&b.left, u8::MAX, false);
            self.w.push_str(").partial_cmp(&(");
            let prev = self.emitting_format_arg;
            self.emitting_format_arg = false;
            self.emit_expr(&b.right);
            self.emitting_format_arg = prev;
            self.w.push_str(")).map_or(0, |__o| __o as isize)");
            return;
        }
        // Integer `/` and `%` route through the checked prelude
        // helpers (`__jux_idiv`/`__jux_irem`) so a zero divisor
        // throws a catchable `ArithmeticException("/ by zero")`
        // per ERRATA.md E1 instead of raw-panicking — and a literal
        // `1 / 0` no longer trips rustc's deny-by-default
        // `unconditional_panic` lint on the emitted code. Only fires
        // when BOTH operands are known integers; float and
        // unknown-typed (e.g. generic) operands keep the plain
        // operator, where rustc remains the backstop. Const
        // positions (`static`/`const` initializers) also keep the
        // plain operator — the helper isn't `const fn`-callable,
        // and a zero divisor there is a compile-time error, not a
        // runtime throw.
        if !self.emitting_const_context
            && matches!(b.op, BinaryOp::Div | BinaryOp::Rem)
            && self.operand_is_float(&b.left) == Some(false)
            && self.operand_is_float(&b.right) == Some(false)
        {
            self.w.push_str(if matches!(b.op, BinaryOp::Div) {
                "crate::__jux_idiv("
            } else {
                "crate::__jux_irem("
            });
            let prev = self.emitting_format_arg;
            self.emitting_format_arg = false;
            self.emit_expr(&b.left);
            self.w.push_str(", ");
            self.emit_expr(&b.right);
            self.emitting_format_arg = prev;
            self.w.push(')');
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
        // **Java-style numeric promotion.** In `int <op> double` Java widens the
        // integer operand to the float type; Rust has no implicit numeric
        // coercion, so an un-cast `i * 2.0` is rejected (`isize * f64`). For a
        // mixed integer/float arithmetic op we cast the integer side to `f64`.
        let is_arith = matches!(
            b.op,
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem,
        );
        let (cast_left, cast_right) = if is_arith {
            match (self.operand_is_float(&b.left), self.operand_is_float(&b.right)) {
                (Some(false), Some(true)) => (true, false),
                (Some(true), Some(false)) => (false, true),
                _ => (false, false),
            }
        } else {
            (false, false)
        };

        // Left side of a left-associative op: equal precedence is OK,
        // because emission order already preserves grouping.
        if cast_left {
            self.w.push('(');
        }
        self.emit_expr_with_parent_prec(&b.left, prec, /*right=*/ false);
        if cast_left {
            self.w.push_str(" as f64)");
        }
        self.w.push(' ');
        self.w.push_str(b.op.as_rust_str());
        self.w.push(' ');
        // Right side: equal precedence would *change* grouping
        // (`1 + (2 + 3)` vs `1 + 2 + 3`), so parens are required.
        if cast_right {
            self.w.push('(');
        }
        self.emit_expr_with_parent_prec(&b.right, prec, /*right=*/ true);
        if cast_right {
            self.w.push_str(" as f64)");
        }
        self.emitting_comparison_operand = prev_cmp;
    }

    /// Numeric class of an operand for Java-style promotion: `Some(true)` for a
    /// floating type (`float`/`double`), `Some(false)` for any integer width,
    /// `None` for non-numeric operands or when the type isn't known.
    /// `pub(crate)` because the assignment lowering (`stmts.rs`) also
    /// consults it to decide whether `/=`/`%=` take the checked-division
    /// desugar (ERRATA.md E1).
    pub(crate) fn operand_is_float(&self, e: &Expr) -> Option<bool> {
        use juxc_tycheck::Primitive as P;
        // A numeric literal's shape is authoritative and always available
        // (literals may not carry an `expr_types` entry).
        if let Expr::Literal(lit) = e {
            return match lit {
                juxc_ast::Literal::Float(_) => Some(true),
                juxc_ast::Literal::Int(_) => Some(false),
                _ => None,
            };
        }
        match self.expr_types.get(&expr_span_of(e))? {
            Ty::Primitive(P::Float | P::Double | P::F32 | P::F64) => Some(true),
            Ty::Primitive(P::Bool | P::Char) => None,
            Ty::Primitive(_) => Some(false),
            _ => None,
        }
    }

    /// True when `e`'s recorded type is a user class (or record)
    /// declaring the given operator overload — the dispatch gate for
    /// `obj[i]`, `obj[i] = v`, `obj(args)`, and unary `-obj`
    /// (§O.2.4).
    pub(crate) fn expr_declares_operator(
        &self,
        e: &juxc_ast::Expr,
        kind: OperatorKind,
    ) -> bool {
        // Span-keyed first; bare locals fall back to the name-keyed
        // map (call CALLEES aren't walked by the checker, so their
        // Path spans often have no expr_types entry).
        let ty = self.expr_types.get(&expr_span_of(e)).cloned().or_else(|| {
            if let juxc_ast::Expr::Path(qn) = e {
                if qn.segments.len() == 1 {
                    return self
                        .local_types
                        .iter()
                        .rev()
                        .find_map(|s| s.get(&qn.segments[0].text).cloned());
                }
            }
            None
        });
        let Some(Ty::User { name, .. }) = ty else {
            return false;
        };
        let name = &name;
        if let Some(class) = self.symbols.classes.get(name) {
            if class.operators.get(&kind).is_some_and(|o| !o.is_deleted) {
                return true;
            }
        }
        if let Some(record) = self.symbols.records.get(name) {
            if record.operators.get(&kind).is_some_and(|o| !o.is_deleted) {
                return true;
            }
        }
        false
    }

    /// True when `e`'s static type is a **class** instance (not a record,
    /// not a primitive). Used by `emit_unary` to give `&obj` its §L.6.5
    /// inner-value lowering: a class lowers to `Rc<RefCell<C>>`, so `&obj`
    /// must reach *through* the handle to the value (`obj.as_ptr()`), unlike
    /// a value-typed `&local` which takes the place pointer directly.
    ///
    /// Mirrors `expr_declares_operator`'s type lookup: span-keyed
    /// `expr_types` first, then the name-keyed `local_types` fallback for
    /// bare locals the checker didn't span-annotate. Records are explicitly
    /// excluded (they are value types with no handle).
    pub(crate) fn expr_is_class_instance(&self, e: &juxc_ast::Expr) -> bool {
        let ty = self.expr_types.get(&expr_span_of(e)).cloned().or_else(|| {
            if let juxc_ast::Expr::Path(qn) = e {
                if qn.segments.len() == 1 {
                    return self
                        .local_types
                        .iter()
                        .rev()
                        .find_map(|s| s.get(&qn.segments[0].text).cloned());
                }
            }
            None
        });
        let Some(Ty::User { name, .. }) = ty else {
            return false;
        };
        self.symbols.classes.contains_key(&name)
    }

    /// True when `e` is statically a **raw pointer** (`T*`). The lowered `Ty`
    /// drops `ptr_depth`, so we recover pointer-ness from the names tracked in
    /// `pointer_locals` (raw-pointer locals/params, §L.6) plus the syntactic
    /// forms that are intrinsically pointers: address-of (`&x` / `&obj`) and a
    /// chain of raw-pointer derefs/indexes off a pointer. Drives the
    /// `p == null` peephole to pick the `is_null()` lowering.
    pub(crate) fn expr_is_raw_pointer(&self, e: &juxc_ast::Expr) -> bool {
        match e {
            // `&x` / `&obj` always produce a `*mut T`.
            juxc_ast::Expr::Unary(u) => matches!(u.op, juxc_ast::UnaryOp::AddrOf),
            // A bare name that is a raw pointer: either a local/param (tracked
            // in `pointer_locals`) or, failing that, an implicit-`this`
            // reference to a `T*` FIELD of the enclosing class (`ptr == null`
            // inside a method). A local/param of the same name shadows the field.
            juxc_ast::Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = qn.segments[0].text.as_str();
                if self.pointer_locals.contains(name) {
                    return true;
                }
                let shadowed = self.local_types.iter().any(|s| s.contains_key(name))
                    || self.current_fn_params.contains(name);
                if !shadowed {
                    if let Some(cls) = self.enclosing_class.as_ref() {
                        if let Some(class) = self.symbols.classes.get(cls) {
                            if let Some(field) = class.fields.get(name) {
                                return field.ty.ptr_depth > 0;
                            }
                        }
                    }
                }
                false
            }
            // A field declared `T*` (`this.ptr`, `obj.ptr`) — resolve the
            // receiver's class and read the field's declared `ptr_depth` (the
            // erased `Ty` drops it). Lets `ptr == null` lower to `is_null()`.
            juxc_ast::Expr::Field(f) => {
                if let Some(juxc_tycheck::Ty::User { name, .. }) =
                    self.expr_types.get(&crate::exprs::expr_span_of(&f.object))
                {
                    if let Some(class) = self.symbols.classes.get(name) {
                        if let Some(field) = class.fields.get(&f.field.text) {
                            return field.ty.ptr_depth > 0;
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// If `b`'s LHS is a known user class that defines the matching
    /// operator overload, return the synthetic inherent method name
    /// (`__op_add`, `__op_sub`, …) we should dispatch through. Returns
    /// `None` for primitives, unknown types, comparison/logical ops
    /// (which don't consume operands), and class types that don't
    /// declare the relevant operator.
    fn class_op_method_for_binary(&self, b: &BinaryExpr) -> Option<&'static str> {
        let kind = match b.op {
            // `<=>` on a class with `operator<=>` → `__op_cmp`.
            BinaryOp::Cmp => OperatorKind::Cmp,
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
            // Wrap nullable operands in `JuxOpt(&v)` so
            // `"prefix " + maybeName + " suffix"` prints "null" for
            // None rather than failing the `Display` bound.
            self.emit_format_arg(op);
        }
        self.emitting_format_arg = prev;
        self.w.push(')');
    }
}
