//! Jux enum declarations → Rust `pub enum` + auto-Display impl. Enums
//! can also host operator overrides in their body (§O.3.4) — both
//! real overrides and the `= delete;` suppression form, same as
//! records. The natural variant-order semantics cover most use cases
//! so explicit operator overrides on enums are rare; the spec
//! supports them mainly for `operator string() = delete;` on
//! security-sensitive types.

use juxc_ast::OperatorKind;

use crate::analysis::{field_supports_copy, field_supports_eq, field_supports_hash};
use crate::backend_fqn::to_rust_ident;
use crate::RustEmitter;

impl RustEmitter {
    /// Emit a Jux enum declaration as a Rust `pub enum` with auto-derives
    /// and a hand-written `Display` impl per `JUX-LANG-V1.md` §7.7.2:
    /// `"VariantName"` for unit variants, `"VariantName(v1, v2, …)"`
    /// for positional payloads, and `"VariantName(field: v1, …)"`
    /// when the user named the payload slots.
    ///
    /// **Derives.** Per `JUX-OPERATORS-ADDENDUM.md` §O.3.3 sealed enums
    /// auto-provide `operator==`, `operator hash`, and copy-on-assign
    /// — all conditional on their payload types. The conditional
    /// derive list emits `Debug`, `Clone` unconditionally and adds
    /// `PartialEq`, `Eq`, `Hash`, `Copy` when every payload slot
    /// across every variant qualifies. Per §O.3.4, `= delete;` on a
    /// matching operator suppresses the corresponding Rust derive.
    ///
    /// **Display.** The auto-derived `operator string()` destructures
    /// each variant's payload so values are rendered (per spec
    /// §7.7.2). When the user overrides `operator string` we emit
    /// their version instead; when they delete it we skip the
    /// Display impl entirely (the user opted into "this enum has
    /// no default formatting").
    pub(crate) fn emit_enum_decl(&mut self, enum_decl: &juxc_ast::EnumDecl) {
        // **Migrated to the indent-aware `Writer` API as a proof of
        // concept for Phase 2 of the backend-split work.** See git
        // history for the pattern notes.

        // `#[derive(...)] pub enum Name {` — deletion-aware just like
        // records (`record_derive_attribute` shape).
        self.w.line(&enum_derive_attribute(enum_decl));
        self.w.emit_indent();
        self.emit_visibility(enum_decl.visibility);
        self.w.push_str("enum ");
        self.w.push_str(&enum_decl.name.text);
        self.w.push_str(" {\n");

        self.w.indent_inc();
        for variant in &enum_decl.variants {
            self.w.emit_indent();
            self.w.push_str(&variant.name.text);
            if !variant.payload.is_empty() {
                self.w.push('(');
                for (i, slot) in variant.payload.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    // Payload slots act like class fields — owned
                    // values, so reuse the field-type mapping.
                    self.emit_field_type_as_rust(&slot.ty);
                }
                self.w.push(')');
            }
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();

        // If the enum has any non-deleted operators, wrap them in an
        // inherent `impl Name { ... }` block. Otherwise skip the
        // block entirely (matches the historical no-impl-block output
        // for plain enums).
        let has_inherent_ops = enum_decl.operators.iter().any(|o| !o.is_deleted);
        if has_inherent_ops {
            self.w.emit_indent();
            self.w.push_str("impl ");
            self.w.push_str(&enum_decl.name.text);
            self.w.push_str(" {\n");
            for op in &enum_decl.operators {
                self.emit_operator_as_method(op);
            }
            self.w.line("}");
            self.w.newline();
        }

        // Auto `Display` — mirrors Java's `enum.name()`. Skipped when:
        //   - the enum has no variants (uninhabited; can't be
        //     instantiated, so emitting `match self {}` would
        //     trip Rust's E0004 on the `&Empty` borrow at the
        //     formatter boundary — empty enums have no Display
        //     because there's nothing to display),
        //   - the user overrode `operator string` (their wrapper
        //     supplies Display), or
        //   - the user deleted `operator string` (intentional opt-out
        //     for security-sensitive enums).
        let has_string_override = enum_decl
            .operators
            .iter()
            .any(|o| o.kind == OperatorKind::ToString && !o.is_deleted);
        let string_deleted = enum_decl
            .operators
            .iter()
            .any(|o| o.kind == OperatorKind::ToString && o.is_deleted);
        if !enum_decl.variants.is_empty() && !has_string_override && !string_deleted {
            self.emit_enum_auto_display(enum_decl);
        }

        // Operator trait wrappers — Display, PartialEq override,
        // Hash, etc. Deletion is filtered inside the emitter; the
        // class-level emitter pattern is shared.
        for op in &enum_decl.operators {
            self.emit_operator_trait_impl(&enum_decl.name.text, op);
        }
    }

