//! Field-access emission — `obj.field` reads, with the auto-`.clone()`
//! and `.length` special cases, plus the supporting type-lookup that
//! decides when a clone is needed.

use juxc_ast::{Expr, FieldExpr};
use juxc_tycheck::Ty;

use crate::exprs::{expr_span_of, ty_kind_from_ref_with_params};
use crate::RustEmitter;

impl RustEmitter {
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
        // Static-field access: `ClassName.X` (or `pkg.Cls.X`) where
        // the path resolves to a known class lowers to Rust's
        // `Path::X` form. Cross-package paths get the same
        // `crate::` rooting `new` uses.
        if let Expr::Path(qn) = &*f.object {
            if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                let cls = self.symbols.classes.get(&class_fqn);
                if let Some(field) = cls.and_then(|c| c.fields.get(f.field.text.as_str())) {
                    if field.is_static {
                        self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                        self.w.push_str("::");
                        self.w.push_str(&f.field.text);
                        return;
                    }
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
}
