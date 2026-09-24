//! Jux record declarations → Rust `pub struct` + canonical constructor
//! + auto-derived `Display` impl when every component supports it.
//! Records can also carry operator overrides in their body (per
//! §O.3.4) — both real overrides and the `= delete;` suppression form.

use juxc_ast::OperatorKind;

use crate::analysis::{
    field_supports_copy, field_supports_eq,
};
use crate::RustEmitter;
use juxc_lex::to_rust_ident;

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
    /// **`Hash` and `Eq`** come from the shared hash plan
    /// ([`crate::decls::hashing`]): derived when every component hashes
    /// natively, written by hand when a component is a float (hashed by its
    /// bits, §O.3.1), and absent when a component has no hash at all.
    pub(crate) fn emit_record_decl(&mut self, record_decl: &juxc_ast::RecordDecl) {
        // Inside a record's methods and operators `this` is `&Self`; see
        // `in_record_body`.
        let prev = std::mem::replace(&mut self.in_record_body, true);
        self.emit_record_decl_inner(record_decl);
        self.in_record_body = prev;
    }

    fn emit_record_decl_inner(&mut self, record_decl: &juxc_ast::RecordDecl) {
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
        // Whether the record has a default value is the checker's question as
        // much as the backend's (`new R[n]`, JUX-LANG-V1 §5.5), so it is
        // answered once, in tycheck, for both.
        let pkg = self
            .current_unit_idx
            .and_then(|i| self.symbols.units.get(i))
            .map(|unit| unit.package.join("."))
            .unwrap_or_default();
        let fqn = if pkg.is_empty() {
            record_decl.name.text.clone()
        } else {
            format!("{pkg}.{}", record_decl.name.text)
        };
        let has_default = if self.symbols.records.contains_key(&fqn) {
            record_decl
                .components
                .iter()
                .all(|c| juxc_tycheck::defaults::member_has_default(&c.ty, &fqn, &self.symbols))
        } else {
            record_decl
                .components
                .iter()
                .all(|c| crate::analysis::field_supports_default(&c.ty))
        };
        let components: Vec<&juxc_ast::TypeRef> = record_decl.components.iter().map(|c| &c.ty).collect();
        let legacy_eq = components.iter().all(|t| field_supports_eq(t));
        let mut hash_plan = self.value_hash_plan(&fqn, &components, &record_decl.operators, legacy_eq);
        // A component held as a trait-object handle (an interface, or a
        // polymorphic base class: `record Paren(Expr inner)` stores
        // `Rc<dyn Expr>`) cannot go through `#[derive(PartialEq)]`: the derive
        // writes `self.inner == other.inner`, and because the handle itself
        // implements the trait, rustc coerces the right side to `dyn Expr` and
        // reports E0507. Such a record gets `PartialEq` written by hand, which
        // compares the objects themselves (identity, as `==` on interface
        // values is), and its `Eq` / `Hash` move off the derive with it.
        let dyn_components: Vec<bool> = components.iter().map(|t| self.is_dyn_handle_type(t)).collect();
        let manual_eq = dyn_components.iter().any(|d| *d)
            && !record_decl.operators.iter().any(|o| o.kind == OperatorKind::Eq);
        if manual_eq {
            if hash_plan.derive_eq {
                hash_plan.derive_eq = false;
                hash_plan.eq_marker = true;
            }
            if hash_plan.derive_hash {
                hash_plan.derive_hash = false;
                hash_plan.manual_hash = true;
            }
        }
        self.w.line(&record_derive_attribute(record_decl, has_default, hash_plan, manual_eq));
        // `@layout(c) record` (§L.1.2): fields in declaration order at their C
        // offsets. The checker has already held every component to a C
        // `Copy` type, so the derive above includes `Copy`.
        if crate::has_layout_c(&record_decl.annotations) {
            self.w.line("#[repr(C)]");
        }
        crate::emit_align_attribute(&mut self.w, &record_decl.annotations);

        // pub struct Name<T, U> { …components… }
        self.w.emit_indent();
        self.emit_visibility(record_decl.visibility);
        self.w.push_str("struct ");
        self.w.push_str(&to_rust_ident(&record_decl.name.text));
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
            self.w.push_str(&to_rust_ident(&comp.name.text));
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
        {
            // A parameter formatted in one of the record's methods needs
            // `Display`, exactly as a class's does (§T.2.1).
            let displayed = self.record_displayed_generic_params(record_decl);
            let none: std::collections::HashSet<String> = std::collections::HashSet::new();
            self.emit_generic_params_with_clone_bound_plus_display(
                &record_decl.generic_params,
                &displayed,
                &none,
            );
        }
        self.w.push(' ');
        self.w.push_str(&to_rust_ident(&record_decl.name.text));
        self.emit_generic_params_as_args(&record_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        // The compact constructor (§7.6.1) is the canonical constructor's
        // body: it runs on the components as parameters, and a component it
        // reassigns is a `mut` parameter.
        let compact_muts: std::collections::HashSet<String> = match &record_decl.compact_ctor {
            Some(compact) => {
                let mut muts = std::collections::HashSet::new();
                crate::analysis::collect_mutated_names(&compact.body, &mut muts, &self.user_mut_methods);
                muts
            }
            None => std::collections::HashSet::new(),
        };
        self.w.emit_indent();
        self.w.push_str("pub fn new(");
        for (i, comp) in record_decl.components.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            if compact_muts.contains(&comp.name.text) {
                self.w.push_str("mut ");
            }
            self.w.push_str(&to_rust_ident(&comp.name.text));
            self.w.push_str(": ");
            // Post Fix 1 Jux `String` lowers to owned Rust `String`
            // in every position — params included. Field init below
            // is therefore a plain move (`name: name`), so the parameter
            // has exactly the field's type: an interface-typed component
            // (`record Paren(Expr inner)`) is `Rc<dyn Expr>` on both, where
            // the bare trait name did not compile (rustc E0782).
            self.emit_value_type_as_rust(&comp.ty);
        }
        self.w.push_str(") -> Self {\n");
        self.w.indent_inc();
        if let Some(compact) = record_decl.compact_ctor.clone() {
            let params: Vec<juxc_ast::Param> = record_decl
                .components
                .iter()
                .map(|c| juxc_ast::Param {
                    name: c.name.clone(),
                    ty: c.ty.clone(),
                    is_final: false,
                    is_ref: false,
                    is_mut_ref: false,
                    default: None,
                    is_varargs: false,
                    is_out: false,
                    is_shared_ref: false,
                    is_weak: false,
                    span: c.span,
                })
                .collect();
            self.emit_record_ctor_body(record_decl, &params, &compact.body.statements, None);
        }
        self.w.line("Self {");
        self.w.indent_inc();
        for comp in &record_decl.components {
            // Field == param-name → use Rust's struct shorthand
            // (`Self { x, y }` rather than `Self { x: x, y: y }`).
            // Records always satisfy this — the canonical ctor binds
            // each component to its own name — so the branch is
            // unconditional here.
            self.w.emit_indent();
            self.w.push_str(&to_rust_ident(&comp.name.text));
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        // Now at depth 2 — close `pub fn new(...) -> Self { ... }`.
        self.w.indent_dec();
        self.w.line("}");
        // Additional constructors (§7.6.1), `new__K` at the index the symbol
        // table gave them (the canonical one is 0). Each begins with
        // `this(...)`: the delegated constructor builds the value, and the
        // rest of the body runs with `this` bound to it.
        let sigs = self
            .symbols
            .records
            .get(&fqn)
            .map(|r| r.constructors.clone())
            .unwrap_or_default();
        for ctor in &record_decl.constructors {
            let Some(idx) = sigs.iter().position(|c| c.span == ctor.span) else { continue };
            let Some((call, rest)) = ctor.body.statements.split_first() else { continue };
            let juxc_ast::Stmt::Expr(juxc_ast::Expr::Call(delegation)) = call else { continue };
            let mut muts = std::collections::HashSet::new();
            let rest_block = juxc_ast::Block { statements: rest.to_vec(), span: ctor.body.span };
            crate::analysis::collect_mutated_names(&rest_block, &mut muts, &self.user_mut_methods);
            self.w.emit_indent();
            self.emit_visibility(ctor.visibility);
            self.w.push_str(&format!("fn new__{idx}("));
            for (i, p) in ctor.params.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                if muts.contains(&p.name.text) {
                    self.w.push_str("mut ");
                }
                self.w.push_str(&to_rust_ident(&p.name.text));
                self.w.push_str(": ");
                self.emit_type_as_rust(&p.ty);
            }
            self.w.push_str(") -> Self {\n");
            self.w.indent_inc();
            // The delegation, with each argument shaped for the parameter
            // it lands in (a `T?` slot wraps a plain value).
            let target = self
                .symbols
                .ctor_selections
                .get(&delegation.span)
                .copied()
                .unwrap_or(0);
            let target_params = sigs.get(target).map(|c| c.params.clone()).unwrap_or_default();
            let prev_params = std::mem::replace(
                &mut self.current_fn_params,
                ctor.params.iter().map(|p| p.name.text.clone()).collect(),
            );
            self.w.emit_indent();
            if rest.is_empty() {
                self.w.push_str("Self::new");
            } else {
                self.w.push_str("let __self = Self::new");
            }
            if target > 0 {
                self.w.push_str(&format!("__{target}"));
            }
            self.w.push('(');
            for (i, arg) in delegation.args.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                let nullable = target_params.get(i).is_some_and(|p| p.ty.nullable);
                self.emit_arg_with_nullable_wrap(arg, nullable);
                if !nullable && self.wrapper_value_needs_clone(arg) {
                    self.w.push_str(".clone()");
                }
            }
            self.w.push(')');
            self.current_fn_params = prev_params;
            if rest.is_empty() {
                self.w.push('\n');
            } else {
                self.w.push_str(";\n");
                self.emit_record_ctor_body(record_decl, &ctor.params, rest, Some("__self"));
                self.w.line("__self");
            }
            self.w.indent_dec();
            self.w.line("}");
        }
        // Depth 1 — inside the `impl Name { ... }` block. Emit
        // inherent operator methods, then user-declared methods.
        // `emit_operator_as_method` skips deleted operators (no
        // inherent method for a `= delete;` declaration).
        let prev_record = self.enclosing_record.replace(record_decl.clone());
        for op in &record_decl.operators {
            self.emit_operator_as_method(op);
        }
        // Records can declare methods (per grammar §A.2.4). They
        // share the same emission path as class methods — `emit_method`
        // is host-agnostic.
        for method in &record_decl.methods {
            self.emit_method(method);
        }
        self.enclosing_record = prev_record;
        // Static fields declared inside the record body (JEP 395 §3,
        // Java-records-with-static). `final` / `const` ones lower as
        // associated `pub const`; mutable ones get the same
        // module-scope `LazyLock<Mutex<T>>` shape class statics use,
        // but emitted after the impl block closes.
        for field in &record_decl.static_fields {
            if field.is_final {
                self.emit_static_field(field);
            }
        }
        // Close the `impl Name { ... }` block.
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        // `record Pt(int x, int y) implements Point` — one delegating trait
        // impl per interface, exactly like a class's. A record's components
        // already emit as public fields AND as accessor methods, so the
        // delegation targets the inherent method the interface names.
        self.emit_record_trait_impls(record_decl);
        // Module-scope mutable statics for the record's non-final
        // static fields.
        for field in &record_decl.static_fields {
            if !field.is_final {
                self.emit_mutable_static_field(&record_decl.name.text, field);
            }
        }

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
        // Only an `operator string` of the record's own stops the derived
        // one. A component that is not a plain value (an interface, a class,
        // a collection) no longer does: it prints through the universal
        // show helper instead, so EVERY record has a `Display` -- which an
        // interface it implements now requires (§O.4.1: an interface-typed
        // value prints as its object).
        if !has_string_override {
            self.emit_record_display_impl(record_decl);
        }
        // A value's identity is its own address; interfaces it implements
        // require the impl (see the prelude's `JuxIdentity`).
        self.emit_jux_identity_impl(&record_decl.name.text, &record_decl.generic_params, "self as *const Self");

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
        // `PartialEq` by hand for a record holding a trait-object handle (see
        // `manual_eq` above).
        if manual_eq {
            self.emit_record_manual_partial_eq(record_decl, &dyn_components);
        }
        // `Hash` by hand when a float component rules out the derive, and the
        // `Eq` promise a hash key needs when it was not derived.
        if hash_plan.manual_hash {
            let fields: Vec<(&str, &juxc_ast::TypeRef)> =
                record_decl.components.iter().map(|c| (c.name.text.as_str(), &c.ty)).collect();
            self.emit_value_hash_for_fields(&record_decl.name.text, &record_decl.generic_params, &fields);
        }
        if hash_plan.eq_marker {
            self.emit_value_eq_marker(&record_decl.name.text, &record_decl.generic_params);
        }
        // `<=>` totally orders the record (§7.14.4): `Ord`, and the `Eq` it
        // requires when the hash plan did not give one. Non-generic records
        // only, like the operator bridges above.
        if record_decl.generic_params.is_empty() {
            if let Some(cmp) = record_decl.operators.iter().find(|o| o.kind == OperatorKind::Cmp && !o.is_deleted) {
                if !hash_plan.derive_eq && !hash_plan.eq_marker {
                    self.emit_value_eq_marker(&record_decl.name.text, &record_decl.generic_params);
                }
                let arg = self.operator_other_arg(cmp);
                self.emit_ord_from_cmp(&record_decl.name.text, arg);
            }
        }
    }

    /// `impl PartialEq for R`, component by component. A trait-object handle
    /// is compared through the object (`*a == *b`, or `as_deref()` for a
    /// nullable one), which is what the derive could not spell; every other
    /// component compares the way the derive would have.
    fn emit_record_manual_partial_eq(&mut self, record_decl: &juxc_ast::RecordDecl, dyn_components: &[bool]) {
        let name = record_decl.name.text.clone();
        self.emit_value_impl_head("PartialEq", &name, &record_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line("fn eq(&self, other: &Self) -> bool {");
        self.w.indent_inc();
        if record_decl.components.is_empty() {
            self.w.line("true");
        }
        for (i, (comp, is_dyn)) in record_decl.components.iter().zip(dyn_components).enumerate() {
            let field = to_rust_ident(&comp.name.text);
            let test = match (*is_dyn, comp.ty.nullable) {
                (true, false) => format!("*self.{field} == *other.{field}"),
                (true, true) => format!("self.{field}.as_deref() == other.{field}.as_deref()"),
                (false, _) => format!("self.{field} == other.{field}"),
            };
            let line = if i + 1 < record_decl.components.len() { format!("{test} &&") } else { test };
            self.w.line(&line);
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// Emit the statements of a record constructor body: the compact
    /// constructor's (inside the canonical `new`, `this` unset, since the
    /// record does not exist yet) or the part of an additional constructor
    /// after its `this(...)` (`this` is the delegated value, `this_name`).
    /// The body's parameters shadow the components' field names, exactly as
    /// a method's parameters shadow fields.
    fn emit_record_ctor_body(
        &mut self,
        record_decl: &juxc_ast::RecordDecl,
        params: &[juxc_ast::Param],
        statements: &[juxc_ast::Stmt],
        this_name: Option<&str>,
    ) {
        let body = juxc_ast::Block { statements: statements.to_vec(), span: record_decl.span };
        let prev_record = self.enclosing_record.replace(record_decl.clone());
        let prev_alias = std::mem::replace(&mut self.this_alias, this_name.map(str::to_string));
        let mut muts = std::collections::HashSet::new();
        crate::analysis::collect_mutated_names(&body, &mut muts, &self.user_mut_methods);
        self.collect_mut_slot_locals(&body, &mut muts);
        let prev_muts = std::mem::replace(&mut self.mutated_in_fn, muts);
        self.nullable_locals.clear();
        for p in params {
            if p.ty.nullable {
                self.nullable_locals.insert(p.name.text.clone());
            }
        }
        let prev_params = std::mem::replace(
            &mut self.current_fn_params,
            params.iter().map(|p| p.name.text.clone()).collect(),
        );
        let prev_return = self.current_return_type.replace(juxc_ast::ReturnType::Void);
        self.emit_fn_body_at(&body, &juxc_ast::ReturnType::Void);
        self.current_return_type = prev_return;
        self.current_fn_params = prev_params;
        self.mutated_in_fn = prev_muts;
        self.this_alias = prev_alias;
        self.enclosing_record = prev_record;
    }

    /// Generate the `impl std::fmt::Display for Name { … }` block for a
    /// record. Format mirrors §O.3.1's example: `"Name(field: value,
    /// other: value)"`. Empty-component records emit `"Name()"`.
    ///
    /// Emit one delegating `impl <Iface> for <Record>` per interface the record
    /// declares (grammar §A.2.5 allows `implements` on a record; a record has no
    /// `extends`, so this is its whole supertype list).
    ///
    /// Two shapes satisfy an interface method. A method the record DECLARES
    /// delegates to the inherent one. A no-argument method whose name matches a
    /// COMPONENT is the record's accessor — records expose components as public
    /// fields, so the impl reads the field. Anything else is left to the
    /// interface's own default body.
    fn emit_record_trait_impls(&mut self, record_decl: &juxc_ast::RecordDecl) {
        for interface_ty in &record_decl.implements {
            let Some(iface_name) = interface_ty.name.segments.last() else { continue };
            let Some((_, iface)) = self.lookup_interface_by_bare_or_fqn(&iface_name.text) else {
                continue;
            };
            // `implements Holder<int>` binds the interface's params to the
            // record's arguments; installing it as the Kind substitution makes
            // every type emitted below read in the record's vocabulary.
            let subst: std::collections::HashMap<String, juxc_ast::TypeRef> = iface
                .generic_params
                .iter()
                .zip(interface_ty.generic_args.iter())
                .filter_map(|(p, a)| a.as_type().map(|t| (p.name.text.clone(), t.clone())))
                .collect();
            let mut methods: Vec<(String, juxc_tycheck::symbol_table::MethodSig)> = iface
                .methods
                .iter()
                .map(|(n, m)| (n.clone(), m.clone()))
                .collect();
            methods.sort_by(|a, b| a.0.cmp(&b.0));

            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params_with_clone_bound(&record_decl.generic_params);
            self.w.push(' ');
            self.emit_type_as_rust(interface_ty);
            self.w.push_str(" for ");
            self.w.push_str(&to_rust_ident(&record_decl.name.text));
            self.emit_generic_params_as_args(&record_decl.generic_params);

            let saved = std::mem::replace(&mut self.kind_type_subst, subst);
            let provided: Vec<(String, juxc_tycheck::symbol_table::MethodSig)> = methods
                .into_iter()
                .filter(|(name, sig)| {
                    record_decl.methods.iter().any(|m| &m.name.text == name)
                        || (sig.params.is_empty()
                            && record_decl.components.iter().any(|c| &c.name.text == name))
                })
                .collect();
            // A value typed as the interface may be this record at run time:
            // `x => Ins i` asks the `__jux_as_Ins` hook, whose default answers
            // `None`, so the record answers for itself.
            let hooks: Vec<String> = self
                .interface_hook_targets(&iface_name.text)
                .into_iter()
                .filter(|t| t == &record_decl.name.text)
                .collect();
            if provided.is_empty() && hooks.is_empty() {
                self.w.push_str(" {}\n\n");
            } else {
                self.w.push_str(" {\n");
                self.w.indent_inc();
                for (name, sig) in &provided {
                    if record_decl.methods.iter().any(|m| &m.name.text == name) {
                        self.emit_kind_delegating_method(&record_decl.name.text, name, sig);
                    } else {
                        self.emit_record_component_accessor(name, sig);
                    }
                }
                for t in &hooks {
                    self.emit_downcast_hook_impl(t, &iface_name.text);
                }
                self.w.indent_dec();
                self.w.emit_indent();
                self.w.push_str("}\n\n");
            }
            self.kind_type_subst = saved;
        }
    }

    /// `fn x(&self) -> isize { self.x.clone() }` — an interface accessor
    /// satisfied by a record component of the same name. The clone keeps the
    /// record's own copy intact for a non-`Copy` component; for a `Copy` one it
    /// is free.
    fn emit_record_component_accessor(&mut self, name: &str, sig: &juxc_tycheck::symbol_table::MethodSig) {
        self.w.emit_indent();
        self.w.push_str("fn ");
        self.w.push_str(&to_rust_ident(name));
        self.w.push_str("(&self)");
        if let juxc_ast::ReturnType::Type(t) = &sig.return_type {
            self.w.push_str(" -> ");
            self.emit_return_type_as_rust(t);
        }
        self.w.push_str(" { self.");
        self.w.push_str(&to_rust_ident(name));
        self.w.push_str(".clone() }
");
    }

    /// Called by [`Self::emit_record_decl`] only when every component is
    /// displayable (`field_supports_display_in`). A GENERIC record gets the
    /// impl too, with a `Display` bound on each parameter it uses as a bare
    /// component type -- without one it could not be another generic's type
    /// argument, since a formatted parameter carries that bound (§T.2.1).
    fn emit_record_display_impl(&mut self, record_decl: &juxc_ast::RecordDecl) {
        let name = &record_decl.name.text;
        let own_params: std::collections::HashSet<String> = record_decl
            .generic_params
            .iter()
            .map(|p| p.name.text.clone())
            .collect();
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
            // A float component prints the way every Jux float does (LANG-V1
            // 3.4): `Display` wrote `Pt(x: 1, y: 2.5)` for `new Pt(1.0, 2.5)`.
            // The label above is text; the field access is a Rust path, where a
            // component named `match` is `r#match`.
            let field = to_rust_ident(&comp.name.text);
            if crate::analysis::type_ref_is_float(&comp.ty) {
                args.push(format!("crate::jux_float(self.{field})"));
            } else if crate::analysis::field_supports_display_in(&comp.ty, &own_params) {
                args.push(format!("self.{field}"));
            } else {
                // An object, a collection, a nullable: the helper picks the
                // value's own text where it has one and its debug form
                // otherwise, which is what `print` does everywhere else.
                args.push(format!("crate::__jux_show!(self.{field})"));
            }
        }
        fmt_body.push(')');

        self.w.emit_indent();
        self.w.push_str("impl");
        if !record_decl.generic_params.is_empty() {
            let displayed = crate::analysis::displayed_bare_params(
                &record_decl.generic_params,
                record_decl.components.iter().map(|c| &c.ty),
            );
            let none: std::collections::HashSet<String> = std::collections::HashSet::new();
            self.emit_generic_params_with_clone_bound_plus_display(
                &record_decl.generic_params,
                &displayed,
                &none,
            );
        }
        self.w.push_str(" std::fmt::Display for ");
        self.w.push_str(&to_rust_ident(name));
        self.emit_generic_params_as_args(&record_decl.generic_params);
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
fn record_derive_attribute(
    record_decl: &juxc_ast::RecordDecl,
    all_default: bool,
    hash_plan: crate::decls::hashing::HashPlan,
    manual_eq: bool,
) -> String {
    let mut derives: Vec<&str> = vec!["Debug", "Clone"];

    let has_eq_op = record_decl
        .operators
        .iter()
        .any(|o| o.kind == OperatorKind::Eq);
    let component_tys: Vec<&juxc_ast::TypeRef> =
        record_decl.components.iter().map(|c| &c.ty).collect();
    // A `@layout(c)` record is a C value: every component is a C `Copy` type
    // (tycheck E0509 otherwise), including raw pointers, which the generic
    // field test does not count.
    let all_copy = crate::has_layout_c(&record_decl.annotations)
        || component_tys.iter().all(|t| field_supports_copy(t));

    // PartialEq: derived unless the user wrote operator== (override
    // or delete). The user's override path emits its own `impl
    // PartialEq`; `= delete;` opts out entirely. A record holding a
    // trait-object handle has it written by hand instead (`manual_eq`).
    if !has_eq_op && !manual_eq {
        derives.push("PartialEq");
    }
    // Eq and Hash follow the shared hash plan (§O.3.1): derived when every
    // component hashes natively; a float component is hashed by hand after
    // the declaration instead.
    if hash_plan.derive_eq {
        derives.push("Eq");
    }
    if hash_plan.derive_hash {
        derives.push("Hash");
    }
    // Copy: always conditional on field types. Deletion doesn't
    // affect Copy — copy semantics are a value-type property, not an
    // operator-level decision.
    if all_copy {
        derives.push("Copy");
    }
    // Default: derived when every component is Default-able. Lets a
    // class storing this record as a field flow through the
    // struct-init shim's `field: Default::default()` fallback when
    // the user didn't supply an explicit field initializer.
    if all_default {
        derives.push("Default");
    }
    format!("#[derive({})]", derives.join(", "))
}
