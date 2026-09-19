//! Jux enum declarations → Rust `pub enum` + auto-Display impl. Enums
//! can also host operator overrides in their body (§O.3.4) — both
//! real overrides and the `= delete;` suppression form, same as
//! records. The natural variant-order semantics cover most use cases
//! so explicit operator overrides on enums are rare; the spec
//! supports them mainly for `operator string() = delete;` on
//! security-sensitive types.

use juxc_ast::OperatorKind;

use crate::analysis::{field_supports_copy, field_supports_eq};
use crate::backend_fqn::to_rust_ident;
use std::collections::HashSet;

/// The C repr type for a `@layout(c, repr = "…")` enum (§L.1.3), or `None` when
/// the enum is not a C enum. `@layout(c)` with no explicit `repr` defaults to
/// `i32` (the C `int` width). Mirrors the `@layout(c)` detection used for value
/// structs (`is_layout_c_struct`).
fn layout_c_enum_repr(annotations: &[juxc_ast::Annotation]) -> Option<String> {
    use juxc_ast::{AnnotationArg, Expr, Literal};
    let ann = annotations.iter().find(|a| {
        a.name
            .segments
            .last()
            .map(|s| s.text.eq_ignore_ascii_case("layout"))
            .unwrap_or(false)
    })?;
    let has_c = ann.args.iter().any(|arg| {
        matches!(arg,
            AnnotationArg::Positional(Expr::Path(qn))
                if qn.segments.last()
                    .map(|s| s.text.eq_ignore_ascii_case("c"))
                    .unwrap_or(false))
    });
    if !has_c {
        return None;
    }
    let repr = ann
        .args
        .iter()
        .find_map(|arg| match arg {
            AnnotationArg::Named { name, value } if name.text.eq_ignore_ascii_case("repr") => {
                if let Expr::Literal(Literal::String(s)) = value {
                    Some(s.clone())
                } else {
                    None
                }
            }
            _ => None,
        })
        .unwrap_or_else(|| "i32".to_string());
    Some(repr)
}

/// True when an enum variant payload slot is **self-referential** — its head
/// type is the very enum being declared, e.g. `Branch(Tree, Tree)` inside
/// `enum Tree`, or `Branch(Tree<T>, Tree<T>)` inside `enum Tree<T>`. Such a slot
/// embeds the enum by value, so the emitted Rust enum is infinitely sized
/// (rustc E0072) unless the WHOLE slot is `Box`ed.
///
/// We box only when the enum is the slot's *direct head* and the value is
/// otherwise unindirected:
/// - generic args are allowed (`Tree<T>` is still by-value recursion → box);
/// - an array (`Tree[]` → `Vec<Tree>`), pointer (`Tree*`), or function-typed
///   slot is already sized/indirected, so it is NOT boxed;
/// - a slot whose head is a *different* generic carrying the enum (`Vec<Tree>`)
///   has head `Vec`, not `Tree`, so the name check already excludes it.
///
/// A trailing `?` (nullable self-reference → `Option<Tree>`, also infinite) is
/// left unboxed here: correctly fixing it needs boxing the inner type
/// (`Option<Box<Tree>>`), a different emission shape; the idiomatic Jux linked
/// structure uses a class (`Rc`) reference instead.
///
/// Construction (`Box::new(arg)`) and pattern binders (`*binder` on use) mirror
/// exactly this predicate so the three sites stay type-consistent.
pub(crate) fn is_recursive_enum_slot(slot_ty: &juxc_ast::TypeRef, enum_name: &str) -> bool {
    slot_ty.array_shape.is_none()
        && !slot_ty.nullable
        && slot_ty.ptr_depth == 0
        && slot_ty.fn_shape.is_none()
        && slot_ty
            .name
            .segments
            .last()
            .map(|s| s.text.as_str())
            == Some(enum_name)
}
use crate::RustEmitter;

impl RustEmitter {

    /// The enum's type parameters that need a `std::fmt::Display` bound —
    /// those whose values reach a **format position** in one of its methods.
    ///
    /// The class version of this reads fields; an enum's values arrive instead
    /// through a `switch (this)` pattern binder, so this maps each binder to
    /// the variant payload it destructures and marks the parameter when that
    /// binder is formatted. Without it, `case Result.Err(var e) -> "err:" + e`
    /// resolved to `Debug` and printed a `String` with quotes around it —
    /// wrong output, not an error.
    fn enum_displayed_generic_params(
        &self,
        enum_decl: &juxc_ast::EnumDecl,
    ) -> HashSet<String> {
        let mut displayed: HashSet<String> = HashSet::new();
        if enum_decl.generic_params.is_empty() {
            return displayed;
        }
        let params: HashSet<&str> = enum_decl
            .generic_params
            .iter()
            .map(|p| p.name.text.as_str())
            .collect();
        // Binder name → the enum type param it is bound to, across every
        // `case Variant(var x)` in the enum's own bodies. A binder for a
        // payload of a concrete type contributes nothing.
        let mut binders: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for m in &enum_decl.methods {
            let Some(body) = &m.body else { continue };
            Self::collect_enum_pattern_binders(body, enum_decl, &params, &mut binders);
        }
        if binders.is_empty() {
            return displayed;
        }
        for m in &enum_decl.methods {
            if let Some(body) = &m.body {
                Self::scan_block_for_displayed_fields(body, &binders, &mut displayed);
            }
        }
        displayed
    }

