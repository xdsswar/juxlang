//! Jux record declarations → Rust `pub struct` + canonical constructor
//! + auto-derived `Display` impl when every component supports it.
//! Records can also carry operator overrides in their body (per
//! §O.3.4) — both real overrides and the `= delete;` suppression form.

use juxc_ast::OperatorKind;

use crate::analysis::{
    field_supports_copy, field_supports_display, field_supports_eq, field_supports_hash,
};
use crate::RustEmitter;

impl RustEmitter {
    /// Emit a Jux record declaration as a Rust `pub struct` with the
    /// auto-derives that Java records guarantee — Debug/Clone for free
    /// use and `PartialEq` for record-equality. The auto-canonical
    /// constructor lives in an `impl` block as `pub fn new(...)`.
    ///
    /// **Position-aware String handling** mirrors classes: a `String`
    /// component lowers to an owned Rust `String` field, the
    /// constructor parameter is `&str`, and the field init injects
    /// `.to_string()`. Reads of String fields (and generic fields)
    /// auto-`.clone()` via the same machinery — so the user can write
    /// `print(v.x)` without thinking about ownership.
    ///
    /// **`Hash` and `Eq`** are intentionally not derived in Turn 1 —
    /// records carrying `f32`/`f64` components break both. A future
    /// pass can derive them conditionally per component types.
    pub(crate) fn emit_record_decl(&mut self, record_decl: &juxc_ast::RecordDecl) {
        // (Migrated to Writer indent-aware API)
        // Per `JUX-OPERATORS-ADDENDUM.md` §O.3.1 records auto-provide
        // `operator==`, `operator hash`, and copy-on-assignment when
        // their fields permit. The conditional derive list reflects
        // that: Debug/Clone/PartialEq are unconditional, and Eq, Hash,
        // and Copy are added when every component type qualifies.
        //
        // **Deletion (§O.3.4).** `= delete;` operators on the record
        // suppress the corresponding Rust derive — `operator==(...) =
        // delete;` drops `PartialEq` (and `Eq`); `operator hash() =
        // delete;` drops `Hash` (and `Eq`); `operator string()` is a
        // separate impl below and is suppressed there. A user-written
        // operator override that ISN'T `= delete;` doesn't change the
        // derive list — the override goes onto the inherent impl and
        // a trait wrapper bridges to it, same as on classes.
        self.w.line(&record_derive_attribute(record_decl));

        // pub struct Name<T, U> { …components… }
        self.w.emit_indent();
        self.emit_visibility(record_decl.visibility);
        self.w.push_str("struct ");
        self.w.push_str(&record_decl.name.text);
        self.emit_generic_params(&record_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        for comp in &record_decl.components {
            self.w.emit_indent();
            // Records expose their components publicly — matching Java's
            // `record X(int a)` where `x.a()` is part of the API.
            // (Rust public fields are the simplest analog; auto-accessor
            // methods would be polish-only.)
            self.w.push_str("pub ");
            self.w.push_str(&comp.name.text);
            self.w.push_str(": ");
            self.emit_field_type_as_rust(&comp.ty);
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();

        // impl[<T: Clone, U: Clone>] Name<T, U> { pub fn new(…) }
        self.w.emit_indent();
        self.w.push_str("impl");
        self.emit_generic_params_with_clone_bound(&record_decl.generic_params);
        self.w.push(' ');
        self.w.push_str(&record_decl.name.text);
        self.emit_generic_params_as_args(&record_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.emit_indent();
        self.w.push_str("pub fn new(");
        for (i, comp) in record_decl.components.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&comp.name.text);
            self.w.push_str(": ");
            // Post Fix 1 Jux `String` lowers to owned Rust `String`
            // in every position — params included. Field init below
            // is therefore a plain move (`name: name`).
            self.emit_type_as_rust(&comp.ty);
        }
        self.w.push_str(") -> Self {\n");
        self.w.indent_inc();
        self.w.line("Self {");
        self.w.indent_inc();
        for comp in &record_decl.components {
            self.w.emit_indent();
            self.w.push_str(&comp.name.text);
            self.w.push_str(": ");
            self.w.push_str(&comp.name.text);
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        // Now at depth 2 — close `pub fn new(...) -> Self { ... }`.
        self.w.indent_dec();
        self.w.line("}");
        // Depth 1 — inside the `impl Name { ... }` block. Emit
        // inherent operator methods, then user-declared methods.
        // `emit_operator_as_method` skips deleted operators (no
        // inherent method for a `= delete;` declaration).
        for op in &record_decl.operators {
            self.emit_operator_as_method(op);
        }
        // Records can declare methods (per grammar §A.2.4). They
        // share the same emission path as class methods — `emit_method`
        // is host-agnostic.
        for method in &record_decl.methods {
            self.emit_method(method);
        }
        // Close the `impl Name { ... }` block.
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();

        // Auto-derived `operator string` per §O.3.1 — `"Point(x: 1.5, y: 2.7)"`.
        //
        // Skipped when:
        //   - the record is generic (we don't yet thread the
        //     `T: Display` bound through `emit_generic_params_*`), or
        //   - any component's type doesn't support Display (arrays,
        //     nullables, user-defined classes), or
        //   - the user overrode `operator string` (their own Display
        //     impl will be emitted by the trait-wrapper loop below), or
        //   - the user wrote `operator string() = delete;` (§O.3.4
        //     suppression — skip the auto-Display entirely).
        // In the skipped cases the record still gets `Debug` from the
        // derive line above, so `println!("{:?}", r)` keeps working
        // unless `Debug` itself was deleted via `is_deleted` (which
        // would land as a future extension).
        let has_string_override = record_decl
            .operators
            .iter()
            .any(|o| o.kind == OperatorKind::ToString);
        let display_ok = record_decl.generic_params.is_empty()
            && !has_string_override
            && record_decl
                .components
                .iter()
                .all(|c| field_supports_display(&c.ty));
        if display_ok {
            self.emit_record_display_impl(record_decl);
        }

        // Operator trait wrappers — non-generic records only (bound
        // propagation deferred, same as classes). Each non-deleted
        // operator gets a trait impl bridging from `std::ops::Add` /
        // `PartialEq` / `Display` / etc. to the inherent `__op_*`
        // method emitted above. The wrapper emitter is shared with
        // classes; deletion is filtered inside it.
        if record_decl.generic_params.is_empty() {
            for op in &record_decl.operators {
                self.emit_operator_trait_impl(&record_decl.name.text, op);
            }
        }
    }

    /// Generate the `impl std::fmt::Display for Name { … }` block for a
    /// record. Format mirrors §O.3.1's example: `"Name(field: value,
    /// other: value)"`. Empty-component records emit `"Name()"`.
    ///
    /// Called by [`Self::emit_record_decl`] only when
    /// [`field_supports_display`] returns true for every component AND
    /// the record has no generic parameters — keeps the emitted Rust
    /// guaranteed to compile.
    fn emit_record_display_impl(&mut self, record_decl: &juxc_ast::RecordDecl) {
        let name = &record_decl.name.text;
        // Build the format string and arg list in one pass — keeping
        // them in lockstep is important so the `{}` count matches the
        // arg count exactly.
        let mut fmt_body = format!("{name}(");
        let mut args = Vec::new();
        for (i, comp) in record_decl.components.iter().enumerate() {
            if i > 0 {
                fmt_body.push_str(", ");
            }
            fmt_body.push_str(&comp.name.text);
            fmt_body.push_str(": {}");
            args.push(format!("self.{}", comp.name.text));
        }
        fmt_body.push(')');

        self.w.emit_indent();
        self.w.push_str("impl std::fmt::Display for ");
        self.w.push_str(name);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line("fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {");
        self.w.indent_inc();
        self.w.emit_indent();
        if args.is_empty() {
            // Zero-component record — just write the literal name.
            // `write!` accepts a no-arg format string.
            self.w.push_str(&format!("write!(f, \"{fmt_body}\")\n"));
        } else {
            self.w.push_str(&format!(
                "write!(f, \"{fmt_body}\", {})\n",
                args.join(", "),
            ));
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }
}

/// Compute the `#[derive(...)]` attribute line for a record,
/// respecting `= delete;` suppression per §O.3.4.
///
/// Base set per §O.3.1: `Debug, Clone, PartialEq`, plus `Eq`, `Hash`,
/// `Copy` when every component type qualifies (see
/// [`field_supports_eq`] / [`field_supports_hash`] /
/// [`field_supports_copy`]).
///
/// Suppression rules:
/// - `operator==(...) = delete;` drops `PartialEq` (and therefore
///   `Eq`, since `Eq: PartialEq`).
/// - `operator hash() = delete;` drops `Hash` (and `Eq`, since the
///   Eq marker only makes sense alongside hashing).
/// - `operator string()` (override OR delete) does NOT affect the
///   derive list — Display is a separate `impl` emitted outside the
///   derive attribute.
///
/// A user-written override that ISN'T `= delete;` also drops the
/// corresponding auto-derive: when the user wrote
/// `operator==(...) { ... }` we emit `impl PartialEq` from the
/// override and don't want a competing derive.
fn record_derive_attribute(record_decl: &juxc_ast::RecordDecl) -> String {
    let mut derives: Vec<&str> = vec!["Debug", "Clone"];

    let has_eq_op = record_decl
        .operators
        .iter()
        .any(|o| o.kind == OperatorKind::Eq);
    let has_hash_op = record_decl
        .operators
        .iter()
        .any(|o| o.kind == OperatorKind::Hash);
    let eq_deleted = record_decl
        .operators
        .iter()
        .any(|o| o.kind == OperatorKind::Eq && o.is_deleted);
    let hash_deleted = record_decl
        .operators
        .iter()
        .any(|o| o.kind == OperatorKind::Hash && o.is_deleted);

    let component_tys: Vec<&juxc_ast::TypeRef> =
        record_decl.components.iter().map(|c| &c.ty).collect();
    let all_eq = component_tys.iter().all(|t| field_supports_eq(t));
    let all_hash = component_tys.iter().all(|t| field_supports_hash(t));
    let all_copy = component_tys.iter().all(|t| field_supports_copy(t));

    // PartialEq: derived unless the user wrote operator== (override
    // or delete). The user's override path emits its own `impl
    // PartialEq`; `= delete;` opts out entirely.
    if !has_eq_op {
        derives.push("PartialEq");
        if all_eq {
            derives.push("Eq");
        }
    }
    // Hash: derived only when not user-supplied and not deleted. The
    // Eq marker logic on classes lives in `emit_class_decl`; for
    // records the Hash derive only fires when PartialEq is also
    // present (otherwise the Eq derive above is skipped).
    if !has_hash_op && !hash_deleted && !eq_deleted && all_hash {
        derives.push("Hash");
    }
    // Copy: always conditional on field types. Deletion doesn't
    // affect Copy — copy semantics are a value-type property, not an
    // operator-level decision.
    if all_copy {
        derives.push("Copy");
    }
    format!("#[derive({})]", derives.join(", "))
}
