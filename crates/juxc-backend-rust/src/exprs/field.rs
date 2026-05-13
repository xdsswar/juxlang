//! Field-access emission — `obj.field` reads, with the auto-`.clone()`
//! and `.length` special cases, plus the supporting type-lookup that
//! decides when a clone is needed.

use juxc_ast::{Expr, FieldExpr};
use juxc_tycheck::Ty;

use crate::exprs::{expr_span_of, ty_kind_from_ref_with_params};
use crate::RustEmitter;

impl RustEmitter {
    pub(crate) fn emit_field(&mut self, f: &FieldExpr) {
        // Safe-navigation field access (`obj?.field`) lowers via
        // `Option::map`: the closure runs only when the receiver
        // is `Some`, and the result is `Option<FieldType>`. We
        // emit through `as_ref()` so the original `Option<T>` isn't
        // moved — the user is free to keep reading `obj` after.
        // A `?.field` access on a method-call result (`f()?.field`)
        // works the same way: the inner expression's value goes
        // through `.as_ref()` then `.map(...)`.
        //
        // Field clones inside the closure use `.clone()` for
        // ownership; the closure receives a `&T`, so we clone the
        // field out. Every Jux user type derives `Clone`, so this
        // is always valid (primitives are `Copy` and ignore the
        // call). The `length` short-circuit below stays
        // safe-aware: `obj?.length` on a nullable array produces
        // an `Option<isize>` length.
        if f.safe {
            self.emit_safe_field(f);
            return;
        }
        if f.field.text == "length" {
            // `xs.length` → `xs.len() as isize`. Wrap the receiver
            // in parens only when its shape might otherwise bind
            // looser than `.` (e.g. binary or range expression);
            // atoms like idents, field-chains, method calls, and
            // indexes don't need them and the output reads as
            // handwritten Rust without the parens. The `as isize`
            // cast is required because Rust's `.len()` returns
            // `usize` but Jux's `int` is platform-signed.
            let needs_parens = receiver_needs_parens(&f.object);
            if needs_parens {
                self.w.push('(');
            }
            self.emit_expr(&f.object);
            if needs_parens {
                self.w.push(')');
            }
            self.w.push_str(".len() as isize");
            return;
        }
        // Enum variant access: `Color.Red` (a Field whose object is a
        // single-segment Path naming a known enum type) lowers to
        // Rust's path syntax `Color::Red`. Tuple-payload variant
        // construction (`Color.Red(args)`) reuses this path through
        // the enclosing `emit_call`, which appends the arg list.
        if let Expr::Path(qn) = &*f.object {
            if qn.segments.len() == 1 {
                let bare = &qn.segments[0].text;
                // Direct FQN match (single-package programs and
                // explicitly-FQN'd uses).
                if self.symbols.enums.contains_key(bare) {
                    self.w.push_str(bare);
                    self.w.push_str("::");
                    self.w.push_str(&f.field.text);
                    return;
                }
                // Import-alias-aware: the current unit's
                // `unqualified` map carries `alias → FQN` for both
                // bare imports and grouped `{ X as Y }` aliases. A
                // hit there resolves enum-variant constructions
                // through the user's chosen alias name. Emit the
                // alias name on the LHS (Rust scope has it via the
                // emitted `use X as Y;`) while the FQN match
                // confirms the enum is real.
                if let Some(idx) = self.current_unit_idx {
                    if let Some(ctx) = self.symbols.units.get(idx) {
                        if let Some(fqn) = ctx.unqualified.get(bare.as_str()) {
                            if self.symbols.enums.contains_key(fqn) {
                                self.w.push_str(bare);
                                self.w.push_str("::");
                                self.w.push_str(&f.field.text);
                                return;
                            }
                        }
                    }
                }
                // Bare-name reference to an enum imported from
                // another package: scan all enum FQNs and pick one
                // whose last segment matches. Same shape the
                // class- and interface-FQN walks use elsewhere.
                for enum_fqn in self.symbols.enums.keys() {
                    let last = enum_fqn.rsplit('.').next().unwrap_or(enum_fqn.as_str());
                    if last == bare.as_str() {
                        self.w.push_str(bare);
                        self.w.push_str("::");
                        self.w.push_str(&f.field.text);
                        return;
                    }
                }
            }
        }
        // Static-field access: `ClassName.X` (or `pkg.Cls.X`) where
        // the path resolves to a known class. Two emission shapes:
        //
        //   - `final` static  → Rust associated const inside the
        //     inherent impl, accessed as `Path::X`. Cross-package
        //     paths get the same `crate::`-rooting `new` uses.
        //   - Plain `static`  → module-scope `LazyLock<Mutex<T>>`
        //     named `Class_X` (see `emit_mutable_static_field`).
        //     Lvalue context emits `*Class_X.lock().unwrap()` so
        //     the surrounding `=` produces a valid place
        //     expression; rvalue context emits
        //     `Class_X.lock().unwrap().clone()` to materialize an
        //     owned value before the guard drops.
        if let Expr::Path(qn) = &*f.object {
            if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                let cls = self.symbols.classes.get(&class_fqn);
                if let Some(field) = cls.and_then(|c| c.fields.get(f.field.text.as_str())) {
                    if field.is_static {
                        if field.is_final {
                            self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                            self.w.push_str("::");
                            self.w.push_str(&f.field.text);
                            return;
                        }
                        // Mutable static — guarded `LazyLock<Mutex<T>>`.
                        if self.emitting_lvalue {
                            self.w.push('*');
                            self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                            self.w.push('_');
                            self.w.push_str(&f.field.text);
                            self.w.push_str(".lock().unwrap()");
                        } else {
                            self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                            self.w.push('_');
                            self.w.push_str(&f.field.text);
                            self.w.push_str(".lock().unwrap().clone()");
                        }
                        return;
                    }
                }
            }
            // Interface static field: `IfaceName.FIELD` lowers to
            // `Iface_FIELD`. The free-`pub const` definition is
            // emitted by `emit_interface_decl` alongside the trait.
            if let Some(iface_fqn) = self.path_resolves_to_interface_in_emit(qn) {
                let iface = self.symbols.interfaces.get(&iface_fqn);
                if iface
                    .and_then(|i| i.fields.get(f.field.text.as_str()))
                    .is_some()
                {
                    self.emit_fqn_path_in_rust(&iface_fqn, qn.segments.len() > 1);
                    self.w.push('_');
                    self.w.push_str(&f.field.text);
                    return;
                }
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
        // Auto-`.clone()` is suppressed in any **borrow context** —
        // a position where the surrounding code only needs to *read*
        // the field, not own it. Today three such positions:
        //
        // - **lvalue context**: `self.x = ...` must never become
        //   `self.x.clone() = ...`.
        // - **format-arg context**: `println!`/`format!` borrow via
        //   `Display`; a `&String` is as good as `String` and we
        //   save the alloc.
        // - **comparison operand**: `==`, `!=`, `<`, `<=`, `>`, `>=`
        //   on Strings borrow both sides through `PartialEq`/
        //   `PartialOrd`, so the clone is redundant.
        let in_borrow_context =
            self.emitting_format_arg || self.emitting_comparison_operand;
        if !self.emitting_lvalue && !in_borrow_context && self.field_read_needs_clone(f) {
            self.w.push_str(".clone()");
        }
    }

    /// Lower `obj?.field` to a closure that runs only when the
    /// receiver is `Some`. Two shapes depending on whether the
    /// field itself is nullable:
    ///
    /// - **Non-nullable field** →
    ///   `obj.as_ref().map(|__t| __t.field.clone())`. The closure
    ///   returns the field's value; the whole expression is
    ///   `Option<FieldType>`.
    /// - **Nullable field** →
    ///   `obj.as_ref().and_then(|__t| __t.field.clone())`. The
    ///   field already returns `Option<T>`; `and_then` flattens
    ///   the two layers so the result stays `Option<T>` instead
    ///   of the wrong `Option<Option<T>>` `.map` would produce.
    ///
    /// The receiver is borrowed (via `as_ref`) so the original
    /// `Option<T>` stays usable. Inside the closure the field is
    /// cloned so the result is owned.
    ///
    /// Method-call variant `obj?.method(args)` is handled at the
    /// `emit_call` level (`emit_safe_method_call`).
    /// Emit a bare reference to a static field of the enclosing
    /// class. Mirrors the explicit-`Class.field` branch in
    /// [`Self::emit_field`] but takes the class name and field
    /// metadata directly because the caller (in `Expr::Path`
    /// emission) has already resolved both.
    ///
    /// `is_final` picks the lowering shape:
    /// - `true`  → `pub const`-style access, `Class::field`.
    /// - `false` → `LazyLock<Mutex<T>>`-style at module scope,
    ///   `Class_field`. Lvalue/rvalue context drives the lock
    ///   shape, identical to the qualified-form rule.
    pub(crate) fn emit_enclosing_class_static_ref(
        &mut self,
        class_name: &str,
        field_name: &str,
        is_final: bool,
    ) {
        if is_final {
            self.w.push_str(class_name);
            self.w.push_str("::");
            self.w.push_str(field_name);
            return;
        }
        if self.emitting_lvalue {
            self.w.push('*');
            self.w.push_str(class_name);
            self.w.push('_');
            self.w.push_str(field_name);
            self.w.push_str(".lock().unwrap()");
        } else {
            self.w.push_str(class_name);
            self.w.push('_');
            self.w.push_str(field_name);
            self.w.push_str(".lock().unwrap().clone()");
        }
    }

    pub(crate) fn emit_safe_field(&mut self, f: &FieldExpr) {
        let needs_parens = receiver_needs_parens(&f.object);
        if needs_parens {
            self.w.push('(');
        }
        self.emit_expr(&f.object);
        if needs_parens {
            self.w.push(')');
        }
        let combinator = if self.safe_field_is_nullable(f) {
            ".as_ref().and_then(|__t| __t."
        } else {
            ".as_ref().map(|__t| __t."
        };
        self.w.push_str(combinator);
        self.w.push_str(&f.field.text);
        self.w.push_str(".clone())");
    }

    /// True iff the field named by `f` is declared `T?` on the
    /// receiver's class/record. Used by `emit_safe_field` to
    /// pick between `.map` (non-nullable field) and
    /// `.and_then` (nullable field; flattens
    /// `Option<Option<T>>`).
    ///
    /// Resolution: tycheck records the receiver's full type in
    /// `expr_types`. For `obj.inner?.note`, the receiver of
    /// `?.note` is `obj.inner` which infers to
    /// `Ty::Nullable(Inner)`; we peel the nullable wrap before
    /// looking up the field. Missing info (unrecognized class,
    /// unknown field) returns false — `.map` is the safer default
    /// when in doubt; Rust surfaces real shape mismatches.
    fn safe_field_is_nullable(&self, f: &FieldExpr) -> bool {
        let object_ty = self.expr_types.get(&crate::exprs::expr_span_of(&f.object));
        let receiver_name = match object_ty {
            Some(juxc_tycheck::Ty::Nullable(inner)) => match inner.as_ref() {
                juxc_tycheck::Ty::User { name, .. } => name.as_str(),
                _ => return false,
            },
            Some(juxc_tycheck::Ty::User { name, .. }) => name.as_str(),
            _ => return false,
        };
        if let Some(class) = self.symbols.classes.get(receiver_name) {
            if let Some(field) = class.fields.get(&f.field.text) {
                return field.ty.nullable;
            }
        }
        if let Some(record) = self.symbols.records.get(receiver_name) {
            if let Some(c) = record.components.iter().find(|c| c.name == f.field.text) {
                return c.ty.nullable;
            }
        }
        false
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
            return self.ty_needs_clone_on_field_read(&ty);
        }
        false
    }