    /// Walk an enum method body for `case <Variant>(var x)` patterns, recording
    /// each binder that destructures a payload typed as one of the enum's own
    /// parameters.
    fn collect_enum_pattern_binders(
        block: &juxc_ast::Block,
        enum_decl: &juxc_ast::EnumDecl,
        params: &HashSet<&str>,
        out: &mut std::collections::HashMap<String, String>,
    ) {
        use juxc_ast::{Expr, Pattern, Stmt};
        fn from_pattern(
            p: &Pattern,
            enum_decl: &juxc_ast::EnumDecl,
            params: &HashSet<&str>,
            out: &mut std::collections::HashMap<String, String>,
        ) {
            let Pattern::EnumVariant { path, args, .. } = p else { return };
            let Some(variant_name) = path.segments.last() else { return };
            let Some(variant) = enum_decl
                .variants
                .iter()
                .find(|v| v.name.text == variant_name.text)
            else {
                return;
            };
            for (arg, payload) in args.iter().zip(&variant.payload) {
                let Pattern::Bind(name) = arg else { continue };
                if payload.ty.generic_args.is_empty()
                    && payload.ty.array_shape.is_none()
                    && payload.ty.name.segments.len() == 1
                {
                    let head = payload.ty.name.segments[0].text.as_str();
                    if params.contains(head) {
                        out.insert(name.text.clone(), head.to_string());
                    }
                }
            }
        }
        fn scan_expr(
            e: &Expr,
            enum_decl: &juxc_ast::EnumDecl,
            params: &HashSet<&str>,
            out: &mut std::collections::HashMap<String, String>,
        ) {
            if let Expr::Switch(sw) = e {
                for arm in &sw.arms {
                    from_pattern(&arm.pattern, enum_decl, params, out);
                    match &arm.body {
                        juxc_ast::SwitchBody::Expr(b) => scan_expr(b, enum_decl, params, out),
                        juxc_ast::SwitchBody::Block(b) => {
                            RustEmitter::collect_enum_pattern_binders(b, enum_decl, params, out)
                        }
                    }
                }
            }
        }
        for stmt in &block.statements {
            match stmt {
                Stmt::Expr(e) | Stmt::Return(Some(e), _) => scan_expr(e, enum_decl, params, out),
                Stmt::VarDecl(v) => {
                    if let Some(init) = &v.init {
                        scan_expr(init, enum_decl, params, out);
                    }
                }
                _ => {}
            }
        }
    }
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
        let hash_plan = self.enum_hash_plan(enum_decl);
        self.w.line(&enum_derive_attribute(enum_decl, hash_plan));
        // `@layout(c, repr = "…")` (§L.1.3): a C-compatible integer enum. Emit
        // `#[repr(<int>)]` so the value is bit-identical to a C `int` enum; the
        // explicit per-variant discriminants are emitted below.
        let c_enum_repr = layout_c_enum_repr(&enum_decl.annotations);
        if let Some(repr) = &c_enum_repr {
            self.w.line(&format!("#[repr({repr})]"));
        }
        self.w.emit_indent();
        self.emit_visibility(enum_decl.visibility);
        self.w.push_str("enum ");
        self.w.push_str(&to_rust_ident(&enum_decl.name.text));
        // `enum Name<T, U>` — generic parameters per §A.2.4.
        self.emit_generic_params(&enum_decl.generic_params);
        self.w.push_str(" {\n");

