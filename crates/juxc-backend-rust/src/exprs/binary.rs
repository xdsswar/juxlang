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
fn match_null_comparison(b: &BinaryExpr) -> Option<(&Expr, bool)> {
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
/// The overloadable operator a binary operator dispatches to, if any.
pub(crate) fn binary_operator_kind(op: BinaryOp) -> Option<OperatorKind> {
    Some(match op {
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
        _ => return None,
    })
}

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

/// Whether emitted Rust ends in an `as <Type>` cast at its top level, the one
/// shape `<` or `<<` cannot follow without parentheses.
fn ends_with_cast(text: &str) -> bool {
    let text = text.trim_end();
    let Some(at) = text.rfind(" as ") else { return false };
    let ty = &text[at + 4..];
    !ty.is_empty() && ty.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

impl RustEmitter {
    /// Whether `b` is one of the shapes [`Self::emit_pointer_arithmetic`]
    /// lowers. Each lowers to a method call (or a parenthesized cast), which
    /// binds tighter than any operator, so it never needs parens of its own.
    pub(crate) fn is_lowered_pointer_arithmetic(&self, b: &BinaryExpr) -> bool {
        match b.op {
            BinaryOp::Add => self.expr_is_raw_pointer(&b.left) != self.expr_is_raw_pointer(&b.right),
            BinaryOp::Sub => self.expr_is_raw_pointer(&b.left),
            _ => false,
        }
    }

    /// Lower `p + n`, `n + p`, `p - n` and `q - p` over raw pointers, returning
    /// whether `b` was one of them.
    ///
    /// The integer operand is cast to `isize`, the offset type, so any integer
    /// width works as an index. `offset` rather than `add` keeps a negative
    /// step (`p - 1`, `p + delta`) meaningful, as it is in C.
    fn emit_pointer_arithmetic(&mut self, b: &BinaryExpr) -> bool {
        let left_ptr = self.expr_is_raw_pointer(&b.left);
        let right_ptr = self.expr_is_raw_pointer(&b.right);
        let (base, step, negate) = match (b.op, left_ptr, right_ptr) {
            (BinaryOp::Add, true, false) => (&b.left, &b.right, false),
            (BinaryOp::Add, false, true) => (&b.right, &b.left, false),
            (BinaryOp::Sub, true, false) => (&b.left, &b.right, true),
            (BinaryOp::Sub, true, true) => {
                // Pointer difference: a count of pointee-sized steps, typed
                // `long` by §L.6.2. `offset_from` returns `isize`, so the cast
                // makes the Rust value the `i64` every slot expects. The parens
                // keep `as i64` from reading as a generic when a `<` follows.
                self.w.push('(');
                self.emit_pointer_receiver(&b.left);
                self.w.push_str(".offset_from(");
                self.emit_expr(&b.right);
                self.w.push_str(") as i64)");
                return true;
            }
            _ => return false,
        };
        self.emit_pointer_receiver(base);
        self.w.push_str(".offset(");
        if negate {
            self.w.push('-');
        }
        self.emit_pointer_step(step);
        self.w.push(')');
        true
    }

    /// A pointer in method-receiver position, parenthesized when composite.
    pub(crate) fn emit_pointer_receiver(&mut self, e: &Expr) {
        if receiver_needs_parens(e) {
            self.w.push('(');
            self.emit_expr(e);
            self.w.push(')');
        } else {
            self.emit_expr(e);
        }
    }

    /// An integer used as a pointer offset: `isize`, which is what `offset`
    /// takes. A plain literal needs no cast; anything else is cast whatever
    /// its width, so `p[i]` works for an `int`, a `uint` or a `byte` alike.
    pub(crate) fn emit_pointer_step(&mut self, step: &Expr) {
        let prev = std::mem::take(&mut self.emitting_format_arg);
        // An integer literal, negated or not, already takes `isize` from the
        // parameter: `p.offset(-1)`, not `p.offset((-1) as isize)`.
        let literal = match step {
            Expr::Unary(u) if u.op == juxc_ast::UnaryOp::Neg => u.operand.as_ref(),
            other => other,
        };
        if matches!(literal, Expr::Literal(juxc_ast::Literal::Int(_))) {
            self.emit_expr(step);
        } else {
            self.w.push('(');
            self.emit_expr(step);
            self.w.push_str(") as isize");
        }
        self.emitting_format_arg = prev;
    }

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
        // A bare interface constant read from a `default` method body. It
        // carries no `expr_types` entry, so `PREFIX + this.name()` used to
        // miss the concat trigger and fall to the numeric `+`, emitting the
        // invalid Rust `String + String`.
        if let Expr::Path(qn) = e {
            if qn.segments.len() == 1 {
                let named = self.enclosing_interface_const_type(&qn.segments[0].text);
                if named.as_ref().is_some_and(type_ref_is_string) {
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
        // A char LITERAL carries its type in the token, and tycheck does not
        // always record a span for it. Without this `ch - '0'` saw one typed
        // operand and one untyped one, skipped promotion entirely, and asked
        // Rust to subtract a `char` from a `char`.
        if let Expr::Literal(juxc_ast::Literal::Char(_)) = e {
            return Some(juxc_tycheck::Primitive::Char);
        }
        // A pointer difference is a `long` (§L.6.2), and its lowering already
        // carries the `as i64`. A parenthesized `(q - p)` has no recorded type
        // under its own span, so without this it read as untyped and took a
        // second, redundant cast.
        if let Expr::Binary(b) = e {
            if b.op == BinaryOp::Sub && self.expr_is_raw_pointer(&b.left) && self.expr_is_raw_pointer(&b.right) {
                return Some(juxc_tycheck::Primitive::Long);
            }
        }
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
        // An arithmetic node with no recorded type of its own has the type its
        // operands promote to, which is how the checker computed it. Without
        // this `'a' + (c - 'a' + shift) % 26` saw a typed `char` on the left
        // and an untyped right, skipped promotion, and asked Rust to add an
        // `isize` to a `char`.
        if let Expr::Binary(b) = e {
            if matches!(
                b.op,
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem
            ) {
                use juxc_tycheck::ty::{promote_numeric, NumericPromotion};
                let as_arith = |p: juxc_tycheck::Primitive| {
                    if p == juxc_tycheck::Primitive::Char { juxc_tycheck::Primitive::Int } else { p }
                };
                let untyped = juxc_tycheck::infer::untyped_int_literal;
                match (self.operand_primitive(&b.left), self.operand_primitive(&b.right)) {
                    (Some(l), Some(r)) => {
                        if let NumericPromotion::To(p) = promote_numeric(l, r) {
                            return Some(as_arith(p));
                        }
                    }
                    (Some(l), None) if untyped(&b.right) => return Some(as_arith(l)),
                    (None, Some(r)) if untyped(&b.left) => return Some(as_arith(r)),
                    _ => {}
                }
            }
        }
        // A bare name inside a method can be an implicit `this.field`, and a
        // field read carries no `expr_types` entry of its own. Without this
        // the operand looked untyped, promotion was skipped for the whole
        // expression, and a mixed-signedness pair reached rustc unchanged:
        // `s.len() - pos` emitted `usize - isize` and failed to compile, while
        // the same expression with a LOCAL on the right worked. Walk the
        // enclosing class's `extends` chain, as an inherited field is bare too.
        if let Expr::Path(qn) = e {
            if qn.segments.len() == 1 {
                let bare = qn.segments[0].text.as_str();
                let mut cursor = self.enclosing_class.clone();
                while let Some(class_name) = cursor {
                    let Some(sig) = self.lookup_class_by_bare_or_fqn(&class_name) else {
                        break;
                    };
                    if let Some(field) = sig.fields.get(bare) {
                        let ty = field.ty.clone();
                        return self.type_ref_primitive(&ty);
                    }
                    cursor = sig
                        .extends
                        .as_ref()
                        .and_then(|t| t.name.segments.last())
                        .map(|s| s.text.clone());
                }
            }
        }
        literal_numeric_ty(e)
    }

    pub(crate) fn emit_binary(&mut self, b: &BinaryExpr) {
        // `k * v` resolved to a free-function operator (LANG-V1 §7.14): emit
        // the call `__op_mul(k, v)` through the ordinary call path, which
        // handles overload suffixes, argument conversion and handle sharing.
        if let Some((name, _)) = self.symbols.free_operator_calls.get(&b.span).cloned() {
            let call = juxc_ast::CallExpr {
                callee: Box::new(Expr::Path(juxc_ast::QualifiedName {
                    segments: vec![juxc_ast::Ident { text: name, span: b.span }],
                    span: b.span,
                })),
                args: vec![(*b.left).clone(), (*b.right).clone()],
                arg_names: vec![None, None],
                explicit_generic_args: Vec::new(),
                eval_order: Vec::new(),
                span: b.span,
            };
            self.emit_call(&call);
            return;
        }
        // Armed by `numeric_widen_or_arm` for exactly this expression, and
        // taken here so it cannot reach any other.
        let signed_slot = self.signed_slot_target.take();
        // **Pointer arithmetic (§L.6.2).** Rust has no `+` / `-` on raw
        // pointers, so `p + n` was a rustc error. Offsets step by the pointee's
        // size, exactly as in C: `p + n` is `p.offset(n)`, `p - n` is
        // `p.offset(-n)`, and `q - p` counts the elements between them.
        if matches!(b.op, BinaryOp::Add | BinaryOp::Sub) && self.emit_pointer_arithmetic(b) {
            return;
        }
        // **`==` on objects without their own equality is identity** (§O.2.6).
        // Two handles of one class meet the class's identity `PartialEq`; a
        // base-typed or interface-typed handle, or two different classes, go
        // through the object address, since those handles are different Rust
        // types.
        if matches!(b.op, BinaryOp::Eq | BinaryOp::NotEq)
            && !matches!(&*b.left, Expr::Literal(juxc_ast::Literal::Null))
            && !matches!(&*b.right, Expr::Literal(juxc_ast::Literal::Null))
        {
            if let (Some(l), Some(r)) = (self.identity_operand(&b.left), self.identity_operand(&b.right)) {
                if !l.declares_equality && !r.declares_equality && (l.is_dyn || r.is_dyn || l.name != r.name) {
                    self.emit_identity_compare(&b.left, &b.right, b.op == BinaryOp::Eq);
                    return;
                }
                // A base-typed operand with a user `operator==` calls the
                // operator through its `Kind` slot, which runs the runtime
                // class's override (§O.2.9). Rust's `==` on two `Rc<dyn …>`
                // would move its right operand, and has no impl at all between
                // a handle and a concrete class.
                if l.declares_equality && (l.is_dyn || r.is_dyn) {
                    if let Some(param) = self.class_operator_param(&l.name, OperatorKind::Eq) {
                        if b.op == BinaryOp::NotEq {
                            self.w.push('!');
                        }
                        self.emit_expr_with_parent_prec(&b.left, u8::MAX, false);
                        self.w.push_str(".__op_eq(");
                        self.emit_operator_argument(&b.right, &r, &param);
                        self.w.push(')');
                        return;
                    }
                }
            }
        }
        // **A null test narrows the other side of `&&` / `||`** (§7.10).
        // `it != null && it.qty() < 5` evaluates the right only when `it` is
        // there, so the right reads the value, not the `Option`.
        if matches!(b.op, BinaryOp::And | BinaryOp::Or) {
            let proven = self.null_narrowed_locals(&b.left, b.op == BinaryOp::And);
            if !proven.is_empty() {
                let prec = binary_prec(b.op);
                self.emit_expr_with_parent_prec(&b.left, prec, false);
                self.w.push_str(if b.op == BinaryOp::And { " && " } else { " || " });
                let depth = self.expr_narrowed.len();
                self.expr_narrowed.extend(proven);
                self.emit_expr_with_parent_prec(&b.right, prec, true);
                self.expr_narrowed.truncate(depth);
                return;
            }
        }
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
            // `===` on an `any` (§T.1.2): the held value decides, identity
            // for an object and equality for a value.
            if self.expr_is_any(&b.left) || self.expr_is_any(&b.right) {
                if !is_eq {
                    self.w.push('!');
                }
                let any_ty = crate::analysis::synth_iface_type_ref("any", b.span);
                // The `any` side is the receiver; the other side, if it is not
                // one already, goes into an `any` to be compared.
                let (recv, other) =
                    if self.expr_is_any(&b.left) { (&b.left, &b.right) } else { (&b.right, &b.left) };
                self.emit_expr(recv);
                self.w.push_str(".same(&");
                if self.expr_is_any(other) {
                    self.emit_expr(other);
                } else {
                    self.emit_expr_into_any(&any_ty, other);
                }
                self.w.push(')');
                return;
            }
            // **`===` on a VALUE type is `==` (§7.14.3).** "For value types
            // (struct, record, primitive) it is identical to `==`" -- a
            // `String` or an `int` has no address to compare, and emitting
            // `Rc::ptr_eq` on one reached rustc as a type error the Jux source
            // could not explain. Only positively-identified value types take
            // this path, so anything the backend is unsure about keeps the
            // identity shape it had.
            if self.refeq_operand_is_value(&b.left) || self.refeq_operand_is_value(&b.right) {
                self.emit_expr(&b.left);
                self.w.push_str(if is_eq { " == " } else { " != " });
                self.emit_expr(&b.right);
                return;
            }
            // A `dyn` handle (base-typed or interface-typed) boxes a clone of
            // the class handle, so it shares no `Rc` with any other view of the
            // object; ask both sides for the object's address instead.
            if let (Some(l), Some(r)) = (self.identity_operand(&b.left), self.identity_operand(&b.right)) {
                if l.is_dyn || r.is_dyn || l.name != r.name {
                    self.emit_identity_compare(&b.left, &b.right, is_eq);
                    return;
                }
            }
            if !is_eq {
                self.w.push('!');
            }
            // The `Box` rep is a unique owner whose `.0` is a `Box`, not an `Rc`,
            // so identity compares the boxed addresses with `std::ptr::eq`
            // instead of `Rc::ptr_eq`.
            if self.receiver_is_box_class(&b.left) || self.receiver_is_box_class(&b.right) {
                self.w.push_str("std::ptr::eq(&*");
                self.emit_expr(&b.left);
                self.w.push_str(".0, &*");
                self.emit_expr(&b.right);
                self.w.push_str(".0)");
                return;
            }
            let left_wrapper = self.receiver_is_wrapper_class(&b.left);
            let right_wrapper = self.receiver_is_wrapper_class(&b.right);
            // A worker-shared class carries the atomic handle, whose identity
            // check is an inherent method rather than `Rc`'s associated one.
            if self.receiver_is_sync_class(&b.left) || self.receiver_is_sync_class(&b.right) {
                self.emit_expr(&b.left);
                if left_wrapper {
                    self.w.push_str(".0");
                }
                self.w.push_str(".ptr_eq(&");
                self.emit_expr(&b.right);
                if right_wrapper {
                    self.w.push_str(".0");
                }
                self.w.push(')');
                return;
            }
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
            let method = if is_map { "contains_key" } else { "contains" };
            // A collection or array is a shared handle (§6.5.1/§6.5.2); the
            // lookup method lives on what it holds, so borrow first.
            if let Some(borrow) = self.collection_handle_borrow(&b.right, method) {
                self.w.push_str(borrow);
            } else if matches!(self.receiver_ty_of(&b.right), Some(Ty::Array { .. })) && self.expr_is_collection_handle(&b.right) {
                self.w.push_str(".borrow()");
            }
            self.w.push('.');
            self.w.push_str(method);
            self.w.push('(');
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
            // Non-nullable generic param compared to `null`: a bare `T` value
            // (NOT `T?`) is never `Option`-shaped, so `val.is_none()` would be
            // E0599 (no such method on a type parameter). Such a comparison is
            // statically constant — `== null` is always false, `!= null` always
            // true. Emit the constant, but still evaluate the target for any
            // side effects (`&(target)` borrows without moving a non-Copy `T`).
            // Scoped to `Ty::Param`: a `T?` param is recorded as
            // `Ty::Nullable(Param)` and correctly keeps the `.is_none()` path.
            // Resolve the target's type as a bare generic `Ty::Param` via the
            // span-keyed `expr_types`, falling back to the name-keyed
            // `local_types` — a generic PARAM's use site is often `Unknown` in
            // `expr_types` but carries its `Ty::Param` in `local_types` (same
            // dual-lookup the wrapper-clone predicate uses).
            let target_is_nonnull_generic = matches!(
                self.expr_types.get(&crate::exprs::expr_span_of(target)),
                Some(juxc_tycheck::Ty::Param(_))
            ) || matches!(
                target,
                Expr::Path(qn) if qn.segments.len() == 1
                    && matches!(
                        self.local_types
                            .iter()
                            .rev()
                            .find_map(|s| s.get(qn.segments[0].text.as_str())),
                        Some(juxc_tycheck::Ty::Param(_))
                    )
            );
            if target_is_nonnull_generic {
                let lit = if is_eq { "false" } else { "true" };
                if matches!(
                    target,
                    Expr::Path(_) | Expr::This(_) | Expr::Super(_)
                ) {
                    // Side-effect-free place — emit the bare constant.
                    self.w.push_str(lit);
                } else {
                    self.w.push_str("{ let _ = &(");
                    self.emit_expr(target);
                    self.w.push_str("); ");
                    self.w.push_str(lit);
                    self.w.push_str(" }");
                }
                return;
            }
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
        if matches!(b.op, juxc_ast::BinaryOp::Cmp)
            && (self.operand_is_float(&b.left) == Some(true) || self.operand_is_float(&b.right) == Some(true))
        {
            // On floats `<=>` is IEEE total order (§S.2.3): `-0.0 < +0.0`, and
            // NaN sorts after `+Infinity`. `partial_cmp` answered `0` for both.
            // Both sides are compared as `f64`, which keeps the order of every
            // `float` and integer operand; `jux_fcmp` makes every NaN one value.
            let rust_name_of = |e: &Expr| self.operand_primitive(e).map(crate::exprs::rust_primitive_name);
            let (cast_l, cast_r) = (rust_name_of(&b.left) != Some("f64"), rust_name_of(&b.right) != Some("f64"));
            let prev = self.emitting_format_arg;
            self.emitting_format_arg = false;
            self.w.push_str("crate::jux_fcmp(");
            self.emit_expr_with_parent_prec(&b.left, if cast_l { u8::MAX } else { 0 }, false);
            if cast_l {
                self.w.push_str(" as f64");
            }
            self.w.push_str(", ");
            self.emit_expr_with_parent_prec(&b.right, if cast_r { u8::MAX } else { 0 }, false);
            if cast_r {
                self.w.push_str(" as f64");
            }
            self.w.push(')');
            self.emitting_format_arg = prev;
            return;
        }
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
        // **Java-style numeric promotion.** Rust has no implicit numeric
        // coercion, so a mixed-type op (`isize * f64`, `isize + i64`,
        // `isize < usize`) is a hard rustc error. For an arithmetic, bitwise, or
        // comparison op whose operands differ in numeric type we widen both
        // sides to a common type (see `numeric_promote_target`) by emitting an
        // `as <T>` on whichever side(s) differ. Same-type and non-numeric
        // operands are emitted untouched.
        let is_arith = matches!(
            b.op,
            BinaryOp::Add
                | BinaryOp::Sub
                | BinaryOp::Mul
                | BinaryOp::Div
                | BinaryOp::Rem
                | BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor,
        );
        let promote = if is_arith || is_cmp {
            self.numeric_promote_target(&b.left, &b.right, is_arith)
        } else {
            None
        };
        // **A signed/unsigned comparison compares the values** (§S.2.6): when
        // neither type holds the other's range (`isize < usize`), both sides
        // go to `i128`, which holds every 64-bit integer of either sign. Casting
        // to the unsigned side, as promotion used to, made `-1 < 5u` false.
        let wide_compare = is_cmp && self.comparison_needs_i128(&b.left, &b.right);
        let target_name = if let Some(slot) = signed_slot {
            Some(crate::exprs::rust_primitive_name(slot))
        } else if wide_compare {
            Some("i128")
        } else {
            promote.map(crate::exprs::rust_primitive_name)
        };
        // **Shifts never overflow** (§S.2.5): the count is taken modulo the
        // width of the left operand, in a debug build as in a release one. A
        // literal count inside the width is an ordinary `<<`; any other count
        // goes through `wrapping_shl`/`wrapping_shr`, which mask it the same
        // way, where a bare `<<` panicked in debug and masked in release.
        // An unsuffixed literal on the left (`1 << n`) takes its type from the
        // context, so it has no width to mask by here and keeps the operator.
        if matches!(b.op, BinaryOp::Shl | BinaryOp::Shr) && !juxc_tycheck::infer::untyped_int_literal(&b.left) {
            if let Some(width) = self.operand_primitive(&b.left).and_then(juxc_tycheck::ty::integer_bits) {
                if !shift_count_is_in_width(&b.right, width) {
                    self.emitting_comparison_operand = prev_cmp;
                    let prev = self.emitting_format_arg;
                    self.emitting_format_arg = false;
                    self.emit_expr_with_parent_prec(&b.left, u8::MAX, false);
                    self.w.push_str(if b.op == BinaryOp::Shl { ".wrapping_shl(" } else { ".wrapping_shr(" });
                    // `as` binds tighter than a unary minus in Rust: `-1 as u32`
                    // is `-(1 as u32)`, which does not compile. A negative or
                    // composite count is parenthesized first.
                    // An untyped literal count (`-1`) is typed `i64` first:
                    // cast straight to `u32` it would be inferred unsigned and
                    // could not be negative.
                    let count_parens = matches!(&*b.right, Expr::Unary(_) | Expr::Binary(_) | Expr::Ternary(_) | Expr::Cast(_));
                    let untyped_count = juxc_tycheck::infer::untyped_int_literal(&b.right)
                        || matches!(&*b.right, Expr::Unary(u) if juxc_tycheck::infer::untyped_int_literal(&u.operand));
                    if count_parens || untyped_count {
                        self.w.push('(');
                    }
                    self.emit_expr_with_parent_prec(&b.right, u8::MAX, false);
                    if untyped_count {
                        self.w.push_str(" as i64");
                    }
                    if count_parens || untyped_count {
                        self.w.push(')');
                    }
                    self.w.push_str(" as u32)");
                    self.emitting_format_arg = prev;
                    return;
                }
            }
        }
        // Compared by the Rust spelling, as `numeric_widen_to` does: `long` and
        // `i64` are distinct Jux primitives but one Rust type, and a cast
        // between them (`(x as i64) * 2i64`) is noise.
        let rust_name_of = |e: &Expr| self.operand_primitive(e).map(crate::exprs::rust_primitive_name);
        // In a signed slot (`int last = xs.len() - 1;`) every `uint` leaf is
        // converted to the slot's type and nested arithmetic is emitted the
        // same way, so the subtraction happens signed; a literal needs nothing.
        let slot_leaf = |e: &Expr| {
            signed_slot.is_some() && !Self::is_signed_slot_arith_node(e) && !juxc_tycheck::infer::untyped_int_literal(e)
        };
        let (cast_left, cast_right) = if signed_slot.is_some() {
            (slot_leaf(&b.left), slot_leaf(&b.right))
        } else {
            (
                target_name.is_some() && rust_name_of(&b.left) != target_name,
                target_name.is_some() && rust_name_of(&b.right) != target_name,
            )
        };
        // Inside an enum method `self` is `&Self`; comparing it to a variant
        // value (`this == Op.Add`) is `&Op == Op`, which the derived `PartialEq`
        // does not cover (rustc E0277). Deref the `this`/`super` operand so both
        // sides are the enum value (`(*self) == Op::Add` — `==` re-borrows, so it
        // is safe even for payload-carrying variants). Enums only ever take
        // `==`/`!=` here, so gating on the operand shape is enough.
        let deref_left =
            self.in_enum_method && matches!(b.left.as_ref(), Expr::This(_) | Expr::Super(_));
        let deref_right =
            self.in_enum_method && matches!(b.right.as_ref(), Expr::This(_) | Expr::Super(_));

        // **A cast on the left of a shift needs parentheses.** Rust reads
        // `r as u32 << 16` as the start of generic arguments for `u32`, not as
        // a shift, and rejects it outright -- so the ordinary `(u32) r << 16`
        // that any pixel-packing routine is written with did not compile. The
        // precedence is not in question; the syntax is.
        let cast_operand_needs_parens = matches!(b.op, BinaryOp::Shl | BinaryOp::Shr)
            && matches!(b.left.as_ref(), Expr::Cast(_));
        // Left side of a left-associative op: equal precedence is OK,
        // because emission order already preserves grouping.
        if cast_left || cast_operand_needs_parens {
            self.w.push('(');
        }
        if deref_left {
            self.w.push_str("(*");
        }
        let left_mark = self.w.mark();
        // A cast binds tighter than any binary operator, so the operand it
        // applies to must be a single unit: `a * b * 0.5` promotes `a * b`, and
        // emitting it at the operator's own precedence wrote `(a * b as f64)`,
        // which Rust reads as `a * (b as f64)`.
        let left_prec = if cast_left { u8::MAX } else { prec };
        if signed_slot.is_some() && Self::is_signed_slot_arith_node(&b.left) {
            self.signed_slot_target = signed_slot;
        }
        self.emit_expr_with_parent_prec(&b.left, left_prec, /*right=*/ false);
        self.signed_slot_target = None;
        if deref_left {
            self.w.push(')');
        }
        // **Any left operand that ENDS in a cast needs parentheses before `<`
        // or `<<`**, not only a written one: `xs.length` emits
        // `xs.borrow().len() as isize` and `s.length()` emits a `count() as
        // isize`, and Rust reads `… as isize < w` as the start of `isize<…>`.
        // `if (xs.length < w)` is ordinary code, and it did not compile.
        if !cast_left
            && !cast_operand_needs_parens
            && matches!(b.op, BinaryOp::Lt | BinaryOp::Shl)
            && ends_with_cast(self.w.text_from(left_mark))
        {
            self.w.insert_at(left_mark, "(");
            self.w.push(')');
        }
        if cast_left {
            self.w.push_str(" as ");
            self.w.push_str(target_name.unwrap());
            self.w.push(')');
        } else if cast_operand_needs_parens {
            self.w.push(')');
        }
        self.w.push(' ');
        self.w.push_str(b.op.as_rust_str());
        self.w.push(' ');
        // Right side: equal precedence would *change* grouping
        // (`1 + (2 + 3)` vs `1 + 2 + 3`), so parens are required.
        if cast_right {
            self.w.push('(');
        }
        if deref_right {
            self.w.push_str("(*");
        }
        let right_prec = if cast_right { u8::MAX } else { prec };
        if signed_slot.is_some() && Self::is_signed_slot_arith_node(&b.right) {
            self.signed_slot_target = signed_slot;
        }
        self.emit_expr_with_parent_prec(&b.right, right_prec, /*right=*/ true);
        self.signed_slot_target = None;
        if deref_right {
            self.w.push(')');
        }
        if cast_right {
            self.w.push_str(" as ");
            self.w.push_str(target_name.unwrap());
            self.w.push(')');
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

    /// The common numeric type two operands of a binary op are cast to, or
    /// `None` when no cast is needed (same type, a non-numeric operand, an
    /// untyped literal, or an unknown type). Rust has no implicit numeric
    /// coercion, so `isize + i64` or `isize * f64` does not compile; both sides
    /// go to the type §S.2.6 names, which `juxc_tycheck::ty::promote_numeric`
    /// computes for the checker and the backend alike.
    ///
    /// Used for arithmetic, bitwise, and comparison ops. A signed/unsigned pair
    /// with no common type has already been reported (`E0410`) in an arithmetic
    /// op, and a comparison of one goes to `i128` (`comparison_needs_i128`);
    /// neither is promoted here.
    pub(crate) fn numeric_promote_target(
        &self,
        left: &Expr,
        right: &Expr,
        is_arith: bool,
    ) -> Option<juxc_tycheck::Primitive> {
        use juxc_tycheck::ty::{integer_bits, is_float_primitive, promote_numeric, same_representation, NumericPromotion};
        use juxc_tycheck::Primitive as P;
        let lp = self.operand_primitive(left)?;
        let rp = self.operand_primitive(right)?;
        // **An untyped literal takes the other operand's type** (§S.2.6): in
        // `i32 y = x + 1` the `1` is an `i32`, and Rust infers exactly that
        // for an unsuffixed literal, so no cast is written on either side.
        // Promoting as if the literal were an `int` cast `x` up to `isize`
        // and then failed to store the result back in an `i32`.
        let int_like = |p: P| integer_bits(p).is_some();
        if (juxc_tycheck::infer::untyped_int_literal(left) && int_like(rp))
            || (juxc_tycheck::infer::untyped_int_literal(right) && int_like(lp))
            || (juxc_tycheck::infer::untyped_float_literal(left) && is_float_primitive(rp))
            || (juxc_tycheck::infer::untyped_float_literal(right) && is_float_primitive(lp))
        {
            return None;
        }
        // A `char` operand is an `int` in an arithmetic or bitwise op (rule 5).
        // Two chars COMPARED stay chars, because Rust orders them identically
        // and a cast would only add noise; a char compared against a number
        // still promotes, since the two are not comparable otherwise.
        if !is_arith && lp == P::Char && rp == P::Char {
            return None;
        }
        match promote_numeric(lp, rp) {
            // Two chars in an arithmetic op agree only AFTER promotion, so they
            // still both need the cast Rust has no implicit form of.
            NumericPromotion::To(_) if same_representation(lp, rp) && lp != P::Char => None,
            NumericPromotion::To(p) => Some(p),
            NumericPromotion::NoCommonType | NumericPromotion::NotNumeric => None,
        }
    }

    /// The cast a value needs to fit a numeric slot of `target` (see
    /// [`Self::numeric_widen_to`]), with two cases written in place instead of
    /// cast (§S.2.7), each armed for the expression about to be emitted:
    ///
    /// - a `uint` expression in a signed integer slot is computed in the slot's
    ///   type: `int last = xs.len() - 1;` gives `-1` for an empty collection
    ///   instead of an unsigned underflow;
    /// - an unsuffixed integer literal in a float slot is written as a float
    ///   literal: `d += 1` is `d += 1.0`.
    pub(crate) fn numeric_widen_or_arm(
        &mut self,
        value: &Expr,
        target: juxc_tycheck::Primitive,
    ) -> Option<&'static str> {
        if self.is_signed_slot_arith(value, target) {
            self.signed_slot_target = Some(target);
            return None;
        }
        if juxc_tycheck::ty::is_float_primitive(target) && juxc_tycheck::infer::untyped_int_literal(value) {
            self.int_literal_as_float = true;
            return None;
        }
        self.numeric_widen_to(value, target)
    }

    /// A `+`, `-` or `*` node, the operators a signed slot computes in its own
    /// type. Division keeps its checked helper and is left as it is.
    fn is_signed_slot_arith_node(e: &Expr) -> bool {
        matches!(e, Expr::Binary(b) if matches!(b.op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul))
    }

    /// Whether `value` is arithmetic over `uint` values and untyped literals
    /// only, with at least one `uint`, flowing into the signed integer slot
    /// `target`.
    fn is_signed_slot_arith(&self, value: &Expr, target: juxc_tycheck::Primitive) -> bool {
        use juxc_tycheck::Primitive as P;
        if !matches!(target, P::Int | P::Long | P::I64) || !Self::is_signed_slot_arith_node(value) {
            return false;
        }
        fn leaves<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
            match e {
                Expr::Binary(b) if matches!(b.op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul) => {
                    leaves(&b.left, out);
                    leaves(&b.right, out);
                }
                other => out.push(other),
            }
        }
        let mut found = Vec::new();
        leaves(value, &mut found);
        let mut any_uint = false;
        for leaf in found {
            if juxc_tycheck::infer::untyped_int_literal(leaf) {
                continue;
            }
            match self.operand_primitive(leaf) {
                Some(P::Uint) => any_uint = true,
                _ => return false,
            }
        }
        any_uint
    }

    /// Whether comparing `left` with `right` needs both sides widened to
    /// `i128`: one is a signed and the other an unsigned integer, and neither
    /// type holds the other's values (§S.2.6, "comparisons are exact").
    pub(crate) fn comparison_needs_i128(&self, left: &Expr, right: &Expr) -> bool {
        // A NEGATIVE literal against an unsigned operand (`-1 < 5u`) cannot
        // adapt to the unsigned type, so it compares exactly too. The literal
        // has no primitive of its own, so this is decided before the lookup.
        let negative_literal = |e: &Expr| {
            matches!(e, Expr::Unary(u) if u.op == juxc_ast::UnaryOp::Neg && juxc_tycheck::infer::untyped_int_literal(&u.operand))
        };
        let unsigned = |e: &Expr| {
            use juxc_tycheck::Primitive as P;
            // A suffixed literal (`5u`) carries its type in the token.
            let prim = self.operand_primitive(e).or_else(|| match e {
                Expr::Literal(juxc_ast::Literal::Int(lit)) if lit.kind.is_some() => literal_numeric_ty(e),
                _ => None,
            });
            matches!(prim, Some(P::Ubyte | P::Ushort | P::Uint | P::Ulong | P::U8 | P::U16 | P::U32 | P::U64))
        };
        if (negative_literal(left) && unsigned(right)) || (negative_literal(right) && unsigned(left)) {
            return true;
        }
        let (Some(l), Some(r)) = (self.operand_primitive(left), self.operand_primitive(right)) else {
            return false;
        };
        if juxc_tycheck::infer::untyped_int_literal(left) || juxc_tycheck::infer::untyped_int_literal(right) {
            return false;
        }
        juxc_tycheck::ty::promote_numeric(l, r) == juxc_tycheck::NumericPromotion::NoCommonType
    }

    /// The primitive of the enclosing function's declared return type, when it
    /// is a plain numeric primitive (no array / generics / nullable / pointer).
    /// Used to widen a narrower numeric `return` value to the declared type.
    /// The numeric primitive inside a nullable return type (`int` of `int?`),
    /// else `None`. The value a `return` wraps in `Some(...)` converts to it.
    pub(crate) fn nullable_return_inner_primitive(&self) -> Option<juxc_tycheck::Primitive> {
        let t = match self.current_return_type.as_ref()? {
            juxc_ast::ReturnType::Type(t) | juxc_ast::ReturnType::AsyncType(t) => t,
            _ => return None,
        };
        if !t.nullable {
            return None;
        }
        let mut inner = t.clone();
        inner.nullable = false;
        self.type_ref_primitive(&inner)
    }

    pub(crate) fn return_type_primitive(&self) -> Option<juxc_tycheck::Primitive> {
        let t = match self.current_return_type.as_ref()? {
            juxc_ast::ReturnType::Type(t) | juxc_ast::ReturnType::AsyncType(t) => t,
            _ => return None,
        };
        self.type_ref_primitive(t)
    }

    /// The numeric primitive a plain (non-array / non-generic / non-nullable /
    /// non-pointer, single-segment) `TypeRef` names, else `None`. The target
    /// type for a numeric coercion at a declaration slot (local init, parameter,
    /// field) — see [`Self::numeric_widen_to`].
    pub(crate) fn type_ref_primitive(
        &self,
        t: &juxc_ast::TypeRef,
    ) -> Option<juxc_tycheck::Primitive> {
        use juxc_tycheck::Primitive as P;
        if t.array_shape.is_some()
            || !t.generic_args.is_empty()
            || t.nullable
            || t.ptr_depth > 0
            || t.name.segments.len() != 1
        {
            return None;
        }
        // A type alias names whatever it stands for: skia's `scalar` is an
        // `f32`, and a `scalar` parameter takes the conversions a `float` one
        // does. Followed a few steps, never forever.
        let bare = t.name.segments[0].text.as_str();
        if let Some(target) = self
            .resolve_bare_type_fqn(bare)
            .and_then(|fqn| self.symbols.aliases.get(&fqn))
            .map(|a| a.target.clone())
        {
            if target.name.segments.last().map(|s| s.text.as_str()) != Some(bare) {
                return self.type_ref_primitive(&target);
            }
        }
        match bare {
            "byte" => Some(P::Byte),
            "ubyte" => Some(P::Ubyte),
            "short" => Some(P::Short),
            "ushort" => Some(P::Ushort),
            "int" => Some(P::Int),
            "uint" => Some(P::Uint),
            "long" => Some(P::Long),
            "ulong" => Some(P::Ulong),
            "float" => Some(P::Float),
            "double" => Some(P::Double),
            "i8" => Some(P::I8),
            "u8" => Some(P::U8),
            "i16" => Some(P::I16),
            "u16" => Some(P::U16),
            "i32" => Some(P::I32),
            "u32" => Some(P::U32),
            "i64" => Some(P::I64),
            "u64" => Some(P::U64),
            "f32" => Some(P::F32),
            "f64" => Some(P::F64),
            _ => None,
        }
    }

    /// The Rust cast spelling (`as i64`, `as f64`, …) needed to WIDEN `value`
    /// to `target`, or `None` when no widening applies (same type, narrowing,
    /// non-numeric, `bool`/`char`, or an unknown source). Java-family implicit
    /// widening: smaller int -> bigger int, any int -> float/double, float ->
    /// double. NEVER narrows (rustc / Java both forbid silent narrowing). Used
    /// for `return <int> ;` into a `long`/`double` slot, where tycheck already
    /// accepts the widening but the backend otherwise emits the bare value and
    /// leaks an isize into an i64 slot (rustc E0308).
    pub(crate) fn numeric_widen_to(
        &self,
        value: &Expr,
        target: juxc_tycheck::Primitive,
    ) -> Option<&'static str> {
        use juxc_tycheck::Primitive as P;
        let src = self.operand_primitive(value)?;
        if src == target
            || matches!(src, P::Bool | P::Char)
            || matches!(target, P::Bool | P::Char)
        {
            return None;
        }
        // Distinct `Primitive` variants can still lower to the SAME Rust type
        // (e.g. `Long` and `I64` both emit `i64`, `Int`/`Long` differ but
        // `Long`/`I64` don't). A cast between two names that render identically
        // is a no-op that only clutters the output, so skip it. Genuine
        // coercions survive: `Uint`->`Int` is `usize`->`isize`, `Int`->`Long`
        // is `isize`->`i64` -- different names, real `as`.
        if crate::exprs::rust_primitive_name(src) == crate::exprs::rust_primitive_name(target) {
            return None;
        }
        let is_float = |p: P| matches!(p, P::Float | P::Double | P::F32 | P::F64);
        let is_f64 = |p: P| matches!(p, P::Double | P::F64);
        let rank = |p: P| -> u8 {
            match p {
                P::Byte | P::I8 | P::Ubyte | P::U8 => 1,
                P::Short | P::I16 | P::Ushort | P::U16 => 2,
                P::I32 | P::U32 => 3,
                P::Int | P::Uint => 4,
                P::Long | P::I64 | P::Ulong | P::U64 => 5,
                _ => 0,
            }
        };
        let widens = if is_float(target) && !is_float(src) {
            true // any integer -> any float is a widening
        } else if is_float(target) && is_float(src) {
            is_f64(target) && !is_f64(src) // float -> double
        } else if !is_float(target) && !is_float(src) {
            // Wider integer (int -> long) OR same-width signedness reinterpret
            // (uint -> int, e.g. a `len()` returned as `int`). Both need the
            // `as <T>`; never narrows (rank(target) < rank(src) stays false).
            rank(target) >= rank(src)
        } else {
            false // float source -> integer target is narrowing
        };
        if widens {
            Some(crate::exprs::rust_primitive_name(target))
        } else {
            None
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
        self.expr_recorded_ty(e).is_some_and(|ty| self.ty_declares_operator(&ty, kind))
    }

    /// The checker's type for `e`. Span-keyed first; bare locals fall back to
    /// the name-keyed map (call CALLEES aren't walked by the checker, so their
    /// Path spans often have no expr_types entry).
    pub(crate) fn expr_recorded_ty(&self, e: &juxc_ast::Expr) -> Option<Ty> {
        self.expr_types.get(&expr_span_of(e)).cloned().or_else(|| {
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
        })
    }

    /// True when `ty` is a user class or record declaring (and not deleting)
    /// the given operator.
    pub(crate) fn ty_declares_operator(&self, ty: &Ty, kind: OperatorKind) -> bool {
        let Ty::User { name, .. } = ty else {
            return false;
        };
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
        // A value struct is NOT the `Rc<RefCell>` handle shape, so `&obj` on it
        // takes the plain place pointer (`addr_of_mut!`), not `obj.0.as_ptr()`.
        // Excluding it here routes `&valueStruct` correctly.
        self.symbols
            .classes
            .get(&name)
            .is_some_and(|c| !c.is_struct)
    }

    /// True when `e` is statically a **raw pointer** (`T*`). The lowered `Ty`
    /// drops `ptr_depth`, so we recover pointer-ness from the names tracked in
    /// `pointer_locals` (raw-pointer locals/params, §L.6) plus the syntactic
    /// forms that are intrinsically pointers: address-of (`&x` / `&obj`) and a
    /// chain of raw-pointer derefs/indexes off a pointer. Drives the
    /// `p == null` peephole to pick the `is_null()` lowering.
    pub(crate) fn expr_is_raw_pointer(&self, e: &juxc_ast::Expr) -> bool {
        self.pointer_depth(e) > 0
    }

    /// How many raw-pointer levels `e` has: 1 for a `T*`, 2 for a `T**`, 0 for
    /// anything that is not a pointer. The lowered `Ty` erases `ptr_depth`, so
    /// it is recovered from declared `TypeRef`s -- locals and parameters
    /// (`pointer_locals`), fields, casts, and function and method returns --
    /// and carried through the pointer operators: `&x` adds a level, `*p` and
    /// `p[i]` remove one, and `p ± n` keeps it.
    pub(crate) fn pointer_depth(&self, e: &juxc_ast::Expr) -> u8 {
        use juxc_ast::{BinaryOp, Expr, UnaryOp};
        match e {
            Expr::Unary(u) => match u.op {
                UnaryOp::AddrOf => self.pointer_depth(&u.operand).saturating_add(1),
                UnaryOp::Deref => self.pointer_depth(&u.operand).saturating_sub(1),
                _ => 0,
            },
            Expr::Index(i) => self.pointer_depth(&i.array).saturating_sub(1),
            Expr::Cast(c) => c.ty.ptr_depth,
            Expr::Binary(b) => {
                let (l, r) = (self.pointer_depth(&b.left), self.pointer_depth(&b.right));
                match b.op {
                    // `p + n` / `n + p`: the pointer's depth; `p + q` is not a
                    // pointer operation at all.
                    BinaryOp::Add if (l > 0) != (r > 0) => l.max(r),
                    // `p - n` keeps the depth; `q - p` is a count.
                    BinaryOp::Sub if l > 0 && r == 0 => l,
                    _ => 0,
                }
            }
            Expr::Ternary(t) => self.pointer_depth(&t.then_branch).max(self.pointer_depth(&t.else_branch)),
            // A bare name: a pointer local or parameter, or -- when nothing of
            // that name is in scope -- an implicit-`this` pointer field.
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = qn.segments[0].text.as_str();
                if let Some(depth) = self.pointer_locals.get(name) {
                    return *depth;
                }
                let shadowed = self.local_types.iter().any(|s| s.contains_key(name))
                    || self.current_fn_params.contains(name);
                if shadowed {
                    return 0;
                }
                self.enclosing_class
                    .clone()
                    .map(|c| self.class_field_ptr_depth(&c, name))
                    .unwrap_or(0)
            }
            // `this.ptr` / `obj.ptr`: the field's declared depth, inherited
            // fields included.
            Expr::Field(f) => {
                let class = if matches!(&*f.object, Expr::This(_)) {
                    self.enclosing_class.clone()
                } else {
                    self.receiver_class_key(&f.object)
                };
                class.map(|c| self.class_field_ptr_depth(&c, &f.field.text)).unwrap_or(0)
            }
            Expr::Call(c) => self.call_return_ptr_depth(&c.callee),
            _ => 0,
        }
    }

    /// The declared pointer depth of field `field` on class `class`, walking
    /// the resolved `extends` chain.
    fn class_field_ptr_depth(&self, class: &str, field: &str) -> u8 {
        let mut cursor = Some(class.to_string());
        for _ in 0..64 {
            let Some(name) = cursor else { return 0 };
            let Some(sig) = self.lookup_class_by_bare_or_fqn(&name) else { return 0 };
            if let Some(f) = sig.fields.get(field) {
                return f.ty.ptr_depth;
            }
            cursor = sig.extends_fqn.clone();
        }
        0
    }

    /// The pointer depth a call returns: a free or native function's declared
    /// return type, a static method's, or an instance method's on the
    /// receiver's class (inherited ones included).
    fn call_return_ptr_depth(&self, callee: &juxc_ast::Expr) -> u8 {
        let depth_of = |rt: &juxc_ast::ReturnType| match rt {
            juxc_ast::ReturnType::Type(t) | juxc_ast::ReturnType::AsyncType(t) => t.ptr_depth,
            juxc_ast::ReturnType::Void => 0,
        };
        match callee {
            juxc_ast::Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = qn.segments[0].text.as_str();
                if self.pointer_locals.contains_key(name) || self.current_fn_params.contains(name) {
                    return 0;
                }
                let via_unit = self
                    .current_unit_idx
                    .and_then(|i| self.symbols.units.get(i))
                    .and_then(|ctx| ctx.unqualified.get(name))
                    .and_then(|fqn| self.symbols.functions.get(fqn));
                via_unit
                    .or_else(|| self.symbols.lookup_function(name).map(|(_, f)| f))
                    .map(|f| depth_of(&f.return_type))
                    .unwrap_or(0)
            }
            juxc_ast::Expr::Field(f) => {
                let class = match &*f.object {
                    juxc_ast::Expr::This(_) => self.enclosing_class.clone(),
                    juxc_ast::Expr::Path(qn) if self.path_resolves_to_class_in_emit(qn).is_some() => {
                        self.path_resolves_to_class_in_emit(qn)
                    }
                    other => self.receiver_class_key(other),
                };
                let mut cursor = class;
                for _ in 0..64 {
                    let Some(name) = cursor else { return 0 };
                    let Some(sig) = self.lookup_class_by_bare_or_fqn(&name) else { return 0 };
                    if let Some(m) = sig.methods.get(f.field.text.as_str()) {
                        return depth_of(&m.return_type);
                    }
                    cursor = sig.extends_fqn.clone();
                }
                0
            }
            _ => 0,
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

    /// The first parameter type of operator `kind` on class `class`, inherited
    /// operators included.
    pub(crate) fn class_operator_param(&self, class: &str, kind: OperatorKind) -> Option<juxc_ast::TypeRef> {
        let sig = self.lookup_class_by_bare_or_fqn(class)?;
        sig.operators.get(&kind)?.params.first().map(|p| p.ty.clone())
    }

    /// Emit `arg` for an operator parameter typed `param`, converting a class
    /// handle to the parameter's shape: a concrete class into a base's
    /// `Rc<dyn …Kind>` with `.into()`, a subclass handle into the base handle
    /// with a trait-object upcast.
    fn emit_operator_argument(
        &mut self,
        arg: &Expr,
        arg_side: &crate::exprs::field::IdentityOperand,
        param: &juxc_ast::TypeRef,
    ) {
        let param_class = param
            .name
            .segments
            .last()
            .map(|s| s.text.clone())
            .filter(|n| !param.nullable && param.array_shape.is_none() && self.is_poly_base_class(n));
        self.emit_expr_with_parent_prec(arg, u8::MAX, false);
        match param_class {
            Some(base) if arg_side.is_dyn && arg_side.name != base => {
                let prefix = self.cross_package_prefix(&base);
                self.w.push_str(&format!(".clone() as std::rc::Rc<dyn {prefix}{base}Kind>"));
            }
            Some(_) if !arg_side.is_dyn => self.w.push_str(".clone().into()"),
            _ => self.w.push_str(".clone()"),
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
        // An operator over a polymorphic base takes the base handle, so a
        // concrete or subclass-typed operand converts on the way in.
        let param = match (&b.op, self.identity_operand(&b.left)) {
            (op, Some(left)) => binary_operator_kind(*op).and_then(|k| self.class_operator_param(&left.name, k)),
            _ => None,
        };
        match (param, self.identity_operand(&b.right)) {
            (Some(param), Some(right)) => self.emit_operator_argument(&b.right, &right, &param),
            _ => {
                self.emit_expr(&b.right);
                self.w.push_str(".clone()");
            }
        }
        self.w.push(')');
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

/// Whether a shift count is a literal already inside the left operand's
/// width, so a plain `<<` / `>>` means what §S.2.5 says without masking.
fn shift_count_is_in_width(count: &Expr, width: u32) -> bool {
    match count {
        Expr::Literal(juxc_ast::Literal::Int(lit)) => lit.value >= 0 && (lit.value as u64) < u64::from(width),
        _ => false,
    }
}
