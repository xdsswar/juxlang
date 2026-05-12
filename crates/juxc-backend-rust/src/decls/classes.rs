//! Jux class declarations → Rust `pub struct` + inherent impl + marker
//! trait + interface trait impls + per-method emission. Constructor
//! and operator emission live in sibling modules ([`super::constructors`],
//! [`super::operators`]) — those got split out because the class file
//! was getting unwieldy.

use std::collections::HashSet;

use juxc_ast::{FnDecl, OperatorKind, ReturnType};
use juxc_tycheck::symbol_table::MethodSig;

use crate::analysis::{body_writes_to_this, collect_mutated_names};
use crate::RustEmitter;

impl RustEmitter {
    /// Emit a Jux class declaration as a Rust `pub struct` plus an
    /// `impl` block carrying its constructor and methods.
    ///
    /// **Constructor lowering.** A Jux constructor body runs statement-
    /// by-statement, doing `this.field = …` assignments. Rust struct
    /// literals require all fields up-front, so we synthesize a builder
    /// pattern:
    ///
    /// ```text
    /// pub fn new(x: isize) -> Self {
    ///     let mut __self = Self { x: 0, y: 0 };  // defaults
    ///     __self.x = x;                          // body, with this→__self
    ///     __self
    /// }
    /// ```
    ///
    /// `this` in the constructor body rewrites to `__self`. Inside
    /// instance methods it rewrites to `self`. This rewrite is controlled
    /// by the [`Self::this_alias`] field, which we set for the duration
    /// of each constructor/method emission.
    ///
    /// **Receiver kind for methods.** Methods that assign to `self.field`
    /// need `&mut self`; otherwise `&self`. We re-run `collect_mutated_names`
    /// over the body and look for the `__this__` sentinel that
    /// `emit_assign` produces for `this.field = …` patterns. Plain locals
    /// in the body still drive `let mut` promotion as before.
    pub(crate) fn emit_class_decl(&mut self, class_decl: &juxc_ast::ClassDecl) {
        // (Migrated to Writer indent-aware API)
        // Derive Clone unconditionally so the `T: Clone` bound used on
        // generic impls (and the auto-`.clone()` injected on field
        // reads) keeps working when the user nests classes — `Box<User>`
        // needs `User: Clone`, which falls out for free here.
        self.w.line("#[derive(Clone)]");
        // pub struct Name<T, U> { …fields… }
        self.w.emit_indent();
        self.emit_visibility(class_decl.visibility);
        self.w.push_str("struct ");
        self.w.push_str(&class_decl.name.text);
        self.emit_generic_params(&class_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        // Inheritance: embed the parent struct as `__parent`. Field
        // access on the child auto-dereffs through `impl Deref<Target=Parent>`
        // (emitted below), so `child.parent_field` and inherited
        // method calls Just Work. Always emit `__parent` first so the
        // struct layout is consistent across the hierarchy.
        if let Some(parent_ty) = &class_decl.extends {
            self.w.emit_indent();
            self.w.push_str("__parent: ");
            self.emit_type_as_rust(parent_ty);
            self.w.push_str(",\n");
        }
        for field in &class_decl.fields {
            self.w.emit_indent();
            self.emit_visibility(field.visibility);
            self.w.push_str(&field.name.text);
            self.w.push_str(": ");
            // Field-position type mapping (String → owned `String`).
            self.emit_field_type_as_rust(&field.ty);
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();

        // Emit Deref + DerefMut impls for child classes so inherited
        // methods and field access flow through Rust's auto-deref —
        // `child.method()` finds methods on the parent transparently,
        // `child.parent_field = x` works via DerefMut, etc.
        if let Some(parent_ty) = &class_decl.extends {
            // impl Deref for Child { type Target = Parent; … }
            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params(&class_decl.generic_params);
            self.w.push_str(" std::ops::Deref for ");
            self.w.push_str(&class_decl.name.text);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str(" {\n");
            self.w.indent_inc();
            self.w.emit_indent();
            self.w.push_str("type Target = ");
            self.emit_type_as_rust(parent_ty);
            self.w.push_str(";\n");
            self.w.line("fn deref(&self) -> &Self::Target { &self.__parent }");
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
            // impl DerefMut for Child { … }
            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params(&class_decl.generic_params);
            self.w.push_str(" std::ops::DerefMut for ");
            self.w.push_str(&class_decl.name.text);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str(" {\n");
            self.w.indent_inc();
            self.w.line("fn deref_mut(&mut self) -> &mut Self::Target { &mut self.__parent }");
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
        }

        // impl[<T: Clone, U: Clone>] Name<T, U> { …members… }
        //
        // For generic classes we emit a `T: Clone` bound on every type
        // parameter (Phase-1 simplification). Reads of generic-typed
        // fields call `.clone()`, so they need the bound. Every Jux
        // primitive and emitted class/enum is `Clone`, so the
        // constraint never blocks a real Jux program. Replace with
        // proper user-declared bounds once `<T extends Animal>` lands.
        self.w.emit_indent();
        self.w.push_str("impl");
        self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
        self.w.push(' ');
        self.w.push_str(&class_decl.name.text);
        self.emit_generic_params_as_args(&class_decl.generic_params);
        self.w.push_str(" {\n");

        // Constructor → `pub fn new(args) -> Self` with the __self pattern.
        for ctor in &class_decl.constructors {
            self.emit_constructor(class_decl, ctor);
        }
        // If no constructor was declared, synthesize an implicit zero-
        // arg `new()` per §7.3.1 (declaring any constructor removes it).
        if class_decl.constructors.is_empty() {
            self.emit_synthetic_default_constructor(class_decl);
        }
        for method in &class_decl.methods {
            self.emit_method(class_decl, method);
        }
        // Operator overloads (§O.2) land as **inherent** methods with
        // synthetic names (`__op_eq`, `__op_string`, …). Trait impls
        // below delegate to these for the operators we know how to
        // map onto Rust traits.
        for op in &class_decl.operators {
            self.emit_operator_as_method(op);
        }
        self.w.line("}");
        self.w.newline();

        // Trait-impl block per recognized operator. Only a handful of
        // operators have a direct Rust-trait counterpart in this
        // turn — see [`Self::emit_operator_trait_impl`] for the table
        // and the bound-propagation caveats. Non-generic classes only;
        // generic-class trait impls need `T: PartialEq` (etc.) bound
        // propagation that's deferred.
        if class_decl.generic_params.is_empty() {
            for op in &class_decl.operators {
                self.emit_operator_trait_impl(&class_decl.name.text, op);
            }
            let has_eq = class_decl
                .operators
                .iter()
                .any(|o| o.kind == OperatorKind::Eq);
            let has_hash = class_decl
                .operators
                .iter()
                .any(|o| o.kind == OperatorKind::Hash);
            let has_cmp = class_decl
                .operators
                .iter()
                .any(|o| o.kind == OperatorKind::Cmp);
            // Spec §O.2.1: `<=>` auto-derives `<`, `<=`, `>`, `>=` but
            // NOT `==`. Rust's PartialOrd requires PartialEq, so when
            // the user defined `<=>` alone, we synthesize a PartialEq
            // bridging through `__op_cmp` ("a == b iff cmp(a, b) == 0").
            // When the user also defined `operator==`, their own
            // PartialEq impl is the one emitted by `emit_operator_trait_impl`
            // — we leave that path alone and skip the synthesized form.
            if has_cmp && !has_eq {
                self.emit_partial_eq_from_cmp(&class_decl.name.text);
            }
            // Per spec §O.2.7 the user MUST define `operator hash` if
            // they define `operator==`. When both are present we
            // additionally emit `impl Eq for Class {}` — the marker
            // trait that signals reflexive equality and unlocks
            // `HashMap`/`HashSet` key usage on top of the Hash impl.
            if has_eq && has_hash {
                self.emit_eq_marker(&class_decl.name.text);
            }
        }

        // For each `implements I`, emit a trait-impl block that
        // **delegates** to the inherent methods on this class. The
        // inherent methods own the canonical bodies; the trait impl
        // forwards every call. This keeps emitted code DRY and lets
        // trait-bound contexts (`<T: I>`) dispatch through `I` while
        // direct `c.method()` calls still hit the inherent path.
        self.emit_class_trait_impls(class_decl);

        // Marker trait — `pub trait <Name>Kind: Clone {}` — and impls
        // for this class plus every ancestor in the chain. Lets
        // `<T extends ClassName>` bounds work for type restriction
        // (the bound rewriter in `emit_generic_params_with_clone_bound`
        // routes class-name bounds through `<Name>Kind`).
        self.emit_class_marker_trait(class_decl);
    }

    /// Emit a class's marker trait and the transitive marker impls
    /// covering its parent chain. Marker traits are empty (`{}`) —
    /// they exist purely to let generic bounds reference Jux classes
    /// in a way Rust's type system accepts. The user can't call
    /// class-defined methods on a marker-bounded `T` (Phase-1
    /// limitation); to expose methods, combine the class bound with an
    /// interface that re-declares them.
    pub(crate) fn emit_class_marker_trait(&mut self, class_decl: &juxc_ast::ClassDecl) {
        // (Migrated to Writer indent-aware API)
        // pub trait <Name>Kind: Clone {}
        self.w.emit_indent();
        self.emit_visibility(class_decl.visibility);
        self.w.push_str("trait ");
        self.w.push_str(&class_decl.name.text);
        self.w.push_str("Kind: Clone {}\n");
        // impl<T: Clone, …> <Name>Kind for <Name><T, …> {}
        //
        // The class's own generic params (with their full bound list)
        // travel onto the marker impl so `<T: Clone>`-style traits
        // satisfy when the class is generic. Without this, an
        // `impl BoxKind for Box<T>` would fail to derive Clone because
        // `Box<T>` only Clones when `T: Clone`.
        self.w.emit_indent();
        self.w.push_str("impl");
        self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
        self.w.push(' ');
        self.w.push_str(&class_decl.name.text);
        self.w.push_str("Kind for ");
        self.w.push_str(&class_decl.name.text);
        self.emit_generic_params_as_args(&class_decl.generic_params);
        self.w.push_str(" {}\n");

        // Walk the ancestor chain and emit one transitive impl per
        // ancestor. So `class Spaniel extends Dog extends Animal`
        // emits `impl DogKind for Spaniel {}` AND
        // `impl AnimalKind for Spaniel {}`.
        let mut ancestor = class_decl.extends.clone();
        while let Some(parent_ty) = ancestor {
            let parent_name = parent_ty
                .name
                .segments
                .first()
                .map(|s| s.text.clone());
            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
            self.w.push(' ');
            self.emit_type_as_rust(&parent_ty);
            self.w.push_str("Kind for ");
            self.w.push_str(&class_decl.name.text);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str(" {}\n");
            // Step up the chain through tycheck's symbol table. Clone
            // the optional TypeRef out of the ClassSig so we don't
            // hold a borrow on `self.symbols` across the next loop
            // iteration (which calls `self.emit_*` mutably).
            ancestor = parent_name
                .and_then(|n| self.symbols.classes.get(&n))
                .and_then(|c| c.extends.clone());
        }
        self.w.newline();
    }

    /// Emit one `impl Interface for Class { … delegating methods … }`
    /// block per interface listed in the class's `implements` clause.
    ///
    /// Each method's **declared signature** comes from the interface's
    /// [`MethodSig`] in tycheck's [`SymbolTable`] (Phase G), not from
    /// the class — that way the trait impl's signature matches the
    /// trait declaration exactly, even when the class has incidental
    /// extra methods or differing param names. The body is always a
    /// delegating `self.method(args)` call.
    ///
    /// **Iteration order.** The symbol table stores methods in a
    /// `HashMap<String, MethodSig>` keyed by name. The map has no
    /// inherent order, so we sort the (name, sig) entries
    /// alphabetically before emission. Trait impls don't care about
    /// method order (Rust resolves by name), and a deterministic sort
    /// keeps the emitted output stable across runs.
    pub(crate) fn emit_class_trait_impls(&mut self, class_decl: &juxc_ast::ClassDecl) {
        if class_decl.implements.is_empty() {
            return;
        }
        // Clone the per-interface signature lists upfront so we can
        // mutably-call `self.emit_*` inside the loop without fighting
        // the borrow checker.
        let interfaces: Vec<juxc_ast::TypeRef> = class_decl.implements.clone();
        for interface_ty in &interfaces {
            // Interface name must be a single-segment path today —
            // imports and module-qualified interfaces are a future
            // extension.
            let Some(iface_name) = interface_ty.name.segments.first() else {
                continue;
            };
            // Pull (name, MethodSig) pairs from the symbol table and
            // sort by name for deterministic emission order. Empty
            // when the interface isn't in the table (e.g. unresolved
            // name — tycheck would have already flagged that).
            let mut methods: Vec<(String, MethodSig)> = self
                .symbols
                .interfaces
                .get(iface_name.text.as_str())
                .map(|sig| {
                    sig.methods
                        .iter()
                        .map(|(name, m)| (name.clone(), m.clone()))
                        .collect()
                })
                .unwrap_or_default();
            if methods.is_empty() {
                continue;
            }
            methods.sort_by(|a, b| a.0.cmp(&b.0));
            self.w.emit_indent();
            self.w.push_str("impl");
            // The class's own generic params (with the Clone bound)
            // travel onto the trait impl too — `impl<T: Clone>
            // Interface for Box<T>`.
            self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
            self.w.push(' ');
            self.emit_type_as_rust(interface_ty);
            self.w.push_str(" for ");
            self.w.push_str(&class_decl.name.text);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str(" {\n");
            self.w.indent_inc();
            for (method_name, method) in &methods {
                self.w.emit_indent();
                self.w.push_str("fn ");
                self.w.push_str(method_name);
                // Match the interface's declared receiver: always
                // `&self` in Turn 1. Implementing classes must also be
                // non-mutating for the delegation to type-check.
                self.w.push_str("(&self");
                // `MethodSig.params` is `Vec<ParamSig>`; ParamSig
                // carries `name: String` (not `Ident`) and `ty: TypeRef`.
                for param in &method.params {
                    self.w.push_str(", ");
                    self.w.push_str(&param.name);
                    self.w.push_str(": ");
                    self.emit_type_as_rust(&param.ty);
                }
                self.w.push(')');
                match &method.return_type {
                    ReturnType::Void => {}
                    ReturnType::Type(t) => {
                        self.w.push_str(" -> ");
                        self.emit_return_type_as_rust(t);
                    }
                    ReturnType::AsyncType(_) => {
                        self.w.push_str(" -> ()");
                    }
                }
                // Delegating body — `self.method(params)` forwards to
                // the inherent impl. Void methods drop the value; the
                // unit-return body still compiles for them.
                self.w.push_str(" {\n");
                self.w.indent_inc();
                self.w.emit_indent();
                self.w.push_str("self.");
                self.w.push_str(method_name);
                self.w.push('(');
                for (i, param) in method.params.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.w.push_str(&param.name);
                }
                self.w.push_str(")\n");
                self.w.indent_dec();
                self.w.line("}");
            }
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
        }
    }

    /// Emit one instance method as an inherent function inside the
    /// class's `impl` block. Caller (`emit_class_decl`) has the writer
    /// positioned at level 0; the method signature sits at depth 1
    /// inside the `impl`, and the body at depth 2.
    pub(crate) fn emit_method(&mut self, _class_decl: &juxc_ast::ClassDecl, method: &FnDecl) {
        // (Migrated to Writer indent-aware API)
        // Caller (`emit_class_decl`) is at level 0; method signature
        // sits at depth 1 (inside the `impl` block), body at depth 2.
        // Walk the body once to decide on `&self` vs `&mut self`. A
        // method that contains `this.field = …` (lvalue base is `this`)
        // needs a mutable receiver in Rust. The lvalue walker we use
        // for locals also recognizes `Expr::This` as a root.
        let body = method.body.as_ref();
        let needs_mut_self = body
            .map(|b| body_writes_to_this(b))
            .unwrap_or(false);

        self.w.indent_inc();
        self.w.emit_indent();
        self.emit_visibility(method.visibility);
        self.w.push_str("fn ");
        self.w.push_str(&method.name.text);
        // Method's own generic parameters (rare but supported).
        self.emit_generic_params(&method.generic_params);
        self.w.push('(');
        if needs_mut_self {
            self.w.push_str("&mut self");
        } else {
            self.w.push_str("&self");
        }
        for param in &method.params {
            self.w.push_str(", ");
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_type_as_rust(&param.ty);
        }
        self.w.push(')');
        match &method.return_type {
            ReturnType::Void => {}
            ReturnType::Type(t) => {
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
            ReturnType::AsyncType(_) => {
                self.w.push_str(" -> ()");
            }
        }
        self.w.push_str(" {\n");
        // Body sits at depth 2 — push one more level so
        // `emit_fn_body_at` sees the writer at the body depth.
        self.w.indent_inc();
        if let Some(body) = body {
            self.this_alias = Some("self".to_string());
            let mut muts = HashSet::new();
            collect_mutated_names(body, &mut muts, &self.user_mut_methods);
            self.mutated_in_fn = muts;
            // Track the declared return type so `return "lit";` in
            // a String-returning method picks up `.to_string()`.
            let saved = self.current_return_type.take();
            self.current_return_type = Some(method.return_type.clone());
            self.emit_fn_body_at(body, &method.return_type);
            self.current_return_type = saved;
            self.this_alias = None;
        } else {
            // Abstract method — no Jux body. Emit `unimplemented!()`
            // so the Rust compiler accepts the function and any
            // accidental call against the base class itself panics
            // clearly. Subclass overrides shadow this body via Rust's
            // inherent-method-shadowing-via-Deref behavior.
            self.w.emit_indent();
            self.w.push_str("unimplemented!(\"abstract method ");
            self.w.push_str(&method.name.text);
            self.w.push_str("\")\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }
}