        self.w.indent_inc();
        // The variant `#[derive(Default)]` above points at: the first one
        // without a payload.
        let default_variant = enum_decl
            .variants
            .iter()
            .find(|v| v.payload.is_empty())
            .map(|v| v.name.text.clone());
        for variant in &enum_decl.variants {
            if default_variant.as_deref() == Some(variant.name.text.as_str()) {
                self.w.line("#[default]");
            }
            self.w.emit_indent();
            self.w.push_str(&to_rust_ident(&variant.name.text));
            if !variant.payload.is_empty() {
                self.w.push('(');
                for (i, slot) in variant.payload.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    // Payload slots act like class fields — owned values, so
                    // reuse the field-type mapping. A **self-referential** slot
                    // (the payload type is this same enum — `Branch(Tree, Tree)`)
                    // must be `Box`ed, or the Rust enum is infinitely sized
                    // (E0072). Construction (`Box::new`) and match (deref binder)
                    // mirror this.
                    let boxed = is_recursive_enum_slot(&slot.ty, &enum_decl.name.text);
                    if boxed {
                        self.w.push_str("std::boxed::Box<");
                    }
                    self.emit_field_type_as_rust(&slot.ty);
                    if boxed {
                        self.w.push('>');
                    }
                }
                self.w.push(')');
            }
            // C enum discriminant — `Ok = 200` (§L.1.3). Only on a `@layout(c)`
            // enum, and only for a const-evaluable value.
            if c_enum_repr.is_some() {
                if let Some(disc) = &variant.discriminant {
                    if let Some(v) = self.try_const_int(disc) {
                        self.w.push_str(&format!(" = {v}"));
                    }
                }
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
        // Every enum with variants also carries the §7.7.3 helpers, so the
        // block opens for those as well -- otherwise `Tier.Bronze.name()`
        // would type-check against a signature with nothing behind it.
        let has_auto_helpers = !enum_decl.variants.is_empty();
        if has_inherent_ops || has_members || has_auto_helpers {
            self.w.emit_indent();
            self.w.push_str("impl");
            // The Clone bound mirrors classes: enum `&self` method
            // bodies clone the receiver for `switch (this)` dispatch
            // (owned payload binders), and derived `Clone` on the
            // enum needs `T: Clone` anyway.
            let displayed = self.enum_displayed_generic_params(enum_decl);
            let params = enum_decl.generic_params.clone();
            // Same key-bound rule as a class: a variant payload or method
            // signature that keys a map by a type param needs `Eq + Hash`
            // (or `Ord`) on the impl.
            let declared: Vec<juxc_ast::TypeRef> = enum_decl
                .variants
                .iter()
                .flat_map(|v| v.payload.iter().map(|p| p.ty.clone()))
                .chain(enum_decl.methods.iter().flat_map(|m| {
                    m.params.iter().map(|p| p.ty.clone()).chain(
                        match &m.return_type {
                            juxc_ast::ReturnType::Type(t)
                            | juxc_ast::ReturnType::AsyncType(t) => Some(t.clone()),
                            juxc_ast::ReturnType::Void => None,
                        },
                    )
                }))
                .collect();
            self.collect_key_bound_params(&params, declared.iter());
            self.emit_generic_params_with_clone_bound_plus_display(
                &params,
                &displayed,
                &HashSet::new(),
            );
            self.w.push(' ');
            self.w.push_str(&to_rust_ident(&enum_decl.name.text));
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
            // A Java-style enum's methods read its per-variant fields
            // (§7.7.4) through `__field`, bare names included.
            let java_style = !enum_decl.fields.is_empty() && !enum_decl.constructors.is_empty();
            let prev_fields = std::mem::replace(
                &mut self.enclosing_enum_fields,
                enum_decl
                    .fields
                    .iter()
                    .filter_map(|f| f.ty.clone().map(|t| (f.name.text.clone(), t)))
                    .collect(),
            );
            if java_style {
                self.w.indent_inc();
                self.emit_enum_field_methods(enum_decl);
                self.w.indent_dec();
            }
            for method in &enum_decl.methods {
                self.emit_enum_method(method);
            }
            self.enclosing_enum_fields = prev_fields;
            self.emit_enum_auto_helpers(enum_decl);
            self.w.line("}");
            self.w.newline();
            if java_style {
                self.emit_enum_field_table(enum_decl);
            }
        }

        // `impl <Iface> for <Enum>` per interface the enum declares (§A.2.5).
        self.emit_enum_trait_impls(enum_decl);

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
        if !enum_decl.variants.is_empty() && !has_string_override && !string_deleted {
            self.emit_enum_auto_display(enum_decl);
        }
        // A value's identity is its own address; interfaces it implements
        // require the impl (see the prelude's `JuxIdentity`).
        self.emit_jux_identity_impl(&enum_decl.name.text, &enum_decl.generic_params, "self as *const Self");

        // Operator trait wrappers — Display, PartialEq override,
        // Hash, etc. Deletion is filtered inside the emitter; the
        // class-level emitter pattern is shared.
        for op in &enum_decl.operators {
            self.emit_operator_trait_impl(&enum_decl.name.text, op);
        }
        // `Hash` by hand when a float payload rules out the derive, and the
        // `Eq` promise a hash key needs when it was not derived (§O.3.1).
        if hash_plan.manual_hash {
            self.emit_value_hash_for_enum(enum_decl);
        }
        if hash_plan.eq_marker {
            self.emit_value_eq_marker(&enum_decl.name.text, &enum_decl.generic_params);
        }
    }