    /// Emit the auto-derived `Display` impl for an enum. Each variant's
    /// payload (if any) is destructured into positional bindings
    /// `f0`, `f1`, … which the format string then renders. If the
    /// user gave a payload slot an explicit name, that name appears
    /// as `name: value` in the printed output (matching the spec's
    /// record-style rendering for payloads). Bindings are routed
    /// through [`to_rust_ident`] so a user-named slot called
    /// `match` lowers to `r#match` and compiles.
    fn emit_enum_auto_display(&mut self, enum_decl: &juxc_ast::EnumDecl) {
        self.w.emit_indent();
        self.w.push_str("impl std::fmt::Display for ");
        self.w.push_str(&enum_decl.name.text);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line("fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {");
        self.w.indent_inc();
        self.w.line("match self {");
        self.w.indent_inc();
        for variant in &enum_decl.variants {
            self.w.emit_indent();
            self.w.push_str(&enum_decl.name.text);
            self.w.push_str("::");
            self.w.push_str(&variant.name.text);

            // Build the destructure bindings and the format spec /
            // argument list in one pass. Synthesized positional
            // names (`f0`, `f1`, …) are always safe; user-named
            // slots go through `to_rust_ident` so reserved words
            // get the `r#` raw-identifier prefix.
            let n = variant.payload.len();
            if n > 0 {
                self.w.push('(');
                for i in 0..n {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.w.push_str(&format!("f{i}"));
                }
                self.w.push(')');
            }

            self.w.push_str(" => ");
            if n == 0 {
                self.w.push_str("write!(f, \"");
                self.w.push_str(&variant.name.text);
                self.w.push_str("\"),\n");
            } else {
                self.w.push_str("write!(f, \"");
                self.w.push_str(&variant.name.text);
                self.w.push('(');
                for (i, slot) in variant.payload.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    if let Some(name) = &slot.name {
                        self.w.push_str(&to_rust_ident(&name.text));
                        self.w.push_str(": {}");
                    } else {
                        self.w.push_str("{}");
                    }
                }
                self.w.push_str(")\"");
                for i in 0..n {
                    self.w.push_str(", ");
                    self.w.push_str(&format!("f{i}"));
                }
                self.w.push_str("),\n");
            }
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }
}

/// Compute the `#[derive(...)]` attribute line for an enum, respecting
/// `= delete;` suppression per §O.3.4. Same shape as the equivalent
/// helper for records — kept separate because the spec's wording
/// applies independently to each value-type kind and an enum-specific
/// helper makes the derives easier to evolve.
fn enum_derive_attribute(enum_decl: &juxc_ast::EnumDecl) -> String {
    let mut derives: Vec<&str> = vec!["Debug", "Clone"];

    let has_eq_op = enum_decl
        .operators
        .iter()
        .any(|o| o.kind == OperatorKind::Eq);
    let has_hash_op = enum_decl
        .operators
        .iter()
        .any(|o| o.kind == OperatorKind::Hash);
    let eq_deleted = enum_decl
        .operators
        .iter()
        .any(|o| o.kind == OperatorKind::Eq && o.is_deleted);
    let hash_deleted = enum_decl
        .operators
        .iter()
        .any(|o| o.kind == OperatorKind::Hash && o.is_deleted);

    let payload_tys: Vec<&juxc_ast::TypeRef> = enum_decl
        .variants
        .iter()
        .flat_map(|v| v.payload.iter().map(|p| &p.ty))
        .collect();
    let all_eq = payload_tys.iter().all(|t| field_supports_eq(t));
    let all_hash = payload_tys.iter().all(|t| field_supports_hash(t));
    let all_copy = payload_tys.iter().all(|t| field_supports_copy(t));

    if !has_eq_op {
        derives.push("PartialEq");
        if all_eq {
            derives.push("Eq");
        }
    }
    if !has_hash_op && !hash_deleted && !eq_deleted && all_hash {
        derives.push("Hash");
    }
    if all_copy {
        derives.push("Copy");
    }
    format!("#[derive({})]", derives.join(", "))
}
