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
        // Track the enclosing class so `Expr::Path` emission can
        // rewrite a bare reference to a static field (`a` inside
        // `class Test` → `Test.a`) to the qualified form the
        // existing static-field lowering knows how to handle. We
        // restore the previous value at the end of emission so
        // nested-class scenarios (Phase-2) compose correctly.
        let prev_enclosing = self.enclosing_class.take();
        self.enclosing_class = Some(class_decl.name.text.clone());
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
            // Static fields live on the class, not the instance —
            // skip them here. They land below as `pub const` /
            // `pub static` items inside the `impl Foo { … }`.
            if field.is_static {
                continue;
            }
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

        // Static fields split two ways:
        //
        //   - `final` / `const` static → emitted as `pub const`
        //     inside this inherent impl, so `Foo::X` works in Rust.
        //   - Plain `static` (mutable) → lifted out and emitted at
        //     module scope as `LazyLock<Mutex<T>>` after the impl
        //     block, because Rust forbids `static` items inside
        //     `impl` blocks. See `emit_mutable_static_field` and
        //     the matching access path in `emit_field`.
        for field in &class_decl.fields {
            if !field.is_static {
                continue;
            }
            if field.is_final {
                self.emit_static_field(field);
            }
        }
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
            self.emit_method(method);
        }
        // **Method body inlining for virtual dispatch.** Walk the
        // `extends` chain and copy every concrete (non-abstract,
        // non-static) inherited method that THIS class doesn't
        // override into the class's own inherent impl. The copy
        // keeps the parent's body verbatim; `self` inside that
        // body is now `&mut Self` (where Self = the subclass),
        // so a call like `self.kind()` resolves to the subclass's
        // override via Rust's inherent-method-first method
        // resolution. Without this copy, `entity.describe()`
        // would Deref to the abstract parent's `describe`, where
        // `self.kind()` finds the parent's abstract stub instead
        // of the subclass override.
        if !class_decl.is_abstract {
            let mut own_method_names: std::collections::HashSet<String> = class_decl
                .methods
                .iter()
                .map(|m| m.name.text.clone())
                .collect();
            let mut cursor: Option<juxc_ast::TypeRef> = class_decl.extends.clone();
            while let Some(parent_ref) = cursor {
                let Some(seg) = parent_ref.name.segments.first() else { break };
                let bare = seg.text.as_str();
                // FQN-aware lookup against the class_asts map.
                let parent_decl: Option<juxc_ast::ClassDecl> = self
                    .class_asts
                    .get(bare)
                    .cloned()
                    .or_else(|| {
                        self.class_asts
                            .iter()
                            .find(|(k, _)| {
                                k.rsplit('.').next().unwrap_or(k.as_str()) == bare
                            })
                            .map(|(_, v)| v.clone())
                    });
                let Some(parent) = parent_decl else { break };
                let parent_methods = parent.methods.clone();
                let parent_extends = parent.extends.clone();
                for m in &parent_methods {
                    if own_method_names.contains(&m.name.text) {
                        continue; // overridden by this class (or a closer parent)
                    }
                    if m.body.is_none() {
                        // Abstract on the parent — the concrete
                        // subclass must override (rustc surfaces
                        // any miss).
                        continue;
                    }
                    if m.modifiers
                        .iter()
                        .any(|mo| matches!(mo, juxc_ast::FnModifier::Static))
                    {
                        continue;
                    }
                    own_method_names.insert(m.name.text.clone());
                    self.emit_method(m);
                }
                cursor = parent_extends;
            }
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

        // Mutable static fields — emitted at module scope as
        // `LazyLock<Mutex<T>>` because Rust forbids `static` items
        // inside `impl` blocks and unsynchronized mutable global
        // state requires `Sync`. Field access (`Foo.x` /
        // `Foo.x = …`) is routed to these in `emit_field` /
        // `emit_assign`. For generic classes, only statics whose
        // declared type doesn't reference the class's type
        // parameters can land at module scope (Java's rule:
        // a generic class's static field can't mention `T`).
        // Non-`T`-mentioning statics still emit cleanly.
        let generic_param_names: std::collections::HashSet<&str> = class_decl
            .generic_params
            .iter()
            .map(|p| p.name.text.as_str())
            .collect();
        for field in &class_decl.fields {
            if field.is_static && !field.is_final {
                if type_ref_mentions_any(&field.ty, &generic_param_names) {
                    continue;
                }
                self.emit_mutable_static_field(&class_decl.name.text, field);
            }
        }

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

        // Marker trait — `pub trait <Name>Kind {}` — and impls for
        // this class plus every ancestor in the chain. Lets `<T
        // extends ClassName>` bounds work for type restriction (the
        // bound rewriter in `emit_generic_params_with_clone_bound`
        // routes class-name bounds through `<Name>Kind`).
        //
        // **No `Clone` supertrait.** Generic bounds add `+ Clone`
        // separately at every use site, so the marker trait stays
        // dyn-compatible — `Box<dyn AnimalKind>` would otherwise
        // hit Rust's "Self: Sized" restriction on Clone and refuse
        // to be a trait object. Storage-position wildcards
        // (`List<? extends Animal>` as a local/field/return) rely
        // on this.
        self.emit_class_marker_trait(class_decl);
        // Restore the previous enclosing-class context. See the
        // matching `take` at the top of this function for the
        // bare-static-name rewrite this powers.
        self.enclosing_class = prev_enclosing;
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
        // pub trait <Name>Kind {} — no `Clone` supertrait so the
        // trait is dyn-compatible. Generic bounds add `+ Clone`
        // explicitly at use sites via
        // `emit_generic_params_with_clone_bound`.
        self.w.emit_indent();
        self.emit_visibility(class_decl.visibility);
        self.w.push_str("trait ");
        self.w.push_str(&class_decl.name.text);
        self.w.push_str("Kind {}\n");
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

        // Walk the ancestor chain using tycheck's pre-resolved
        // `extends_fqn` so cross-package extends find the right
        // parent ClassSig. For each ancestor we emit
        //   `impl <ancestor-marker-path> for <Child> {}`
        // where the marker path is `crate::a::b::AncestorKind` if
        // the ancestor lives in a different package, or just
        // `AncestorKind` for same-package (since the parent's mod
        // has already brought it into scope via the surrounding
        // `pub mod` nest).
        let child_fqn = self.classsig_lookup_fqn(&class_decl.name.text);
        let child_pkg = child_fqn
            .as_deref()
            .and_then(crate::backend_fqn::fqn_package)
            .unwrap_or("");
        let mut cursor_fqn: Option<String> = child_fqn
            .as_deref()
            .and_then(|f| self.symbols.classes.get(f))
            .and_then(|c| c.extends_fqn.clone());
        while let Some(ancestor_fqn) = cursor_fqn.clone() {
            let ancestor_bare = crate::backend_fqn::fqn_bare(&ancestor_fqn);
            let ancestor_pkg = crate::backend_fqn::fqn_package(&ancestor_fqn).unwrap_or("");

            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
            self.w.push(' ');
            if !ancestor_pkg.is_empty() && ancestor_pkg != child_pkg {
                // Different package — anchor the marker-trait path at
                // the crate root so it resolves through the
                // workspace's module nest.
                self.w.push_str("crate::");
                for seg in ancestor_pkg.split('.') {
                    self.w.push_str(seg);
                    self.w.push_str("::");
                }
            }
            self.w.push_str(ancestor_bare);
            self.w.push_str("Kind for ");
            self.w.push_str(&class_decl.name.text);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str(" {}\n");

            // Step up the chain.
            cursor_fqn = self
                .symbols
                .classes
                .get(&ancestor_fqn)
                .and_then(|c| c.extends_fqn.clone());
        }
        self.w.newline();
    }

    /// Find this class's FQN in the workspace symbol table by
    /// scanning for an entry whose bare name matches and whose
    /// package matches the unit currently being emitted. Returns
    /// `None` when the class isn't (yet) registered — happens
    /// during some isolated unit tests that bypass the symbol-
    /// table build.
    fn classsig_lookup_fqn(&self, bare: &str) -> Option<String> {
        for (fqn, sig) in &self.symbols.classes {
            if crate::backend_fqn::fqn_bare(fqn) == bare {
                // Phase 1 simplification: pick the first match. The
                // grammar caps `public` types per file so this only
                // disambiguates across package boundaries, which the
                // current emit doesn't yet need precisely (the
                // marker walk only consults FQNs that came from the
                // pre-resolved `extends_fqn`).
                let _ = sig;
                return Some(fqn.clone());
            }
        }
        None
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
        // Abstract classes don't emit trait impls — they would
        // produce an `impl Iface for AbstractC {}` with no method
        // bodies because the abstract methods have no concrete
        // implementation here, and rustc would reject the empty
        // impl with E0046. The trait-impl walk for each concrete
        // subclass rolls up `extends` so the abstract intermediate
        // still propagates its `implements` clause down to the
        // class that actually carries the method bodies.
        if class_decl.is_abstract {
            return;
        }
        // Concrete classes pick up interfaces from their own
        // `implements` clause AND from every ancestor in the
        // `extends` chain — Java's "an Employee IS-A Payable
        // because Person says so" rule. We walk the chain via the
        // symbol table since the AST only carries the class's own
        // `implements` list.
        let mut implements: Vec<juxc_ast::TypeRef> = class_decl.implements.clone();
        {
            let mut seen: std::collections::HashSet<String> = implements
                .iter()
                .filter_map(|t| t.name.segments.first().map(|s| s.text.clone()))
                .collect();
            // Walk parent chain via the bare-or-FQN helper so a
            // multi-package program (parent class keyed at its
            // FQN, source's `extends` clause carrying only the
            // bare name) still rolls inherited interfaces down to
            // the concrete subclass. Stop at the first missing
            // entry — tycheck already surfaced any broken chain.
            let mut cursor: Option<&juxc_ast::TypeRef> = class_decl.extends.as_ref();
            while let Some(parent_ref) = cursor {
                let Some(parent_name) = parent_ref.name.segments.first() else { break };
                let Some(parent_sig) = self.lookup_class_by_bare_or_fqn(parent_name.text.as_str())
                else { break };
                for inherited in &parent_sig.implements {
                    let Some(iface_seg) = inherited.name.segments.first() else { continue };
                    if seen.insert(iface_seg.text.clone()) {
                        implements.push(inherited.clone());
                    }
                }
                cursor = parent_sig.extends.as_ref();
            }
        }
        if implements.is_empty() {
            return;
        }
        let interfaces: Vec<juxc_ast::TypeRef> = implements;
        for interface_ty in &interfaces {
            // Interface name must be a single-segment path today —
            // imports and module-qualified interfaces are a future
            // extension.
            let Some(iface_name) = interface_ty.name.segments.first() else {
                continue;
            };
            // Build a name→TypeRef substitution from the interface's
            // generic params and the args the class supplied
            // (`implements Box<int>` → `T ↦ int`). Applied to each
            // emitted param/return type below so the trait impl
            // uses the class's concrete type args rather than the
            // interface's bare type parameter, which would otherwise
            // be out of scope inside `impl Trait for Class {}`.
            let iface_sig = self
                .lookup_interface_by_bare_or_fqn(iface_name.text.as_str())
                .map(|(_, i)| i);
            let mut type_subst: std::collections::HashMap<String, juxc_ast::TypeRef> =
                std::collections::HashMap::new();
            if let Some(iface) = iface_sig {
                for (param, arg) in iface
                    .generic_params
                    .iter()
                    .zip(interface_ty.generic_args.iter())
                {
                    // Wildcards in an `implements` clause don't make
                    // sense as concrete substitutions — skip them and
                    // let the emitted type carry through unchanged
                    // (rustc will surface anything we miss).
                    if let Some(arg_ty) = arg.as_type() {
                        type_subst.insert(param.name.text.clone(), arg_ty.clone());
                    }
                }
            }
            // Pull (name, MethodSig) pairs from the symbol table and
            // sort by name for deterministic emission order. Empty
            // when the interface isn't in the table (e.g. unresolved
            // name — tycheck would have already flagged that).
            let mut methods: Vec<(String, MethodSig)> = iface_sig
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
            // Filter to methods this class **overrides** — the
            // ones we need to emit a delegating impl for. Methods
            // the class doesn't define inherently inherit the
            // interface's default body via Rust's native trait
            // default mechanism (no delegation needed). Without
            // this filter every default method would recurse
            // infinitely: `fn greet(&self) { self.greet() }`. We
            // also drop static methods — they're emitted as free
            // functions adjacent to the trait, never as trait
            // items, so they have no place in `impl Trait for ...`.
            // Decide a call target for each trait method:
            //
            //   - **Empty string** → `self.method()` resolves to
            //     this class's inherent (no recursion because
            //     method resolution finds inherent first).
            //   - **Ancestor FQN** → `<crate::pkg::Parent>::method(self, args)`
            //     for methods whose body lives on an ancestor and
            //     this class doesn't override. Bypasses the
            //     trait-impl recursion that bare `self.method()`
            //     would trigger when there's no inherent.
            //   - **Absent from the map** → drop from `methods`,
            //     let Rust's trait default fire (interface
            //     method has a default body).
            //
            // **Caveat.** Ancestor delegation calls the parent's
            // inherent directly, so `self.something_else()`
            // inside the parent's body resolves against the
            // parent (not via virtual dispatch back to the
            // subclass). For methods that depend on virtual
            // dispatch of OTHER methods, mark the interface
            // method `default` so this path drops it and Rust's
            // trait default does the right thing.
            let class_method_names: std::collections::HashSet<&str> = class_decl
                .methods
                .iter()
                .map(|m| m.name.text.as_str())
                .collect();
            let mut method_targets: std::collections::HashMap<String, Option<String>> =
                std::collections::HashMap::new();
            for (name, sig) in &methods {
                if sig.is_static {
                    continue;
                }
                if class_method_names.contains(name.as_str()) {
                    method_targets.insert(name.clone(), Some(String::new()));
                    continue;
                }
                if sig.is_abstract {
                    // Abstract on the interface — must provide a
                    // body. Walk ancestors for an inherent.
                    let mut cursor: Option<&juxc_ast::TypeRef> = class_decl.extends.as_ref();
                    let mut found: Option<String> = None;
                    while let Some(parent_ref) = cursor {
                        let Some(seg) = parent_ref.name.segments.first() else { break };
                        let Some((fqn, parent_sig)) = self
                            .symbols
                            .classes
                            .iter()
                            .find(|(k, _)| {
                                k.as_str() == seg.text.as_str()
                                    || k.rsplit('.').next().unwrap_or(k.as_str())
                                        == seg.text.as_str()
                            })
                            .map(|(k, v)| (k.clone(), v))
                        else { break };
                        if let Some(parent_method) = parent_sig.methods.get(name.as_str()) {
                            // Skip abstract parent methods — they
                            // have no body to delegate to.
                            if !parent_method.is_abstract {
                                found = Some(fqn);
                                break;
                            }
                        }
                        cursor = parent_sig.extends.as_ref();
                    }
                    if let Some(fqn) = found {
                        method_targets.insert(name.clone(), Some(fqn));
                    }
                }
                // Default interface method that nobody overrides:
                // skip entry → Rust trait default fires.
            }
            methods.retain(|(name, _)| method_targets.contains_key(name));
            if methods.is_empty() {
                // The class implements the interface entirely via
                // default methods; we still emit an empty impl
                // block so `impl Trait for Class` registers and
                // trait-dispatch works.
                self.w.emit_indent();
                self.w.push_str("impl");
                self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
                self.w.push(' ');
                self.emit_type_as_rust(interface_ty);
                self.w.push_str(" for ");
                self.w.push_str(&class_decl.name.text);
                self.emit_generic_params_as_args(&class_decl.generic_params);
                self.w.push_str(" {}\n\n");
                continue;
            }
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
                // `&self` matches the interface's declared
                // receiver. When an implementing class's inherent
                // method needs `&mut self` (because it writes to
                // `this.field`), the user must drop `implements
                // Iface` and use inherent methods directly —
                // proper resolution awaits interior-mutability
                // wrapping in a future pass.
                self.w.push_str("(&self");
                // `MethodSig.params` is `Vec<ParamSig>`; ParamSig
                // carries `name: String` (not `Ident`) and `ty: TypeRef`.
                // Substituting the interface's type params with the
                // class's `implements` args here keeps `impl Box<isize>
                // for IntBox { fn unwrap(&self) -> isize }` instead of
                // leaving `T` floating free in the impl scope.
                for param in &method.params {
                    self.w.push_str(", ");
                    self.w.push_str(&param.name);
                    self.w.push_str(": ");
                    let subst = substitute_type_ref(&param.ty, &type_subst);
                    self.emit_type_as_rust(&subst);
                }
                self.w.push(')');
                match &method.return_type {
                    ReturnType::Void => {}
                    ReturnType::Type(t) => {
                        self.w.push_str(" -> ");
                        let subst = substitute_type_ref(t, &type_subst);
                        self.emit_return_type_as_rust(&subst);
                    }
                    ReturnType::AsyncType(_) => {
                        self.w.push_str(" -> ()");
                    }
                }
                // Delegating body. Two shapes per the method_targets
                // table built above:
                //
                //   - **Empty target** (`Some("")`) → method lives
                //     on this class inherently; emit `self.X(args)`
                //     which method-resolves to the inherent first.
                //   - **Ancestor FQN target** → method lives on a
                //     parent class; emit
                //     `<crate::pkg::Parent>::X(self, args)` to
                //     bypass the trait method (which is this
                //     impl's own — using `self.X()` would recurse).
                self.w.push_str(" {\n");
                self.w.indent_inc();
                self.w.emit_indent();
                let target = method_targets.get(method_name.as_str()).cloned().flatten();
                match target {
                    Some(ref fqn) if !fqn.is_empty() => {
                        // Cross-package: FQN-rooted path, `crate::`
                        // when the FQN has a package portion.
                        if fqn.contains('.') {
                            self.w.push_str("crate::");
                        }
                        let parent_path: String =
                            fqn.split('.').collect::<Vec<_>>().join("::");
                        self.w.push_str(&parent_path);
                        self.w.push_str("::");
                        self.w.push_str(method_name);
                        self.w.push_str("(self");
                        for param in &method.params {
                            self.w.push_str(", ");
                            self.w.push_str(&param.name);
                        }
                        self.w.push(')');
                    }
                    _ => {
                        self.w.push_str("self.");
                        self.w.push_str(method_name);
                        self.w.push('(');
                        for (i, param) in method.params.iter().enumerate() {
                            if i > 0 {
                                self.w.push_str(", ");
                            }
                            self.w.push_str(&param.name);
                        }
                        self.w.push(')');
                    }
                }
                self.w.push('\n');
                self.w.indent_dec();
                self.w.line("}");
            }
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
        }
    }

    /// Emit one instance method as an inherent function inside the
    /// class's `impl` block. Caller (`emit_class_decl` or
    /// `emit_record_decl`) has the writer positioned at level 0; the
    /// method signature sits at depth 1 inside the `impl`, and the
    /// body at depth 2. Method emission is host-agnostic — the same
    /// shape works for classes and records.
    /// Emit a `static` class field as an associated item inside the
    /// inherent `impl` block. `final` / `const` fields land as
    /// `pub const`; bare `static` fields emit `pub static`.
    ///
    /// **Phase-1 caveat.** A static field without an initializer
    /// would need `Default::default()` evaluated at compile time,
    /// which Rust doesn't permit. We emit a TODO marker — the
    /// resulting Rust won't compile, but `cargo build` produces a
    /// clear error pointing at the field. A future pass either
    /// rejects this at tycheck or routes through `lazy_static!`.
    pub(crate) fn emit_static_field(&mut self, field: &juxc_ast::FieldDecl) {
        self.w.indent_inc();
        self.w.emit_indent();
        self.emit_visibility(field.visibility);
        if field.is_final {
            self.w.push_str("const ");
        } else {
            self.w.push_str("static ");
        }
        self.w.push_str(&field.name.text);
        self.w.push_str(": ");
        // `const`/`static` slots can't run `.to_string()` at init
        // time, so the const-context flag asks both the type
        // emitter (`String` → `&'static str`) and `emit_literal`
        // (suppress the Fix-1 `.to_string()` wrap) to stay
        // const-evaluatable. See `emit_const_decl` for the
        // top-level mirror.
        self.emitting_const_context = true;
        self.emit_field_type_as_rust(&field.ty);
        self.w.push_str(" = ");
        if let Some(init) = &field.default {
            self.emit_expr(init);
        } else {
            // No initializer — Rust requires one at the const/static
            // site. Emit a placeholder so the build fails with a
            // clear error rather than silently producing wrong code.
            self.emit_field_default_value_for(&field.ty);
        }
        self.emitting_const_context = false;
        self.w.push_str(";\n");
        self.w.indent_dec();
    }

    /// Emit a non-`final` `static` class field as a module-scope
    /// `LazyLock<Mutex<T>>` named `<Class>_<field>`.
    ///
    /// Rust forbids `static` items inside `impl` blocks and requires
    /// any global mutable state to be `Sync`. `LazyLock<Mutex<T>>`
    /// satisfies both: the `LazyLock` defers initializer evaluation
    /// to first access (so runtime-allocated initializers like
    /// `new Foo()` work), and the `Mutex` provides interior
    /// mutability with `Sync` for free. The cost is one lock per
    /// access, which is acceptable for Phase-1 — perf-sensitive
    /// users have `final` for the const path.
    ///
    /// **Access pattern (mirrored in `emit_field` / `emit_assign`):**
    ///
    /// - Read  (`Foo.x`)   → `Foo_x.lock().unwrap().clone()`
    /// - Write (`Foo.x = e`) → `*Foo_x.lock().unwrap() = e`
    ///
    /// Caller-positioned at depth 0; this emits the declaration at
    /// depth 0 too (module scope, not nested in any impl).
    pub(crate) fn emit_mutable_static_field(
        &mut self,
        class_name: &str,
        field: &juxc_ast::FieldDecl,
    ) {
        self.w.emit_indent();
        // `pub` so cross-package reads (`other_pkg.MyClass.x`) can see
        // the module-scope static through the emitted `pub mod`
        // package tree. Visibility checks already gate access at the
        // Jux level (tycheck E0414/E0415/E0416); the Rust pub is
        // the structural minimum that lets the path resolve.
        self.w.push_str("pub static ");
        self.w.push_str(class_name);
        self.w.push('_');
        self.w.push_str(&field.name.text);
        self.w.push_str(": std::sync::LazyLock<std::sync::Mutex<");
        // Field-position type mapping (`String` → owned `String`) —
        // we want the inner storage to own its data, just like a
        // regular instance field would.
        self.emit_field_type_as_rust(&field.ty);
        self.w.push_str(">> = std::sync::LazyLock::new(|| std::sync::Mutex::new(");
        if let Some(init) = &field.default {
            // Not in const-context here — runtime allocation is fine
            // because the closure runs on first access, not at link
            // time. So `String` literals can keep their normal
            // `.to_string()` wrap and `new Foo(…)` works as expected.
            self.emit_expr(init);
        } else {
            self.emit_field_default_value_for(&field.ty);
        }
        self.w.push_str("));\n");
    }

    pub(crate) fn emit_method(&mut self, method: &FnDecl) {
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

        // Wildcard-lift pre-pass (same rule as `emit_fn_decl`):
        // promote each `? extends T` / `? super T` / `?` in a param
        // type to a synthetic `__Wn` generic on this method with the
        // matching bound.
        let mut lifter = crate::analysis::WildcardLifter::new();
        let lifted_param_tys: Vec<juxc_ast::TypeRef> = method
            .params
            .iter()
            .map(|p| {
                if crate::analysis::type_ref_has_wildcard(&p.ty) {
                    lifter.rewrite_type_ref(&p.ty)
                } else {
                    p.ty.clone()
                }
            })
            .collect();
        let mut combined_method_generics = method.generic_params.clone();
        combined_method_generics.extend(lifter.new_params.iter().cloned());

        self.w.indent_inc();
        self.w.emit_indent();
        self.emit_visibility(method.visibility);
        self.w.push_str("fn ");
        self.w.push_str(&method.name.text);
        // Method's own generic parameters plus any synthetic wildcards.
        if combined_method_generics.is_empty() {
            self.emit_generic_params(&method.generic_params);
        } else {
            self.emit_generic_params_with_clone_bound(&combined_method_generics);
        }
        self.w.push('(');
        // Static methods have no implicit receiver in Rust either —
        // skip the `&self` / `&mut self` slot so callers do
        // `Foo::method(args)` directly.
        let is_static = method
            .modifiers
            .iter()
            .any(|m| matches!(m, juxc_ast::FnModifier::Static));
        let mut first_param = true;
        if !is_static {
            if needs_mut_self {
                self.w.push_str("&mut self");
            } else {
                self.w.push_str("&self");
            }
            first_param = false;
        }
        for (i, param) in method.params.iter().enumerate() {
            if !first_param {
                self.w.push_str(", ");
            }
            first_param = false;
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_type_as_rust(&lifted_param_tys[i]);
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
            // Static methods have no `self` — leave `this_alias`
            // unset so an accidental `this` in the body produces a
            // visible Rust error (tycheck E0425 catches it first,
            // but defense-in-depth).
            if !is_static {
                self.this_alias = Some("self".to_string());
            }
            let mut muts = HashSet::new();
            collect_mutated_names(body, &mut muts, &self.user_mut_methods);
            self.mutated_in_fn = muts;
            // Seed nullable-locals from this method's params so
            // value-consuming sites in the body know which paths
            // are already `Option<T>` shape. Reset first to drop
            // any leftover entries from a previous fn's body.
            self.nullable_locals.clear();
            for p in &method.params {
                if p.ty.nullable {
                    self.nullable_locals.insert(p.name.text.clone());
                }
            }
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

/// Walk `ty` and substitute any name in `subst` with its
/// replacement TypeRef. The substitution is structural — generic
/// args, array element, and fn-shape param/return types all
/// recurse. Used by `emit_class_trait_impls` to propagate the
/// class's `implements Box<int>` choice down to the trait
/// method's `T` references so the emitted Rust doesn't dangle a
/// free `T` inside the impl scope.
/// True iff `ty` (or any of its nested generic args / array element /
/// fn-shape param/return types) names any identifier in `names`.
/// Used to gate mutable-static emission on generic classes — a
/// static whose type mentions the class's `T` can't lift to module
/// scope since `T` isn't in scope there.
fn type_ref_mentions_any(
    ty: &juxc_ast::TypeRef,
    names: &std::collections::HashSet<&str>,
) -> bool {
    if ty.name.segments.len() == 1
        && names.contains(ty.name.segments[0].text.as_str())
    {
        return true;
    }
    for arg in &ty.generic_args {
        if let Some(t) = arg.as_type() {
            if type_ref_mentions_any(t, names) {
                return true;
            }
        }
    }
    if let Some(fn_shape) = &ty.fn_shape {
        for p in &fn_shape.params {
            if type_ref_mentions_any(p, names) {
                return true;
            }
        }
        if type_ref_mentions_any(&fn_shape.return_type, names) {
            return true;
        }
    }
    false
}

fn substitute_type_ref(
    ty: &juxc_ast::TypeRef,
    subst: &std::collections::HashMap<String, juxc_ast::TypeRef>,
) -> juxc_ast::TypeRef {
    // Single-segment names with no generic args or shape: bare
    // type-parameter reference. Look it up in the table and
    // return the replacement directly — but preserve the
    // outer ty's `nullable` flag so `T?` becomes `Replacement?`,
    // not just `Replacement`.
    if ty.fn_shape.is_none()
        && ty.array_shape.is_none()
        && ty.generic_args.is_empty()
        && ty.name.segments.len() == 1
    {
        let key = &ty.name.segments[0].text;
        if let Some(replacement) = subst.get(key) {
            let mut out = replacement.clone();
            if ty.nullable {
                out.nullable = true;
            }
            return out;
        }
    }
    // Recurse into composite shapes. Generic-arg wildcards keep
    // their original shape; only the `Type(...)` variant carries a
    // TypeRef that can be substituted.
    let generic_args: Vec<juxc_ast::GenericArg> = ty
        .generic_args
        .iter()
        .map(|a| match a {
            juxc_ast::GenericArg::Type(t) => {
                juxc_ast::GenericArg::Type(substitute_type_ref(t, subst))
            }
            other => other.clone(),
        })
        .collect();
    let fn_shape = ty.fn_shape.as_ref().map(|fs| {
        Box::new(juxc_ast::FnTypeShape {
            params: fs
                .params
                .iter()
                .map(|p| substitute_type_ref(p, subst))
                .collect(),
            return_type: substitute_type_ref(&fs.return_type, subst),
            is_async: fs.is_async,
            throws: fs.throws.clone(),
        })
    });
    juxc_ast::TypeRef {
        name: ty.name.clone(),
        generic_args,
        nullable: ty.nullable,
        array_shape: ty.array_shape.clone(),
        fn_shape,
        span: ty.span,
    }
}
