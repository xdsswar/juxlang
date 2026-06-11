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
        // `enum Name<T, U>` — generic parameters per §A.2.4.
        self.emit_generic_params(&enum_decl.generic_params);
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
        let has_members = !enum_decl.methods.is_empty() || !enum_decl.constants.is_empty();
        if has_inherent_ops || has_members {
            self.w.emit_indent();
            self.w.push_str("impl");
            // The Clone bound mirrors classes: enum `&self` method
            // bodies clone the receiver for `switch (this)` dispatch
            // (owned payload binders), and derived `Clone` on the
            // enum needs `T: Clone` anyway.
            self.emit_generic_params_with_clone_bound(&enum_decl.generic_params);
            self.w.push(' ');
            self.w.push_str(&enum_decl.name.text);
            self.emit_generic_params_as_args(&enum_decl.generic_params);
            self.w.push_str(" {\n");
            for op in &enum_decl.operators {
                self.emit_operator_as_method(op);
            }
            // Enum CONSTANTS (§A.2.5) — associated consts (same
            // `pub const` shape static-final class fields use).
            for c in &enum_decl.constants {
                self.emit_static_field(c);
            }
            // Enum METHODS (§A.2.5) — `this` is the enum VALUE
            // (`&self` receiver); bodies typically dispatch via
            // `switch (this)`. Enums are plain Rust value enums, so
            // none of the wrapper machinery threads here.
            for method in &enum_decl.methods {
                self.emit_enum_method(method);
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
        // Auto-Display is skipped for **generic** enums (same convention as
        // generic records, `generic_record_skips_display_for_now`): a correct
        // `impl<T> Display` would need `T: Display` bounds derived per payload,
        // which the conditional-derive machinery doesn't yet compute. A generic
        // enum still lowers, prints via `Debug`, and is fully usable; the
        // value-rendering Display lands with the bound-inference work.
        if !enum_decl.variants.is_empty()
            && !has_string_override
            && !string_deleted
            && enum_decl.generic_params.is_empty()
        {
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

impl crate::RustEmitter {
    /// Emit one ENUM METHOD (§A.2.5) as an inherent `fn` on the Rust
    /// enum. Mirrors `emit_operator_as_method`'s receiver/body
    /// discipline: `this` aliases `self`, the receiver is `&self`
    /// (`&mut self` when the body writes through `this` — rare on a
    /// value enum, but `this = …`-style reassignment isn't a thing, so
    /// in practice this stays `&self`), static methods drop the
    /// receiver entirely.
    pub(crate) fn emit_enum_method(&mut self, method: &juxc_ast::FnDecl) {
        use juxc_ast::ReturnType;
        let body = method.body.as_ref();
        let is_static = method
            .modifiers
            .iter()
            .any(|m| matches!(m, juxc_ast::FnModifier::Static));

        self.w.indent_inc();
        self.w.emit_indent();
        if matches!(method.return_type, ReturnType::AsyncType(_)) {
            self.w.push_str("pub async fn ");
        } else {
            self.w.push_str("pub fn ");
        }
        self.w.push_str(&method.name.text);
        self.w.push('(');
        if !is_static {
            self.w.push_str("&self");
        }
        for (i, param) in method.params.iter().enumerate() {
            if i > 0 || !is_static {
                self.w.push_str(", ");
            }
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_value_type_as_rust(&param.ty);
        }
        self.w.push(')');
        match &method.return_type {
            ReturnType::Void => {}
            ReturnType::Type(t) | ReturnType::AsyncType(t) => {
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
        }
        self.w.push_str(" {\n");
        self.w.indent_inc();
        if let Some(body) = body {
            let prev_alias = self.this_alias.take();
            let prev_enum_method = self.in_enum_method;
            self.in_enum_method = true;
            if !is_static {
                self.this_alias = Some("self".to_string());
            }
            let mut muts = std::collections::HashSet::new();
            crate::analysis::collect_mutated_names(body, &mut muts, &self.user_mut_methods);
            self.mutated_in_fn = muts;
            self.current_fn_params =
                method.params.iter().map(|p| p.name.text.clone()).collect();
            let saved = self.current_return_type.take();
            self.current_return_type = Some(method.return_type.clone());
            self.emit_fn_body_at(body, &method.return_type);
            self.current_return_type = saved;
            self.current_fn_params.clear();
            self.this_alias = prev_alias;
            self.in_enum_method = prev_enum_method;
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }
}