    /// True iff a field of type `ty` should auto-`.clone()` on read.
    /// Catches the standard non-`Copy` cases: `String`, generic
    /// parameters (always conservatively cloned), records (records
    /// derive `Clone` but not `Copy` unless every component is
    /// primitive — returning by value would otherwise move out of
    /// `&self`), and class references (classes always derive
    /// `Clone`, never `Copy`).
    fn ty_needs_clone_on_field_read(&self, ty: &Ty) -> bool {
        match ty {
            Ty::String | Ty::Param(_) => true,
            Ty::User { name, .. } => {
                // The `Ty::User { name }` here can be either an FQN
                // (multi-package programs) or a bare class name
                // (`ty_kind_from_ref_with_params` doesn't resolve
                // FQNs from a TypeRef). Try direct lookup, then
                // fall back to a suffix scan on each kind of
                // user-type slot in the symbol table.
                let resolve_record = || -> Option<&juxc_tycheck::symbol_table::RecordSig> {
                    self.symbols.records.get(name.as_str()).or_else(|| {
                        self.symbols
                            .records
                            .iter()
                            .find(|(k, _)| {
                                k.rsplit('.').next().unwrap_or(k.as_str()) == name.as_str()
                            })
                            .map(|(_, v)| v)
                    })
                };
                if let Some(record) = resolve_record() {
                    let all_copy = record
                        .components
                        .iter()
                        .all(|c| crate::analysis::field_supports_copy(&c.ty));
                    return !all_copy;
                }
                // Class / enum / unknown user type — always clone
                // (classes derive Clone, never Copy; enums derive
                // Clone via the auto-derive set).
                let class_hit = self.symbols.classes.contains_key(name.as_str())
                    || self.symbols.classes.keys().any(|k| {
                        k.rsplit('.').next().unwrap_or(k.as_str()) == name.as_str()
                    });
                let enum_hit = self.symbols.enums.contains_key(name.as_str())
                    || self.symbols.enums.keys().any(|k| {
                        k.rsplit('.').next().unwrap_or(k.as_str()) == name.as_str()
                    });
                class_hit || enum_hit
            }
            _ => false,
        }
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
}

/// True when emitting `expr` as the receiver of a method call (e.g.
/// `expr.len()`) requires wrapping it in parentheses to keep the
/// `.` binding correct. Atoms — identifiers, `this`, field-chains,
/// method calls, indexes — bind tighter than `.` already, so
/// they're paren-free. Composite shapes (binary ops, ranges,
/// switch-as-expression, lambdas) bind looser and need the
/// wrapping.
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
