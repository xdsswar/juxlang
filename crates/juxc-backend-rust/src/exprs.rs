//! Expression-level lowering — the `emit_expr` dispatch and every
//! sub-helper for binary, unary, range, cast, call, field, index, array
//! literal, and `new array` expressions.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{
    BinaryExpr, BinaryOp, CallExpr, CastExpr, Expr, FieldExpr, IndexExpr, Literal, NewArrayExpr,
    NewArrayLitExpr, RangeExpr, UnaryExpr,
};
use juxc_tycheck::Ty;

use crate::analysis::{is_jux_string_type_ref, is_string_literal};
use crate::RustEmitter;

/// Discriminator for `emit_interp_string`'s deferred-arg emission —
/// records the order in which Bare-ident and full-expression arguments
/// appear in the format-string placeholders so we can emit them in
/// matching order after the format string is closed.
pub(crate) enum ArgRef {
    Bare(usize),
    Expr(usize),
}

/// Precedence value for prefix unary operators. Per §A.4 level 18 —
/// tighter than every binary operator currently modeled.
pub(crate) const UNARY_PREC: u8 = 18;

impl RustEmitter {
    pub(crate) fn emit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal(lit) => self.emit_literal(lit),
            Expr::Path(qn) => {
                // Dot-separated Jux paths become `::`-separated Rust paths.
                // Module mapping is a TODO — for milestone 1 we emit
                // identical structure on faith.
                let path = qn
                    .segments
                    .iter()
                    .map(|i| i.text.as_str())
                    .collect::<Vec<_>>()
                    .join("::");
                self.w.push_str(&path);
            }
            Expr::Call(c) => self.emit_call(c),
            Expr::Binary(b) => self.emit_binary(b),
            Expr::Unary(u) => self.emit_unary(u),
            Expr::Range(r) => self.emit_range(r),
            Expr::Cast(c) => self.emit_cast(c),
            Expr::SizeOf(s) => self.emit_sizeof(s),
            Expr::NewArray(n) => self.emit_new_array(n),
            Expr::NewArrayLit(n) => self.emit_new_array_lit(n),
            Expr::Index(i) => self.emit_index(i),
            Expr::Field(f) => self.emit_field(f),
            Expr::InterpString(s) => self.emit_interp_string(s),
            Expr::This(_) => {
                // Lowers to `self` in a method or `__self` in a
                // constructor. `this_alias` is set by `emit_method` /
                // `emit_constructor` before they walk the body. Outside
                // any class body it'd be `None`, but the resolver has
                // already flagged that as a use-before-declared.
                let alias = self.this_alias.as_deref().unwrap_or("self");
                self.w.push_str(alias);
            }
            Expr::Switch(s) => self.emit_switch(s),
            Expr::NewObject(n) => {
                // `new Foo(args)`        → `Foo::new(args)`.
                // `new Foo<int>(args)`   → `Foo::<isize>::new(args)`
                //                          (Rust turbofish — required
                //                          on the type position before
                //                          the method-call `::new`).
                // The class path is single-segment in practice today
                // but stays `path-joined` for forward compatibility.
                let path = n
                    .class_name
                    .segments
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join("::");
                self.w.push_str(&path);
                if !n.generic_args.is_empty() {
                    self.w.push_str("::<");
                    // Clone to release the immutable borrow on `n` before
                    // the `emit_type_as_rust` calls (which need `&mut self`).
                    let args: Vec<juxc_ast::TypeRef> = n.generic_args.clone();
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        self.emit_type_as_rust(arg);
                    }
                    self.w.push('>');
                }
                self.w.push_str("::new(");
                for (i, arg) in n.args.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.w.push(')');
            }
        }
    }

    pub(crate) fn emit_field(&mut self, f: &FieldExpr) {
        if f.field.text == "length" {
            self.w.push('(');
            self.emit_expr(&f.object);
            self.w.push_str(").len() as isize");
            return;
        }
        // Enum variant access: `Color.Red` (a Field whose object is a
        // single-segment Path naming a known enum type) lowers to
        // Rust's path syntax `Color::Red`. Tuple-payload variant
        // construction (`Color.Red(args)`) reuses this path through
        // the enclosing `emit_call`, which appends the arg list.
        if let Expr::Path(qn) = &*f.object {
            if qn.segments.len() == 1 && self.symbols.enums.contains_key(&qn.segments[0].text) {
                self.w.push_str(&qn.segments[0].text);
                self.w.push_str("::");
                self.w.push_str(&f.field.text);
                return;
            }
        }
        // Generic member access — emit verbatim and rely on Rust to
        // resolve.
        self.emit_expr(&f.object);
        self.w.push('.');
        self.w.push_str(&f.field.text);
        // Auto-`.clone()` on field reads in two cases:
        //   1. String-field reads — so `return this.name;` and similar
        //      don't move out of `&self`.
        //   2. Generic-field reads — `class Box<T> { T value; … }`'s
        //      `return this.value;` faces the same move-out-of-&self
        //      problem; we clone the same way. The Phase-1 `T: Clone`
        //      bound emitted on the impl makes this always valid.
        // Both paths share the lvalue-suppression — we never want
        // `self.x.clone() = ...` on an assignment target.
        //
        // Phase H: the decision used to consult two name-keyed
        // `HashSet`s (`string_field_names` / `generic_field_names`)
        // computed by a pre-pass over class field decls. That worked
        // but mis-fired when a same-named field on a different class
        // had a different type. Now we consult tycheck's per-expression
        // type map directly: the field expression `obj.name` was
        // recorded with its precise `Ty`. A missing entry falls back
        // to the conservative "do the .clone()" path, matching the
        // old heuristic for the (rare) cases tycheck didn't visit.
        if !self.emitting_lvalue && self.field_read_needs_clone(f) {
            self.w.push_str(".clone()");
        }
    }

    /// Decide whether a `.clone()` should follow a field read.
    ///
    /// Looks up the field expression's recorded `Ty` in `expr_types`:
    /// `Ty::String` or `Ty::Param(_)` require the clone (matching the
    /// two cases the old name-based pre-pass tagged). Everything else
    /// — primitives, user types, arrays — gets no clone, since their
    /// Rust counterparts are `Copy` or already passed by value.
    ///
    /// **Fallback.** When the field's type isn't in `expr_types` (the
    /// expression wasn't visited, or carries a dummy span), we fall
    /// back to looking the field up directly in `symbols.classes` /
    /// `symbols.records` via [`Self::lookup_field_type`]. If that also
    /// misses, we return `false` — the safer default in the absence of
    /// type info, since unnecessary clones on non-`Clone` types would
    /// fail to compile, while a missing clone on a `Clone` type usually
    /// just shifts a move-error around but keeps emitted Rust valid.
    pub(crate) fn field_read_needs_clone(&self, f: &FieldExpr) -> bool {
        // Resolve the field's declared type through the symbol table
        // by way of the receiver's recorded type. This is more
        // reliable than a direct `expr_types.get(&f.span)` lookup
        // because the latter is keyed by an absolute source span, and
        // interpolated-string segments (`$"… ${expr} …"`) reparse
        // their inner expressions against the segment substring —
        // those inner expressions carry spans local to the substring,
        // so several distinct interpolation sites can collide on the
        // same key in `expr_types`. Verifying via the field-name
        // lookup on the receiver's class/record signature side-steps
        // the collision: a stale receiver type just means the field
        // lookup fails and we fall back to "no clone."
        if let Some(ty) = self.lookup_field_type(f) {
            return matches!(ty, Ty::String | Ty::Param(_));
        }
        false
    }

    /// Resolve a field access's declared type via the symbol table.
    /// Walks `f.object`'s recorded type to find the owning class /
    /// record, then looks up `f.field.text` on it. Returns `None` for
    /// anything we can't resolve (non-user-typed receiver, missing
    /// entries, etc.).
    ///
    /// Phase H: this replaces the heuristic `string_field_names` /
    /// `generic_field_names` sets that used to drive the
    /// `.clone()` / `.to_string()` decision. The new path keys on the
    /// receiver's class/record name, which means same-named fields on
    /// unrelated classes are correctly distinguished. The class's own
    /// generic-params list flows into [`ty_kind_from_ref_with_params`]
    /// so a single-segment name matching a type param (e.g. `T` in
    /// `class Box<T> { T value; }`) lands as [`Ty::Param`] rather than
    /// the misleading `Ty::User { name: "T", … }`.
    pub(crate) fn lookup_field_type(&self, f: &FieldExpr) -> Option<Ty> {
        let object_ty = self.expr_types.get(&expr_span_of(&f.object))?;
        let Ty::User { name, .. } = object_ty else {
            return None;
        };
        // Class field — walk the inheritance chain. The chain walk
        // is generic-params-aware so a class field of type `T`
        // resolves to `Ty::Param("T")` instead of `Ty::User`.
        if let Some(ty) = self.lookup_class_field_ty_in_chain(name, &f.field.text) {
            return Some(ty);
        }
        // Record component — pull the record's own generic params for
        // the same param-vs-user distinction class fields get.
        if let Some(record) = self.symbols.records.get(name) {
            if let Some(c) = record.components.iter().find(|c| c.name == f.field.text) {
                let params: std::collections::HashSet<&str> = record
                    .generic_params
                    .iter()
                    .map(|p| p.name.text.as_str())
                    .collect();
                return Some(ty_kind_from_ref_with_params(&c.ty, &params));
            }
        }
        None
    }

    /// Walk the `extends` chain of `class_name` to find a field by
    /// name, returning its declared [`Ty`]. Mirrors the lookup tycheck
    /// does in `check::Checker::lookup_field_in_chain`. The class's
    /// own generic-params list flows through
    /// [`ty_kind_from_ref_with_params`] so single-segment names
    /// matching a type parameter resolve to [`Ty::Param`]; everything
    /// else falls through to the primitive / String / user-type
    /// branches.
    fn lookup_class_field_ty_in_chain(&self, class_name: &str, field_name: &str) -> Option<Ty> {
        let mut cursor: Option<&str> = Some(class_name);
        let mut depth = 0usize;
        while let Some(name) = cursor {
            if depth > 64 {
                return None;
            }
            let class = self.symbols.classes.get(name)?;
            if let Some(field) = class.fields.get(field_name) {
                let params: std::collections::HashSet<&str> = class
                    .generic_params
                    .iter()
                    .map(|p| p.name.text.as_str())
                    .collect();
                return Some(ty_kind_from_ref_with_params(&field.ty, &params));
            }
            cursor = class
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()));
            depth += 1;
        }
        None
    }

    /// Lower `arr[index]` to Rust `arr[index_as_usize]`.
    ///
    /// Rust requires `usize` for array/slice/Vec indexing. Jux's
    /// platform-sized `int` lowers to Rust `isize`, so a Jux user
    /// writing `arr[i]` with `int i` would hit a Rust type error
    /// without coercion. We sidestep that by:
    ///
    /// - **Integer literal indices** (`arr[0]`) → emit raw; Rust infers
    ///   `usize` from the indexing context.
    /// - **Anything else** (`arr[i]`, `arr[i + 1]`) → wrap as
    ///   `(expr) as usize`. The redundant cast is a no-op when the
    ///   operand is already `usize`.
    ///
    /// A future pass with a real type table can drop the cast when the
    /// index expression's static type is already `usize` (Jux `uint`).
    pub(crate) fn emit_index(&mut self, i: &IndexExpr) {
        self.emit_expr(&i.array);
        self.w.push('[');
        if matches!(&*i.index, Expr::Literal(Literal::Int(_))) {
            self.emit_expr(&i.index);
        } else {
            self.w.push('(');
            self.emit_expr(&i.index);
            self.w.push_str(") as usize");
        }
        self.w.push(']');
    }

    /// Lower `new T[size]` to Rust `[default_for_T; size]`.
    ///
    /// Rust's `[VALUE; N]` literal requires `N` to be a `const` expr
    /// and `VALUE` to be `Copy` (or evaluated once for `const`). For
    /// Turn 1 we emit:
    ///
    /// - `new int[10]`     → `[0; 10]`
    /// - `new bool[5]`     → `[false; 5]`
    /// - `new double[3]`   → `[0.0; 3]`
    /// - `new char[8]`     → `['\\0'; 8]`
    /// - `new MyType[N]`   → `[Default::default(); N]` (works iff MyType: Default + Copy)
    pub(crate) fn emit_new_array(&mut self, n: &NewArrayExpr) {
        self.w.push('[');
        self.emit_default_value_for(&n.element_type);
        self.w.push_str("; ");
        self.emit_expr(&n.size);
        self.w.push(']');
    }

    /// Lower an array initializer literal — `new T[]{a, b, c}` or the
    /// bare `{a, b, c}` form in a typed-local RHS.
    ///
    /// Dispatch is on `n.fixed`:
    ///
    /// - **`fixed: true`** → Rust array literal `[a, b, c]`. Used when
    ///   the binding's LHS type is `T[N]` (compile-time-known size).
    ///   Rust verifies the element count matches `N` at compile time.
    /// - **`fixed: false`** → `vec![a, b, c]` (or `Vec::<T>::new()`
    ///   when the list is empty — `vec![]` alone is type-ambiguous).
    ///   Used when the binding's LHS type is `T[]` or when the literal
    ///   came from a `new T[]{…}` new-expression.
    ///
    /// Element-type inference quirk (dynamic case): `let xs = vec![1, 2, 3];`
    /// alone defaults to `Vec<i32>` even when the Jux source said
    /// `int` (isize). That's fine for printing/indexing; a future pass
    /// with full type-tracking can emit a `: Vec<isize>` annotation
    /// when a typed local makes the intended element type explicit.
    pub(crate) fn emit_new_array_lit(&mut self, n: &NewArrayLitExpr) {
        // Fixed → Rust array literal `[a, b, c]`. Empty fixed literals
        // can't be written in Jux (the parser never produces them) so
        // we don't have a special path for them.
        if n.fixed {
            self.w.push('[');
            for (i, elem) in n.elements.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.emit_expr(elem);
            }
            self.w.push(']');
            return;
        }

        // Dynamic — Vec lowering.
        if n.elements.is_empty() {
            // Empty literal — turbofish-constructed empty Vec so Rust
            // knows the element type without an annotation.
            self.w.push_str("Vec::<");
            self.emit_type_as_rust(&n.element_type);
            self.w.push_str(">::new()");
            return;
        }
        self.w.push_str("vec![");
        for (i, elem) in n.elements.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.emit_expr(elem);
        }
        self.w.push(']');
    }

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
        self.w.push_str(u.op.as_rust_str());
        // Unary precedence is higher than any binary; reusing
        // emit_expr_with_parent_prec at UNARY_PREC gives the right
        // wrapping for free.
        self.emit_expr_with_parent_prec(&u.operand, UNARY_PREC, /*right=*/ false);
    }

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

    /// Emit `e` inside a parent context with the given precedence,
    /// wrapping in `( … )` only when grouping would otherwise be lost.
    ///
    /// `right_of_left_assoc` indicates that `e` sits on the right side
    /// of a left-associative parent operator — in that case an
    /// equal-precedence child also needs parens.
    pub(crate) fn emit_expr_with_parent_prec(
        &mut self,
        e: &Expr,
        parent_prec: u8,
        right_of_left_assoc: bool,
    ) {
        let needs_paren = match e {
            Expr::Binary(b) => {
                let p = binary_prec(b.op);
                if right_of_left_assoc {
                    p <= parent_prec
                } else {
                    p < parent_prec
                }
            }
            // Unary expressions sit at level 18, tighter than every
            // binary we model — so they never need wrapping under a
            // binary parent. (Inside another unary, multiple prefix
            // operators chain naturally as `--x` without extra parens.)
            Expr::Unary(_) => false,
            // Atomic and postfix expressions never need parens — they
            // bind tighter than any binary operator.
            _ => false,
        };
        if needs_paren {
            self.w.push('(');
        }
        self.emit_expr(e);
        if needs_paren {
            self.w.push(')');
        }
    }

    /// Emit a call expression. Special-cases the built-in `print` to
    /// `println!(…)`. Every other callee is emitted verbatim (the
    /// resolver guarantees the name exists).
    pub(crate) fn emit_call(&mut self, call: &CallExpr) {
        // Recognize a single-segment path `print` for the built-in.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "print" {
                return self.emit_print_call(call);
            }
        }
        // Detect enum-variant construction so we can inject
        // `.to_string()` on String-typed payload slots. Shape we want:
        // `Call(Field(Path(EnumName), VariantName), args)`.
        //
        // Phase H: the per-variant slot table used to be a pre-pass
        // collection (`enum_string_slots`). It now derives directly
        // from tycheck's `SymbolTable.enums[name].variants[variant]`
        // payload `TypeRef`s — same logical lookup, no parallel
        // shadow table to keep in sync. The slot booleans tell
        // `emit_call` which positional args want `.to_string()`.
        let string_slots: Option<Vec<bool>> = if let Expr::Field(f) = &*call.callee {
            if let Expr::Path(qn) = &*f.object {
                if qn.segments.len() == 1 {
                    self.symbols
                        .enums
                        .get(&qn.segments[0].text)
                        .and_then(|e| e.variants.get(&f.field.text))
                        .filter(|v| !v.payload.is_empty())
                        .map(|v| v.payload.iter().map(is_jux_string_type_ref).collect())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Generic call: emit `callee(args, …)` literally, with the
        // optional per-arg `.to_string()` coercion when an enum
        // variant slot wants `String`.
        self.emit_expr(&call.callee);
        self.w.push('(');
        for (i, arg) in call.args.iter().enumerate() {
            if i > 0 { self.w.push_str(", "); }
            self.emit_expr(arg);
            if let Some(slots) = string_slots.as_ref() {
                if slots.get(i).copied().unwrap_or(false) {
                    self.w.push_str(".to_string()");
                }
            }
        }
        self.w.push(')');

        // Phase-1 workaround: Rust's `Vec::pop` returns `Option<T>` but
        // Jux doesn't yet have an `Option` type, so Jux user code uses
        // `var top = stack.pop();` expecting a `T`-typed value. We
        // bridge that by appending `.unwrap()` here — pop on an empty
        // Vec then panics, which mirrors Java's `NoSuchElementException`
        // shape. Remove this special case once `Option<T>` lands and
        // pop can return `T?` directly.
        if let Expr::Field(f) = &*call.callee {
            if f.field.text == "pop" && call.args.is_empty() {
                self.w.push_str(".unwrap()");
            }
        }
    }

    /// Lower a call to the built-in `print(…)` into the most natural Rust
    /// `println!` shape we can.
    ///
    /// Rules:
    /// - `print("literal")` → `println!("literal")`. We bake the string
    ///   directly into the format-string slot, doubling any `{` / `}` so
    ///   `println!`'s parser keeps its hands off them.
    /// - `print(expr)` (single non-literal arg) → `println!("{}", expr)`.
    /// - `print(a, b, …)` (multi-arg) → `println!("{} {} …", a, b, …)`
    ///   with one `{}` per argument. This is a placeholder shape until
    ///   `std.io.print` is properly specced.
    pub(crate) fn emit_print_call(&mut self, call: &CallExpr) {
        // Hot path: one string-literal argument. Inline it as the format.
        if call.args.len() == 1 {
            if let Expr::Literal(Literal::String(s)) = &call.args[0] {
                self.w.push_str("println!(");
                self.emit_rust_format_string_literal(s);
                self.w.push(')');
                return;
            }
            // Hot path: one interpolated-string argument. Inline its
            // segments directly into the println! call instead of
            // emitting `println!("{}", format!("…", args))`. Same
            // shape format!() would produce, one less call frame.
            if let Expr::InterpString(s) = &call.args[0] {
                self.w.push_str("println!(\"");
                let mut bare_args: Vec<&juxc_ast::Ident> = Vec::new();
                let mut expr_args: Vec<&Expr> = Vec::new();
                let mut arg_order: Vec<ArgRef> = Vec::new();
                for seg in &s.segments {
                    match seg {
                        juxc_ast::InterpSegment::Literal(text) => {
                            self.emit_interp_literal_chunk(text);
                        }
                        juxc_ast::InterpSegment::Bare(ident) => {
                            self.w.push_str("{}");
                            bare_args.push(ident);
                            arg_order.push(ArgRef::Bare(bare_args.len() - 1));
                        }
                        juxc_ast::InterpSegment::Expr(expr) => {
                            self.w.push_str("{}");
                            expr_args.push(expr);
                            arg_order.push(ArgRef::Expr(expr_args.len() - 1));
                        }
                    }
                }
                self.w.push('"');
                for arg_ref in &arg_order {
                    self.w.push_str(", ");
                    match arg_ref {
                        ArgRef::Bare(i) => self.w.push_str(&bare_args[*i].text),
                        ArgRef::Expr(i) => self.emit_expr(expr_args[*i]),
                    }
                }
                self.w.push(')');
                return;
            }
        }
        // General path: one `{}` placeholder per arg, then the args.
        self.w.push_str("println!(\"");
        for i in 0..call.args.len() {
            if i > 0 {
                self.w.push(' ');
            }
            self.w.push_str("{}");
        }
        self.w.push('"');
        for arg in &call.args {
            self.w.push_str(", ");
            self.emit_expr(arg);
        }
        self.w.push(')');
    }
}

/// Reach into an expression for its span — companion to tycheck's
/// `check::expr_span`. Lets backend helpers look up an expression's
/// type via `expr_types[expr.span]` without exposing each variant's
/// inner span field at call sites. Synthesized expressions without a
/// real source span return [`juxc_source::Span::DUMMY`], which is the
/// same value the recorder sentinels out — so `expr_types.get(...)`
/// will simply miss and the caller falls back conservatively.
pub(crate) fn expr_span_of(e: &Expr) -> juxc_source::Span {
    match e {
        Expr::Literal(_) => juxc_source::Span::DUMMY,
        Expr::Path(qn) => qn.span,
        Expr::Call(c) => c.span,
        Expr::Binary(b) => b.span,
        Expr::Unary(u) => u.span,
        Expr::Range(r) => r.span,
        Expr::Cast(c) => c.span,
        Expr::SizeOf(s) => s.span,
        Expr::NewArray(n) => n.span,
        Expr::NewArrayLit(n) => n.span,
        Expr::Index(i) => i.span,
        Expr::Field(f) => f.span,
        Expr::InterpString(s) => s.span,
        Expr::This(s) => *s,
        Expr::NewObject(n) => n.span,
        Expr::Switch(s) => s.span,
    }
}

/// Cheap "what kind of Ty would this TypeRef lower to?" — primitives,
/// String, arrays, and bare class/generic names. Used by
/// [`RustEmitter::lookup_field_type`] to classify a field's declared
/// `TypeRef` without round-tripping through tycheck's full
/// `ty_from_ref` (which needs a `TypeEnv` we don't have at emission
/// time). The `generic_params` set carries the names declared on the
/// enclosing class/record so a single-segment name matching a param
/// resolves to [`Ty::Param`]. Anything more nuanced (qualified paths,
/// generic instantiations) returns [`Ty::Unknown`].
pub(crate) fn ty_kind_from_ref_with_params(
    t: &juxc_ast::TypeRef,
    generic_params: &std::collections::HashSet<&str>,
) -> Ty {
    use juxc_tycheck::{ArrayKind, Primitive};
    if let Some(shape) = &t.array_shape {
        let element_ref = juxc_ast::TypeRef {
            name: t.name.clone(),
            generic_args: t.generic_args.clone(),
            nullable: t.nullable,
            array_shape: None,
            span: t.span,
        };
        let element = ty_kind_from_ref_with_params(&element_ref, generic_params);
        let kind = match shape {
            juxc_ast::ArrayShape::Fixed(_) => ArrayKind::Fixed,
            juxc_ast::ArrayShape::Dynamic => ArrayKind::Dynamic,
        };
        return Ty::Array {
            element: Box::new(element),
            kind,
        };
    }
    if t.name.segments.len() != 1 || !t.generic_args.is_empty() {
        return Ty::Unknown;
    }
    let name = t.name.segments[0].text.as_str();
    let prim = match name {
        "bool" => Some(Primitive::Bool),
        "byte" => Some(Primitive::Byte),
        "ubyte" => Some(Primitive::Ubyte),
        "short" => Some(Primitive::Short),
        "ushort" => Some(Primitive::Ushort),
        "int" => Some(Primitive::Int),
        "uint" => Some(Primitive::Uint),
        "long" => Some(Primitive::Long),
        "ulong" => Some(Primitive::Ulong),
        "float" => Some(Primitive::Float),
        "double" => Some(Primitive::Double),
        "char" => Some(Primitive::Char),
        "i8" => Some(Primitive::I8),
        "u8" => Some(Primitive::U8),
        "i16" => Some(Primitive::I16),
        "u16" => Some(Primitive::U16),
        "i32" => Some(Primitive::I32),
        "u32" => Some(Primitive::U32),
        "i64" => Some(Primitive::I64),
        "u64" => Some(Primitive::U64),
        "f32" => Some(Primitive::F32),
        "f64" => Some(Primitive::F64),
        _ => None,
    };
    if let Some(p) = prim {
        return Ty::Primitive(p);
    }
    if name == "String" {
        return Ty::String;
    }
    // Generic-params-aware: a single-segment name that matches a type
    // parameter of the enclosing class/record resolves to `Ty::Param`.
    // Other identifiers — typically class names — land as `Ty::User`.
    if generic_params.contains(name) {
        Ty::Param(name.to_string())
    } else {
        Ty::User {
            name: name.to_string(),
            generic_args: Vec::new(),
        }
    }
}

/// Precedence value for a binary operator. Higher = binds tighter.
///
/// **Values match Rust's relative ordering**, not Jux's. The Jux source
/// grammar (§A.4) follows Java/Python precedence — bitwise `& | ^` is
/// **looser** than equality, the opposite of Rust. The parser builds the
/// AST according to Jux's rules. When emitting Rust, we use this table
/// (Rust ordering) so the paren-on-precedence-mismatch logic adds parens
/// wherever necessary to preserve the Jux tree shape under Rust's parser.
///
/// | Level | Operators                                            |
/// |-------|------------------------------------------------------|
/// | 4     | `\|\|` (logical OR)                                  |
/// | 5     | `&&` (logical AND)                                   |
/// | 6     | `==`, `!=`                                            |
/// | 7     | `<`, `<=`, `>`, `>=`                                  |
/// | 8     | `\|` (bitwise OR)                                    |
/// | 9     | `^` (bitwise XOR)                                    |
/// | 10    | `&` (bitwise AND)                                    |
/// | 11    | `<<`, `>>` (shifts)                                   |
/// | 12    | `+`, `-`                                              |
/// | 13    | `*`, `/`, `%`                                         |
pub(crate) fn binary_prec(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Or     => 4,
        BinaryOp::And    => 5,
        BinaryOp::Eq | BinaryOp::NotEq => 6,
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => 7,
        BinaryOp::BitOr  => 8,
        BinaryOp::BitXor => 9,
        BinaryOp::BitAnd => 10,
        BinaryOp::Shl | BinaryOp::Shr => 11,
        BinaryOp::Add | BinaryOp::Sub => 12,
        BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => 13,
    }
}