    /// The enum's `Eq` / `Hash` plan: the payloads are its components.
    fn enum_hash_plan(&self, enum_decl: &juxc_ast::EnumDecl) -> crate::decls::hashing::HashPlan {
        let pkg = self
            .current_unit_idx
            .and_then(|i| self.symbols.units.get(i))
            .map(|unit| unit.package.join("."))
            .unwrap_or_default();
        let fqn = if pkg.is_empty() {
            enum_decl.name.text.clone()
        } else {
            format!("{pkg}.{}", enum_decl.name.text)
        };
        let payloads: Vec<&juxc_ast::TypeRef> =
            enum_decl.variants.iter().flat_map(|v| v.payload.iter().map(|p| &p.ty)).collect();
        let legacy_eq = payloads.iter().all(|t| field_supports_eq(t));
        self.value_hash_plan(&fqn, &payloads, &enum_decl.operators, legacy_eq)
    }

    /// Emit one delegating `impl <Iface> for <Enum>` per interface the enum
    /// declares (§A.2.5 allows `implements` on an enum; an enum is implicitly
    /// final and has no `extends`, so this is its whole supertype list).
    ///
    /// The shape is the record's ([`Self::emit_record_trait_impls`]) minus the
    /// component-accessor case: an enum has no components, so a method the enum
    /// declares delegates to the inherent one and anything else is left to the
    /// interface's own default body.
    fn emit_enum_trait_impls(&mut self, enum_decl: &juxc_ast::EnumDecl) {
        for interface_ty in &enum_decl.implements {
            let Some(iface_name) = interface_ty.name.segments.last() else { continue };
            let Some((_, iface)) = self.lookup_interface_by_bare_or_fqn(&iface_name.text) else {
                continue;
            };
            // `implements Keyed<String>` binds the interface's params to the
            // enum's arguments; installing it as the Kind substitution makes
            // every type emitted below read in the enum's vocabulary.
            let subst: std::collections::HashMap<String, juxc_ast::TypeRef> = iface
                .generic_params
                .iter()
                .zip(interface_ty.generic_args.iter())
                .filter_map(|(p, a)| a.as_type().map(|t| (p.name.text.clone(), t.clone())))
                .collect();
            let mut methods: Vec<(String, juxc_tycheck::symbol_table::MethodSig)> = iface
                .methods
                .iter()
                .filter(|(_, m)| !m.is_static)
                .map(|(n, m)| (n.clone(), m.clone()))
                .collect();
            methods.sort_by(|a, b| a.0.cmp(&b.0));

            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params_with_clone_bound(&enum_decl.generic_params);
            self.w.push(' ');
            self.emit_type_as_rust(interface_ty);
            self.w.push_str(" for ");
            self.w.push_str(&to_rust_ident(&enum_decl.name.text));
            self.emit_generic_params_as_args(&enum_decl.generic_params);

            let saved = std::mem::replace(&mut self.kind_type_subst, subst);
            let provided: Vec<(String, juxc_tycheck::symbol_table::MethodSig)> = methods
                .into_iter()
                .filter(|(name, _)| enum_decl.methods.iter().any(|m| &m.name.text == name))
                .collect();
            if provided.is_empty() {
                self.w.push_str(" {}\n\n");
            } else {
                self.w.push_str(" {\n");
                self.w.indent_inc();
                for (name, sig) in &provided {
                    self.emit_kind_delegating_method(&enum_decl.name.text, name, sig);
                }
                self.w.indent_dec();
                self.w.emit_indent();
                self.w.push_str("}\n\n");
            }
            self.kind_type_subst = saved;
        }
    }
    /// The auto-derived helpers of JUX-LANG-V1 §7.7.3: `name()`, `ordinal()`
    /// and, for a payload-free enum, the static `values()`.
    ///
    /// Emitted from the variant list, which is the same list the symbol table
    /// synthesized the signatures from -- an enum that grows a variant grows
    /// all three. A user declaration of one of these names wins: the symbol
    /// table keeps theirs, and this skips any name already declared.
    fn emit_enum_auto_helpers(&mut self, enum_decl: &juxc_ast::EnumDecl) {
        if enum_decl.variants.is_empty() {
            return;
        }
        let declared: HashSet<&str> =
            enum_decl.methods.iter().map(|m| m.name.text.as_str()).collect();
        let bare = to_rust_ident(&enum_decl.name.text);

        // A variant's pattern needs its payload elided: `Shape::Rect(..)`.
        let pattern = |v: &juxc_ast::EnumVariant| -> String {
            let name = to_rust_ident(&v.name.text);
            if v.payload.is_empty() {
                format!("{bare}::{name}")
            } else {
                format!("{bare}::{name}(..)")
            }
        };

        if !declared.contains("name") {
            self.w.line("pub fn name(&self) -> String {");
            self.w.indent_inc();
            self.w.line("match self {");
            self.w.indent_inc();
            for v in &enum_decl.variants {
                self.w
                    .line(&format!("{} => \"{}\".to_string(),", pattern(v), v.name.text));
            }
            self.w.indent_dec();
            self.w.line("}");
            self.w.indent_dec();
            self.w.line("}");
        }

        if !declared.contains("ordinal") {
            self.w.line("pub fn ordinal(&self) -> isize {");
            self.w.indent_inc();
            self.w.line("match self {");
            self.w.indent_inc();
            for (i, v) in enum_decl.variants.iter().enumerate() {
                self.w.line(&format!("{} => {i},", pattern(v)));
            }
            self.w.indent_dec();
            self.w.line("}");
            self.w.indent_dec();
            self.w.line("}");
        }

        // §7.7.3 restricts `values()` to payload-free enums: a variant with a
        // payload cannot be enumerated without inventing one.
        let payload_free = enum_decl.variants.iter().all(|v| v.payload.is_empty());
        if payload_free && !declared.contains("values") {
            // A plain `Vec`, not a handle: the call site wraps an array-typed
            // result itself, and returning a handle here made it wrap twice --
            // `crate::jux_arr(Tier::values())` around something that already
            // was one, which no `for-each` could iterate.
            self.w
                .line(&format!("pub fn values() -> std::vec::Vec<{bare}> {{"));
            self.w.indent_inc();
            let items = enum_decl
                .variants
                .iter()
                .map(|v| format!("{bare}::{}", to_rust_ident(&v.name.text)))
                .collect::<Vec<_>>()
                .join(", ");
            self.w.line(&format!("std::vec![{items}]"));
            self.w.indent_dec();
            self.w.line("}");
        }

        // `fromName` / `fromNameStrict` / `fromOrdinal` (§7.7.3): the reverse
        // of `name()` and `ordinal()`, `None` on a miss. Payload-free enums
        // only, like `values()`.
        let self_ty = if enum_decl.generic_params.is_empty() {
            bare.clone()
        } else {
            let params: Vec<String> =
                enum_decl.generic_params.iter().map(|p| to_rust_ident(&p.name.text)).collect();
            format!("{bare}<{}>", params.join(", "))
        };
        let unit = |v: &juxc_ast::EnumVariant| format!("{bare}::{}", to_rust_ident(&v.name.text));
        if payload_free && !declared.contains("fromNameStrict") {
            self.w.line(&format!("pub fn fromNameStrict(name: String) -> Option<{self_ty}> {{"));
            self.w.indent_inc();
            self.w.line("match name.as_str() {");
            self.w.indent_inc();
            for v in &enum_decl.variants {
                self.w.line(&format!("\"{}\" => Some({}),", v.name.text, unit(v)));
            }
            self.w.line("_ => None,");
            self.w.indent_dec();
            self.w.line("}");
            self.w.indent_dec();
            self.w.line("}");
        }
        if payload_free && !declared.contains("fromName") {
            // Case-insensitive, so `"north"` finds `North`. When two variants
            // differ only in case (`Red`, `RED`), the exact spelling decides,
            // and otherwise the first declared wins: the lookup never guesses
            // between two equally good answers.
            let mut seen = HashSet::new();
            let folded: Vec<(String, &juxc_ast::EnumVariant)> = enum_decl
                .variants
                .iter()
                .filter_map(|v| {
                    let key = v.name.text.to_lowercase();
                    seen.insert(key.clone()).then_some((key, v))
                })
                .collect();
            let collides = folded.len() < enum_decl.variants.len();
            self.w.line(&format!("pub fn fromName(name: String) -> Option<{self_ty}> {{"));
            self.w.indent_inc();
            if collides {
                self.w.line("match name.as_str() {");
                self.w.indent_inc();
                for v in &enum_decl.variants {
                    self.w.line(&format!("\"{}\" => return Some({}),", v.name.text, unit(v)));
                }
                self.w.line("_ => {}");
                self.w.indent_dec();
                self.w.line("}");
            }
            self.w.line("match name.to_lowercase().as_str() {");
            self.w.indent_inc();
            for (key, v) in &folded {
                self.w.line(&format!("\"{key}\" => Some({}),", unit(v)));
            }
            self.w.line("_ => None,");
            self.w.indent_dec();
            self.w.line("}");
            self.w.indent_dec();
            self.w.line("}");
        }
        if payload_free && !declared.contains("fromOrdinal") {
            self.w.line(&format!("pub fn fromOrdinal(ordinal: isize) -> Option<{self_ty}> {{"));
            self.w.indent_inc();
            self.w.line("match ordinal {");
            self.w.indent_inc();
            for (i, v) in enum_decl.variants.iter().enumerate() {
                self.w.line(&format!("{i} => Some({}),", unit(v)));
            }
            self.w.line("_ => None,");
            self.w.indent_dec();
            self.w.line("}");
            self.w.indent_dec();
            self.w.line("}");
        }

        // `cases()` (§7.7.3): one `EnumCase` per variant, on every enum. A
        // payload-free variant carries itself as `value()`; a payload variant
        // is described by its declared payload and has no value.
        if !declared.contains("cases") && enum_decl.generic_params.is_empty() {
            let case = "crate::jux::std::meta::EnumCase";
            self.w.line(&format!(
                "pub fn cases() -> crate::JuxArr<std::vec::Vec<{case}<{self_ty}>>> {{"
            ));
            self.w.indent_inc();
            self.w.line("crate::jux_arr(std::vec![");
            self.w.indent_inc();
            for (i, v) in enum_decl.variants.iter().enumerate() {
                let (payload, value) = if v.payload.is_empty() {
                    (String::new(), format!("Some({})", unit(v)))
                } else {
                    let slots: Vec<String> = v
                        .payload
                        .iter()
                        .map(|p| {
                            let ty = juxc_tycheck::symbol_table::render_type_ref(&p.ty);
                            match &p.name {
                                Some(n) => format!("{ty} {}", n.text),
                                None => ty,
                            }
                        })
                        .collect();
                    (format!("({})", slots.join(", ")), "None".to_string())
                };
                self.w.line(&format!(
                    "{case}::new(\"{}\".to_string(), {i}, \"{}\".to_string(), {value}),",
                    v.name.text,
                    payload.replace('\\', "\\\\").replace('"', "\\\""),
                ));
            }
            self.w.indent_dec();
            self.w.line("])");
            self.w.indent_dec();
            self.w.line("}");
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
        self.w.push_str("impl");
        // A GENERIC enum gets the impl too, bounding each parameter it uses as
        // a bare PAYLOAD type -- those are the ones the arms format. Skipping
        // the impl left the enum printable only through `Debug`, and left it
        // unable to be another generic's type argument, since a formatted
        // parameter carries a `Display` bound (§T.2.1).
        if !enum_decl.generic_params.is_empty() {
            let payloads: Vec<&juxc_ast::TypeRef> = enum_decl
                .variants
                .iter()
                .flat_map(|v| v.payload.iter().map(|p| &p.ty))
                .collect();
            let displayed = crate::analysis::displayed_bare_params(
                &enum_decl.generic_params,
                payloads.into_iter(),
            );
            let none: std::collections::HashSet<String> = std::collections::HashSet::new();
            self.emit_generic_params_with_clone_bound_plus_display(
                &enum_decl.generic_params,
                &displayed,
                &none,
            );
        }
        self.w.push_str(" std::fmt::Display for ");
        self.w.push_str(&to_rust_ident(&enum_decl.name.text));
        self.emit_generic_params_as_args(&enum_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line("fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {");
        self.w.indent_inc();
        self.w.line("match self {");
        self.w.indent_inc();
        for variant in &enum_decl.variants {
            self.w.emit_indent();
            self.w.push_str(&to_rust_ident(&enum_decl.name.text));
            self.w.push_str("::");
            self.w.push_str(&to_rust_ident(&variant.name.text));

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
            // Inside the format string the names are TEXT, printed as the user
            // wrote them: a `r#` raw-identifier prefix belongs to Rust paths,
            // not to output (Codegen Fix 4).
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
                        self.w.push_str(&name.text);
                        self.w.push_str(": ");
                    }
                    self.w.push_str("{}");
                }
                self.w.push_str(")\"");
                for (i, slot) in variant.payload.iter().enumerate() {
                    self.w.push_str(", ");
                    // A float payload prints the way every Jux float does, as
                    // in a record.
                    if crate::analysis::type_ref_is_float(&slot.ty) {
                        self.w.push_str(&format!("crate::jux_float(f{i})"));
                    } else {
                        self.w.push_str(&format!("f{i}"));
                    }
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
fn enum_derive_attribute(enum_decl: &juxc_ast::EnumDecl, hash_plan: crate::decls::hashing::HashPlan) -> String {
    let mut derives: Vec<&str> = vec!["Debug", "Clone"];

    let has_eq_op = enum_decl
        .operators
        .iter()
        .any(|o| o.kind == OperatorKind::Eq);
    let payload_tys: Vec<&juxc_ast::TypeRef> = enum_decl
        .variants
        .iter()
        .flat_map(|v| v.payload.iter().map(|p| &p.ty))
        .collect();
    let all_copy = payload_tys.iter().all(|t| field_supports_copy(t));

    if !has_eq_op {
        derives.push("PartialEq");
    }
    // Eq and Hash follow the shared hash plan (§O.3.1); a float payload is
    // hashed by hand after the declaration instead.
    if hash_plan.derive_eq {
        derives.push("Eq");
    }
    if hash_plan.derive_hash {
        derives.push("Hash");
    }
    if all_copy {
        derives.push("Copy");
    }
    // A field of enum type with no initializer is seeded with the type's
    // default before the constructor body assigns it, so the enum needs one.
    // Rust derives `Default` for an enum only when a variant is marked, so the
    // first PAYLOAD-FREE variant is marked below; an enum whose variants all
    // carry payloads has no sensible default and gets none.
    if enum_decl.variants.iter().any(|v| v.payload.is_empty()) {
        derives.push("Default");
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
    /// The Java-style form's runtime (JUX-LANG-V1 §7.7.4, ERRATA E34), for an
    /// enum with per-variant fields. The Rust enum keeps plain unit variants;
    /// the values live in a table beside it:
    ///
    /// ```text
    /// struct Planet__Fields { mass: f64, radius: f64 }
    /// thread_local! { static PLANET__FIELDS: Vec<Planet__Fields> = vec![
    ///     Planet::__new_fields(3.303e23, 2.4397e6),   // Mercury
    ///     ... ] }
    /// ```
    ///
    /// The table is built on first use, one row per variant in declaration
    /// order, so each constructor call runs once, as Java's do when the enum
    /// is first used. `__new_fields` is the constructor, `__field` reads a
    /// value of `self`'s row. Called while the `impl` block is open (for the
    /// methods) and again after it (for the struct and the table).
    pub(crate) fn emit_enum_field_methods(&mut self, enum_decl: &juxc_ast::EnumDecl) {
        let bare = to_rust_ident(&enum_decl.name.text);
        let table = format!("{}__FIELDS", enum_decl.name.text.to_uppercase());
        let fields_ty = format!("{bare}__Fields");
        self.w.line(&format!(
            "/// One of this variant's fields (§7.7.4), read from the `{table}` table."
        ));
        self.w.line(&format!(
            "fn __field<R>(&self, read: impl FnOnce(&{fields_ty}) -> R) -> R {{"
        ));
        self.w.indent_inc();
        self.w.line(&format!("{table}.with(|all| read(&all[*self as usize]))"));
        self.w.indent_dec();
        self.w.line("}");
        for (idx, ctor) in enum_decl.constructors.iter().enumerate() {
            let suffix = if idx == 0 { String::new() } else { format!("__{idx}") };
            self.w.line("/// A constructor (§7.7.4): the field values one variant's arguments give.");
            self.w.emit_indent();
            self.w.push_str(&format!("fn __new_fields{suffix}("));
            for (i, p) in ctor.params.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.w.push_str(&to_rust_ident(&p.name.text));
                self.w.push_str(": ");
                self.emit_value_type_as_rust(&p.ty);
            }
            self.w.push_str(&format!(") -> {fields_ty} {{\n"));
            self.w.indent_inc();
            self.w.line(&format!("{fields_ty} {{"));
            self.w.indent_inc();
            let params: std::collections::HashSet<String> =
                ctor.params.iter().map(|p| p.name.text.clone()).collect();
            let prev_params = std::mem::replace(&mut self.current_fn_params, params.clone());
            let prev_alias = self.this_alias.take();
            for field in &enum_decl.fields {
                // The checker guarantees exactly one `this.f = value;` per
                // field (E0495).
                let value = ctor.body.statements.iter().find_map(|st| match st {
                    juxc_ast::Stmt::Assign(a) => {
                        let target = match &a.target {
                            juxc_ast::Expr::Field(f) if matches!(f.object.as_ref(), juxc_ast::Expr::This(_)) => {
                                Some(f.field.text.as_str())
                            }
                            juxc_ast::Expr::Path(qn) if qn.segments.len() == 1 && !params.contains(&qn.segments[0].text) => {
                                Some(qn.segments[0].text.as_str())
                            }
                            _ => None,
                        };
                        (target == Some(field.name.text.as_str())).then_some(&a.value)
                    }
                    _ => None,
                });
                let Some(value) = value else { continue };
                let name = to_rust_ident(&field.name.text);
                let shorthand = matches!(value, juxc_ast::Expr::Path(qn)
                    if qn.segments.len() == 1 && qn.segments[0].text == field.name.text)
                    && !field.ty.as_ref().is_some_and(|t| t.nullable);
                self.w.emit_indent();
                if shorthand {
                    // `this.mass = mass;` -- Rust's field shorthand.
                    self.w.push_str(&name);
                } else {
                    self.w.push_str(&name);
                    self.w.push_str(": ");
                    let nullable = field.ty.as_ref().is_some_and(|t| t.nullable);
                    self.emit_arg_with_nullable_wrap(value, nullable);
                }
                self.w.push_str(",\n");
            }
            self.this_alias = prev_alias;
            self.current_fn_params = prev_params;
            self.w.indent_dec();
            self.w.line("}");
            self.w.indent_dec();
            self.w.line("}");
        }
    }

    /// The fields struct and the per-variant table (see
    /// [`Self::emit_enum_field_methods`]), emitted after the enum's `impl`.
    pub(crate) fn emit_enum_field_table(&mut self, enum_decl: &juxc_ast::EnumDecl) {
        let bare = to_rust_ident(&enum_decl.name.text);
        let table = format!("{}__FIELDS", enum_decl.name.text.to_uppercase());
        let fields_ty = format!("{bare}__Fields");
        self.w.line(&format!(
            "/// The per-variant fields of `{}` (JUX-LANG-V1 §7.7.4).",
            enum_decl.name.text
        ));
        self.w.line("#[derive(Debug, Clone)]");
        self.w.line(&format!("pub(crate) struct {fields_ty} {{"));
        self.w.indent_inc();
        for field in &enum_decl.fields {
            let Some(ty) = &field.ty else { continue };
            self.w.emit_indent();
            self.w.push_str(&format!("{}: ", to_rust_ident(&field.name.text)));
            self.emit_field_type_as_rust(ty);
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.line("thread_local! {");
        self.w.indent_inc();
        self.w.line("/// Built on first use, one row per variant, in declaration order.");
        self.w.line(&format!("static {table}: std::vec::Vec<{fields_ty}> = std::vec!["));
        self.w.indent_inc();
        let sigs = self
            .symbols
            .enums
            .iter()
            .find(|(k, _)| k.rsplit('.').next() == Some(enum_decl.name.text.as_str()))
            .map(|(_, e)| e.constructors.clone())
            .unwrap_or_default();
        for variant in &enum_decl.variants {
            let pick = self.symbols.ctor_selections.get(&variant.span).copied().unwrap_or(0);
            let suffix = if pick == 0 { String::new() } else { format!("__{pick}") };
            let params = sigs.get(pick).map(|c| c.params.clone()).unwrap_or_default();
            self.w.emit_indent();
            self.w.push_str(&format!("{bare}::__new_fields{suffix}("));
            for (i, arg) in variant.args.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                let nullable = params.get(i).is_some_and(|p| p.ty.nullable);
                self.emit_arg_with_nullable_wrap(arg, nullable);
            }
            self.w.push_str(&format!("), // {}\n", variant.name.text));
        }
        self.w.indent_dec();
        self.w.line("];");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

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
        self.w.push_str(&to_rust_ident(&method.name.text));
        // The method's own type parameters (`<R> Option<R> map((T) -> R f)`,
        // Core lib §K.3), bounded the way a class method's are: `Clone` for
        // the value model, `Display` when a value reaches a format position.
        if !method.generic_params.is_empty() {
            let displayed = self.fn_displayed_generic_params(method);
            let defaulted = crate::analysis::new_array_element_params(
                &method.generic_params,
                &method.body.iter().collect::<Vec<_>>(),
                &[],
            );
            self.emit_generic_params_with_clone_bound_plus_display(
                &method.generic_params,
                &displayed,
                &defaulted,
            );
        }
        self.w.push('(');
        if !is_static {
            self.w.push_str("&self");
        }
        for (i, param) in method.params.iter().enumerate() {
            if i > 0 || !is_static {
                self.w.push_str(", ");
            }
            self.w.push_str(&to_rust_ident(&param.name.text));
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
            // Nullable params start out `Option<T>` shaped, as in a class
            // method: without them here a guard clause (`if (v == null)
            // return ...;`) never shadowed `v` with its contents, and
            // `v!!` after it reached rustc as the whole `Option`.
            self.nullable_locals.clear();
            for p in &method.params {
                if p.ty.nullable {
                    self.nullable_locals.insert(p.name.text.clone());
                }
            }
            self.current_fn_params =
                method.params.iter().map(|p| p.name.text.clone()).collect();
            let saved = self.current_return_type.take();
            self.current_return_type = Some(method.return_type.clone());
            // The method's own type parameters are in scope for its body,
            // on top of the enum's.
            let prev_type_params = self.current_type_params.clone();
            self.current_type_params
                .extend(crate::collect_type_param_names(&method.generic_params));
            let prev_type_param_bounds = self.type_param_bounds.clone();
            self.type_param_bounds
                .extend(crate::collect_type_param_bounds(&method.generic_params));
            self.emit_fn_body_at(body, &method.return_type);
            self.current_type_params = prev_type_params;
            self.type_param_bounds = prev_type_param_bounds;
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
