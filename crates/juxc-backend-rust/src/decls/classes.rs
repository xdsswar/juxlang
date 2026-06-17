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
        // **Stdlib intrinsic skip.** A small fixed set of stdlib class names
        // lower to Rust std host types — the Jux source files under `jux.std/*`
        // document their API but the compiler owns the actual implementation.
        // Suppress struct emission for those so we don't end up with a duplicate
        // definition next to the std container.
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
        // Track the class params' BOUNDS too, so `<R extends K>` method generics
        // can expand `K` to its bounds when emitting (`R: Id + Named + …`).
        let prev_type_param_bounds = std::mem::take(&mut self.type_param_bounds);
        self.type_param_bounds = crate::collect_type_param_bounds(&class_decl.generic_params);
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
            self.type_param_bounds = prev_type_param_bounds;
            return;
        }
        // Derive Clone unconditionally. Debug is also derived, EXCEPT
        // when a field is function-typed (`() -> T`): `dyn Fn()` doesn't
        // implement Debug, so the derive would fail. Instead we emit a
        // manual `impl Debug` stub after the struct that prints the class
        // name — satisfying the marker-trait `Debug` supertrait bound
        // without requiring Debug on the stored closure.
        let has_fn_field = class_decl.fields.iter().any(|f| {
            f.ty.as_ref().map(|t| t.fn_shape.is_some()).unwrap_or(false)
                // §P.2: `observer<T>` fields lower to `Rc<dyn Fn(…)>` —
                // same no-Debug shape as fn-typed fields.
                || f.ty
                    .as_ref()
                    .map(|t| {
                        t.fn_shape.is_none()
                            && t.name.segments.len() == 1
                            && t.name.segments[0].text == "observer"
                    })
                    .unwrap_or(false)
        });
        // A `@layout(c)` value struct (§L.1.2) gets a C-compatible layout and is
        // `Copy` (its fields are primitives / pointers / other `@layout(c)`
        // structs), giving Jux's "copied on assignment" value semantics.
        let is_value_struct = crate::is_layout_c_struct(class_decl);
        if is_value_struct {
            self.w.line("#[repr(C)]");
            self.w.line("#[derive(Clone, Copy, Debug)]");
        } else if has_fn_field {
            self.w.line("#[derive(Clone)]");
        } else {
            self.w.line("#[derive(Clone, Debug)]");
        }
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
            let fty = juxc_tycheck::resolved_field_type(field);
            // §P.2: `observer<T>` fields — arity-aware lowering, same
            // rule as the wrapper path (shape read from the lambda
            // initializer; recorded for `.observers.attach` routing).
            if fty.name.segments.len() == 1
                && fty.name.segments[0].text == "observer"
                && fty.fn_shape.is_none()
            {
                let arity = match &field.default {
                    Some(juxc_ast::Expr::Lambda(l)) => l.params.len(),
                    _ => 2,
                };
                self.observer_shapes.insert(field.name.text.clone(), arity);
                self.emit_observer_var_type(&fty, arity);
                self.w.push_str(",\n");
                continue;
            }
            // `ref` field (§M.13): the slot is a SHARED reference cell.
            if field.is_ref {
                self.w.push_str("std::rc::Rc<std::cell::RefCell<");
                self.emit_field_type_as_rust(&fty);
                self.w.push_str(">>,\n");
                continue;
            }
            // Field-position type mapping (String → owned `String`).
            self.emit_field_type_as_rust(&fty);
            self.w.push_str(",\n");
        }
        // PhantomData for type params used only in method/sub-param bounds
        // (`Registry<K, V, N>` where `K` constrains `V` but appears in no
        // field). Rust's E0392 "type parameter never used" requires every
        // declared param to appear in the struct body; a zero-sized
        // `PhantomData<K>` field satisfies that without changing layout.
        // `#[derive(Clone, Debug)]` cover `PhantomData` already.
        for phantom in crate::unused_class_type_params(class_decl) {
            self.w.emit_indent();
            self.w.push_str("pub(crate) __phantom_");
            self.w.push_str(&phantom);
            self.w.push_str(": std::marker::PhantomData<");
            self.w.push_str(&phantom);
            self.w.push_str(">,\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        // Manual `impl Debug` for classes with function-typed fields —
        // `dyn Fn()` doesn't implement Debug so `#[derive(Debug)]` would fail.
        // The stub prints the class name so marker-trait `Debug` supertrait
        // bounds are satisfied and `throw` lowering can still format the type.
        if has_fn_field {
            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
            self.w.push_str(" std::fmt::Debug for ");
            self.w.push_str(&class_decl.name.text);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str(" { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, \"");
            self.w.push_str(&class_decl.name.text);
            self.w.push_str("\") } }\n");
        }
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
                        // Generic classes need `impl<T: Clone + Debug>` before
                        // `From<Child<T>>` — otherwise `T` is out of scope (E0412).
                        self.w.push_str("impl");
                        self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
                        self.w.push_str(" From<");
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
            // Use emit_generic_params_with_clone_bound so generic children like
            // `Child<T>` get `impl<T: Clone + Debug>` — plain `impl<T>` fails
            // to satisfy the struct's own `T: Clone + Debug` requirement (E0277).
            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
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
            self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
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
            // Generic class: plain static methods are lifted to free functions
            // (`<Class>_<method>`) AFTER the impl — see
            // `emit_generic_class_static_fns` — so they don't require the
            // class's K/V/N to be inferred at the call site. Skip them here.
            // Property accessors / observer helpers (`__…`) stay associated.
            if Self::generic_class_lifts_static(class_decl, method) {
                continue;
            }
            let occ = seen_names.entry(method.name.text.clone()).or_insert(0);
            if *occ > 0 {
                self.pending_decl_suffix = Some(format!("__ov{occ}"));
            }
            *occ += 1;
            // P7: a STATIC observable property's setter gets the
            // observer fire bracket on the inline path too (instance
            // observable props force the wrapper path, but statics
            // don't reclassify the class).
            if let Some(prop_name) = method.name.text.strip_prefix("__set_") {
                if let Some(prop) = crate::decls::observers::static_observable_props(class_decl)
                    .into_iter()
                    .find(|p| p.name.text == prop_name)
                {
                    self.pending_setter_observer = Some((
                        prop_name.to_string(),
                        self.property_type_is_comparable(&prop.ty),
                        Vec::new(),
                        true,
                        0,
                    ));
                }
            }
            self.emit_method(method);
        }
        // P7: static-property observer helpers (associated fns over
        // the module-scope thread_local storage emitted below).
        self.emit_static_observer_helper_methods(class_decl);
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
                        // Package-aware: resolve the parent's bare `extends`
                        // name within the current unit's package so a same-named
                        // class in another package can't be picked.
                        self.resolve_bare_class_fqn(bare)
                            .and_then(|fqn| self.class_asts.get(&fqn))
                            .cloned()
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
        // **Interface-default forwarders (§7.4.3)** — same as the wrapper
        // path: emit a `pub fn` forwarding to the trait default for every
        // `default` interface method this (non-wrapper) class doesn't
        // override, so direct `obj.method()` resolves inherent-first.
        self.emit_inherited_default_methods(class_decl);
        self.w.line("}");
        self.w.newline();

        // `drop { }` destructor (§6.6 / §S.5) — inline classes get
        // `impl Drop` on the struct itself.
        let inline_name = class_decl.name.text.clone();
        self.emit_drop_impl(class_decl, &inline_name);

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
        // P7: thread_local observer storage for static observable
        // properties, beside their LazyLock value backing.
        self.emit_static_observer_storage(class_decl);

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
        self.type_param_bounds = prev_type_param_bounds;
        // Generic-class static methods → module-scope free functions
        // (`<Class>_<method>`). Emitted AFTER the class params are out of
        // scope so they don't dangle into the free fn's signature.
        self.emit_generic_class_static_fns(class_decl);
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
    /// True when `ty` names a foreign type from a NON-`std` crate
    /// (`rust.<crate>.*`, crate != `std`). Such a type may not implement `Clone`
    /// — `minifb::Window` is the canonical example — whereas `rust.std`
    /// collections (`Vec`, `HashMap`, …) always are. Used to decide whether a
    /// wrapper's `*_Inner` can `#[derive(Clone)]`.
    fn type_is_nonstd_foreign(&self, ty: &juxc_ast::TypeRef) -> bool {
        let qn = &ty.name;
        let fqn = if qn.segments.len() == 1 {
            self.symbols.find_fqn_by_bare(&qn.segments[0].text)
        } else {
            Some(
                qn.segments
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join("."),
            )
        };
        let Some(fqn) = fqn else { return false };
        let is_external = self
            .symbols
            .classes
            .get(&fqn)
            .map(|s| s.is_external)
            .unwrap_or(false)
            || self
                .symbols
                .enums
                .get(&fqn)
                .map(|s| s.is_external)
                .unwrap_or(false);
        if !is_external {
            return false;
        }
        let segs: Vec<&str> = fqn.split('.').collect();
        segs.first() == Some(&"rust") && segs.get(1) != Some(&"std")
    }

    /// True when this class — or any ancestor it embeds as `__parent` — holds a
    /// non-`std`-foreign instance field. Such a field may not be `Clone`, so the
    /// flattened `*_Inner` can't `#[derive(Clone)]`. The wrapper newtype stays
    /// `Clone` regardless (it shares the instance BY REFERENCE through its `Rc`,
    /// which never deep-copies the inner value), so dropping the inner derive is
    /// safe for a wrapper class.
    fn wrapper_inner_blocks_clone(&self, class_decl: &juxc_ast::ClassDecl) -> bool {
        let own = |fields: &[juxc_ast::FieldDecl]| {
            fields.iter().any(|f| {
                !f.is_static
                    && f.ty
                        .as_ref()
                        .map(|t| self.type_is_nonstd_foreign(t))
                        .unwrap_or(false)
            })
        };
        if own(&class_decl.fields) {
            return true;
        }
        let mut cursor = class_decl.extends.clone();
        let mut depth = 0usize;
        while let Some(p) = cursor {
            if depth > 64 {
                break;
            }
            let Some(bare) = p.name.segments.last().map(|s| s.text.clone()) else {
                break;
            };
            let Some(pd) = self.lookup_class_ast_by_bare_or_fqn(&bare) else {
                break;
            };
            if own(&pd.fields) {
                return true;
            }
            cursor = pd.extends.clone();
            depth += 1;
        }
        false
    }

    pub(crate) fn emit_wrapper_class_decl(&mut self, class_decl: &juxc_ast::ClassDecl) {
        let name = &class_decl.name.text;
        let inner = format!("{name}_Inner");

        // ---- C_Inner: the instance fields ----
        // Debug joins Clone so the newtype's derived Debug resolves
        // (`Rc<RefCell<C_Inner>>: Debug` requires `C_Inner: Debug`).
        // EXCEPT when a field is function-typed or an `observer<T>`
        // (§P.2) — both lower to `Rc<dyn Fn(…)>`, which has no Debug;
        // those classes get a manual name-printing impl after the
        // struct instead.
        let inner_has_fn_field = class_decl.fields.iter().any(|f| {
            f.ty.as_ref()
                .map(|t| {
                    t.fn_shape.is_some()
                        || (t.name.segments.len() == 1
                            && t.name.segments[0].text == "observer")
                })
                .unwrap_or(false)
        });
        // `Clone` is dropped from the inner when a non-`std`-foreign field may
        // not be `Clone` (e.g. `minifb::Window`). The wrapper newtype still
        // clones — it shares the instance by reference through its `Rc`, which
        // never deep-copies the inner — so the inner never needs `Clone` for a
        // wrapper class. `Debug` is dropped only for fn/observer fields (no
        // `Debug`), which instead get the manual name-printing impl below.
        let blocks_clone = self.wrapper_inner_blocks_clone(class_decl);
        let derive = match (blocks_clone, inner_has_fn_field) {
            (false, false) => "#[derive(Clone, Debug)]",
            (false, true) => "#[derive(Clone)]",
            (true, false) => "#[derive(Debug)]",
            (true, true) => "",
        };
        if !derive.is_empty() {
            self.w.line(derive);
        }
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
            let fty = juxc_tycheck::resolved_field_type(field);
            // §P.2: `observer<T>` fields. The Rust shape depends on the
            // lambda's arity, read from the field initializer (0 =
            // invalidation `Fn()`, 2 = full `Fn(T, T)`, 3 = full with
            // property reference `Fn(String, T, T)`); the arity is
            // recorded so `.observers.attach(...)` call sites route to
            // the matching storage vec.
            if fty.name.segments.len() == 1
                && fty.name.segments[0].text == "observer"
                && fty.fn_shape.is_none()
            {
                let arity = match &field.default {
                    Some(juxc_ast::Expr::Lambda(l)) => l.params.len(),
                    _ => 2,
                };
                self.observer_shapes.insert(field.name.text.clone(), arity);
                self.emit_observer_var_type(&fty, arity);
                self.w.push_str(",\n");
                continue;
            }
            if field.is_weak {
                // `weak` field (§6.5): store a non-owning `Weak` at the
                // target class's inner cell, so it does NOT contribute to the
                // refcount and breaks the cycle. The strong view is recovered
                // by `.get()` (→ `upgrade().map(Target)`). The target is a
                // plain class (tycheck E0455 guarantees it), so its payload is
                // `Target_Inner`.
                let target =
                    fty.name.segments.last().map_or("", |s| s.text.as_str());
                self.w.push_str("std::rc::Weak<std::cell::RefCell<");
                self.w.push_str(target);
                self.w.push_str("_Inner>>");
            } else if field.is_ref {
                // `ref` field (§M.13): a SHARED reference cell.
                self.w.push_str("std::rc::Rc<std::cell::RefCell<");
                self.emit_field_type_as_rust(&fty);
                self.w.push_str(">>");
            } else {
                self.emit_field_type_as_rust(&fty);
            }
            self.w.push_str(",\n");
        }
        // Observable-property storage (§P.3.3): two lazy observer vecs
        // + a binding keep-alive per writable property. `None` until
        // the first attach — a never-observed property costs nothing
        // beyond the slots.
        self.emit_observer_inner_fields(class_decl);
        // PhantomData for type params used only in method/sub-param bounds —
        // same E0392 fix as the plain (non-wrapper) struct path above. The
        // inner struct carries the params, so the phantom field lives here.
        for phantom in crate::unused_class_type_params(class_decl) {
            self.w.emit_indent();
            self.w.push_str("pub(crate) __phantom_");
            self.w.push_str(&phantom);
            self.w.push_str(": std::marker::PhantomData<");
            self.w.push_str(&phantom);
            self.w.push_str(">,\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        // Manual Debug stand-in for inner structs holding `Rc<dyn Fn>`
        // (fn-typed / observer fields) — prints the class name so the
        // newtype's derived Debug and the marker trait's `Debug`
        // supertrait both resolve.
        if inner_has_fn_field {
            self.w.emit_indent();
            self.w.push_str("impl");
            self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
            self.w.push_str(" std::fmt::Debug for ");
            self.w.push_str(&inner);
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str(" { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, \"");
            self.w.push_str(name);
            self.w.push_str("\") } }\n");
        }
        self.w.newline();

        // ---- the newtype handle: C<T>(Rc<RefCell<C_Inner<T>>>) ----
        // The newtype declares the generic params (with the `T: Clone`
        // bound so the derived `Clone` resolves) and threads them onto
        // the inner type inside the `Rc<RefCell<…>>`. `Debug` is *not*
        // bounded here — it flows through `Rc<RefCell<C_Inner<T>>>: Debug`
        // whenever `C_Inner<T>: Debug`, which holds when `T: Debug`; the
        // derive emits the right `where` clause for us.
        // Handle shape by rep (§CR.3.3 / §CR.4.1): `RcRefCell` wraps the inner in
        // `Rc<RefCell<..>>`; bare `Rc` (read-only share) drops the cell; `Box`
        // (escapes-but-unaliased, unique owner) is `Box<..>`.
        let is_box = self.box_classes.contains(&class_decl.name.text);
        let refcell = self.refcell_classes.contains(&class_decl.name.text);
        let (open, close): (&str, &str) = if is_box {
            ("(std::boxed::Box<", ">);\n")
        } else if refcell {
            ("(std::rc::Rc<std::cell::RefCell<", ">>);\n")
        } else {
            ("(std::rc::Rc<", ">);\n")
        };
        self.w.line("#[derive(Clone, Debug)]");
        self.w.emit_indent();
        self.emit_visibility(class_decl.visibility);
        self.w.push_str("struct ");
        self.w.push_str(name);
        self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
        self.w.push_str(open);
        self.w.push_str(&inner);
        self.emit_generic_params_as_args(&class_decl.generic_params);
        self.w.push_str(close);
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
            // Generic class: plain static methods lift to free functions after
            // the impl (see `emit_generic_class_static_fns`). Skip here.
            if Self::generic_class_lifts_static(class_decl, method) {
                continue;
            }
            let occ = seen_names.entry(method.name.text.clone()).or_insert(0);
            if *occ > 0 {
                self.pending_decl_suffix = Some(format!("__ov{occ}"));
            }
            *occ += 1;
            // §P setter firing: a synthesized property setter
            // (`__set_<X>` for an observable property `X`) gets an
            // old/new capture around its body and a post-body observer
            // fire. `emit_method` consumes the pending marker.
            if let Some(prop_name) = method.name.text.strip_prefix("__set_") {
                if let Some(prop) = crate::decls::observers::observable_props(class_decl)
                    .into_iter()
                    .find(|p| p.name.text == prop_name)
                {
                    // §P.1.5: computed properties whose getter reads
                    // THIS property re-fire from this setter too.
                    let dependents: Vec<(String, bool)> =
                        crate::decls::observers::computed_observable_props(class_decl)
                            .into_iter()
                            .filter(|c| {
                                crate::decls::observers::computed_prop_deps(class_decl, c)
                                    .iter()
                                    .any(|d| d == prop_name)
                            })
                            .map(|c| {
                                (
                                    c.name.text.clone(),
                                    self.property_type_is_comparable(&c.ty),
                                )
                            })
                            .collect();
                    self.pending_setter_observer = Some((
                        prop_name.to_string(),
                        self.property_type_is_comparable(&prop.ty),
                        dependents,
                        false,
                        0,
                    ));
                } else if let Some(prop) =
                    crate::decls::observers::static_observable_props(class_decl)
                        .into_iter()
                        .find(|p| p.name.text == prop_name)
                {
                    // P7: static setter — class-scoped observer fire,
                    // no computed-dep tracking (static computed props
                    // are out of Phase-1 scope).
                    self.pending_setter_observer = Some((
                        prop_name.to_string(),
                        self.property_type_is_comparable(&prop.ty),
                        Vec::new(),
                        true,
                        0,
                    ));
                }
            }
            self.emit_method(method);
        }
        // §P observer plumbing: attach/detach/clear/size/fire helpers,
        // one set per observable property.
        self.emit_observer_helper_methods(class_decl);
        // P7: static-property observer helpers (associated fns).
        self.emit_static_observer_helper_methods(class_decl);
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
        // **Interface-default forwarders (§7.4.3).** For every `default`
        // interface method this class doesn't override, emit a `pub fn`
        // that fully-qualifies the trait — so `obj.label()` resolves
        // inherent-first instead of failing E0599 ("trait not in scope").
        // Runs for the WRAPPER class path; the legacy path has its own
        // call beside its inherent-impl close.
        self.emit_inherited_default_methods(class_decl);
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

        // `drop { }` destructor (§6.6 / §S.5) — wrapper classes get
        // `impl Drop` on the INNER struct, so the body runs exactly
        // once, when the LAST strong reference releases (the Rc's
        // payload drop), never per-handle.
        let inner_name = format!("{name}_Inner");
        self.emit_drop_impl(class_decl, &inner_name);

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
        // P7: thread_local observer storage for static observable
        // properties, beside their LazyLock value backing.
        self.emit_static_observer_storage(class_decl);

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
        // Generic-class static methods → module-scope free functions
        // (`<Class>_<method>`), so a `Class.method(args)` static call doesn't
        // require inferring the class's K/V/N (E0284) — see
        // `emit_generic_class_static_fns`.
        self.emit_generic_class_static_fns(class_decl);
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
        // `__parent` hop distance of the ancestor being copied from —
        // 1 for the direct parent, +1 per level. Drives the §P fire
        // bracket on copied property setters.
        let mut depth = 1usize;
        while let Some(parent_ref) = cursor {
            let Some(seg) = parent_ref.name.segments.first() else { break };
            let bare = seg.text.as_str();
            let parent_decl: Option<juxc_ast::ClassDecl> = self
                .class_asts
                .get(bare)
                .cloned()
                .or_else(|| {
                    // Package-aware parent resolution (see the inherited-method
                    // walk above) — avoids picking a same-named class from
                    // another package on the `extends` chain.
                    self.resolve_bare_class_fqn(bare)
                        .and_then(|fqn| self.class_asts.get(&fqn))
                        .cloned()
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
                // §P + inheritance (Java semantics): a copied
                // `__set_<X>` of an ANCESTOR's observable property
                // gets the same fire bracket the ancestor's own
                // emission gets — the storage lives `depth` `__parent`
                // hops up, and this class's depth-aware
                // `__obs_<X>_fire` helpers (see
                // `emit_observer_helper_methods`) reach it.
                if let Some(prop_name) = m.name.text.strip_prefix("__set_") {
                    if let Some(prop) = crate::decls::observers::observable_props(&parent)
                        .into_iter()
                        .find(|p| p.name.text == prop_name)
                    {
                        let dependents: Vec<(String, bool)> =
                            crate::decls::observers::computed_observable_props(&parent)
                                .into_iter()
                                .filter(|c| {
                                    crate::decls::observers::computed_prop_deps(&parent, c)
                                        .iter()
                                        .any(|d| d == prop_name)
                                })
                                .map(|c| {
                                    (
                                        c.name.text.clone(),
                                        self.property_type_is_comparable(
                                            &c.ty,
                                        ),
                                    )
                                })
                                .collect();
                        self.pending_setter_observer = Some((
                            prop_name.to_string(),
                            self.property_type_is_comparable(&prop.ty),
                            dependents,
                            false,
                            depth,
                        ));
                    }
                }
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
            depth += 1;
        }
    }

    /// **Inherited interface-default inlining (§7.4.3).** For every
    /// `default` method on an interface this class `implements` (directly
    /// or rolled up through `extends`) that the class does **not**
    /// override, emit a *forwarding inherent method* into the class's own
    /// `impl` block, e.g.
    ///
    /// ```text
    /// pub fn label(&self) -> String {
    ///     <Self as crate::xss::it::some::Holder<crate::xss::it::some::Object>>::label(self)
    /// }
    /// ```
    ///
    /// **Why.** A Jux interface lowers to a Rust trait, and a `default`
    /// method becomes a Rust trait *default body* — emitted on the trait,
    /// not in the `impl Trait for Class`. Rust resolves `holder.label()`
    /// inherent-methods-FIRST; since the class's inherent `impl` only
    /// carries the *overridden* methods (`write`/`test`), `label` misses
    /// and rustc demands the trait be in scope at the call site (E0599 +
    /// "trait `Holder` … is implemented but not in scope"). The call site
    /// has no `use Holder`, so the program won't build. The forwarding
    /// method makes `label` resolve inherent-first — exactly like an
    /// overridden method — while reusing the trait's default body via the
    /// fully-qualified `<Self as Trait<args>>::label(self)` form (a
    /// fully-qualified call needs no `use`).
    ///
    /// This mirrors [`Self::emit_class_trait_impls`] for the interface
    /// roll-up + per-interface `type_subst` (so a `Holder<Object>`
    /// default returning `T` forwards as returning `Object`), and reuses
    /// [`Self::emit_inherited_wrapper_methods`]'s ancestor-walk to skip a
    /// default already inlined as an inherent by a concrete ancestor.
    fn emit_inherited_default_methods(&mut self, class_decl: &juxc_ast::ClassDecl) {
        // Abstract classes never get an inherent forwarding method — they
        // carry no concrete `impl Trait for …`, and any concrete subclass
        // re-runs this walk and emits the forwarder itself.
        if class_decl.is_abstract {
            return;
        }
        // Roll up implemented interfaces: the class's own `implements`
        // clause PLUS interfaces inherited through the `extends` chain
        // (Java's "IS-A through the parent" rule). Identical to the
        // roll-up at the top of `emit_class_trait_impls`; the symbol
        // table carries each ancestor's `implements` because the AST only
        // holds the class's own list.
        let mut implements: Vec<juxc_ast::TypeRef> = class_decl.implements.clone();
        {
            let mut seen: std::collections::HashSet<String> = implements
                .iter()
                .filter_map(|t| t.name.segments.first().map(|s| s.text.clone()))
                .collect();
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
        // Methods this class overrides directly — a default with one of
        // these names already has an inherent body (the override), so no
        // forwarder is needed (and emitting one would be a duplicate-name
        // E0592). Also serves as the cross-interface dedup seed below.
        let mut provided: std::collections::HashSet<String> = class_decl
            .methods
            .iter()
            .map(|m| m.name.text.clone())
            .collect();
        for interface_ty in &implements {
            let Some(iface_name) = interface_ty.name.segments.first() else {
                continue;
            };
            // Build the interface-param → class-arg substitution
            // (`implements Holder<Object>` → `T ↦ Object`) so a default
            // returning `T` forwards as returning `Object`. Same shape as
            // `emit_class_trait_impls`.
            let iface_sig = self
                .lookup_interface_by_bare_or_fqn(iface_name.text.as_str())
                .map(|(_, i)| i);
            let Some(iface) = iface_sig else { continue };
            let mut type_subst: std::collections::HashMap<String, juxc_ast::TypeRef> =
                std::collections::HashMap::new();
            for (param, arg) in iface
                .generic_params
                .iter()
                .zip(interface_ty.generic_args.iter())
            {
                if let Some(arg_ty) = arg.as_type() {
                    type_subst.insert(param.name.text.clone(), arg_ty.clone());
                }
            }
            // Pull (name, MethodSig) pairs and sort for deterministic
            // emission order.
            let mut methods: Vec<(String, MethodSig)> = iface
                .methods
                .iter()
                .map(|(name, m)| (name.clone(), m.clone()))
                .collect();
            methods.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, sig) in &methods {
                // A **default** interface method = has a body on the
                // interface = `!is_static && !is_abstract`. Static methods
                // lower to free functions (never trait items), and abstract
                // methods have no body to forward to — skip both.
                if sig.is_static || sig.is_abstract {
                    continue;
                }
                // Already overridden by this class, or already emitted as a
                // forwarder for an earlier interface (first interface to
                // name a default wins; if two interfaces gave the same
                // default name the source had to override it — E0431 — so
                // it's in `provided` via the override and we skip).
                if provided.contains(name) {
                    continue;
                }
                // Skip if a concrete ancestor already provides this method
                // as an inherent — `emit_inherited_wrapper_methods` (and the
                // legacy inline-copy loop) already inlined that body onto
                // `Self`, so resolution finds it inherent-first without a
                // forwarder. Walk the `extends` chain for a non-abstract
                // method of this name (same pattern as the ancestor walk in
                // `emit_class_trait_impls`).
                let mut ancestor_has_inherent = false;
                let mut cursor: Option<&juxc_ast::TypeRef> = class_decl.extends.as_ref();
                while let Some(parent_ref) = cursor {
                    let Some(seg) = parent_ref.name.segments.first() else { break };
                    let Some((_, parent_sig)) = self
                        .symbols
                        .classes
                        .iter()
                        .find(|(k, _)| {
                            k.as_str() == seg.text.as_str()
                                || k.rsplit('.').next().unwrap_or(k.as_str()) == seg.text.as_str()
                        })
                        .map(|(k, v)| (k.clone(), v))
                    else { break };
                    if let Some(parent_method) = parent_sig.methods.get(name.as_str()) {
                        if !parent_method.is_abstract {
                            ancestor_has_inherent = true;
                            break;
                        }
                    }
                    cursor = parent_sig.extends.as_ref();
                }
                if ancestor_has_inherent {
                    continue;
                }
                // Mark provided so a later interface's same-named default
                // doesn't emit a duplicate forwarder.
                provided.insert(name.clone());
                // ---- Emit the forwarding inherent method ----
                // Signature mirrors `emit_class_trait_impls`'s delegating
                // method (so wrapper/ref/nullable param + return types
                // render identically), but the body fully-qualifies the
                // trait so no `use` is required at the call site.
                self.w.emit_indent();
                // Inherent forwarders are always `pub` (matching the rest of
                // the class's inherent surface) and `&self` (interface
                // defaults take a shared receiver; mutation goes through the
                // wrapper's interior `RefCell`).
                self.w.push_str("pub ");
                if matches!(sig.return_type, ReturnType::AsyncType(_)) {
                    self.w.push_str("async ");
                }
                self.w.push_str("fn ");
                self.w.push_str(name);
                self.w.push_str("(&self");
                for param in &sig.params {
                    self.w.push_str(", ");
                    self.w.push_str(&param.name);
                    self.w.push_str(": ");
                    let psub = substitute_type_ref(&param.ty, &type_subst);
                    self.emit_value_type_as_rust(&psub);
                }
                self.w.push(')');
                match &sig.return_type {
                    ReturnType::Void => {}
                    ReturnType::Type(t) | ReturnType::AsyncType(t) => {
                        self.w.push_str(" -> ");
                        let rsub = substitute_type_ref(t, &type_subst);
                        self.emit_return_type_as_rust(&rsub);
                    }
                }
                self.w.push_str(" { ");
                // Body: `<Self as <FQ-trait-path><args>>::<name>(self, …)`.
                // `emit_type_as_rust(interface_ty)` spells the trait exactly
                // as the `impl Trait for Class` header does — crate-qualified
                // path + the `implements`-clause generic args
                // (`crate::xss::it::some::Holder<crate::xss::it::some::Object>`)
                // — so the fully-qualified call resolves with no `use`.
                self.w.push_str("<Self as ");
                self.emit_type_as_rust(interface_ty);
                self.w.push_str(">::");
                self.w.push_str(name);
                self.w.push_str("(self");
                for param in &sig.params {
                    self.w.push_str(", ");
                    self.w.push_str(&param.name);
                }
                self.w.push(')');
                // `async` defaults: the trait method returns a Future, so
                // await it to yield the declared value type (the enclosing
                // forwarder was emitted `async fn`, so `.await` is legal).
                if matches!(sig.return_type, ReturnType::AsyncType(_)) {
                    self.w.push_str(".await");
                }
                self.w.push_str(" }\n");
            }
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
        // exactly like `emit_inherited_wrapper_methods`. Emit the ENTIRE chain
        // of concrete ancestor bodies, one shim per level: the nearest concrete
        // ancestor is `__jux_super_<m>` (level 0), the next is
        // `__jux_super_<m>__1`, and so on. A `super.<m>()` inside a level-`d`
        // shim body climbs to level `d + 1` (see `super_shim_depth`), so a 3+
        // level `super.<m>()` chain resolves grandparent→great-grandparent
        // instead of re-entering the same shim (infinite recursion).
        let mut depth_by_method: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
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
                    // Package-aware parent resolution (see the inherited-method
                    // walk above) — avoids picking a same-named class from
                    // another package on the `extends` chain.
                    self.resolve_bare_class_fqn(bare)
                        .and_then(|fqn| self.class_asts.get(&fqn))
                        .cloned()
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
                if !overridden.contains(&m.name.text) {
                    continue;
                }
                if m.body.is_none()
                    || m.modifiers
                        .iter()
                        .any(|mo| matches!(mo, juxc_ast::FnModifier::Static))
                {
                    // Abstract / static here — not a concrete shim level; keep
                    // walking for a concrete body (the depth slot is unused).
                    continue;
                }
                let depth = *depth_by_method.get(&m.name.text).unwrap_or(&0);
                let mut renamed = if subst.is_empty() {
                    m.clone()
                } else {
                    substitute_fn_signature(m, &subst)
                };
                let shim_name = if depth == 0 {
                    format!("__jux_super_{}", m.name.text)
                } else {
                    format!("__jux_super_{}__{}", m.name.text, depth)
                };
                renamed.name = juxc_ast::Ident { text: shim_name, span: m.name.span };
                let prev_depth = self.super_shim_depth;
                self.super_shim_depth = Some(depth);
                self.emit_method(&renamed);
                self.super_shim_depth = prev_depth;
                depth_by_method.insert(m.name.text.clone(), depth + 1);
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
                if matches!(
                    ty.array_shape.as_ref().map(|s| s.outer()),
                    Some(juxc_ast::ArrayDim::Fixed(_)),
                )
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
                Stmt::Return(Some(e), _) => scan_expr(e, generic_fields, out),
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

    /// The **introduced virtual methods** of `class_bare` — every non-static
    /// instance method it declares that no ancestor declares. Java dispatches
    /// public, protected, internal AND package-private instance methods
    /// virtually, so all of them go on the `<Name>Kind` trait (`final` included
    /// — still callable through a base ref); only `static` and `private` are
    /// excluded (never virtual). Sorted by name for deterministic output.
    fn class_introduced_virtual_methods(&self, class_bare: &str) -> Vec<(String, MethodSig)> {
        let Some(sig) = self.lookup_class_by_bare_or_fqn(class_bare) else {
            return Vec::new();
        };
        let mut out: Vec<(String, MethodSig)> = sig
            .methods
            .iter()
            .filter(|(_, m)| {
                !m.is_static && !matches!(m.visibility, juxc_ast::Visibility::Private)
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
                // A non-private instance field is reachable through a base ref
                // (Java semantics); only `private`/`static` aren't. Must match the
                // `__get_`/`__set_` accessor gate in `emit_field` (field.rs).
                !f.is_static
                    && !matches!(f.visibility, juxc_ast::Visibility::Private)
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
        // **Method-carrying generic marker** (generics Step 7 / gap 1). A
        // generic class used in BOUND position (`V extends Container<? extends
        // K>`) needs its marker trait to actually expose its public instance
        // methods, so a bounded param `V: ContainerKind<K>` can call them
        // (`this.backing.peek()`). The empty-marker path below can't express
        // that, so we route qualifying classes to a dedicated synthesizer.
        // Gated to NON-dispatch-relevant generic classes — a polymorphic base
        // already owns a populated (object-`dyn`) Kind trait on a different
        // shape, and mixing the two would clash.
        if !class_decl.generic_params.is_empty()
            && self.bound_position_classes.contains(&class_decl.name.text)
            && !self.is_dispatch_relevant_class(&class_decl.name.text)
            && !class_decl.is_abstract
        {
            self.emit_generic_bound_marker_trait(class_decl);
            return;
        }
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
        // §P + inheritance: a polymorphic base's observable props get
        // observer-helper signatures on the trait, so `.observers`
        // operations dispatch through a base-typed reference too.
        let has_observer_sigs = c_is_poly && self.class_has_kind_observer_props(&class_bare);
        if own_methods.is_empty()
            && hook_targets.is_empty()
            && accessor_fields.is_empty()
            && !has_observer_sigs
        {
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
            if has_observer_sigs {
                self.emit_observer_trait_sigs(&class_bare);
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
            if own_methods.is_empty()
                && self_hooks.is_empty()
                && accessor_fields.is_empty()
                && !has_observer_sigs
            {
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
                if has_observer_sigs {
                    self.emit_observer_impl_methods(&class_bare, &class_bare);
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
                let anc_observer_sigs = self.poly_base_classes.contains(&ancestor_bare)
                    && self.class_has_kind_observer_props(&ancestor_bare);
                if anc_methods.is_empty()
                    && anc_hooks.is_empty()
                    && anc_accessor_fields.is_empty()
                    && !anc_observer_sigs
                {
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
                    if anc_observer_sigs {
                        self.emit_observer_impl_methods(&ancestor_bare, &class_bare);
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

    /// Emit a **method-carrying generic marker trait** for a generic class
    /// that appears in bound position (generics Step 7 / gap 1).
    ///
    /// For `class Container<T> { … T peek() { … } }` used as
    /// `V extends Container<? extends K>` this emits:
    ///
    /// ```text
    /// pub trait ContainerKind<T: Clone + Debug>: Debug { fn peek(&self) -> T; }
    /// impl<T: Clone + Debug> ContainerKind<T> for Container<T> {
    ///     fn peek(&self) -> T { Container::<T>::peek(self) }
    /// }
    /// ```
    ///
    /// so a bounded param `V: ContainerKind<K>` can call `v.peek()` and get a
    /// `K` back. The trait stays **object-safe**: every method takes `&self`
    /// and methods carrying their OWN generic params are skipped (a generic
    /// method would make the trait non-dyn-compatible, and they're rarely the
    /// surface a bound actually needs).
    ///
    /// The trait is parameterized by the class's own (non-const) type params,
    /// reusing their names, so method return/param types referencing `T`
    /// resolve directly. The bound use site (`emit_bound_type`) supplies the
    /// concrete element type for these params.
    fn emit_generic_bound_marker_trait(&mut self, class_decl: &juxc_ast::ClassDecl) {
        let class_bare = &class_decl.name.text;
        // Public, non-static, instance methods with no own generic params —
        // the object-safe surface a bound can call. Property-accessor methods
        // (`is_property`) are included: they're plain `&self` getters.
        let trait_methods: Vec<&juxc_ast::FnDecl> = class_decl
            .methods
            .iter()
            .filter(|m| {
                matches!(m.visibility, juxc_ast::Visibility::Public)
                    && !m.modifiers.contains(&juxc_ast::FnModifier::Static)
                    && m.generic_params.is_empty()
            })
            .collect();

        // --- trait ContainerKind<T…>: Debug { fn peek(&self) -> T; } ---
        self.w.emit_indent();
        self.emit_visibility(class_decl.visibility);
        self.w.push_str("trait ");
        self.w.push_str(class_bare);
        self.w.push_str("Kind");
        // Trait generic params = the class's params (with the Clone/Debug tail
        // so method bodies that `.clone()` a `T` value typecheck).
        self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
        self.w.push_str(": std::fmt::Debug {\n");
        self.w.indent_inc();
        for m in &trait_methods {
            self.w.emit_indent();
            self.w.push_str("fn ");
            self.w.push_str(&m.name.text);
            self.w.push_str("(&self");
            for p in &m.params {
                self.w.push_str(", ");
                self.w.push_str(&p.name.text);
                self.w.push_str(": ");
                self.emit_value_type_as_rust(&p.ty);
            }
            self.w.push(')');
            match &m.return_type {
                juxc_ast::ReturnType::Void => {}
                juxc_ast::ReturnType::Type(t) | juxc_ast::ReturnType::AsyncType(t) => {
                    self.w.push_str(" -> ");
                    self.emit_return_type_as_rust(t);
                }
            }
            self.w.push_str(";\n");
        }
        self.w.indent_dec();
        self.w.emit_indent();
        self.w.push_str("}\n");

        // --- impl<T…> ContainerKind<T…> for Container<T…> { … } ---
        self.w.emit_indent();
        self.w.push_str("impl");
        self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
        self.w.push(' ');
        self.w.push_str(class_bare);
        self.w.push_str("Kind");
        self.emit_generic_params_as_args(&class_decl.generic_params);
        self.w.push_str(" for ");
        self.w.push_str(class_bare);
        self.emit_generic_params_as_args(&class_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        for m in &trait_methods {
            self.w.emit_indent();
            self.w.push_str("fn ");
            self.w.push_str(&m.name.text);
            self.w.push_str("(&self");
            for p in &m.params {
                self.w.push_str(", ");
                self.w.push_str(&p.name.text);
                self.w.push_str(": ");
                self.emit_value_type_as_rust(&p.ty);
            }
            self.w.push(')');
            match &m.return_type {
                juxc_ast::ReturnType::Void => {}
                juxc_ast::ReturnType::Type(t) | juxc_ast::ReturnType::AsyncType(t) => {
                    self.w.push_str(" -> ");
                    self.emit_return_type_as_rust(t);
                }
            }
            // Delegating body — call the class's inherent method via the
            // fully-qualified `Container::<T…>::peek(self, args)` path so it
            // resolves to the inherent impl (not this trait method, which
            // would recurse). The turbofish keeps the generic args explicit.
            self.w.push_str(" { ");
            self.w.push_str(class_bare);
            if !class_decl.generic_params.is_empty() {
                self.w.push_str("::");
            }
            self.emit_generic_params_as_args(&class_decl.generic_params);
            self.w.push_str("::");
            self.w.push_str(&m.name.text);
            self.w.push_str("(self");
            for p in &m.params {
                self.w.push_str(", ");
                self.w.push_str(&p.name.text);
            }
            self.w.push_str(") }\n");
        }
        self.w.indent_dec();
        self.w.emit_indent();
        self.w.push_str("}\n");
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
        // **Transitive interface impls (Java "an Entity IS-A Id").** When a
        // class `implements Entity<User>` and `interface Entity<E> extends
        // Id, Named, Comparable<E>`, rustc needs `impl Id for User`,
        // `impl Named for User`, `impl Comparable<User> for User` too —
        // a `User: Id` bound (e.g. through `K extends Id`) won't resolve
        // otherwise. We expand each directly-implemented interface's
        // `extends` chain, substituting the interface's type params with the
        // concrete args the class supplied (`E ↦ User`), and add each
        // ancestor interface to the impl list. The delegating-impl loop
        // below then emits a body for each (the class already defines the
        // shared inherent methods `id()` / `name()` / `compareTo()`).
        {
            let mut seen: std::collections::HashSet<String> = implements
                .iter()
                .filter_map(|t| t.name.segments.first().map(|s| s.text.clone()))
                .collect();
            let direct: Vec<juxc_ast::TypeRef> = implements.clone();
            for iface_ty in &direct {
                let ancestors = self.transitive_interface_supers(iface_ty);
                for anc in ancestors {
                    let Some(seg) = anc.name.segments.first() else { continue };
                    if seen.insert(seg.text.clone()) {
                        implements.push(anc);
                    }
                }
            }
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
                // **Marker interface** (no methods — e.g. `interface
                // Entity<E> extends Id, Named, Comparable<E> {}`). Still
                // emit an empty `impl Iface<Args> for Class {}` so a bound
                // like `E extends Entity<E>` resolves (`User: Entity<User>`).
                // Skipping it entirely (the old behavior) left the marker
                // un-implemented and broke F-bounded generic call sites.
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
                        // Turbofish in **call** position — `Box::<T>::get(self)`.
                        // The leading `::` is required: `Box<T>::get` parses as
                        // chained comparison operators ("comparison operators
                        // cannot be chained"). `emit_generic_params_as_args` is
                        // correct only in type position, so add the `::` here.
                        if !class_decl.generic_params.is_empty() {
                            self.w.push_str("::");
                        }
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

    /// Expand an interface `TypeRef` (with concrete args, e.g.
    /// `Entity<User>`) into the list of **transitive parent interfaces** it
    /// pulls in, each carrying the args substituted down the chain.
    ///
    /// `interface Entity<E> extends Id, Named, Comparable<E>` invoked with
    /// `Entity<User>` yields `[Id, Named, Comparable<User>]` — the chain is
    /// walked breadth-first, substituting each level's type params (here
    /// `E ↦ User`) into its own `extends` clause. Cycles and re-visits are
    /// guarded by a `seen` set keyed on bare interface name. The returned
    /// list does NOT include the input interface itself.
    ///
    /// Used by [`Self::emit_class_trait_impls`] to satisfy Java's "an Entity
    /// IS-A Id" rule: a class implementing `Entity<User>` must also produce
    /// `impl Id for User`, etc., so a `User: Id` bound resolves.
    fn transitive_interface_supers(
        &self,
        iface_ty: &juxc_ast::TypeRef,
    ) -> Vec<juxc_ast::TypeRef> {
        let mut out: Vec<juxc_ast::TypeRef> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        // Work queue of (interface TypeRef with concrete args) to expand.
        let mut queue: std::collections::VecDeque<juxc_ast::TypeRef> =
            std::collections::VecDeque::new();
        queue.push_back(iface_ty.clone());
        while let Some(cur) = queue.pop_front() {
            let Some(name_seg) = cur.name.segments.first() else { continue };
            let Some(iface) = self
                .lookup_interface_by_bare_or_fqn(name_seg.text.as_str())
                .map(|(_, i)| i.clone())
            else {
                continue;
            };
            // Build this level's param → arg substitution from the
            // interface's declared params zipped with the supplied args.
            let mut subst: std::collections::HashMap<String, juxc_ast::TypeRef> =
                std::collections::HashMap::new();
            for (param, arg) in iface.generic_params.iter().zip(cur.generic_args.iter()) {
                if let Some(arg_ty) = arg.as_type() {
                    subst.insert(param.name.text.clone(), arg_ty.clone());
                }
            }
            for parent in &iface.extends {
                // Substitute this level's params into the parent ref so a
                // generic parent (`Comparable<E>`) lands as `Comparable<User>`.
                let parent_subst = substitute_type_ref(parent, &subst);
                let Some(parent_seg) = parent_subst.name.segments.first() else { continue };
                if seen.insert(parent_seg.text.clone()) {
                    out.push(parent_subst.clone());
                    // Recurse into the parent's own `extends` chain.
                    queue.push_back(parent_subst);
                }
            }
        }
        out
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

    /// Emit `impl Drop for <target>` from the class's `drop { }`
    /// block (§6.6 / §S.5). `target` is the struct that owns the
    /// fields — the class itself for inline classes, `<C>_Inner` for
    /// wrapper classes (so the body runs once, on last-handle
    /// release). The body emits in INLINE style — `this.f` →
    /// `self.f`, no `.0.borrow()` — which is achieved for wrapper
    /// classes by lifting the class out of `wrapper_classes` for the
    /// duration. Phase-1 limitation: instance METHOD calls inside a
    /// wrapper class's `drop` don't resolve (methods live on the
    /// wrapper handle, which no longer exists) — keep destructor
    /// bodies to field access plus free/static calls.
    fn emit_drop_impl(&mut self, class_decl: &juxc_ast::ClassDecl, target: &str) {
        use crate::stmts::stmt_span;
        if class_decl.drop_blocks.is_empty() {
            return;
        }
        self.w.emit_indent();
        self.w.push_str("impl");
        self.emit_generic_params_with_clone_bound(&class_decl.generic_params);
        self.w.push_str(" Drop for ");
        self.w.push_str(target);
        self.emit_generic_params_as_args(&class_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.line("fn drop(&mut self) {");
        self.w.indent_inc();
        let removed = self.wrapper_classes.remove(&class_decl.name.text);
        let prev_this = self.this_alias.replace("self".to_string());
        let mut muts = std::collections::HashSet::new();
        for block in &class_decl.drop_blocks {
            collect_mutated_names(block, &mut muts, &self.user_mut_methods);
        }
        let prev_muts = std::mem::replace(&mut self.mutated_in_fn, muts);
        for block in &class_decl.drop_blocks {
            for stmt in &block.statements {
                self.emit_source_marker(stmt_span(stmt));
                self.w.emit_indent();
                self.emit_stmt(stmt);
            }
        }
        self.mutated_in_fn = prev_muts;
        self.this_alias = prev_this;
        if removed {
            self.wrapper_classes.insert(class_decl.name.text.clone());
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }

    /// True for a method declared `static`.
    fn fn_is_static(method: &FnDecl) -> bool {
        method
            .modifiers
            .iter()
            .any(|m| matches!(m, juxc_ast::FnModifier::Static))
    }

    /// True when `method` should be lifted out of `class_decl`'s impl into a
    /// module-scope free function: a **generic class's plain static method**.
    /// Property accessors / operator / observer helpers (synthesized names
    /// beginning with `__`) are excluded — they're invoked via `Self::…` from
    /// the observer machinery and must stay associated. The single shared
    /// predicate keeps the skip (in the method loop) and the emit (in
    /// `emit_generic_class_static_fns`) in lock-step.
    fn generic_class_lifts_static(class_decl: &juxc_ast::ClassDecl, method: &FnDecl) -> bool {
        !class_decl.generic_params.is_empty()
            && Self::fn_is_static(method)
            && !method.name.text.starts_with("__")
    }

    /// Emit a **generic class's static methods as free functions** named
    /// `<Class>_<method>` at module scope (generics: static-on-generic-class).
    ///
    /// A static method of `class Registry<K, V, int N>` lives, by default,
    /// inside `impl<K, V, const N: usize> Registry<K, V, N>`, so calling
    /// `Registry.maxById(xs)` forces rustc to infer K/V/N — impossible for a
    /// static that never names them, and outright unsupported for the const
    /// `N` (E0284). Lifting them to free functions drops that dependency: the
    /// function carries ONLY its own generics (`fn Registry_maxById<E>(…)`).
    /// The call-site rewrite in `emit_call` maps `Registry.maxById(args)` to
    /// `Registry_maxById(args)` for generic classes.
    ///
    /// Non-generic classes keep their associated-function form (no inference
    /// problem), so this runs only when `generic_params` is non-empty.
    pub(crate) fn emit_generic_class_static_fns(&mut self, class_decl: &juxc_ast::ClassDecl) {
        if class_decl.generic_params.is_empty() {
            return;
        }
        for method in &class_decl.methods {
            if !Self::generic_class_lifts_static(class_decl, method) {
                continue;
            }
            self.emit_static_free_fn(class_decl, method);
        }
    }

    /// Emit one static class method as a module-scope free function
    /// `<Class>_<method>`. Shares the body pipeline (`emit_fn_body_at`) with
    /// the associated-function path; only the header differs (free `fn`, no
    /// `&self`, no enclosing impl). See [`Self::emit_generic_class_static_fns`].
    fn emit_static_free_fn(&mut self, class_decl: &juxc_ast::ClassDecl, method: &FnDecl) {
        let prev_enclosing = self.enclosing_class.clone();
        self.enclosing_class = Some(class_decl.name.text.clone());
        self.w.emit_indent();
        self.emit_visibility(method.visibility);
        if matches!(method.return_type, ReturnType::AsyncType(_)) {
            self.w.push_str("async ");
        }
        if method.modifiers.contains(&juxc_ast::FnModifier::Unsafe) {
            self.w.push_str("unsafe ");
        }
        self.w.push_str("fn ");
        self.w.push_str(&class_decl.name.text);
        self.w.push('_');
        self.w.push_str(&method.name.text);
        // The free function carries ONLY the method's own generics (plus any
        // wildcard lifts) — never the class's params, which is the whole point.
        let mut in_scope = crate::collect_type_param_names(&method.generic_params);
        let mut lifter = crate::analysis::WildcardLifter::new(in_scope.clone());
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
        let mut combined = method.generic_params.clone();
        combined.extend(lifter.new_params.iter().cloned());
        in_scope.extend(crate::collect_type_param_names(&lifter.new_params));
        if combined.is_empty() {
            self.emit_generic_params(&method.generic_params);
        } else {
            self.emit_generic_params_with_clone_bound(&combined);
        }
        self.w.push('(');
        for (i, param) in method.params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_value_type_as_rust(&lifted_param_tys[i]);
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
        if let Some(body) = &method.body {
            let mut muts = HashSet::new();
            collect_mutated_names(body, &mut muts, &self.user_mut_methods);
            self.mutated_in_fn = muts;
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
            let prev_const_ints = self.const_int_params.clone();
            self.const_int_params
                .extend(crate::collect_const_int_params(&method.generic_params));
            let prev_type_params = self.current_type_params.clone();
            self.current_type_params.extend(in_scope);
            // A static use triggers the class's `static { }` init (§S.4.1).
            self.emit_static_init_trigger();
            self.emit_fn_body_at(body, &method.return_type);
            self.const_int_params = prev_const_ints;
            self.current_type_params = prev_type_params;
            self.current_return_type = saved;
            self.current_fn_params.clear();
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.enclosing_class = prev_enclosing;
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
        // C6: a body with a self-aliasing by-`&mut` foreign-collection
        // call (`this.m(this.field)`) lowers to a `std::mem::take`
        // write-back that ASSIGNS `self.field`, so the method genuinely
        // needs `&mut self` — fold that into the receiver decision.
        let has_self_aliasing_byref = body
            .map(|b| {
                let mut found = false;
                self.scan_block_for_self_aliasing_byref(b, &mut found);
                found
            })
            .unwrap_or(false);
        let needs_mut_self = !self.emitting_wrapper_class
            && (has_self_aliasing_byref
                || body
                    .map(|b| {
                        body_writes_to_this(b)
                            || crate::analysis::body_calls_mut_method_on_this(
                                b,
                                &self.user_mut_methods,
                            )
                    })
                    .unwrap_or(false));

        // Wildcard-lift pre-pass (same rule as `emit_fn_decl`):
        // promote each `? extends T` / `? super T` / `?` in a param
        // type to a synthetic `__Wn` generic on this method with the
        // matching bound. In-scope params = the enclosing class's
        // (`current_type_params`) plus this method's own — so a wildcard
        // bounded by one (`MyList<? extends E>`) substitutes it directly.
        let mut in_scope = self.current_type_params.clone();
        in_scope.extend(crate::collect_type_param_names(&method.generic_params));
        let mut lifter = crate::analysis::WildcardLifter::new(in_scope);
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
        // P2 (§P.4.2): an OBSERVABLE property's setter emits its real
        // body under `__set_X_raw` (always `pub` — binding closures in
        // other modules call it; Jux-level access stays enforced by
        // tycheck E0972), and a thin public `__set_X` gate wrapper is
        // appended after the method (E0973 bound-assignment guard).
        let p2_setter = matches!(&self.pending_setter_observer, Some((_, _, _, false, _)))
            && method.name.text.starts_with("__set_");
        if p2_setter {
            self.w.push_str("pub ");
        } else {
            self.emit_visibility(method.visibility);
        }
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
        if p2_setter {
            self.w.push_str("_raw");
        }
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
        // Params the body mutates in place (`xs.push(…)` on a by-value
        // collection param, reassignment) need Rust's `mut` binding —
        // same inference the `let mut` choice uses for locals.
        let mut param_muts = HashSet::new();
        if let Some(b) = &method.body {
            collect_mutated_names(b, &mut param_muts, &self.user_mut_methods);
        }
        // C6: foreign-collection params the body mutates lower to `&mut T`.
        // Keyed by the enclosing (bare) class — the SAME key every call
        // site uses (`m::Class::method`), so decl and call never diverge.
        let byref_idxs = self
            .enclosing_class
            .as_ref()
            .map(|cls| format!("m::{cls}::{}", method.name.text))
            .and_then(|k| self.byref_params.get(&k).cloned())
            .unwrap_or_default();
        for (i, param) in method.params.iter().enumerate() {
            if !first_param {
                self.w.push_str(", ");
            }
            first_param = false;
            let is_byref = byref_idxs.contains(&i);
            if !is_byref
                && !param.is_out
                && !param.is_shared_ref
                && param_muts.contains(&param.name.text)
            {
                self.w.push_str("mut ");
            }
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            if param.is_out {
                self.w.push_str("&mut "); // `out T` (§M.4) lowers to `&mut T`
            }
            if is_byref {
                self.w.push_str("&mut "); // C6: foreign collection by exclusive ref
            }
            if param.is_shared_ref {
                // `ref T` (§M.13) — shared reference to a value object.
                self.w.push_str("std::rc::Rc<std::cell::RefCell<");
                self.emit_value_type_as_rust(&lifted_param_tys[i]);
                self.w.push_str(">>");
            } else {
                self.emit_value_type_as_rust(&lifted_param_tys[i]);
            }
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
        // §P setter observer bracket: capture the property's value
        // BEFORE the setter body runs. The matching post-body fire is
        // emitted after the body below. An early `return` in a custom
        // setter body skips the fire (W0973 semantics).
        let setter_observer = self.pending_setter_observer.take();
        if let Some((prop, _, dependents, observer_static, _)) = &setter_observer {
            // Static setters read through the associated getter; the
            // class-scoped storage has no `self`.
            let recv = if *observer_static { "Self::" } else { "self." };
            self.w.line(&format!("let __jux_old = {recv}{prop}();"));
            // §P.1.5: pre-capture every dependent COMPUTED property's
            // value so the post-body bracket can fire on change.
            for (c, _) in dependents {
                self.w
                    .line(&format!("let __jux_cold_{c} = {recv}{c}();"));
            }
        }
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
            // `ref` bindings (§M.13): reset per method, seeded from
            // `ref` params.
            self.ref_locals.clear();
            // `weak` params (§M.14.3): reset per method, mapped to target class.
            self.weak_params.clear();
            for p in &method.params {
                if p.is_shared_ref {
                    self.ref_locals.insert(p.name.text.clone());
                }
                if p.is_weak {
                    let cls = p.ty.name.segments.last().map_or("", |s| s.text.as_str());
                    self.weak_params.insert(p.name.text.clone(), cls.to_string());
                }
            }
            // Raw-pointer params (§L.6): reset + seed for the `p == null` peephole.
            self.seed_pointer_params(&method.params);
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
            // `out` params (§M.4): in scope for the body so reads/writes deref.
            let prev_out = std::mem::replace(
                &mut self.out_params,
                method
                    .params
                    .iter()
                    .filter(|p| p.is_out)
                    .map(|p| p.name.text.clone())
                    .collect(),
            );
            // C6: register `&mut T` foreign-collection params for the body.
            let prev_byref = std::mem::replace(
                &mut self.byref_param_names,
                method
                    .params
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| byref_idxs.contains(i))
                    .map(|(_, p)| p.name.text.clone())
                    .collect(),
            );
            // First-use trigger for `static { }` blocks (§S.4.1): a static
            // method call is an observable use. (Instance methods aren't —
            // constructing the receiver already triggered init.)
            if is_static {
                self.emit_static_init_trigger();
            }
            self.emit_fn_body_at(body, &method.return_type);
            self.byref_param_names = prev_byref;
            self.out_params = prev_out;
            self.const_int_params = prev_const_ints;
            self.current_type_params = prev_type_params;
            self.current_return_type = saved;
            self.current_fn_params.clear();
            self.this_alias = None;
            // §P setter observer fire — after the body, before the
            // close. Comparable property types fire only on an actual
            // value change; user-class types (no `PartialEq` on the
            // wrapper) fire on every completed set.
            if let Some((prop, comparable, dependents, observer_static, _)) = &setter_observer {
                let recv = if *observer_static { "Self::" } else { "self." };
                self.w.line(&format!("let __jux_now = {recv}{prop}();"));
                if *comparable {
                    // §P.3.6 re-entrant sets: an observer may set this
                    // same property — the nested set COMMITS its value
                    // but its firing pass is a no-op (the observer
                    // list is detached during the pass). Detect the
                    // change after each pass and fire the next
                    // transition, looping until quiescent. Every
                    // distinct transition fires exactly once, in
                    // order (JavaFX-equivalent).
                    self.w.line(&format!(
                        "if __jux_old != __jux_now {{ {recv}__obs_{prop}_fire(&__jux_old, &__jux_now); }}"
                    ));
                    self.w.line("let mut __jux_prev = __jux_now;");
                    self.w.line("loop {");
                    self.w.indent_inc();
                    self.w.line(&format!("let __jux_cur = {recv}{prop}();"));
                    self.w.line("if __jux_cur == __jux_prev { break; }");
                    self.w.line(&format!(
                        "{recv}__obs_{prop}_fire(&__jux_prev, &__jux_cur);"
                    ));
                    self.w.line("__jux_prev = __jux_cur;");
                    self.w.indent_dec();
                    self.w.line("}");
                } else {
                    // Non-comparable property types can't detect the
                    // nested change, so they keep the single-pass fire.
                    self.w
                        .line(&format!("{recv}__obs_{prop}_fire(&__jux_old, &__jux_now);"));
                }
                // §P.1.5: recompute each dependent COMPUTED property
                // (after the quiescence loop, so the final value is
                // read) and fire its observers. Comparable computed
                // types fire only on a real change; non-comparable
                // ones fire whenever the driving setter completed.
                for (c, c_comparable) in dependents {
                    self.w.line(&format!("let __jux_cnow_{c} = {recv}{c}();"));
                    if *c_comparable {
                        self.w.line(&format!(
                            "if __jux_cold_{c} != __jux_cnow_{c} {{ {recv}__obs_{c}_fire(&__jux_cold_{c}, &__jux_cnow_{c}); }}"
                        ));
                    } else {
                        self.w.line(&format!(
                            "{recv}__obs_{c}_fire(&__jux_cold_{c}, &__jux_cnow_{c});"
                        ));
                    }
                }
            }
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
        // P2 gate wrapper (§P.4.2): every existing call site reaches
        // the property through `__set_X`, which now refuses a direct
        // assignment while a binding drives the property — an
        // `IllegalStateException` in debug builds (E0973). The binding
        // machinery writes through `__set_X_raw` above.
        if let Some((prop, _, _, false, depth)) = &setter_observer {
            if let Some(p0) = method.params.first() {
                let mark = self.w.len();
                self.emit_value_type_as_rust(&p0.ty);
                let vt = self.w.split_off_from(mark);
                // The bind slot lives on the DECLARING class's slice —
                // `depth` `__parent` hops up for an inherited setter copy.
                let h = "__parent.".repeat(*depth);
                self.w
                    .line(&format!("pub fn __set_{prop}(&self, value: {vt}) {{"));
                self.w.indent_inc();
                // Only ONE-WAY bindings refuse direct sets — both
                // sides of a bidirectional binding stay settable
                // (JavaFX semantics; the third slot flag records it).
                self.w.line(&format!(
                    "if cfg!(debug_assertions) && matches!(&self.0.borrow().{h}__bind_{prop}, Some((_, _, false))) {{"
                ));
                self.w.indent_inc();
                self.w.line(&format!(
                    "std::panic::panic_any(crate::jux::std::exceptions::IllegalStateException::new(\"E0973: property `{prop}` is bound - direct assignment is not allowed while a binding drives it; unbind() first\".to_string()));"
                ));
                self.w.indent_dec();
                self.w.line("}");
                self.w.line(&format!("self.__set_{prop}_raw(value);"));
                self.w.indent_dec();
                self.w.line("}");
                self.w.newline();
            }
        }
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
            is_out: p.is_out,
            is_shared_ref: p.is_shared_ref,
            is_weak: p.is_weak,
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
        is_c_variadic: false,
        span: m.span,
    }
}

/// Substitute generic type parameters inside a [`TypeRef`] using `subst`
/// (param name -> replacement type), recursing through generic args, array,
/// and nullable shapes. A bare single-segment name present in the table is
/// replaced wholesale; everything else is rebuilt with its sub-terms
/// substituted. Used to specialize a generic method/interface signature with a
/// concrete or inferred instantiation (e.g. `Container<T>::peek -> T` becomes
/// `peek -> K` when emitting the `ContainerKind<K>` marker-trait impl).
pub(crate) fn substitute_type_ref(
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
            // A wildcard's bound can name a substitutable param too — e.g.
            // `Sink<? super K>` with `K ↦ User` becomes `Sink<? super User>`.
            // Without this, the wildcard kept its original `K` and a call-site
            // coercion lowered it to a dangling `dyn K` (gap 5).
            juxc_ast::GenericArg::Wildcard(w) => {
                let bound = w.bound.as_ref().map(|b| match b {
                    juxc_ast::WildcardBound::Extends(t) => {
                        juxc_ast::WildcardBound::Extends(substitute_type_ref(t, subst))
                    }
                    juxc_ast::WildcardBound::Super(t) => {
                        juxc_ast::WildcardBound::Super(substitute_type_ref(t, subst))
                    }
                });
                juxc_ast::GenericArg::Wildcard(juxc_ast::WildcardArg {
                    bound,
                    span: w.span,
                })
            }
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
