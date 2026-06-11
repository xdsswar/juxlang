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
        // **Stdlib intrinsic skip.** The Jux source files under
        // `jux.std/collections/` declare `ArrayList<T>` and
        // `HashMap<K, V>` as ordinary classes, but their bodies
        // are placeholders — every method is replaced by the
        // backend's existing `BUILTIN_ARRAY_METHODS` /
        // `BUILTIN_MAP_METHODS` dispatch, which lowers operations
        // onto Rust's `Vec` / `HashMap` directly. Suppress the
        // struct emission for these so we don't end up with two
        // competing definitions, and the user gets the std
        // container's full API via the dispatch table.
        //
        // The check uses the FQN form `jux.std.collections.X`
        // built from the unit's package + class name. Single-
        // segment names (without a `package` declaration) can't
        // collide with the stdlib so they fall through normally.
        // **Stdlib intrinsic skip.** A small fixed set of stdlib
        // class names lower to Rust std containers — the Jux
        // source files under `jux.std/*` document their API but
        // the compiler owns the actual implementation. Suppress
        // struct emission for those so we don't end up with a
        // duplicate definition next to the std container.
        if class_decl.name.text == "ArrayList"
            || class_decl.name.text == "HashMap"
            || class_decl.name.text == "HashSet"
            || class_decl.name.text == "Deque"
        {
            let pkg = self.symbols.package.join(".");
            if pkg == "jux.std.collections" {
                return;
            }
        }
        if class_decl.name.text == "File"
            || class_decl.name.text == "Path"
            || class_decl.name.text == "Console"
        {
            let pkg = self.symbols.package.join(".");
            if pkg == "jux.std.io" {
                return;
            }
        }
        if class_decl.name.text == "Worker"
            || class_decl.name.text == "Task"
            || class_decl.name.text == "AtomicInt"
            || class_decl.name.text == "AtomicLong"
        {
            let pkg = self.symbols.package.join(".");
            if pkg == "jux.std.concurrent" {
                return;
            }
        }
        if class_decl.name.text == "Clock" || class_decl.name.text == "Instant" {
            let pkg = self.symbols.package.join(".");
            if pkg == "jux.std.time" {
                return;
            }
        }
        // **Sealed-class lowering.** A `sealed class Light permits
        // Red, Yellow, Green {}` becomes a Rust enum whose variants
        // wrap each permitted subclass struct:
        //
        // ```rust
        // pub enum Light { Red(Red), Yellow(Yellow), Green(Green) }
        // impl From<Red> for Light { ... }
        // ```
        //
        // The subclass declarations themselves still emit as
        // normal structs, but with `__parent: Light` *omitted* —
        // they ARE the variant, they don't contain one. This is
        // what makes Java upcasting actually work: `new Red(30)`
        // followed by `.into()` produces `Light::Red(Red{..})`
        // which carries the subclass's identity through any slot
        // typed as `Light`.
        //
        // Any sealed class with a non-empty permits list lowers
        // as a Rust enum so upcasting actually carries the
        // subclass's identity through function boundaries. When
        // the sealed parent has its own methods, the enum's
        // inherent impl block emits a match-dispatching wrapper
        // for each method — `Shape::describe(&self)` becomes
        // `match self { Shape::Circle(c) => c.describe(), … }`,
        // and each subclass picks up the inherited body through
        // the existing method-inlining pass.
        if class_decl.is_sealed && !class_decl.permits.is_empty() {
            self.emit_sealed_enum(class_decl);
            return;
        }
        // (Migrated to Writer indent-aware API)
        // Track the enclosing class so `Expr::Path` emission can
        // rewrite a bare reference to a static field (`a` inside
        // `class Test` → `Test.a`) to the qualified form the
        // existing static-field lowering knows how to handle. We
        // restore the previous value at the end of emission so
        // nested-class scenarios (Phase-2) compose correctly.
        let prev_enclosing = self.enclosing_class.take();
        self.enclosing_class = Some(class_decl.name.text.clone());
        let prev_has_static_init = self.emitting_class_has_static_init;
        self.emitting_class_has_static_init = !class_decl.static_init_blocks.is_empty();
        // `int`-typed const-generic params (`<int N>`) are visible to
        // every body in the class; bare value reads of `N` emit
        // `(N as isize)` (see `const_int_params`). Restored at the end
        // alongside `enclosing_class`.
        let prev_const_ints = std::mem::take(&mut self.const_int_params);
        self.const_int_params = crate::collect_const_int_params(&class_decl.generic_params);
        let prev_type_params = std::mem::take(&mut self.current_type_params);
        self.current_type_params = crate::collect_type_param_names(&class_decl.generic_params);
        // **Wrapper-shape branch (Phase A, §CR.4.1 / §CR.5.1 / §CR.6).**
        // Classes in `wrapper_classes` lower to the shared-mutation,
        // interior-mutable wrapper shape so Jux gets Java reference
        // semantics: every alias of an instance shares one
        // `Rc<RefCell<C_Inner>>`, and a mutation through any handle is
        // visible through all of them. The set is computed globally by
        // `compute_wrapper_classes`, which already excludes sealed /
        // generic / exception / intrinsic classes and rolls each
        // non-sealed `extends` hierarchy up as a unit — so member ship
        // is the only gate needed here. Both leaf simple classes AND
        // hierarchy members (incl. abstract parents) flow into
        // `emit_wrapper_class_decl`, which branches on `extends`.
        if self.wrapper_classes.contains(&class_decl.name.text) {
            self.emit_wrapper_class_decl(class_decl);
            self.enclosing_class = prev_enclosing;
            self.emitting_class_has_static_init = prev_has_static_init;
            self.const_int_params = prev_const_ints;
            self.current_type_params = prev_type_params;
            return;
        }
        // Derive Clone unconditionally so the `T: Clone` bound used on
        // generic impls (and the auto-`.clone()` injected on field
        // reads) keeps working when the user nests classes — `Box<User>`
        // needs `User: Clone`, which falls out for free here.
        // Debug joins Clone so `format!("{:?}", obj)` works for any
        // class — used by `throw` lowering (panic-payload format)
        // and by user code that wants a quick diagnostic dump.
        // Classes whose fields don't implement Debug will surface
        // a clean rustc error pointing at the offending field.
        self.w.line("#[derive(Clone, Debug)]");
        // pub struct Name<T, U> { …fields… }
        self.w.emit_indent();
        self.emit_visibility(class_decl.visibility);
        self.w.push_str("struct ");
        self.w.push_str(&class_decl.name.text);
        // The struct's own type params carry `Clone + Debug` (the `#[derive]`
        // above needs them, and a generic field `Box<T>` propagates Box's own
        // `T: Clone + Debug` bound) — same as the wrapper-inner struct and every
        // impl header for this class. Without it the declaration is bare `<T>`
        // while the field/derive demand the bound (rustc E0277).
        self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        // Inheritance: embed the parent struct as `__parent`. Field
        // access on the child auto-dereffs through `impl Deref<Target=Parent>`
        // (emitted below), so `child.parent_field` and inherited
        // method calls Just Work. Always emit `__parent` first so the
        // struct layout is consistent across the hierarchy.
        // Sealed-parent detection: when the parent is a sealed
        // class, this class IS one of the parent enum's variants
        // — there's no struct to embed. Skip the `__parent` field
        // and the Deref impls (those are only meaningful for the
        // value-class hierarchy).
        let parent_is_sealed = class_decl
            .extends
            .as_ref()
            .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
            .and_then(|bare| self.lookup_class_by_bare_or_fqn(bare).map(|c| c.is_sealed))
            .unwrap_or(false);
        if let Some(parent_ty) = &class_decl.extends {
            if !parent_is_sealed {
                self.w.emit_indent();
                // `pub` so cross-package consumers can reach the
                // slice — catch-clause upcasts (`(*payload).__parent`)
                // run in the CATCHING package, not the declaring one.
                self.w.push_str("pub __parent: ");
                self.emit_type_as_rust(parent_ty);
                self.w.push_str(",\n");
            }
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
            self.emit_field_type_as_rust(&juxc_tycheck::resolved_field_type(field));
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();

        // Auto-`From<Sub> for Parent` for **non-sealed open
        // hierarchies**. Extracts the parent slice from the
        // subclass via `__parent`, giving the user a working
        // `void greet(Animal a) { } ;  greet(new Dog(...))` shape
        // at the cost of dropping the subclass's identity at the
        // upcast boundary. Phase-1 limitation: methods overridden
        // in the subclass DO NOT fire after upcasting through
        // this conversion — Java's virtual dispatch through value
        // types isn't expressible without dyn dispatch, which is
        // a larger refactor (each class's marker trait would have
        // to carry method signatures).
        //
        // **Recommended idiom for full polymorphism: declare the
        // parent `sealed` and list permits.** That path uses the
        // enum lowering and preserves subclass identity.
        if let Some(parent_ty) = &class_decl.extends {
            if !parent_is_sealed {
                if let Some(parent_bare) = parent_ty
                    .name
                    .segments
                    .last()
                    .map(|s| s.text.as_str())
                {
                    // Skip the slicing upcast for a **polymorphic base**: a
                    // `Parent`-typed slot is now `Rc<dyn ParentKind>`, and the
                    // upcast wraps (`Rc::new(child) as Rc<dyn ParentKind>`,
                    // identity-preserving) instead of extracting `__parent`.
                    if !self.poly_base_classes.contains(parent_bare) {
                        self.w.emit_indent();
                        self.w.push_str("impl From<");
                        self.w.push_str(&class_decl.name.text);
                        self.emit_generic_params_as_args(&class_decl.generic_params);
                        self.w.push_str("> for ");
                        // Route through the type emitter (NOT the bare
                        // name) so a cross-package parent gets its
                        // `crate::…` rooting — `extends Exception`
                        // reaches `crate::jux::std::exceptions::
                        // Exception`, same as the Deref target below.
                        self.emit_type_as_rust(parent_ty);
                        self.w.push_str(" { fn from(v: ");
                        self.w.push_str(&class_decl.name.text);
                        self.emit_generic_params_as_args(&class_decl.generic_params);
                        self.w.push_str(") -> Self { v.__parent } }\n");
                    }
                }
            }
        }
        // Emit Deref + DerefMut impls for child classes so inherited
        // methods and field access flow through Rust's auto-deref —
        // `child.method()` finds methods on the parent transparently,
        // `child.parent_field = x` works via DerefMut, etc.
        //
        // Skipped when the parent is `sealed` — those parents lower
        // as Rust enums, not structs, and the subclass is just one
        // of the variants. There's nothing to deref *to*.
        if let Some(parent_ty) = &class_decl.extends {
            if parent_is_sealed {
                // Sealed parent: no struct, no Deref impl.
                // Continue past this block.
            } else {
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
            } // end else (parent_is_sealed)
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
        // A generic param that's formatted in a method body
        // (interpolation / print / concat) additionally needs
        // `std::fmt::Display` on the inherent impl so the emitted
        // `format!`/`println!` type-checks. Only the formatted params
        // pick up the bound; purely-stored params keep `Clone + Debug`.
        let displayed = self.class_displayed_generic_params(class_decl);
        let defaulted = Self::class_default_bound_params(class_decl);
        self.emit_generic_params_with_clone_bound_plus_display(
            &class_decl.generic_params,
            &displayed,
            &defaulted,
        );
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
            if field.is_final
                && !self.final_static_needs_runtime_init(&juxc_tycheck::resolved_field_type(field))
            {
                self.emit_static_field(field);
            }
        }
        // Constructor → `pub fn new(args) -> Self` with the __self pattern.
        for (idx, ctor) in class_decl.constructors.iter().enumerate() {
            self.emit_constructor(class_decl, ctor, idx);
        }
        // If no constructor was declared, synthesize an implicit zero-
        // arg `new()` per §7.3.1 (declaring any constructor removes it).
        if class_decl.constructors.is_empty() {
            self.emit_synthetic_default_constructor(class_decl);
        }
        // `static { }` first-use initializer (§S.4.1), if any.
        self.emit_static_init_fn(class_decl);
        let mut seen_names: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for method in &class_decl.methods {
            let occ = seen_names.entry(method.name.text.clone()).or_insert(0);
            if *occ > 0 {
                self.pending_decl_suffix = Some(format!("__ov{occ}"));
            }
            *occ += 1;
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
            // `final` statics normally emit as `pub const` associated
            // items — EXCEPT when the payload is `!Send`/non-const
            // (a wrapper-class object): those route here so the
            // thread_local form carries them (rustc E0015 otherwise).
            let final_needs_tl = field.is_final
                && self.final_static_needs_runtime_init(&juxc_tycheck::resolved_field_type(field));
            if field.is_static && (!field.is_final || final_needs_tl) {
                if type_ref_mentions_any(&juxc_tycheck::resolved_field_type(field), &generic_param_names) {
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
            // §O.4.1 identity default: no `operator string` → the
            // class still prints, as `ClassName@<addr>`.
            let has_to_string = class_decl
                .operators
                .iter()
                .any(|o| o.kind == OperatorKind::ToString && !o.is_deleted);
            if !has_to_string {
                self.emit_identity_display(&class_decl.name.text, false);
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
        self.emitting_class_has_static_init = prev_has_static_init;
        self.const_int_params = prev_const_ints;
        self.current_type_params = prev_type_params;
    }

    /// Emit a **simple** class in the shared-mutation wrapper shape
    /// (class-representation addendum §CR.4.1 / §CR.6).
    ///
    /// Output shape for `class C { int v; C(int v){…} void set(int v){…} }`:
    ///
    /// ```text
    /// #[derive(Clone, Debug)]
    /// pub struct C_Inner { pub v: isize }
    /// #[derive(Clone, Debug)]
    /// pub struct C(std::rc::Rc<std::cell::RefCell<C_Inner>>);
    /// impl C {
    ///     pub fn new(v: isize) -> C {
    ///         C(std::rc::Rc::new(std::cell::RefCell::new(C_Inner { v })))
    ///     }
    ///     pub fn set(&self, v: isize) { self.0.borrow_mut().v = v; }
    /// }
    /// ```
    ///
    /// The `C` newtype IS the user-visible class type, so every
    /// `C`-typed field / param / return / local stays spelled `C`
    /// (type emission is unchanged). `#[derive(Clone)]` on the
    /// newtype is the cheap `Rc` refcount bump that gives "assignment
    /// shares" semantics; `Debug` flows through `Rc<RefCell<T>>: Debug`
    /// when `C_Inner: Debug`.
    ///
    /// Phase A only handles simple classes — the caller
    /// ([`Self::emit_class_decl`]) gates entry on
    /// [`crate::class_decl_uses_wrapper`], so there's no `extends`,
    /// `sealed`, generic, or abstract handling here.
    pub(crate) fn emit_wrapper_class_decl(&mut self, class_decl: &juxc_ast::ClassDecl) {
        let name = &class_decl.name.text;
        let inner = format!("{name}_Inner");

        // ---- C_Inner: the instance fields ----
        // Debug joins Clone so the newtype's derived Debug resolves
        // (`Rc<RefCell<C_Inner>>: Debug` requires `C_Inner: Debug`).
        self.w.line("#[derive(Clone, Debug)]");
        self.w.emit_indent();
        // The inner struct is `pub` so the wrapper (and any
        // same-crate path) can name it; the user-facing visibility
        // lives on the newtype below.
        //
        // **Generics (Phase A GENERICS pass).** A generic class
        // `class Box<T> { T value; }` lowers its inner to
        // `pub struct Box_Inner<T: Clone> { value: T }`. The `T: Clone`
        // bound mirrors the legacy generic-class path — reads of a
        // generic-typed field auto-`.clone()`, so the param has to carry
        // the bound (a `#[derive(Clone)]` on a generic struct only Clones
        // when its params do). Non-generic classes emit no `<…>` at all.
        self.w.push_str("pub struct ");
        self.w.push_str(&inner);
        self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        // **Inheritance embed (§CR.3.5 / §CR.5.1).** When this wrapper
        // class extends another wrapper class, embed the parent's
        // *inner* struct as `__parent` — NOT the parent's wrapper
        // newtype. Embedding the inner means a `Child` handle's single
        // `Rc<RefCell<Child_Inner>>` owns the whole flattened state
        // (parent slice + child fields) in one cell, so a mutation
        // through any alias is visible through all of them and through
        // inherited-field access. (Embedding the wrapper would give the
        // parent slice its OWN `Rc<RefCell<...>>`, splitting identity.)
        if let Some(parent_ty) = &class_decl.extends {
            if let Some(seg) = parent_ty.name.segments.last() {
                self.w.emit_indent();
                // `pub` for the same cross-package reach as the plain
                // class shape (catch upcasts, subclass chains).
                self.w.push_str("pub __parent: ");
                self.w.push_str(&seg.text);
                self.w.push_str("_Inner");
                // Thread the parent's generic args onto its inner type
                // (`extends Container<int>` → `__parent: Container_Inner<isize>`).
                // Without this, a child that binds its parent's type
                // parameter to a concrete type (`IntBox extends
                // Container<int>`) would reference a bare `Container_Inner`
                // and rustc would demand the missing `<…>`.
                self.emit_parent_inner_generic_args(parent_ty);
                self.w.push_str(",\n");
            }
        }
        for field in &class_decl.fields {
            // Static fields live on the class, not the instance — they
            // emit as `pub const` / module-scope `LazyLock<Mutex<T>>`
            // below, same as the legacy path.
            if field.is_static {
                continue;
            }
            self.w.emit_indent();
            self.emit_visibility(field.visibility);
            self.w.push_str(&field.name.text);
            self.w.push_str(": ");
            self.emit_field_type_as_rust(&juxc_tycheck::resolved_field_type(field));
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();

        // ---- the newtype handle: C<T>(Rc<RefCell<C_Inner<T>>>) ----
        // The newtype declares the generic params (with the `T: Clone`
        // bound so the derived `Clone` resolves) and threads them onto
        // the inner type inside the `Rc<RefCell<…>>`. `Debug` is *not*
        // bounded here — it flows through `Rc<RefCell<C_Inner<T>>>: Debug`
        // whenever `C_Inner<T>: Debug`, which holds when `T: Debug`; the
        // derive emits the right `where` clause for us.
        self.w.line("#[derive(Clone, Debug)]");
        self.w.emit_indent();
        self.emit_visibility(class_decl.visibility);
        self.w.push_str("struct ");
        self.w.push_str(name);
        self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
        self.w.push_str("(std::rc::Rc<std::cell::RefCell<");
        self.w.push_str(&inner);
        self.emit_generic_params_as_args(&class_decl.generic_params);
        self.w.push_str(">>);\n");
        self.w.newline();

        // ---- impl[<T: Clone>] C<T> { … } ----
        self.w.emit_indent();
        self.w.push_str("impl");
        // Generic params formatted in a method body (interpolation /
        // print / concat) additionally get a `std::fmt::Display` bound
        // so the emitted `format!`/`println!` type-checks — Jux
        // toString/interpolation semantics require a printed generic
        // field's instantiated type to be `Display`. Purely-stored
        // params keep only `Clone + Debug`.
        let displayed = self.class_displayed_generic_params(class_decl);
        let defaulted = Self::class_default_bound_params(class_decl);
        self.emit_generic_params_with_clone_bound_plus_display(
            &class_decl.generic_params,
            &displayed,
            &defaulted,
        );
        self.w.push(' ');
        self.w.push_str(name);
        self.emit_generic_params_as_args(&class_decl.generic_params);
        self.w.push_str(" {\n");

        // Static `final` fields → `pub const` associated items, same
        // as the legacy path.
        for field in &class_decl.fields {
            if field.is_static
                && field.is_final
                && !self.final_static_needs_runtime_init(&juxc_tycheck::resolved_field_type(field))
            {
                self.emit_static_field(field);
            }
        }

        // Constructors / methods / operators run with the wrapper flag
        // set so their bodies adopt the interior-mutability lowering.
        let prev_wrapper = self.emitting_wrapper_class;
        self.emitting_wrapper_class = true;
        for (idx, ctor) in class_decl.constructors.iter().enumerate() {
            self.emit_wrapper_constructor(class_decl, ctor, idx);
        }
        if class_decl.constructors.is_empty() {
            self.emit_wrapper_synthetic_default_constructor(class_decl);
        }
        // `static { }` first-use initializer (§S.4.1), if any.
        self.emit_static_init_fn(class_decl);
        let mut seen_names: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for method in &class_decl.methods {
            let occ = seen_names.entry(method.name.text.clone()).or_insert(0);
            if *occ > 0 {
                self.pending_decl_suffix = Some(format!("__ov{occ}"));
            }
            *occ += 1;
            self.emit_method(method);
        }
        // **Inherited-method inlining (§CR.5.1).** Same pass the legacy
        // path runs: walk the `extends` chain and copy every concrete
        // (non-abstract, non-static) inherited method this class
        // doesn't override into its own inherent impl. Because the
        // wrapper hierarchy has NO `Deref` (the `__parent` slot is the
        // parent's inner struct, not a wrapper to deref *to*), this
        // copy is what makes `child.inheritedMethod()` resolve at all.
        // The copied body's `this`/`self` field accesses walk the
        // `__parent` chain via the depth logic in `emit_field` /
        // `emit_assign` (keyed on the current `enclosing_class`).
        if !class_decl.is_abstract {
            self.emit_inherited_wrapper_methods(class_decl);
        }
        // `super.method()` shims (§6.9.4): for each method this class
        // overrides, emit the nearest concrete ancestor's body under
        // `__jux_super_<m>` so a `super.m()` call dispatches statically to it.
        self.emit_super_shims(class_decl);
        for op in &class_decl.operators {
            self.emit_operator_as_method(op);
        }
        self.emitting_wrapper_class = prev_wrapper;
        self.w.line("}");
        self.w.newline();

        // **Upcast `From<Child> for Parent` (§CR.3.5).** Clone the
        // parent slice out of the child's inner and wrap it in a fresh
        // `Parent(Rc::new(RefCell::new(child.0.borrow().__parent.clone())))`.
        // This makes `greet(new Dog(...))` work where `greet` takes the
        // parent type — at the cost of LOSING subclass identity at the
        // upcast boundary (the new parent handle is a distinct cell, so
        // later mutations through the child don't reflect into the
        // upcast copy and vice-versa). That's the same Phase-1
        // limitation the legacy path documents; the identity-preserving
        // route is a `sealed` parent. We only emit this when the parent
        // is itself a wrapper class (the only shape `__parent` embeds).
        if let Some(parent_ty) = &class_decl.extends {
            if let Some(parent_bare) = parent_ty.name.segments.last().map(|s| s.text.as_str()) {
                // A polymorphic base uses `Rc<dyn ParentKind>` dispatch — the
                // identity-preserving wrap coercion replaces this slicing
                // `From`, so don't emit it (it would be dead, confusing code).
                if self.wrapper_classes.contains(parent_bare)
                    && !self.poly_base_classes.contains(parent_bare)
                {
                    // `impl[<T: Clone>] From<Child<T>> for Parent<pargs> { … }`.
                    // The child's own generic params (with the Clone bound)
                    // travel onto the impl header and onto `Child<T>`; the
                    // PARENT's type args come from the `extends` clause
                    // (`extends Container<int>` → `for Container<isize>`),
                    // so a child that binds its parent's `T` to a concrete
                    // type upcasts into the right monomorphization.
                    self.w.emit_indent();
                    self.w.push_str("impl");
                    self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
                    self.w.push_str(" From<");
                    self.w.push_str(name);
                    self.emit_generic_params_as_args(&class_decl.generic_params);
                    self.w.push_str("> for ");
                    self.w.push_str(parent_bare);
                    self.emit_parent_newtype_generic_args(parent_ty);
                    self.w.push_str(" { fn from(v: ");
                    self.w.push_str(name);
                    self.emit_generic_params_as_args(&class_decl.generic_params);
                    self.w.push_str(") -> Self { ");
                    self.w.push_str(parent_bare);
                    self.w.push_str(
                        "(std::rc::Rc::new(std::cell::RefCell::new(v.0.borrow().__parent.clone()))) } }\n",
                    );
                    self.w.newline();
                }
            }
        }

        // Mutable static fields — module-scope `LazyLock<Mutex<T>>`,
        // identical to the legacy path (statics live on the class,
        // not the instance, so the wrapper shape doesn't touch them).
        for field in &class_decl.fields {
            // `final`+`!Send` payloads route here too (thread_local form);
            // see the inline-class site for the rationale.
            let final_needs_tl = field.is_final
                && self.final_static_needs_runtime_init(&juxc_tycheck::resolved_field_type(field));
            if field.is_static && (!field.is_final || final_needs_tl) {
                self.emit_mutable_static_field(name, field);
            }
        }

        // Operator trait impls + interface trait impls + marker trait
        // reuse the existing emitters unchanged — they delegate to the
        // inherent methods we just emitted on the newtype, and the
        // newtype is the only `C` Rust knows about.
        //
        // **Non-generic gate on operator trait impls.** Same as the
        // legacy path (`emit_class_decl`): `emit_operator_trait_impl` /
        // `emit_partial_eq_from_cmp` / `emit_eq_marker` don't yet
        // propagate the `T: PartialEq` / `T: Hash` bounds a generic
        // operator overload would need, so they're emitted only for
        // non-generic classes. A generic class with operators keeps its
        // inherent `__op_*` methods (emitted above) but no trait bridge —
        // matching the deferral the legacy path documents.
        if class_decl.generic_params.is_empty() {
            for op in &class_decl.operators {
                self.emit_operator_trait_impl(name, op);
            }
            // §O.4.1 identity default — wrapper shape: the address is
            // the shared Rc cell, stable across aliases.
            let has_to_string = class_decl
                .operators
                .iter()
                .any(|o| o.kind == OperatorKind::ToString && !o.is_deleted);
            if !has_to_string {
                self.emit_identity_display(name, true);
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
            if has_cmp && !has_eq {
                self.emit_partial_eq_from_cmp(name);
            }
            if has_eq && has_hash {
                self.emit_eq_marker(name);
            }
        }
        self.emit_class_trait_impls(class_decl);
        self.emit_class_marker_trait(class_decl);
    }

    /// Emit the parent's generic args as a `<…>` suffix on its **inner**
    /// type — used for the `__parent: Parent_Inner<…>` embed in a wrapper
    /// child's `C_Inner`. The args come straight from the `extends`
    /// clause (`extends Container<int>` carries `generic_args = [int]`),
    /// so a child that binds its parent's type parameter to a concrete
    /// type pins the right `Parent_Inner` monomorphization. A child that
    /// passes its OWN type parameter through (`class Wrap<T> extends
    /// Container<T>`) emits `Container_Inner<T>`, which resolves because
    /// `T` is in scope on the child's inner struct. No-op when the parent
    /// has no generic args.
    ///
    /// Field-position arg mapping is used (Jux `String` → owned Rust
    /// `String`) so a stored `__parent` slot doesn't carry an elided
    /// lifetime — same rule the field-type emitter applies.
    fn emit_parent_inner_generic_args(&mut self, parent_ty: &juxc_ast::TypeRef) {
        if parent_ty.generic_args.is_empty() {
            return;
        }
        self.w.push('<');
        for (i, arg) in parent_ty.generic_args.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.emit_generic_arg_type_as_rust(arg);
        }
        self.w.push('>');
    }

    /// Emit the parent's generic args as a `<…>` suffix on its **newtype**
    /// — used in the `From<Child<…>> for Parent<…>` upcast header. Same
    /// args and same mapping as [`Self::emit_parent_inner_generic_args`];
    /// kept as a separate seam so the two call sites read clearly even
    /// though their bodies coincide today.
    fn emit_parent_newtype_generic_args(&mut self, parent_ty: &juxc_ast::TypeRef) {
        self.emit_parent_inner_generic_args(parent_ty);
    }

    /// Copy every concrete (non-abstract, non-static) inherited method
    /// this class doesn't override into its inherent impl, for the
    /// **wrapper-hierarchy** path. Mirrors the inlining pass baked into
    /// [`Self::emit_class_decl`], but is a standalone method so
    /// [`Self::emit_wrapper_class_decl`] can reuse it.
    ///
    /// The caller has `enclosing_class` set to THIS class and
    /// `emitting_wrapper_class` true, so each copied body's
    /// `this.field` access resolves against the child's flattened
    /// inner via the `__parent`-walk depth logic in `emit_field` /
    /// `emit_assign`. A copied parent method that reads `this.name`
    /// (declared two ancestors up) emits
    /// `self.0.borrow().__parent.__parent.name`.
    fn emit_inherited_wrapper_methods(&mut self, class_decl: &juxc_ast::ClassDecl) {
        let mut own_method_names: std::collections::HashSet<String> = class_decl
            .methods
            .iter()
            .map(|m| m.name.text.clone())
            .collect();
        let mut cursor: Option<juxc_ast::TypeRef> = class_decl.extends.clone();
        // **Generic-substitution accumulator (§CR.5.3).** When a child
        // binds its parent's type parameter to a concrete type
        // (`IntBox extends Container<int>`) the inherited method
        // `name(&self) -> T` must lower as `-> isize`, NOT `-> T` (which
        // isn't in scope on the non-generic child's impl). We walk the
        // chain composing a `parent-param → concrete-type` map: at each
        // ancestor, zip the ancestor's declared `generic_params` with the
        // `extends`-clause `generic_args` that reach it, then apply the
        // accumulated map to every copied method's signature. A child
        // that threads its OWN param through (`class Wrap<U> extends
        // Container<U>`) maps `T → U`, which stays in scope on `Wrap`.
        let mut subst: std::collections::HashMap<String, juxc_ast::TypeRef> =
            std::collections::HashMap::new();
        while let Some(parent_ref) = cursor {
            let Some(seg) = parent_ref.name.segments.first() else { break };
            let bare = seg.text.as_str();
            let parent_decl: Option<juxc_ast::ClassDecl> = self
                .class_asts
                .get(bare)
                .cloned()
                .or_else(|| {
                    self.class_asts
                        .iter()
                        .find(|(k, _)| k.rsplit('.').next().unwrap_or(k.as_str()) == bare)
                        .map(|(_, v)| v.clone())
                });
            let Some(parent) = parent_decl else { break };
            // Extend the substitution with this ancestor's bindings. The
            // `generic_args` from the `extends` clause are themselves
            // first run through the *current* subst so a child param
            // threaded up two levels resolves to its concrete root
            // (`A extends B<U>`, `B<X> extends C<X>` → `C`'s `X ↦ U`).
            for (param, arg) in parent
                .generic_params
                .iter()
                .zip(parent_ref.generic_args.iter())
            {
                if let juxc_ast::GenericArg::Type(arg_ty) = arg {
                    let resolved = substitute_type_ref(arg_ty, &subst);
                    subst.insert(param.name.text.clone(), resolved);
                }
            }
            let parent_methods = parent.methods.clone();
            let parent_extends = parent.extends.clone();
            for m in &parent_methods {
                if own_method_names.contains(&m.name.text) {
                    continue; // overridden by a closer class
                }
                if m.body.is_none() {
                    continue; // abstract — concrete subclass overrides
                }
                if m.modifiers
                    .iter()
                    .any(|mo| matches!(mo, juxc_ast::FnModifier::Static))
                {
                    continue; // statics aren't instance methods
                }
                own_method_names.insert(m.name.text.clone());
                // Apply the accumulated parent-param → concrete-type
                // substitution to the copied method's signature so its
                // return / param types read in the child's scope. No-op
                // when `subst` is empty (non-generic parent).
                if subst.is_empty() {
                    self.emit_method(m);
                } else {
                    let substituted = substitute_fn_signature(m, &subst);
                    self.emit_method(&substituted);
                }
            }
            cursor = parent_extends;
        }
    }

    /// Emit a `fn __jux_super_<m>(&self, …)` inherent shim for every method
    /// `m` this class **overrides** that has a concrete version in an ancestor
    /// (§6.9.4). The shim carries the *nearest* concrete ancestor's body,
    /// emitted in THIS class's context (so `this.field` walks `__parent` and
    /// virtual calls inside it still dispatch to the subclass), under the
    /// reserved `__jux_super_<m>` name. A `super.m(args)` call (see
    /// `emit_call`) lowers to `self.__jux_super_<m>(args)`, giving Java's
    /// static-`super` semantics. No-op for classes that override nothing.
    fn emit_super_shims(&mut self, class_decl: &juxc_ast::ClassDecl) {
        // Instance methods (with bodies) the class declares — each is a
        // potential `super.<m>()` target.
        let overridden: std::collections::HashSet<String> = class_decl
            .methods
            .iter()
            .filter(|m| {
                m.body.is_some()
                    && !m
                        .modifiers
                        .iter()
                        .any(|mo| matches!(mo, juxc_ast::FnModifier::Static))
            })
            .map(|m| m.name.text.clone())
            .collect();
        if overridden.is_empty() {
            return;
        }
        // Walk ancestors (nearest first), composing the generic substitution
        // exactly like `emit_inherited_wrapper_methods`. The first concrete
        // version of each overridden method wins (that's what `super` names).
        let mut shimmed: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut cursor: Option<juxc_ast::TypeRef> = class_decl.extends.clone();
        let mut subst: std::collections::HashMap<String, juxc_ast::TypeRef> =
            std::collections::HashMap::new();
        while let Some(parent_ref) = cursor {
            let Some(seg) = parent_ref.name.segments.first() else { break };
            let bare = seg.text.as_str();
            let parent_decl: Option<juxc_ast::ClassDecl> = self
                .class_asts
                .get(bare)
                .cloned()
                .or_else(|| {
                    self.class_asts
                        .iter()
                        .find(|(k, _)| k.rsplit('.').next().unwrap_or(k.as_str()) == bare)
                        .map(|(_, v)| v.clone())
                });
            let Some(parent) = parent_decl else { break };
            for (param, arg) in parent
                .generic_params
                .iter()
                .zip(parent_ref.generic_args.iter())
            {
                if let juxc_ast::GenericArg::Type(arg_ty) = arg {
                    let resolved = substitute_type_ref(arg_ty, &subst);
                    subst.insert(param.name.text.clone(), resolved);
                }
            }
            for m in &parent.methods {
                if !overridden.contains(&m.name.text) || shimmed.contains(&m.name.text) {
                    continue;
                }
                if m.body.is_none()
                    || m.modifiers
                        .iter()
                        .any(|mo| matches!(mo, juxc_ast::FnModifier::Static))
                {
                    // Abstract / static here — keep walking for a concrete one.
                    continue;
                }
                shimmed.insert(m.name.text.clone());
                let mut renamed = if subst.is_empty() {
                    m.clone()
                } else {
                    substitute_fn_signature(m, &subst)
                };
                renamed.name = juxc_ast::Ident {
                    text: format!("__jux_super_{}", m.name.text),
                    span: m.name.span,
                };
                self.emit_method(&renamed);
            }
            cursor = parent.extends.clone();
        }
    }

    /// Compute the set of this class's generic type-parameter names
    /// that get **formatted** somewhere in its own body — i.e. a value
    /// of that parameter's type flows into a `$"…${…}…"` interpolation,
    /// a `print(…)`, or a string-concat (`"…" + x`) position. Those
    /// params need a `std::fmt::Display` bound on the inherent impl so
    /// the emitted `format!`/`println!` type-checks (Jux toString /
    /// interpolation semantics — a generic field is printable iff its
    /// instantiated type is).
    ///
    /// Detection is conservative-by-inclusion but type-driven: we map
    /// each instance field whose declared type is a single generic
    /// param to that param, then scan every method / constructor /
    /// operator body for a format-position read of such a field
    /// (`this.field`, or a bare `field` reference inside the body).
    /// Anything we can't resolve simply isn't added — the param keeps
    /// only its `Clone + Debug` bound, matching the prior behavior.
    /// Type-param names that need a `+ Default` bound on this class's
    /// inherent impl — every param used as the **element of a fixed
    /// array field** (`T[N] storage;`). Constructing such a field
    /// (`new T[N]`) lowers to `std::array::from_fn(|_| Default::
    /// default())` (see `emit_new_array`), which requires `T: Default`.
    /// The struct declaration itself doesn't need the bound (it merely
    /// stores `[T; N]`), so only the impl-header emission consults
    /// this. A `new T[k]` *local* in a class without such a field is
    /// outside this scan — exotic enough to leave for the const-eval
    /// phase.
    pub(crate) fn class_default_bound_params(
        class_decl: &juxc_ast::ClassDecl,
    ) -> HashSet<String> {
        let mut defaulted: HashSet<String> = HashSet::new();
        if class_decl.generic_params.is_empty() {
            return defaulted;
        }
        let param_names: HashSet<&str> = class_decl
            .generic_params
            .iter()
            .filter(|p| !p.is_const())
            .map(|p| p.name.text.as_str())
            .collect();
        for field in &class_decl.fields {
            if let Some(ty) = &field.ty {
                if matches!(ty.array_shape, Some(juxc_ast::ArrayShape::Fixed(_)))
                    && ty.generic_args.is_empty()
                    && ty.fn_shape.is_none()
                    && ty.name.segments.len() == 1
                    && param_names.contains(ty.name.segments[0].text.as_str())
                {
                    defaulted.insert(ty.name.segments[0].text.clone());
                }
            }
        }
        defaulted
    }

    pub(crate) fn class_displayed_generic_params(
        &self,
        class_decl: &juxc_ast::ClassDecl,
    ) -> HashSet<String> {
        let mut displayed: HashSet<String> = HashSet::new();
        if class_decl.generic_params.is_empty() {
            return displayed;
        }
        let param_names: HashSet<&str> = class_decl
            .generic_params
            .iter()
            .map(|p| p.name.text.as_str())
            .collect();
        // field name -> generic-param name (only fields typed as a bare
        // generic param of THIS class).
        let mut generic_fields: std::collections::HashMap<&str, &str> =
            std::collections::HashMap::new();
        for field in &class_decl.fields {
            if let Some(ty) = &field.ty {
                if ty.array_shape.is_none()
                    && ty.generic_args.is_empty()
                    && ty.fn_shape.is_none()
                    && ty.name.segments.len() == 1
                {
                    let seg = ty.name.segments[0].text.as_str();
                    if param_names.contains(seg) {
                        generic_fields.insert(field.name.text.as_str(), seg);
                    }
                }
            }
        }
        if generic_fields.is_empty() {
            return displayed;
        }
        // Walk every body's format positions.
        for m in &class_decl.methods {
            if let Some(body) = &m.body {
                Self::scan_block_for_displayed_fields(body, &generic_fields, &mut displayed);
            }
        }
        for ctor in &class_decl.constructors {
            Self::scan_block_for_displayed_fields(&ctor.body, &generic_fields, &mut displayed);
        }
        for op in &class_decl.operators {
            if let Some(body) = &op.body {
                Self::scan_block_for_displayed_fields(body, &generic_fields, &mut displayed);
            }
        }
        displayed
    }

    /// Walk a block looking for **format-position** reads of a
    /// generic-typed field (see [`Self::class_displayed_generic_params`]).
    /// Recurses into nested blocks and the format-bearing expression
    /// shapes (interpolated strings, `print(…)` calls, string concats).
    fn scan_block_for_displayed_fields(
        block: &juxc_ast::Block,
        generic_fields: &std::collections::HashMap<&str, &str>,
        out: &mut HashSet<String>,
    ) {
        use juxc_ast::{Expr, Stmt};
        // Record a param as displayed if `e` reads a generic field.
        fn mark_field_read(
            e: &Expr,
            generic_fields: &std::collections::HashMap<&str, &str>,
            out: &mut HashSet<String>,
        ) {
            match e {
                // `this.field`
                Expr::Field(f) => {
                    if matches!(&*f.object, Expr::This(_)) {
                        if let Some(param) = generic_fields.get(f.field.text.as_str()) {
                            out.insert((*param).to_string());
                        }
                    }
                    mark_field_read(&f.object, generic_fields, out);
                }
                // bare `field` (implicit this inside the body)
                Expr::Path(qn) => {
                    if qn.segments.len() == 1 {
                        if let Some(param) = generic_fields.get(qn.segments[0].text.as_str()) {
                            out.insert((*param).to_string());
                        }
                    }
                }
                _ => {}
            }
        }
        // Scan a single expression for format positions.
        fn scan_expr(
            e: &Expr,
            generic_fields: &std::collections::HashMap<&str, &str>,
            out: &mut HashSet<String>,
        ) {
            match e {
                Expr::InterpString(s) => {
                    for seg in &s.segments {
                        if let juxc_ast::InterpSegment::Expr(inner) = seg {
                            mark_field_read(inner, generic_fields, out);
                            scan_expr(inner, generic_fields, out);
                        }
                    }
                }
                Expr::Call(c) => {
                    // `print(arg)` formats its args.
                    if let Expr::Path(qn) = &*c.callee {
                        if qn.segments.len() == 1 && qn.segments[0].text == "print" {
                            for a in &c.args {
                                mark_field_read(a, generic_fields, out);
                            }
                        }
                    }
                    scan_expr(&c.callee, generic_fields, out);
                    for a in &c.args {
                        scan_expr(a, generic_fields, out);
                    }
                }
                Expr::Binary(b) => {
                    // String concat — `"…" + x` or `x + "…"`. We can't
                    // easily know the static type here, so be inclusive:
                    // if either operand is a string literal, the other
                    // formatted operand's generic field is displayed.
                    let lhs_lit = matches!(&*b.left, Expr::Literal(juxc_ast::Literal::String(_)));
                    let rhs_lit = matches!(&*b.right, Expr::Literal(juxc_ast::Literal::String(_)));
                    if b.op == juxc_ast::BinaryOp::Add && (lhs_lit || rhs_lit) {
                        mark_field_read(&b.left, generic_fields, out);
                        mark_field_read(&b.right, generic_fields, out);
                    }
                    scan_expr(&b.left, generic_fields, out);
                    scan_expr(&b.right, generic_fields, out);
                }
                Expr::Field(f) => scan_expr(&f.object, generic_fields, out),
                Expr::Unary(u) => scan_expr(&u.operand, generic_fields, out),
                _ => {}
            }
        }
        for stmt in &block.statements {
            match stmt {
                Stmt::Expr(e) => scan_expr(e, generic_fields, out),
                Stmt::Return(Some(e)) => scan_expr(e, generic_fields, out),
                Stmt::VarDecl(v) => {
                    if let Some(init) = &v.init {
                        scan_expr(init, generic_fields, out);
                    }
                }
                Stmt::Assign(a) => scan_expr(&a.value, generic_fields, out),
                Stmt::If(if_stmt) => {
                    scan_expr(&if_stmt.condition, generic_fields, out);
                    Self::scan_block_for_displayed_fields(
                        &if_stmt.then_block,
                        generic_fields,
                        out,
                    );
                    if let Some(eb) = if_stmt.else_branch.as_deref() {
                        match eb {
                            juxc_ast::ElseBranch::Block(b) => {
                                Self::scan_block_for_displayed_fields(b, generic_fields, out);
                            }
                            juxc_ast::ElseBranch::If(inner) => {
                                let synth = juxc_ast::Block {
                                    statements: vec![Stmt::If(inner.clone())],
                                    span: juxc_source::Span::DUMMY,
                                };
                                Self::scan_block_for_displayed_fields(
                                    &synth,
                                    generic_fields,
                                    out,
                                );
                            }
                        }
                    }
                }
                Stmt::While(w) => {
                    scan_expr(&w.condition, generic_fields, out);
                    Self::scan_block_for_displayed_fields(&w.body, generic_fields, out);
                }
                Stmt::ForEach(f) => {
                    scan_expr(&f.iter, generic_fields, out);
                    Self::scan_block_for_displayed_fields(&f.body, generic_fields, out);
                }
                _ => {}
            }
        }
    }

    /// The bare name of `class_bare`'s direct `extends` parent, if any.
    /// Prefers tycheck's resolved `extends_fqn` (cross-package safe), falling
    /// back to the source `extends` clause's last segment.
    fn direct_parent_bare(&self, class_bare: &str) -> Option<String> {
        let s = self.lookup_class_by_bare_or_fqn(class_bare)?;
        s.extends_fqn
            .as_deref()
            .map(|f| f.rsplit('.').next().unwrap_or(f).to_string())
            .or_else(|| {
                s.extends
                    .as_ref()
                    .and_then(|t| t.name.segments.last().map(|x| x.text.clone()))
            })
    }

    /// True when `class_bare` participates in a polymorphic-base hierarchy —
    /// it is itself a polymorphic base, or one of its ancestors is. Such a
    /// class needs a **populated** `<Name>Kind` trait + delegating impls so
    /// virtual dispatch works; every other class keeps the empty marker.
    fn is_dispatch_relevant_class(&self, class_bare: &str) -> bool {
        if self.poly_base_classes.contains(class_bare) {
            return true;
        }
        let mut cursor = self.direct_parent_bare(class_bare);
        let mut depth = 0usize;
        while let Some(name) = cursor {
            if depth > 64 {
                return false;
            }
            if self.poly_base_classes.contains(&name) {
                return true;
            }
            cursor = self.direct_parent_bare(&name);
            depth += 1;
        }
        false
    }

    /// True if any strict ancestor of `class_bare` declares a method named
    /// `method`. Used to decide which methods a class *introduces* (an
    /// override re-declares an ancestor's method, so it belongs on the
    /// introducing ancestor's `Kind` trait, not the override's).
    fn ancestor_declares_method(&self, class_bare: &str, method: &str) -> bool {
        let mut cursor = self.direct_parent_bare(class_bare);
        let mut depth = 0usize;
        while let Some(name) = cursor {
            if depth > 64 {
                return false;
            }
            if let Some(s) = self.lookup_class_by_bare_or_fqn(&name) {
                if s.methods.contains_key(method) {
                    return true;
                }
            }
            cursor = self.direct_parent_bare(&name);
            depth += 1;
        }
        false
    }

    /// The **introduced virtual methods** of `class_bare` — public/protected,
    /// non-static instance methods it declares that no ancestor declares.
    /// These belong on its own `<Name>Kind` trait (`final` is included — still
    /// callable through a base ref; `static`/`private` are excluded — not
    /// virtual). Sorted by name for deterministic output.
    fn class_introduced_virtual_methods(&self, class_bare: &str) -> Vec<(String, MethodSig)> {
        let Some(sig) = self.lookup_class_by_bare_or_fqn(class_bare) else {
            return Vec::new();
        };
        let mut out: Vec<(String, MethodSig)> = sig
            .methods
            .iter()
            .filter(|(_, m)| {
                !m.is_static
                    && matches!(
                        m.visibility,
                        juxc_ast::Visibility::Public | juxc_ast::Visibility::Protected
                    )
            })
            .filter(|(name, _)| !self.ancestor_declares_method(class_bare, name))
            .map(|(n, m)| (n.clone(), m.clone()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Emit a `<Name>Kind` trait method *signature* from a [`MethodSig`]:
    /// `fn name(&self, p: T, …) -> R;` (or `async fn …`). Param / return types
    /// go through the value-position emitters so an interface or
    /// polymorphic-base type renders as `Rc<dyn …>`, matching the inherent
    /// method's signature exactly.
    fn emit_kind_trait_method_sig(&mut self, name: &str, sig: &MethodSig) {
        self.w.emit_indent();
        let is_async = matches!(sig.return_type, ReturnType::AsyncType(_));
        self.w.push_str(if is_async { "async fn " } else { "fn " });
        self.w.push_str(name);
        self.w.push_str("(&self");
        for p in &sig.params {
            self.w.push_str(", ");
            self.w.push_str(&p.name);
            self.w.push_str(": ");
            self.emit_value_type_as_rust(&p.ty);
        }
        self.w.push(')');
        match &sig.return_type {
            ReturnType::Void => {}
            ReturnType::Type(t) | ReturnType::AsyncType(t) => {
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
        }
        self.w.push_str(";\n");
    }

    /// Emit a delegating `<Name>Kind` impl method that forwards to the
    /// implementing class's inherent method:
    /// `fn name(&self, …) -> R { Class::name(self, …) }`. The inherent always
    /// exists — the class either overrides the method or the inherited-method
    /// inlining pass copied the ancestor body into its inherent impl. Naming
    /// the inherent path explicitly (`Class::name`) resolves ahead of this
    /// trait method, so it never recurses.
    fn emit_kind_delegating_method(&mut self, recv_class_bare: &str, name: &str, sig: &MethodSig) {
        self.w.emit_indent();
        let is_async = matches!(sig.return_type, ReturnType::AsyncType(_));
        self.w.push_str(if is_async { "async fn " } else { "fn " });
        self.w.push_str(name);
        self.w.push_str("(&self");
        for p in &sig.params {
            self.w.push_str(", ");
            self.w.push_str(&p.name);
            self.w.push_str(": ");
            self.emit_value_type_as_rust(&p.ty);
        }
        self.w.push(')');
        match &sig.return_type {
            ReturnType::Void => {}
            ReturnType::Type(t) | ReturnType::AsyncType(t) => {
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
        }
        self.w.push_str(" { ");
        self.w.push_str(recv_class_bare);
        self.w.push_str("::");
        self.w.push_str(name);
        self.w.push_str("(self");
        for p in &sig.params {
            self.w.push_str(", ");
            self.w.push_str(&p.name);
        }
        self.w.push(')');
        if is_async {
            self.w.push_str(".await");
        }
        self.w.push_str(" }\n");
    }

    /// True iff class `c` **IS-A** `t` — the same class, a transitive
    /// subclass, or an implementer of interface `t`. The relation that decides
    /// whether a `__jux_as_<t>` downcast hook on a base can return `Some`.
    pub(crate) fn class_is_a(&self, c: &str, t: &str) -> bool {
        c == t
            || juxc_tycheck::ty::walk_extends_reaches(c, t, &self.symbols)
            || juxc_tycheck::ty::class_implements_interface(c, t, &self.symbols)
    }

    /// The cast / type-test targets (from `downcast_targets`) reachable as a
    /// runtime instance of a value statically typed as base `b` — i.e. some
    /// class IS-A `b` AND IS-A the target, and the target is not `b` itself or
    /// a supertype (that's an upcast, handled by coercion). These become the
    /// `__jux_as_<T>` hooks on `<b>Kind`. Sorted for deterministic output.
    /// True iff some class is an instance of BOTH base `b` and target `t` —
    /// i.e. a value statically typed `b` could, at run time, be a `t`. The
    /// condition under which a `__jux_as_<t>` hook on `b`'s trait is meaningful.
    pub(crate) fn target_reachable_from_base(&self, b: &str, t: &str) -> bool {
        self.symbols.classes.keys().any(|fqn| {
            let c = fqn.rsplit('.').next().unwrap_or(fqn);
            self.class_is_a(c, b) && self.class_is_a(c, t)
        })
    }

    fn hook_targets_for_base(&self, b: &str) -> Vec<String> {
        if self.downcast_targets.is_empty() {
            return Vec::new();
        }
        // A `<Name>Kind` trait's supertrait is its poly-base parent's `Kind`.
        // To avoid the same `__jux_as_<T>` appearing on both a trait and its
        // supertrait (which makes an unqualified call on a mid-hierarchy `dyn`
        // value ambiguous — E0034), emit each hook only on the TOPMOST
        // contiguous poly-base ancestor; lower bases inherit it via the
        // supertrait chain.
        let parent_base = self
            .direct_parent_bare(b)
            .filter(|p| self.poly_base_classes.contains(p));
        let mut out: Vec<String> = self
            .downcast_targets
            .iter()
            .filter(|t| {
                // Skip upcasts (b IS-A t ⟹ t is b or a supertype).
                !self.class_is_a(b, t)
                    && self.target_reachable_from_base(b, t)
                    // Skip if a supertrait (the poly-base parent) carries it.
                    && parent_base
                        .as_deref()
                        .map_or(true, |p| !self.target_reachable_from_base(p, t))
            })
            .cloned()
            .collect();
        out.sort();
        out
    }

    /// Emit the **value type** a `__jux_as_<t>` hook returns inside its
    /// `Option<…>` — `Rc<dyn t>` for an interface, `Rc<dyn tKind>` for a
    /// polymorphic-base class, or the bare wrapper newtype `t` for a concrete
    /// (leaf) class. Mirrors value-position emission for a bare `t`.
    fn emit_hook_target_type(&mut self, t: &str) {
        if self.lookup_interface_by_bare_or_fqn(t).is_some() {
            self.w.push_str("std::rc::Rc<dyn ");
            self.w.push_str(t);
            self.w.push('>');
        } else if self.poly_base_classes.contains(t) {
            self.w.push_str("std::rc::Rc<dyn ");
            self.w.push_str(t);
            self.w.push_str("Kind>");
        } else {
            self.w.push_str(t);
        }
    }

    /// The cast / type-test targets reachable from an **interface** source —
    /// targets some implementer could also be. Unlike class `Kind` traits,
    /// interface traits carry no `Kind` supertrait chain, so there is no
    /// topmost-base skip (each interface gets its own hooks directly).
    pub(crate) fn interface_hook_targets(&self, iface_bare: &str) -> Vec<String> {
        if self.downcast_targets.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<String> = self
            .downcast_targets
            .iter()
            .filter(|t| {
                !self.class_is_a(iface_bare, t)
                    && self.target_reachable_from_base(iface_bare, t)
            })
            .cloned()
            .collect();
        out.sort();
        out
    }

    /// Emit the `__jux_as_<t>(&self) -> Option<…> { None }` hook signature
    /// (default body) on a trait.
    pub(crate) fn emit_downcast_hook_sig(&mut self, t: &str) {
        self.w.emit_indent();
        self.w.push_str("fn __jux_as_");
        self.w.push_str(t);
        self.w.push_str("(&self) -> Option<");
        self.emit_hook_target_type(t);
        self.w.push_str("> { None }\n");
    }

    /// Emit a concrete `__jux_as_<t>` hook override returning `Some(self)` —
    /// `Some(self.clone())` for a leaf target, or
    /// `Some(Rc::new(self.clone()) as <target>)` for a trait-object target
    /// (identity-preserving `Rc` bump sharing the inner cell).
    pub(crate) fn emit_downcast_hook_impl(&mut self, t: &str) {
        self.w.emit_indent();
        self.w.push_str("fn __jux_as_");
        self.w.push_str(t);
        self.w.push_str("(&self) -> Option<");
        self.emit_hook_target_type(t);
        self.w.push_str("> { Some(");
        let is_dyn =
            self.lookup_interface_by_bare_or_fqn(t).is_some() || self.poly_base_classes.contains(t);
        if is_dyn {
            self.w.push_str("std::rc::Rc::new(self.clone()) as ");
            self.emit_hook_target_type(t);
        } else {
            self.w.push_str("self.clone()");
        }
        self.w.push_str(") }\n");
    }

    /// A class's **own public / protected instance fields** — the ones that
    /// get `__get_<f>` / `__set_<f>` accessor methods on its `<Name>Kind`
    /// trait so they're reachable through a base reference (a `dyn` trait
    /// object can't expose struct fields directly). Private and static fields
    /// are excluded. Returns `(field_name, field_type)` pairs, sorted.
    fn class_accessor_fields(&self, owner_bare: &str) -> Vec<(String, juxc_ast::TypeRef)> {
        let cd = self.class_asts.get(owner_bare).or_else(|| {
            self.class_asts
                .iter()
                .find(|(k, _)| k.rsplit('.').next().unwrap_or(k.as_str()) == owner_bare)
                .map(|(_, v)| v)
        });
        let Some(cd) = cd else {
            return Vec::new();
        };
        let mut out: Vec<(String, juxc_ast::TypeRef)> = cd
            .fields
            .iter()
            .filter(|f| {
                !f.is_static
                    && matches!(
                        f.visibility,
                        juxc_ast::Visibility::Public | juxc_ast::Visibility::Protected
                    )
                    && f.ty.is_some()
            })
            .map(|f| (f.name.text.clone(), f.ty.clone().unwrap()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Number of `__parent` hops from class `impl_class` up to `owner` (where
    /// the accessed field is declared). 0 when they're the same class.
    fn field_depth_from(&self, impl_class: &str, owner: &str) -> usize {
        if impl_class == owner {
            return 0;
        }
        let mut depth = 0usize;
        let mut cursor = self.direct_parent_bare(impl_class);
        while let Some(p) = cursor {
            depth += 1;
            if p == owner || depth > 64 {
                return depth;
            }
            cursor = self.direct_parent_bare(&p);
        }
        depth
    }

    /// Emit the `__get_<f>` / `__set_<f>` accessor *signatures* (required trait
    /// methods) for `owner_bare`'s own public/protected fields.
    fn emit_accessor_trait_sigs(&mut self, owner_bare: &str) {
        for (name, ty) in self.class_accessor_fields(owner_bare) {
            self.w.emit_indent();
            self.w.push_str("fn __get_");
            self.w.push_str(&name);
            self.w.push_str("(&self) -> ");
            self.emit_value_type_as_rust(&ty);
            self.w.push_str(";\n");
            self.w.emit_indent();
            self.w.push_str("fn __set_");
            self.w.push_str(&name);
            self.w.push_str("(&self, __v: ");
            self.emit_value_type_as_rust(&ty);
            self.w.push_str(");\n");
        }
    }

    /// Emit the delegating accessor *bodies* for `owner_bare`'s fields in an
    /// `impl <Owner>Kind for <impl_class>` block — reading / writing the field
    /// through `self.0.borrow()[.__parent…]` at the right inheritance depth.
    fn emit_accessor_impl_methods(&mut self, owner_bare: &str, impl_class_bare: &str) {
        let depth = self.field_depth_from(impl_class_bare, owner_bare);
        for (name, ty) in self.class_accessor_fields(owner_bare) {
            // getter — clone out of the borrow guard before it drops.
            self.w.emit_indent();
            self.w.push_str("fn __get_");
            self.w.push_str(&name);
            self.w.push_str("(&self) -> ");
            self.emit_value_type_as_rust(&ty);
            self.w.push_str(" { self.0.borrow()");
            for _ in 0..depth {
                self.w.push_str(".__parent");
            }
            self.w.push('.');
            self.w.push_str(&name);
            self.w.push_str(".clone() }\n");
            // setter — scoped `borrow_mut()` write.
            self.w.emit_indent();
            self.w.push_str("fn __set_");
            self.w.push_str(&name);
            self.w.push_str("(&self, __v: ");
            self.emit_value_type_as_rust(&ty);
            self.w.push_str(") { self.0.borrow_mut()");
            for _ in 0..depth {
                self.w.push_str(".__parent");
            }
            self.w.push('.');
            self.w.push_str(&name);
            self.w.push_str(" = __v; }\n");
        }
    }

    /// Emit a class's marker trait and the transitive marker impls
    /// covering its parent chain.
    ///
    /// For an ordinary class the trait is **empty** (`{}`) — it exists purely
    /// to let generic bounds reference Jux classes in a way Rust's type system
    /// accepts. For a **dispatch-relevant** class (a polymorphic base or a
    /// subclass of one — see [`Self::is_dispatch_relevant_class`]) the trait is
    /// **populated** with the base's virtual method signatures and a supertrait
    /// chain mirroring `extends`, and each `impl <Ancestor>Kind for <Child>`
    /// carries delegating bodies so a `Rc<dyn <Base>Kind>` value dispatches to
    /// the concrete override (Stage-2 virtual dispatch).
    pub(crate) fn emit_class_marker_trait(&mut self, class_decl: &juxc_ast::ClassDecl) {
        // (Migrated to Writer indent-aware API)
        // pub trait <Name>Kind: std::fmt::Debug {} — no `Clone`
        // supertrait so the trait stays dyn-compatible (Clone's
        // `Self: Sized` would forbid `Box<dyn …Kind>`). Generic
        // bounds add `+ Clone` explicitly at use sites via
        // `emit_generic_params_with_clone_bound`.
        //
        // **`Debug` supertrait.** Every Jux class struct derives
        // `Debug`, so this bound always holds, and it makes
        // `dyn <Name>Kind` itself `Debug`. That lets a
        // `#[derive(Debug)]` container holding a `Box<dyn …Kind>`
        // (storage-position wildcards — `Box1<? extends Animal>`
        // erasing to `Box1<Box<dyn AnimalKind>>`) derive `Debug`
        // without a "doesn't implement Debug" error. `Debug` is
        // object-safe (no `Self: Sized`), so the trait stays usable
        // as a trait object.
        let class_bare = class_decl.name.text.clone();
        let relevant = self.is_dispatch_relevant_class(&class_bare);
        let c_is_poly = self.poly_base_classes.contains(&class_bare);

        // --- `trait <Name>Kind: <supertrait> { <method sigs?> }` ---
        self.w.emit_indent();
        self.emit_visibility(class_decl.visibility);
        self.w.push_str("trait ");
        self.w.push_str(&class_bare);
        self.w.push_str("Kind: ");
        // Supertrait: the parent's `Kind` when the parent is itself a
        // polymorphic base (so `dyn ChildKind` can reach inherited methods);
        // `std::fmt::Debug` at the root of the chain (reachable transitively
        // either way, so `dyn …Kind` containers still derive `Debug`).
        let parent_super: Option<String> = if relevant {
            self.direct_parent_bare(&class_bare)
                .filter(|p| self.poly_base_classes.contains(p))
        } else {
            None
        };
        if let Some(parent) = &parent_super {
            self.w.push_str(parent);
            self.w.push_str("Kind");
        } else {
            self.w.push_str("std::fmt::Debug");
        }
        let own_methods = if relevant && c_is_poly {
            self.class_introduced_virtual_methods(&class_bare)
        } else {
            Vec::new()
        };
        // Runtime-type downcast hooks (`__jux_as_<T>`) live on traits that can
        // be a `dyn` value — polymorphic-base Kind traits — for every
        // cast/type-test target reachable from this base.
        let hook_targets = if c_is_poly {
            self.hook_targets_for_base(&class_bare)
        } else {
            Vec::new()
        };
        // Field accessors (`__get_<f>` / `__set_<f>`) for this base's own
        // public/protected fields — so they're reachable through a base
        // reference (a `dyn` can't expose struct fields).
        let accessor_fields = if c_is_poly {
            self.class_accessor_fields(&class_bare)
        } else {
            Vec::new()
        };
        if own_methods.is_empty() && hook_targets.is_empty() && accessor_fields.is_empty() {
            self.w.push_str(" {}\n");
        } else {
            self.w.push_str(" {\n");
            self.w.indent_inc();
            for (name, sig) in &own_methods {
                self.emit_kind_trait_method_sig(name, sig);
            }
            for t in &hook_targets {
                self.emit_downcast_hook_sig(t);
            }
            if !accessor_fields.is_empty() {
                self.emit_accessor_trait_sigs(&class_bare);
            }
            self.w.indent_dec();
            self.w.emit_indent();
            self.w.push_str("}\n");
        }

        // --- impls ---
        // A dispatch-relevant ABSTRACT class provides no concrete bodies, so it
        // emits NO `impl …Kind for Self` blocks (its populated trait's required
        // methods are satisfied by each concrete subclass instead). Every other
        // class emits its self + ancestor impls — empty markers, or delegating
        // bodies where the implemented trait is populated.
        let emit_impls = !(relevant && class_decl.is_abstract);
        if emit_impls {
            // `impl[<T: Clone…>] <Name>Kind for <Name>[<T…>] { … }`. The class's
            // own generic params (with bounds) travel onto the impl so a
            // generic class's marker still satisfies (`Box<T>` only Clones when
            // `T: Clone`).
            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
            self.w.push(' ');
            self.w.push_str(&class_bare);
            self.w.push_str("Kind for ");
            self.w.push_str(&class_bare);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            // Hook overrides this class provides for its OWN Kind trait: the
            // targets `class_bare` IS-A (so `__jux_as_T` returns `Some(self)`).
            let self_hooks: Vec<String> = hook_targets
                .iter()
                .filter(|t| self.class_is_a(&class_bare, t))
                .cloned()
                .collect();
            if own_methods.is_empty() && self_hooks.is_empty() && accessor_fields.is_empty() {
                self.w.push_str(" {}\n");
            } else {
                self.w.push_str(" {\n");
                self.w.indent_inc();
                for (name, sig) in &own_methods {
                    self.emit_kind_delegating_method(&class_bare, name, sig);
                }
                for t in &self_hooks {
                    self.emit_downcast_hook_impl(t);
                }
                if !accessor_fields.is_empty() {
                    self.emit_accessor_impl_methods(&class_bare, &class_bare);
                }
                self.w.indent_dec();
                self.w.emit_indent();
                self.w.push_str("}\n");
            }

            // Walk the ancestor chain via tycheck's pre-resolved `extends_fqn`
            // (cross-package safe). For each ancestor emit
            // `impl <ancestor-marker-path> for <Child> { … }`, populated with
            // delegating bodies when the ancestor is a polymorphic base.
            let child_fqn = self.classsig_lookup_fqn(&class_bare);
            let child_pkg = child_fqn
                .as_deref()
                .and_then(crate::backend_fqn::fqn_package)
                .unwrap_or("");
            let mut cursor_fqn: Option<String> = child_fqn
                .as_deref()
                .and_then(|f| self.symbols.classes.get(f))
                .and_then(|c| c.extends_fqn.clone());
            while let Some(ancestor_fqn) = cursor_fqn.clone() {
                let ancestor_bare = crate::backend_fqn::fqn_bare(&ancestor_fqn).to_string();
                let ancestor_pkg = crate::backend_fqn::fqn_package(&ancestor_fqn).unwrap_or("");

                self.w.emit_indent();
                self.w.push_str("impl");
                self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
                self.w.push(' ');
                if !ancestor_pkg.is_empty() && ancestor_pkg != child_pkg {
                    self.w.push_str("crate::");
                    for seg in ancestor_pkg.split('.') {
                        self.w.push_str(seg);
                        self.w.push_str("::");
                    }
                }
                self.w.push_str(&ancestor_bare);
                self.w.push_str("Kind for ");
                self.w.push_str(&class_bare);
                self.emit_generic_params_as_args(&class_decl.generic_params);
                let anc_methods = if self.poly_base_classes.contains(&ancestor_bare) {
                    self.class_introduced_virtual_methods(&ancestor_bare)
                } else {
                    Vec::new()
                };
                // Hook overrides this class provides for the ANCESTOR's Kind
                // trait: ancestor's hook targets that `class_bare` IS-A.
                let anc_hook_targets = if self.poly_base_classes.contains(&ancestor_bare) {
                    self.hook_targets_for_base(&ancestor_bare)
                } else {
                    Vec::new()
                };
                let anc_hooks: Vec<String> = anc_hook_targets
                    .iter()
                    .filter(|t| self.class_is_a(&class_bare, t))
                    .cloned()
                    .collect();
                let anc_accessor_fields = if self.poly_base_classes.contains(&ancestor_bare) {
                    self.class_accessor_fields(&ancestor_bare)
                } else {
                    Vec::new()
                };
                if anc_methods.is_empty() && anc_hooks.is_empty() && anc_accessor_fields.is_empty() {
                    self.w.push_str(" {}\n");
                } else {
                    self.w.push_str(" {\n");
                    self.w.indent_inc();
                    for (name, sig) in &anc_methods {
                        self.emit_kind_delegating_method(&class_bare, name, sig);
                    }
                    for t in &anc_hooks {
                        self.emit_downcast_hook_impl(t);
                    }
                    if !anc_accessor_fields.is_empty() {
                        self.emit_accessor_impl_methods(&ancestor_bare, &class_bare);
                    }
                    self.w.indent_dec();
                    self.w.emit_indent();
                    self.w.push_str("}\n");
                }

                cursor_fqn = self
                    .symbols
                    .classes
                    .get(&ancestor_fqn)
                    .and_then(|c| c.extends_fqn.clone());
            }
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
        // Pick the lexicographically smallest matching FQN. `classes` is a
        // `HashMap`, so iterating it directly and returning "the first match"
        // is non-deterministic across runs when two packages share a bare
        // name — that surfaced as flaky codegen (e.g. `stress_exceptions`
        // sometimes building, sometimes not). `min()` makes the choice stable.
        self.symbols
            .classes
            .keys()
            .filter(|fqn| crate::backend_fqn::fqn_bare(fqn) == bare)
            .min()
            .cloned()
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
                        // **Wrapper hierarchies inline ancestor methods.**
                        // For a wrapper class, `emit_inherited_wrapper_methods`
                        // already copied the concrete ancestor body into
                        // THIS class's inherent impl, so the method
                        // resolves on `Self` directly. Use the inherent
                        // (empty) target — an `<Ancestor>::method(self, …)`
                        // call would pass `&mut Child` where the ancestor
                        // wants `&Ancestor`, and the wrapper path has no
                        // `Deref` to bridge that (E0308). The legacy path
                        // keeps the ancestor-FQN form (Deref coercion
                        // carries `&mut Child` → `&Ancestor`).
                        if self.wrapper_classes.contains(&class_decl.name.text) {
                            method_targets.insert(name.clone(), Some(String::new()));
                        } else {
                            method_targets.insert(name.clone(), Some(fqn));
                        }
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
                // `async fn` rollup: the trait method may have been
                // declared `async T` — re-emit the keyword on the
                // delegating impl so the trait/impl signatures stay
                // structurally aligned. The body is a plain
                // synchronous call into the inherent method, so the
                // future the trait method returns just awaits to
                // the inherent's value.
                if matches!(method.return_type, ReturnType::AsyncType(_)) {
                    self.w.push_str("async ");
                }
                self.w.push_str("fn ");
                self.w.push_str(method_name);
                // Match the interface's declared receiver: `&self`
                // (stage-1 dispatch). The implementer is a forced wrapper
                // class whose inherent method is also `&self` (mutation via
                // interior `borrow_mut()`), and the delegating body names
                // the inherent path explicitly (`ClassName::method(self,
                // …)`), which resolves to the inherent impl ahead of this
                // trait method — so `&self` here neither recurses nor needs
                // a mutable receiver.
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
                    // Value position, and it must match the trait method's
                    // param type exactly — both render an interface param as
                    // `Rc<dyn Trait>`.
                    self.emit_value_type_as_rust(&subst);
                }
                self.w.push(')');
                match &method.return_type {
                    ReturnType::Void => {}
                    ReturnType::Type(t) => {
                        self.w.push_str(" -> ");
                        let subst = substitute_type_ref(t, &type_subst);
                        self.emit_return_type_as_rust(&subst);
                    }
                    ReturnType::AsyncType(t) => {
                        // `async T` trait method → `async fn (...) -> T`.
                        // The `async` keyword sat in front of `fn`
                        // earlier in this loop; here we only need the
                        // return-type tail.
                        self.w.push_str(" -> ");
                        let subst = substitute_type_ref(t, &type_subst);
                        self.emit_return_type_as_rust(&subst);
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
                // `async` trait methods need the inherent call to be
                // awaited so the rollup yields the trait's declared
                // value type (not the inner Future). The enclosing
                // method header was emitted as `async fn`, so the
                // `.await` is legal here.
                let is_async = matches!(method.return_type, ReturnType::AsyncType(_));
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
                        if is_async {
                            self.w.push_str(".await");
                        }
                    }
                    _ => {
                        // Inherent on this class — emit as an
                        // explicit `ClassName::method(self, args)`
                        // call. Both the trait method we're inside
                        // and the inherent method now take `&self`,
                        // but the fully-qualified `ClassName::method`
                        // path resolves to the inherent impl ahead of
                        // the trait, so this delegates without
                        // recursing.
                        self.w.push_str(&class_decl.name.text);
                        self.emit_generic_params_as_args(&class_decl.generic_params);
                        self.w.push_str("::");
                        self.w.push_str(method_name);
                        self.w.push_str("(self");
                        for param in &method.params {
                            self.w.push_str(", ");
                            self.w.push_str(&param.name);
                        }
                        self.w.push(')');
                        if is_async {
                            self.w.push_str(".await");
                        }
                    }
                }
                self.w.push('\n');
                self.w.indent_dec();
                self.w.line("}");
            }
            // Runtime-type downcast hook overrides: for each target this class
            // IS-A, override the interface's `__jux_as_<T>` hook with
            // `Some(self)` so `(T) ifaceRef` / `ifaceRef => T` works.
            if let Some(iface_seg) = interface_ty.name.segments.first() {
                for t in self.interface_hook_targets(&iface_seg.text) {
                    if self.class_is_a(&class_decl.name.text, &t) {
                        self.emit_downcast_hook_impl(&t);
                    }
                }
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
        self.emit_field_type_as_rust(&juxc_tycheck::resolved_field_type(field));
        self.w.push_str(" = ");
        if let Some(init) = &field.default {
            self.emit_expr(init);
        } else {
            // No initializer — Rust requires one at the const/static
            // site. Emit a placeholder so the build fails with a
            // clear error rather than silently producing wrong code.
            self.emit_field_default_value_for(&juxc_tycheck::resolved_field_type(field));
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
    /// Emit the `fn __static_init()` associated function for a class that
    /// declares `static { }` blocks (§S.4.1). The block bodies run **once**,
    /// guarded by a `std::sync::Once` (thread-safe, runs to completion before
    /// any other thread observes the class as initialized). It's invoked from
    /// the observable-use trigger points — instance construction and static
    /// method calls — via [`Self::emit_static_init_trigger`].
    ///
    /// `enclosing_class` is already set by the caller, so static-field writes
    /// inside the block lower to their module-scope `LazyLock<Mutex<T>>`.
    pub(crate) fn emit_static_init_fn(&mut self, class_decl: &juxc_ast::ClassDecl) {
        if class_decl.static_init_blocks.is_empty() {
            return;
        }
        use crate::analysis::collect_mutated_names;
        use crate::stmts::stmt_span;
        self.w.indent_inc();
        self.w.line("fn __static_init() {");
        self.w.indent_inc();
        // Re-entrancy-safe once-latch. `Once` alone deadlocks/panics if the
        // initializer re-enters (a static block that reads a static field or
        // calls a static method of the same class). A thread-local "in progress"
        // flag lets a re-entrant call return early — reading the partially-set
        // state (a forward reference, which Java permits) — while a *different*
        // thread's first use still blocks on the single `Once` execution.
        self.w.line(
            "thread_local! { static __JUX_STATIC_BUSY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }",
        );
        self.w
            .line("static __JUX_STATIC_GUARD: std::sync::Once = std::sync::Once::new();");
        self.w
            .line("if __JUX_STATIC_GUARD.is_completed() || __JUX_STATIC_BUSY.with(|b| b.get()) { return; }");
        self.w.line("__JUX_STATIC_BUSY.with(|b| b.set(true));");
        self.w.line("__JUX_STATIC_GUARD.call_once(|| {");
        self.w.indent_inc();
        // Static context: no `this`. Collect mutated locals so reassignments
        // inside the block promote to `let mut`.
        let prev_this = self.this_alias.take();
        let mut muts = std::collections::HashSet::new();
        for block in &class_decl.static_init_blocks {
            collect_mutated_names(block, &mut muts, &self.user_mut_methods);
        }
        self.mutated_in_fn = muts;
        for block in &class_decl.static_init_blocks {
            for stmt in &block.statements {
                self.emit_source_marker(stmt_span(stmt));
                self.w.emit_indent();
                self.emit_stmt(stmt);
            }
        }
        self.this_alias = prev_this;
        self.w.indent_dec();
        self.w.line("});");
        self.w.line("__JUX_STATIC_BUSY.with(|b| b.set(false));");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }

    /// Emit the `Self::__static_init();` first-use trigger when the class
    /// being emitted declares `static { }` blocks (per the
    /// `emitting_class_has_static_init` flag). No-op otherwise. Called at the
    /// top of every constructor body and every static method body. The writer
    /// is expected to be at statement depth; `line` supplies the indent.
    pub(crate) fn emit_static_init_trigger(&mut self) {
        if self.emitting_class_has_static_init {
            self.w.line("Self::__static_init();");
        }
    }

    /// True when a static slot of this declared type can't live in the
    /// default `LazyLock<Mutex<T>>` shape because the lowered Rust type
    /// is **`!Send`** — a wrapper class (`Rc<RefCell<…>>`), an interface
    /// or polymorphic-base value (`Rc<dyn …>`), or a container carrying
    /// one as a generic arg / element. Those statics lower to a
    /// `thread_local!` + `RefCell` instead (sound for Phase 1's
    /// single-threaded execution model; a `Mutex` over an `Rc` would be
    /// a rustc E0277 leak).
    pub(crate) fn static_type_needs_thread_local(&self, ty: &juxc_ast::TypeRef) -> bool {
        if let Some(seg) = ty.name.segments.last() {
            let bare = seg.text.as_str();
            if self.wrapper_classes.contains(bare)
                || self.poly_base_classes.contains(bare)
                || self.lookup_interface_by_bare_or_fqn(bare).is_some()
            {
                return true;
            }
        }
        ty.generic_args.iter().any(|a| match a {
            juxc_ast::GenericArg::Type(t) => self.static_type_needs_thread_local(t),
            juxc_ast::GenericArg::Wildcard(_) => true,
        })
    }

    /// True when a **`final` static** of this declared type can't emit
    /// as a Rust `pub const` associated item: constructing a class or
    /// record runs a non-`const` `new` (rustc E0015), so the slot needs
    /// runtime initialization — the module-scope `LazyLock` shape (or
    /// `thread_local!` when the payload is also `!Send`). Primitives,
    /// `String` (as `&'static str`), and enum variants stay `pub const`.
    pub(crate) fn final_static_needs_runtime_init(&self, ty: &juxc_ast::TypeRef) -> bool {
        if self.static_type_needs_thread_local(ty) {
            return true;
        }
        // Primitives and `String` are const-constructible (`&'static
        // str` for String) — the stdlib's `String` CLASS stub in the
        // symbol table must not drag them onto the runtime path.
        if crate::types::jux_primitive_to_rust(ty).is_some()
            || crate::analysis::is_jux_string_type(ty)
        {
            return false;
        }
        let Some(seg) = ty.name.segments.last() else { return false };
        let bare = seg.text.as_str();
        self.lookup_class_by_bare_or_fqn(bare).is_some()
            || self
                .symbols
                .records
                .keys()
                .any(|k| k == bare || k.rsplit('.').next().unwrap_or(k) == bare)
    }

    pub(crate) fn emit_mutable_static_field(
        &mut self,
        class_name: &str,
        field: &juxc_ast::FieldDecl,
    ) {
        // **`!Send` payload → `thread_local!` storage.** See
        // `static_type_needs_thread_local`. The `RefCell` makes the
        // SLOT reassignable (`Registry.global = new Counter()`);
        // sharing/mutation of the held object goes through the
        // object's own wrapper. Reads hand out a shared handle via
        // `.with(|__s| __s.borrow().clone())` — an `Rc` refcount bump,
        // identical semantics to an instance-field read.
        if self.static_type_needs_thread_local(&juxc_tycheck::resolved_field_type(field)) {
            self.w.emit_indent();
            self.w.push_str("thread_local! {\n");
            self.w.indent_inc();
            self.w.emit_indent();
            self.w.push_str("pub static ");
            self.w.push_str(class_name);
            self.w.push('_');
            self.w.push_str(&field.name.text);
            self.w.push_str(": std::cell::RefCell<");
            self.emit_field_type_as_rust(&juxc_tycheck::resolved_field_type(field));
            self.w.push_str("> = std::cell::RefCell::new(");
            if let Some(init) = &field.default {
                self.emit_expr(init);
            } else {
                self.emit_field_default_value_for(&juxc_tycheck::resolved_field_type(field));
            }
            self.w.push_str(");\n");
            self.w.indent_dec();
            self.w.emit_indent();
            self.w.push_str("}\n");
            return;
        }
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
        self.emit_field_type_as_rust(&juxc_tycheck::resolved_field_type(field));
        self.w.push_str(">> = std::sync::LazyLock::new(|| std::sync::Mutex::new(");
        if let Some(init) = &field.default {
            // Not in const-context here — runtime allocation is fine
            // because the closure runs on first access, not at link
            // time. So `String` literals can keep their normal
            // `.to_string()` wrap and `new Foo(…)` works as expected.
            self.emit_expr(init);
        } else {
            self.emit_field_default_value_for(&juxc_tycheck::resolved_field_type(field));
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
        // `&mut self` is needed when the body either directly
        // writes to `this.field` OR calls a `&mut self` method
        // (one in `user_mut_methods`) on a `this`-rooted receiver.
        // The second condition handles the cascade through trait
        // methods: interface methods all emit as `&mut self` now,
        // so any method that calls a trait method on `self.field`
        // propagates the mut-self requirement up.
        // Wrapper-shape classes (§CR.4.1) always take `&self`:
        // interior mutability through `Rc<RefCell<C_Inner>>` means a
        // field write doesn't need a mutable receiver. Mutation goes
        // through `self.0.borrow_mut()` inside the body instead.
        let needs_mut_self = !self.emitting_wrapper_class
            && body
                .map(|b| {
                    body_writes_to_this(b)
                        || crate::analysis::body_calls_mut_method_on_this(b, &self.user_mut_methods)
                })
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
        // `async T` method → `async fn`. Same rule as `emit_fn_decl`:
        // Rust's `async` keyword sits before `fn`, so we prepend it
        // when the declared return type is async.
        if matches!(method.return_type, ReturnType::AsyncType(_)) {
            self.w.push_str("async ");
        }
        // `unsafe T m()` → `unsafe fn m()` (§A.2.4 modifier).
        if method.modifiers.contains(&juxc_ast::FnModifier::Unsafe) {
            self.w.push_str("unsafe ");
        }
        self.w.push_str("fn ");
        self.w.push_str(&method.name.text);
        if let Some(sfx) = self.pending_decl_suffix.take() { self.w.push_str(&sfx); }
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
            self.emit_value_type_as_rust(&lifted_param_tys[i]);
        }
        self.w.push(')');
        match &method.return_type {
            ReturnType::Void => {}
            ReturnType::Type(t) => {
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
            ReturnType::AsyncType(t) => {
                // `async T` → `async fn (...) -> T`. The `async`
                // keyword was already emitted ahead of `fn`.
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
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
            // Record this method's parameter names so the implicit-`this`
            // rewrite (bare instance-field → `this.field`) doesn't fire for a
            // parameter that shadows a field.
            self.current_fn_params = method.params.iter().map(|p| p.name.text.clone()).collect();
            let saved = self.current_return_type.take();
            self.current_return_type = Some(method.return_type.clone());
            // Method-level generic params extend the class-level sets
            // for this body (`T pick<int K>()` / `<U> U map(…)`).
            let prev_const_ints = self.const_int_params.clone();
            self.const_int_params
                .extend(crate::collect_const_int_params(&method.generic_params));
            let prev_type_params = self.current_type_params.clone();
            self.current_type_params
                .extend(crate::collect_type_param_names(&method.generic_params));
            // First-use trigger for `static { }` blocks (§S.4.1): a static
            // method call is an observable use. (Instance methods aren't —
            // constructing the receiver already triggered init.)
            if is_static {
                self.emit_static_init_trigger();
            }
            self.emit_fn_body_at(body, &method.return_type);
            self.const_int_params = prev_const_ints;
            self.current_type_params = prev_type_params;
            self.current_return_type = saved;
            self.current_fn_params.clear();
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
        if let Some(sfx) = self.pending_decl_suffix.take() { self.w.push_str(&sfx); }
            self.w.push_str("\")\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }

    /// Emit a sealed-class declaration as a Rust enum whose variants
    /// wrap each permitted subclass struct. The subclass declarations
    /// themselves still emit as structs (via `emit_class_decl`) but
    /// skip the `__parent` embedding so they aren't recursively-
    /// shaped.
    ///
    /// Output shape for `sealed class Light permits Red, Yellow, Green {}`:
    ///
    /// ```text
    /// #[derive(Clone, Debug)]
    /// pub enum Light {
    ///     Red(Red),
    ///     Yellow(Yellow),
    ///     Green(Green),
    /// }
    /// impl From<Red> for Light { fn from(v: Red) -> Self { Self::Red(v) } }
    /// impl From<Yellow> for Light { fn from(v: Yellow) -> Self { Self::Yellow(v) } }
    /// impl From<Green> for Light { fn from(v: Green) -> Self { Self::Green(v) } }
    /// ```
    ///
    /// The auto-`From` impls make `.into()` at upcast sites (return
    /// statements, function-call args, typed-let initializers) wrap
    /// the subclass into the variant transparently.
    ///
    /// Phase-1 limitation: only sealed classes with an empty body
    /// (no fields, methods, or constructors of their own) take this
    /// path. Sealed classes with bodies fall back to the regular
    /// struct emission so existing tests still build; adding
    /// match-dispatch wrappers for sealed-class methods is a
    /// follow-up.
    pub(crate) fn emit_sealed_enum(&mut self, class_decl: &juxc_ast::ClassDecl) {
        // `#[derive(Clone, Debug)]` mirrors the class-struct shape
        // so the enum participates in the same auto-Clone/Debug
        // rules existing code paths rely on (throw-payload
        // rendering, format-arg JuxOpt wrapping, etc.).
        self.w.line("#[derive(Clone, Debug)]");
        self.w.emit_indent();
        self.emit_visibility(class_decl.visibility);
        self.w.push_str("enum ");
        self.w.push_str(&class_decl.name.text);
        self.emit_generic_params(&class_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        for permitted in &class_decl.permits {
            self.w.emit_indent();
            self.w.push_str(&permitted.text);
            self.w.push('(');
            self.w.push_str(&permitted.text);
            self.w.push_str("),\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        // From<Sub> for Sealed — drives `.into()` at every upcast
        // site. Rust's blanket `From<T> for T` covers identity
        // conversions, so call sites can emit `.into()`
        // unconditionally without breaking same-type passing.
        for permitted in &class_decl.permits {
            self.w.emit_indent();
            self.w.push_str("impl From<");
            self.w.push_str(&permitted.text);
            self.w.push_str("> for ");
            self.w.push_str(&class_decl.name.text);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str(" { fn from(v: ");
            self.w.push_str(&permitted.text);
            self.w.push_str(") -> Self { Self::");
            self.w.push_str(&permitted.text);
            self.w.push_str("(v) } }\n");
        }
        // Marker trait `<Name>Kind` — emitted to match the
        // value-class lowering's contract. Subclasses still emit
        // `impl LightKind for Red {}` from `emit_class_marker_trait`'s
        // ancestor-walk, so the trait must exist for those impls
        // to compile. The trait is empty (no methods), so it
        // costs nothing at runtime.
        self.w.emit_indent();
        self.emit_visibility(class_decl.visibility);
        self.w.push_str("trait ");
        self.w.push_str(&class_decl.name.text);
        self.w.push_str("Kind {}\n");
        // The enum itself satisfies its own marker — keeps the
        // bound `T: LightKind` usable with a value of type Light.
        self.w.emit_indent();
        self.w.push_str("impl ");
        self.w.push_str(&class_decl.name.text);
        self.w.push_str("Kind for ");
        self.w.push_str(&class_decl.name.text);
        self.emit_generic_params_as_args(&class_decl.generic_params);
        self.w.push_str(" {}\n");
        self.w.newline();
        // **Static fields (§CR static-field rule).** A sealed class can
        // still declare statics — `public static int allocated = 0;` on
        // `Shape`. The value-class path emits these too; the sealed
        // (enum) lowering must mirror it or a bare-name access
        // (`allocated = allocated + 1` inside the constructor, which
        // lowers to `Shape_allocated`) dangles with no definition
        // (E0425). Two shapes, same as the value-class path:
        //
        //   - `static final` → an associated `const` on the enum's
        //     inherent impl (`Shape::CONST`).
        //   - non-`final static` (mutable) → a module-scope
        //     `LazyLock<Mutex<T>>` named `<Class>_<field>`, which the
        //     bare-name rewrite and `emit_assign` already target.
        let has_final_static = class_decl
            .fields
            .iter()
            .any(|f| f.is_static && f.is_final);
        if has_final_static {
            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params(&class_decl.generic_params);
            self.w.push(' ');
            self.w.push_str(&class_decl.name.text);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str(" {\n");
            for field in &class_decl.fields {
                if field.is_static
                    && field.is_final
                    && !self.final_static_needs_runtime_init(&juxc_tycheck::resolved_field_type(field))
                {
                    self.emit_static_field(field);
                }
            }
            self.w.line("}");
            self.w.newline();
        }
        // Mutable statics at module scope. A generic sealed class can't
        // have a static field mentioning its own type params (Java's
        // rule), so the value-class guard isn't needed here — sealed
        // statics are always concretely typed.
        for field in &class_decl.fields {
            // `final`+`!Send` payloads route here too (thread_local form).
            let final_needs_tl = field.is_final
                && self.final_static_needs_runtime_init(&juxc_tycheck::resolved_field_type(field));
            if field.is_static && (!field.is_final || final_needs_tl) {
                self.emit_mutable_static_field(&class_decl.name.text, field);
            }
        }
        // Match-dispatching impl block for the sealed parent's
        // own instance methods. Each method emits as
        //   `fn name(&self, args) -> R { match self { Shape::Circle(c)
        //      => c.name(args), Shape::Square(s) => s.name(args), … } }`
        // Subclasses pick up the inherited method body through
        // the existing virtual-dispatch inlining pass, so the
        // `c.name(args)` resolves to the inherited (or overridden)
        // body on each variant.
        //
        // Static methods don't participate in dispatch — they
        // stay on the parent enum as inherent associated fns.
        // Constructor on the sealed parent doesn't make sense
        // (you can't construct an "abstract" enum directly), so
        // those are skipped.
        if !class_decl.methods.is_empty() {
            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params(&class_decl.generic_params);
            self.w.push(' ');
            self.w.push_str(&class_decl.name.text);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str(" {\n");
            self.w.indent_inc();
            for method in &class_decl.methods {
                self.emit_sealed_method_dispatch(class_decl, method);
            }
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
        }
    }

    /// Emit a single sealed-class method as a match-dispatching
    /// wrapper on the enum. Each variant delegates to the
    /// matching subclass's inherent method of the same name.
    fn emit_sealed_method_dispatch(
        &mut self,
        class_decl: &juxc_ast::ClassDecl,
        method: &juxc_ast::FnDecl,
    ) {
        // Static methods on a sealed parent stay as plain
        // associated fns — no dispatch needed.
        let is_static = method
            .modifiers
            .iter()
            .any(|m| matches!(m, juxc_ast::FnModifier::Static));
        if is_static {
            // Static methods on a sealed class don't need
            // dispatch. Fall back to the regular method emit so
            // callers can still reach `Shape::staticHelper(...)`.
            self.emit_method(method);
            return;
        }
        self.w.emit_indent();
        self.emit_visibility(method.visibility);
        // Match async — sealed-method dispatch on `async T`
        // methods just forwards through `.await` on each arm.
        if matches!(method.return_type, ReturnType::AsyncType(_)) {
            self.w.push_str("async fn ");
        } else {
            self.w.push_str("fn ");
        }
        self.w.push_str(&method.name.text);
        if let Some(sfx) = self.pending_decl_suffix.take() { self.w.push_str(&sfx); }
        self.w.push_str("(&self");
        for param in &method.params {
            self.w.push_str(", ");
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_value_type_as_rust(&param.ty);
        }
        self.w.push(')');
        match &method.return_type {
            ReturnType::Void => {}
            ReturnType::Type(t) => {
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
            ReturnType::AsyncType(t) => {
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
        }
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.emit_indent();
        self.w.push_str("match self {\n");
        self.w.indent_inc();
        for permitted in &class_decl.permits {
            self.w.emit_indent();
            self.w.push_str(&class_decl.name.text);
            self.w.push_str("::");
            self.w.push_str(&permitted.text);
            self.w.push_str("(__variant) => __variant.");
            self.w.push_str(&method.name.text);
        if let Some(sfx) = self.pending_decl_suffix.take() { self.w.push_str(&sfx); }
            self.w.push('(');
            for (i, param) in method.params.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.w.push_str(&param.name.text);
            }
            self.w.push(')');
            // Async dispatch needs `.await` on each arm so the
            // outer `async fn` produces the value type, not a
            // Future-of-future.
            if matches!(method.return_type, ReturnType::AsyncType(_)) {
                self.w.push_str(".await");
            }
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
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

/// Clone a method declaration with its **signature types** rewritten
/// through `subst` (a `parent-type-param → concrete-type` map). Used by
/// the wrapper inherited-method inlining pass so a generic parent's
/// `name(&self) -> T` lowers as `-> isize` when copied into a child that
/// bound `T` to `int` via `extends Container<int>`.
///
/// Only the return type and parameter types are substituted — the body
/// is left verbatim. The body's `this.field` reads resolve through the
/// `__parent`-walk + field-type lookup (which already monomorphizes the
/// field's declared type), and value expressions inside it are emitted
/// by the regular expression walkers, so a textual type-param rewrite of
/// the body isn't needed for the signature to type-check.
///
/// A method that declares its OWN generic params shadowing a parent
/// param keeps them: those names are dropped from the effective subst so
/// the method-local parameter isn't accidentally replaced.
fn substitute_fn_signature(
    m: &juxc_ast::FnDecl,
    subst: &std::collections::HashMap<String, juxc_ast::TypeRef>,
) -> juxc_ast::FnDecl {
    // Drop any subst entry shadowed by the method's own type params.
    let effective: std::collections::HashMap<String, juxc_ast::TypeRef> = if m
        .generic_params
        .is_empty()
    {
        subst.clone()
    } else {
        let shadow: std::collections::HashSet<&str> = m
            .generic_params
            .iter()
            .map(|p| p.name.text.as_str())
            .collect();
        subst
            .iter()
            .filter(|(k, _)| !shadow.contains(k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    let return_type = match &m.return_type {
        juxc_ast::ReturnType::Void => juxc_ast::ReturnType::Void,
        juxc_ast::ReturnType::Type(t) => {
            juxc_ast::ReturnType::Type(substitute_type_ref(t, &effective))
        }
        juxc_ast::ReturnType::AsyncType(t) => {
            juxc_ast::ReturnType::AsyncType(substitute_type_ref(t, &effective))
        }
    };
    let params = m
        .params
        .iter()
        .map(|p| juxc_ast::Param {
            name: p.name.clone(),
            ty: substitute_type_ref(&p.ty, &effective),
            is_final: p.is_final,
            is_ref: p.is_ref,
            default: p.default.clone(),
            is_varargs: p.is_varargs,
            span: p.span,
        })
        .collect();
    juxc_ast::FnDecl {
        annotations: m.annotations.clone(),
        visibility: m.visibility,
        modifiers: m.modifiers.clone(),
        return_type,
        name: m.name.clone(),
        generic_params: m.generic_params.clone(),
        params,
        throws: m.throws.clone(),
        wheres: m.wheres.clone(),
        body: m.body.clone(),
        is_property: m.is_property,
        span: m.span,
    }
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
        ptr_depth: ty.ptr_depth,
        span: ty.span,
    }
}
