//! Jux interface declarations → Rust `trait`.
//!
//! Per `JUX-LANG-V1.md` §7.6: interfaces are **public** (no
//! visibility modifier required; the parser enforces) and
//! implicitly **final** (interfaces themselves can't be extended;
//! only implemented). They carry method signatures plus optional
//! default-method bodies. Both shapes lower to Rust trait
//! methods — abstract signatures become required `fn name(&self);`
//! lines and default-bodied methods become `fn name(&self) { … }`
//! with the body inline.
//!
//! **Receiver kind.** Trait methods always use `&self`. Default
//! methods that try to mutate `self` would need `&mut self` (the
//! receiver-kind cross-class analysis isn't in yet); for Phase 1,
//! default methods that need mutation should call back through
//! abstract accessor methods on `&self` instead.

use juxc_ast::ReturnType;

use crate::analysis::collect_mutated_names;
use crate::RustEmitter;
use juxc_lex::to_rust_ident;

/// Whether a default body reads `this` as a VALUE: passes it on, stores it,
/// returns it. `this.m()` and `this.f` only use it as a receiver.
pub(crate) fn default_body_uses_this_as_value(body: &juxc_ast::Block) -> bool {
    let mut total = 0usize;
    let mut as_receiver = 0usize;
    juxc_ast::visit::for_each_expr(body, &mut |e| match e {
        juxc_ast::Expr::This(_) => total += 1,
        juxc_ast::Expr::Field(f) if matches!(f.object.as_ref(), juxc_ast::Expr::This(_)) => {
            as_receiver += 1
        }
        _ => {}
    });
    total > as_receiver
}

/// Whether a default interface method has to be `where Self: Sized + Clone +
/// 'static` (Core lib K.5): one with type parameters of its own, which would
/// otherwise make the trait unusable as the `Rc<dyn Iface>` every
/// interface-typed value is, or one that hands `this` on as a value, which
/// needs an owned handle to it. The trait stays dyn-compatible; such a method
/// is reached through the `Rc` handle, which is `Sized` and `Clone`.
pub(crate) fn default_method_needs_sized_self(method: &juxc_ast::FnDecl) -> bool {
    match &method.body {
        Some(body) => !method.generic_params.is_empty() || default_body_uses_this_as_value(body),
        None => false,
    }
}

impl RustEmitter {
    /// Emit `impl<T: ?Sized + Iface> Iface for Rc<T>` — see the call site.
    ///
    /// Every member forwards through the deref. Default bodies forward too
    /// rather than being inherited: the handle must dispatch to the value
    /// inside it, not run the interface's own default.
    fn emit_interface_rc_forwarding_impl(&mut self, interface: &juxc_ast::InterfaceDecl) {
        let iface_bare = interface.name.text.clone();
        let methods: Vec<juxc_ast::FnDecl> = interface
            .methods
            .iter()
            .filter(|m| !m.modifiers.iter().any(|x| matches!(x, juxc_ast::FnModifier::Static)))
            // A `Self: Sized` default runs on the handle itself: the handle is
            // `Sized`, the value behind it may not be, and the default body
            // reaches the value through the forwarded abstract methods.
            .filter(|m| !default_method_needs_sized_self(m))
            .cloned()
            .collect();
        let hooks = self.interface_hook_targets(&iface_bare);
        self.w.emit_indent();
        // `::core`, not `core`: the emitted crate has a module per Jux
        // package, so a program with a `demo.core` package shadows Rust's
        // own `core` for every path inside `mod demo`. The absolute form
        // cannot be shadowed by anything the user names.
        self.w.push_str("impl<__JuxH: ?::core::marker::Sized + ");
        self.w.push_str(&to_rust_ident(&iface_bare));
        // A GENERIC interface forwards too: it is the supertrait a generic
        // class's `Kind` trait lists, so the handle has to satisfy it.
        let params = interface.generic_params.clone();
        self.emit_generic_params_as_args(&params);
        if !params.is_empty() {
            self.w.push_str(", ");
            // The trait's own `Display` bounds come along: the impl has to
            // satisfy `Store<T>`, and `Store` declares `T: Display` when a
            // default body formats a `T`.
            let displayed = self.interface_displayed_generic_params(interface);
            let empty = std::collections::HashSet::new();
            self.emit_generic_params_bounds_body(&params, &displayed, &empty);
        }
        self.w.push_str("> ");
        self.w.push_str(&to_rust_ident(&iface_bare));
        self.emit_generic_params_as_args(&params);
        self.w.push_str(" for std::rc::Rc<__JuxH> {\n");
        self.w.indent_inc();
        for m in &methods {
            self.w.emit_indent();
            self.w.push_str("fn ");
            self.w.push_str(&to_rust_ident(&m.name.text));
            self.w.push_str("(&self");
            for p in &m.params {
                self.w.push_str(", ");
                self.w.push_str(&to_rust_ident(&p.name.text));
                self.w.push_str(": ");
                let ty = p.ty.clone();
                self.emit_value_type_as_rust(&ty);
            }
            self.w.push(')');
            match &m.return_type {
                ReturnType::Void => {}
                ReturnType::Type(t) => {
                    let t = t.clone();
                    self.w.push_str(" -> ");
                    self.emit_return_type_as_rust(&t);
                }
                ReturnType::AsyncType(t) => {
                    let t = t.clone();
                    self.w
                        .push_str(" -> std::pin::Pin<::std::boxed::Box<dyn std::future::Future<Output = ");
                    self.emit_return_type_as_rust(&t);
                    self.w.push_str("> + '_>>");
                }
            }
            let bounds = crate::decls::functions::where_bounds(&m.wheres);
            if !bounds.is_empty() {
                self.w.push_str(" where ");
                self.w.push_str(&bounds.join(", "));
            }
            self.w.push_str(" { (**self).");
            self.w.push_str(&to_rust_ident(&m.name.text));
            self.w.push('(');
            for (i, p) in m.params.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.w.push_str(&to_rust_ident(&p.name.text));
            }
            self.w.push_str(") }\n");
        }
        for t in &hooks {
            self.w.emit_indent();
            self.w.push_str("fn __jux_as_");
            self.w.push_str(t);
            self.w.push_str("(&self) -> Option<");
            self.emit_hook_target_type(t, &iface_bare);
            self.w.push_str("> { (**self).__jux_as_");
            self.w.push_str(t);
            self.w.push_str("() }\n");
        }
        self.w.indent_dec();
        self.w.emit_indent();
        self.w.push_str("}\n");
        self.w.newline();
    }

    /// The type parameters of `interface` whose values reach a **format
    /// position** inside one of its `default` method bodies.
    ///
    /// `default String describe() { return "store of " + this.load(); }` on
    /// `interface Store<T>` formats a `T`, so the emitted `format!` needs
    /// `T: Display` — and the body is emitted on the TRAIT, so the bound has to
    /// be on the trait. This is the interface counterpart of
    /// [`Self::class_displayed_generic_params`] and reuses its body scan.
    ///
    /// Interfaces this one `extends` are included: `interface Loud<T> extends
    /// Store<T>` inherits the default, so it inherits the bound. The walk is
    /// depth-limited and cycle-guarded, and maps each hop's arguments back to
    /// this interface's own parameter names, so `Loud<V> extends Store<V>`
    /// marks `V`.
    pub(crate) fn interface_displayed_generic_params(
        &self,
        interface: &juxc_ast::InterfaceDecl,
    ) -> std::collections::HashSet<String> {
        let mut out: std::collections::HashSet<String> = std::collections::HashSet::new();
        if interface.generic_params.is_empty() {
            return out;
        }
        let own: std::collections::HashSet<String> = interface
            .generic_params
            .iter()
            .map(|p| p.name.text.clone())
            .collect();
        // (interface, param-name → this interface's param name) pairs to scan.
        let identity: std::collections::HashMap<String, String> =
            own.iter().map(|p| (p.clone(), p.clone())).collect();
        let mut queue: Vec<(juxc_ast::InterfaceDecl, std::collections::HashMap<String, String>)> =
            vec![(interface.clone(), identity)];
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut depth = 0usize;
        while let Some((decl, subst)) = queue.pop() {
            if depth > 64 {
                break;
            }
            depth += 1;
            if !seen.insert(decl.name.text.clone()) {
                continue;
            }
            // A method returning a bare type param makes `this.m()` a read of
            // that param's value — the same rule the class scan uses for a
            // generic field or a param-returning method.
            let mut generic_members: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for m in &decl.methods {
                let juxc_ast::ReturnType::Type(t) = &m.return_type else { continue };
                if !t.generic_args.is_empty()
                    || t.array_shape.is_some()
                    || t.fn_shape.is_some()
                    || t.name.segments.len() != 1
                {
                    continue;
                }
                if let Some(mapped) = subst.get(t.name.segments[0].text.as_str()) {
                    generic_members.insert(m.name.text.clone(), mapped.clone());
                }
            }
            if !generic_members.is_empty() {
                for m in &decl.methods {
                    if let Some(body) = &m.body {
                        Self::scan_block_for_displayed_fields(body, &generic_members, &mut out);
                    }
                }
            }
            // Compose this hop's arguments into the substitution and continue
            // up the `extends` chain.
            for parent_ref in &decl.extends {
                let Some(seg) = parent_ref.name.segments.last() else { continue };
                let Some(parent) = self.interface_ast_by_bare(&seg.text) else { continue };
                let mut next: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for (param, arg) in parent.generic_params.iter().zip(parent_ref.generic_args.iter())
                {
                    let Some(arg_ty) = arg.as_type() else { continue };
                    if arg_ty.generic_args.is_empty() && arg_ty.name.segments.len() == 1 {
                        if let Some(mapped) = subst.get(arg_ty.name.segments[0].text.as_str()) {
                            next.insert(param.name.text.clone(), mapped.clone());
                        }
                    }
                }
                if !next.is_empty() {
                    queue.push((parent.clone(), next));
                }
            }
        }
        out.retain(|p| own.contains(p));
        out
    }

    /// The interface AST for a bare name, matched exactly or by FQN suffix —
    /// the [`Self::interface_asts`] counterpart of `class_ast_by_bare`.
    /// The declared type of a constant named `bare` in the interface being
    /// emitted, or `None` when there is no such constant or a parameter or
    /// local shadows the name.
    ///
    /// Interface constants lower to `<Iface>_<NAME>`, and the emitter asks
    /// about their type in two unrelated places: to choose the spelling (a
    /// `const String` is stored as `&'static str`, and reading one yields an
    /// owned `String`) and to let `+` recognize a concatenation whose operand
    /// is one. Keeping a single answer stops those two from disagreeing --
    /// which they did, emitting the invalid Rust `String + String`.
    pub(crate) fn enclosing_interface_const_type(&self, bare: &str) -> Option<juxc_ast::TypeRef> {
        let iface = self.enclosing_interface.clone()?;
        if self.current_fn_params.contains(bare)
            || self.local_types.iter().any(|s| s.contains_key(bare))
        {
            return None;
        }
        self.interface_ast_by_bare(&iface)?
            .fields
            .iter()
            .find(|fd| fd.name.text == bare)
            .map(juxc_tycheck::resolved_field_type)
    }

    pub(crate) fn interface_ast_by_bare(&self, bare: &str) -> Option<&juxc_ast::InterfaceDecl> {
        // **Resolve in the unit being emitted first** (§M.16). `interface_asts` is
        // keyed by FQN, and a ROOT-package interface is keyed by its bare name, so
        // the exact `get(bare)` used to come first and handed
        // `jux.std.collections.LazyIterable`'s `implements Iterable` a USER
        // interface that happened to be spelled `Iterable`. LazyIterable then got
        // an EMPTY `impl Iterable<T> for LazyIterable<T> {}`, and rustc reported
        // E0046 against a standard-library file (ERRATA E96).
        //
        // Two map probes, no scan: this runs once per signature type of every
        // emitted method, so the package-preferring FQN search is too expensive
        // to put in front of it.
        let ctx = self
            .current_unit_idx
            .and_then(|i| self.symbols.units.get(i));
        if let Some(fqn) = ctx.and_then(|c| c.unqualified.get(bare)) {
            if let Some(d) = self.interface_asts.get(fqn) {
                return Some(d);
            }
        }
        let pkg = self.current_package_path();
        if !pkg.is_empty() {
            if let Some(d) = self.interface_asts.get(&format!("{pkg}.{bare}")) {
                return Some(d);
            }
        }
        // A bare key is a ROOT-package declaration, which a library unit does not
        // see (§M.16.1).
        if !juxc_tycheck::symbol_table::is_library_realm_package(&pkg) {
            if let Some(d) = self.interface_asts.get(bare) {
                return Some(d);
            }
        }
        // The last resort keeps the realm gate: a library unit must not pick a
        // user package's interface here either.
        let here_is_library = juxc_tycheck::symbol_table::is_library_realm_package(&pkg);
        let suffix = format!(".{bare}");
        let mut hits = self.interface_asts.iter().filter(|(k, _)| {
            k.ends_with(&suffix)
                && (!here_is_library
                    || juxc_tycheck::symbol_table::is_library_realm_package(
                        k.rsplit_once('.').map(|(p, _)| p).unwrap_or(""),
                    ))
        });
        match (hits.next(), hits.next()) {
            (Some((_, d)), None) => Some(d),
            _ => None,
        }
    }

    /// Lower a Jux interface to a Rust `trait`. Method signatures
    /// emit directly — `void foo();` becomes `fn foo(&self);` —
    /// and default-bodied methods become `fn foo(&self) { … }`
    /// inline. Rust's native trait-default-method support picks
    /// up the body so implementing classes can omit the method
    /// to inherit the default, or override it by re-declaring.
    pub(crate) fn emit_interface_decl(&mut self, interface: &juxc_ast::InterfaceDecl) {
        // (Migrated to Writer indent-aware API)
        self.w.emit_indent();
        self.emit_visibility(interface.visibility);
        self.w.push_str("trait ");
        self.w.push_str(&to_rust_ident(&interface.name.text));
        // A generic interface's parameter carries the same baseline bounds a
        // generic CLASS's does. The trait itself needs none, but its
        // signatures may name a generic class -- `double over(Ring<T> w)` --
        // and every generic class requires `Clone + Debug + 'static` on its
        // own parameter, so a `T` that reaches one has to satisfy them here.
        // Adding them cannot narrow what the interface accepts: §T.2.1
        // guarantees every Jux type meets the bounds the compiler adds on the
        // user's behalf, which is the property that makes an inferred bound
        // safe to add at all.
        //
        // `Display` is the same rule one step further: a DEFAULT body lives
        // on the trait, so a `T` it formats needs that bound where the
        // `format!` is emitted.
        let displayed = self.interface_displayed_generic_params(interface);
        self.emit_generic_params_with_clone_bound_plus_display(
            &interface.generic_params,
            &displayed,
            &std::collections::HashSet::new(),
        );
        // `: std::fmt::Debug` supertrait — interface values lower to
        // `Rc<dyn Trait>`, which is held in `#[derive(Clone, Debug)]`
        // structs (wrapper-class fields, holders). `dyn Trait` is only
        // `Debug` if the trait requires it, so we make `Debug` a supertrait.
        // Every implementer already derives `Debug` (wrapper, value, and
        // sealed lowerings all carry `#[derive(…, Debug)]`), so the bound is
        // always satisfiable.
        self.w.push_str(": std::fmt::Debug");
        // `Display` too: printing an interface-typed value shows the OBJECT's
        // text -- its `operator string`, a record's `Circle(r: 3.0)`, or the
        // class identity `Plain@<addr>` (Operators §O.2, §O.4.1: "through a
        // base-typed or interface-typed reference as well"). Without the
        // supertrait `dyn Trait` had only `Debug`, and `print(shape)` showed
        // the Rust struct, `Circle { r: 3.0 }`. Every Jux type emits a
        // `Display` (a class at least its identity one), and so does the
        // anonymous implementer, so the bound is always met.
        self.w.push_str(" + std::fmt::Display");
        // `JuxIdentity`: an interface-typed handle still names one object, so
        // `===` and identity hashing reach it through the vtable.
        self.w.push_str(" + crate::JuxIdentity");
        // **Interface `extends` → Rust supertrait bounds.** Jux's
        // `interface Entity<E> extends Id, Named, Comparable<E>` becomes
        // `trait Entity<E>: std::fmt::Debug + Id + Named + Comparable<E>`.
        // This is what makes a generic bound `E: Entity<E>` imply
        // `E: Comparable<E>` (so `x.compareTo(...)` resolves inside a
        // `<E extends Entity<E>>` method) and lets a `dyn Entity` value reach
        // the inherited methods. Each parent flows through `emit_type_as_rust`
        // (interfaces are already Rust traits, generic args preserved).
        for parent in &interface.extends {
            self.w.push_str(" + ");
            self.emit_type_as_rust(parent);
        }
        self.w.push_str(" {\n");
        self.w.indent_inc();
        for method in &interface.methods {
            let is_static = method
                .modifiers
                .iter()
                .any(|m| matches!(m, juxc_ast::FnModifier::Static));
            // Static interface methods don't fit inside Rust
            // traits cleanly — `Trait::staticMethod()` needs
            // `<Type as Trait>::staticMethod()` qualification
            // from the call site, which doesn't match Jux's
            // `Interface.staticMethod(args)` shape. We emit them
            // as **free functions** below the trait instead; the
            // call-site dispatch in `emit_call` rewrites
            // `Iface.foo(args)` to `Iface_foo(args)`.
            if is_static {
                continue;
            }
            // `I.super.m()` somewhere in the program (§T.8.3): the default body
            // moves to `__jux_default_<m>`, which a class overriding `m` can
            // still call, and `m` itself delegates to it.
            let super_called = method.body.is_some()
                && self
                    .interface_super_calls
                    .contains(&(interface.name.text.clone(), method.name.text.clone()));
            let passes: &[bool] = if super_called { &[true, false] } else { &[false] };
            for &as_default_twin in passes {
            self.w.emit_indent();
            // An `async T` interface method lowers to a plain `fn` returning a
            // BOXED future, not to Rust's `async fn` in a trait. Rust has had
            // the latter since 1.75, but a trait carrying one is not
            // dyn-compatible — and a Jux interface exists to be a `Rc<dyn
            // Trait>` value, so `async fn` made every async interface unusable
            // (rustc E0038). The boxed form is what `#[async_trait]` produces,
            // and what the spec's "async interface methods work the same way
            // as their sync counterparts" requires.
            let is_async = matches!(method.return_type, ReturnType::AsyncType(_));
            self.w.push_str("fn ");
            if as_default_twin {
                self.w.push_str("__jux_default_");
                self.w.push_str(&method.name.text);
            } else {
                self.w.push_str(&to_rust_ident(&method.name.text));
            }
            // A default method's own type parameters carry the bounds a class
            // method's get: `Clone` for the value model, `Display` where a
            // value is formatted.
            if method.generic_params.is_empty() {
                self.emit_generic_params(&method.generic_params);
            } else {
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
            // `&self` — interface methods take a shared receiver so the
            // interface can be used as a `dyn` value type (`Rc<dyn Trait>`,
            // which only ever yields `&self`, never `&mut self`). This is
            // sound because every implementer is a wrapper class
            // (`compute_interface_forced_classes` force-wraps them): a
            // `this.field` write goes through `self.0.borrow_mut()` interior
            // mutability, so the concrete method needs no mutable receiver.
            // The inherent method is likewise `&self` (wrapper rule), so
            // method resolution prefers it and never recurses into the trait
            // default.
            self.w.push_str("(&self");
            for param in &method.params {
                self.w.push_str(", ");
                self.w.push_str(&to_rust_ident(&param.name.text));
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
                    // `-> Pin<Box<dyn Future<Output = T> + '_>>`. The `'_` ties
                    // the future to the `&self` borrow, which is what a method
                    // reading its receiver needs.
                    self.w
                        .push_str(" -> std::pin::Pin<::std::boxed::Box<dyn std::future::Future<Output = ");
                    self.emit_return_type_as_rust(t);
                    self.w.push_str("> + '_>>");
                }
            }
            let sized_self = default_method_needs_sized_self(method);
            // `where T has operator<=>(T) -> int` on the method (§O.5) bounds
            // the interface's own `T` for this method only, as Rust allows.
            let mut bounds: Vec<String> = Vec::new();
            if sized_self {
                bounds.push("Self: Sized + Clone + 'static".to_string());
            }
            bounds.extend(crate::decls::functions::where_bounds(&method.wheres));
            if !bounds.is_empty() {
                self.w.push_str(" where ");
                self.w.push_str(&bounds.join(", "));
            }
            // Two shapes: abstract signature (`;`) vs. default
            // body (`{ … }`). The presence of `method.body`
            // discriminates. Default bodies go through the same
            // `emit_fn_body` path as regular function bodies so
            // tail-return elision, format-arg discipline, etc. all
            // apply uniformly.
            if super_called && !as_default_twin {
                // The delegating default: `self.__jux_default_m(args)`.
                self.w.push_str(" {\n");
                self.w.indent_inc();
                self.w.emit_indent();
                self.w.push_str("self.__jux_default_");
                self.w.push_str(&method.name.text);
                self.w.push('(');
                for (i, param) in method.params.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.w.push_str(&to_rust_ident(&param.name.text));
                }
                self.w.push_str(")\n");
                self.w.indent_dec();
                self.w.line("}");
            } else if let Some(body) = &method.body {
                self.w.push_str(" {\n");
                self.w.indent_inc();
                // A default body for an async method IS the future the boxed
                // signature promises.
                if is_async {
                    self.w.emit_indent();
                    self.w.push_str("::std::boxed::Box::pin(async move {
");
                    self.w.indent_inc();
                }
                // `&self` in the interface trait method maps to
                // the Rust `self` keyword as the implicit
                // receiver; set the alias so `this` in the body
                // emits correctly.
                let prev_alias = self.this_alias.take();
                self.this_alias = Some("self".to_string());
                // **A parameter shadows a top-level declaration of the same
                // name** (§M.16.2, ERRATA E96). Every other body kind records its
                // parameters here; a default interface method was the one that did
                // not, and the shadow set is what tells `callee_param_is_nullable`
                // that `pred(x!!)` inside `Iterator.any` calls the method's OWN
                // `pred` parameter. Without it a program that merely DECLARED
                // `void pred(String? s)` made those calls wrap their argument in
                // `Some(...)`, and the build failed with rustc E0308 inside
                // `jux.std\collections/Iterator.jux` -- a file the author cannot
                // open, for a function the program never called.
                let prev_params = std::mem::replace(
                    &mut self.current_fn_params,
                    method.params.iter().map(|p| p.name.text.clone()).collect(),
                );
                // `this` handed on as a value is the interface value an
                // `Rc<dyn Iface>` slot takes: an owned handle to this object,
                // made once (`Self` is a class handle, so the clone shares).
                if sized_self && default_body_uses_this_as_value(body) {
                    self.w.line("let __jux_this_rc = std::rc::Rc::new(self.clone());");
                    self.this_alias = Some("__jux_this_rc".to_string());
                }
                // Track the enclosing interface so a bare-name
                // method call inside the default body (Java rule:
                // `foo()` ≡ `self.foo()` when `foo` is declared on
                // the same interface) rewrites correctly in
                // `emit_call`.
                let prev_iface = self.enclosing_interface.take();
                self.enclosing_interface = Some(interface.name.text.clone());
                let saved_return = self.current_return_type.take();
                self.current_return_type = Some(method.return_type.clone());
                self.seed_mutated_locals(body);
                self.emit_fn_body_at(body, &method.return_type);
                self.current_return_type = saved_return;
                self.enclosing_interface = prev_iface;
                self.this_alias = prev_alias;
                self.current_fn_params = prev_params;
                if is_async {
                    self.w.indent_dec();
                    self.w.emit_indent();
                    self.w.push_str("})
");
                }
                self.w.indent_dec();
                self.w.line("}");
            } else {
                self.w.push_str(";\n");
            }
            }
        }
        // Runtime-type downcast hooks (`__jux_as_<T>`) so a value typed as this
        // interface (`Rc<dyn Iface>`) can be downcast / type-tested — one per
        // cast/type-test target some implementer of this interface could also
        // be. Implementing classes override them in `emit_class_trait_impls`.
        let iface_bare = interface.name.text.clone();
        for t in self.interface_hook_targets(&iface_bare) {
            self.emit_downcast_hook_sig(&t, &iface_bare);
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        // **A handle to an interface behaves as the interface.** An
        // interface-typed value lowers to `Rc<dyn Iface>`, which Rust sees as a
        // smart pointer rather than an implementer — so a bound naming the
        // interface (what `? extends Iface` and `V extends Iface` lift to, and
        // what a class `Kind` trait lists as a supertrait) rejected it.
        // Forwarding the trait over `Rc<T>` states what is already true.
        self.emit_interface_rc_forwarding_impl(interface);
        // An interface-typed value compares and hashes by identity, so it can
        // be a set element or a map key.
        if interface.generic_params.is_empty() {
            self.emit_dyn_identity_eq_hash(&interface.name.text);
        }

        // Static interface methods: free functions named
        // `<Interface>_<method>`. The call-site dispatch in
        // `emit_call` recognizes `Iface.foo(args)` against the
        // symbol table's `is_static` flag and emits the
        // matching name. Same body-emit pipeline as regular
        // free functions.
        for method in &interface.methods {
            let is_static = method
                .modifiers
                .iter()
                .any(|m| matches!(m, juxc_ast::FnModifier::Static));
            if !is_static {
                continue;
            }
            self.w.emit_indent();
            self.emit_visibility(interface.visibility);
            // Static interface methods may carry `async` too — the
            // emitted free function (named `<Iface>_<method>`) becomes
            // an `async fn`, callable as `Iface_method(args).await`.
            if matches!(method.return_type, ReturnType::AsyncType(_)) {
                self.w.push_str("async fn ");
            } else {
                self.w.push_str("fn ");
            }
            self.w.push_str(&to_rust_ident(&interface.name.text));
            self.w.push('_');
            self.w.push_str(&to_rust_ident(&method.name.text));
            // The method's own type parameters take the bounds a free
            // function's do (§T.2.1): the baseline `Clone + Debug + 'static`
            // the body relies on (a returned parameter is cloned), `Display`
            // when one is formatted, `Default` for `new T[n]`, and the key
            // bounds. Written bare, `static <U> U same(U u)` asked rustc to
            // `clone()` a `U` it knew nothing about.
            self.emit_static_method_generic_params(method);
            self.w.push('(');
            for (i, param) in method.params.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.w.push_str(&to_rust_ident(&param.name.text));
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
                    // `async T` static interface method → `async fn …
                    // -> T`. The keyword sat ahead of `fn` above.
                    self.w.push_str(" -> ");
                    self.emit_return_type_as_rust(t);
                }
            }
            if let Some(body) = &method.body {
                self.w.push_str(" {\n");
                self.w.indent_inc();
                let saved_return = self.current_return_type.take();
                self.current_return_type = Some(method.return_type.clone());
                self.seed_mutated_locals(body);
                self.emit_fn_body_at(body, &method.return_type);
                self.current_return_type = saved_return;
                self.w.indent_dec();
                self.w.line("}");
            } else {
                self.w.push_str(" { unimplemented!() }\n");
            }
            self.w.newline();
        }

        // Interface fields — emitted as free `pub const`
        // declarations named `Interface_FIELD` (mirroring the
        // static-method naming) so call sites like
        // `Iface.FIELD` rewrite cleanly. They're always
        // initialized (parser enforced) and always
        // `public static final` by §3.3. The const-context
        // flag re-uses the class-static-field trick: `String`
        // types lower to `&'static str` and string literals
        // skip the `.to_string()` wrap so `const` stays
        // const-evaluatable.
        for field in &interface.fields {
            self.w.emit_indent();
            self.emit_visibility(interface.visibility);
            self.w.push_str("const ");
            self.w.push_str(&to_rust_ident(&interface.name.text));
            self.w.push('_');
            self.w.push_str(&to_rust_ident(&field.name.text));
            self.w.push_str(": ");
            self.emitting_const_context = true;
            let field_ty = juxc_tycheck::resolved_field_type(field);
            self.emit_field_type_as_rust(&field_ty);
            self.w.push_str(" = ");
            if let Some(init) = &field.default {
                match self.const_string_fold(&field_ty, init) {
                    Some(folded) => self.emit_rust_string_literal(&folded),
                    None => self.emit_expr(init),
                }
            } else {
                self.w.push_str("Default::default()");
            }
            self.emitting_const_context = false;
            self.w.push_str(";\n");
        }
        if !interface.fields.is_empty() {
            self.w.newline();
        }
    }

    /// `<U: Clone + std::fmt::Debug + 'static, …>` for a static interface
    /// method's own type parameters, computed as for a free function. The
    /// per-declaration bound sets are collected fresh from this method, so an
    /// earlier declaration's `Hash` / `PartialEq` requirements cannot leak on.
    fn emit_static_method_generic_params(&mut self, method: &juxc_ast::FnDecl) {
        if method.generic_params.is_empty() {
            return;
        }
        let displayed = self.fn_displayed_generic_params(method);
        let defaulted = crate::analysis::new_array_element_params(
            &method.generic_params,
            &method.body.iter().collect::<Vec<_>>(),
            &[],
        );
        let mut declared: Vec<juxc_ast::TypeRef> = method.params.iter().map(|p| p.ty.clone()).collect();
        if let ReturnType::Type(t) | ReturnType::AsyncType(t) = &method.return_type {
            declared.push(t.clone());
        }
        self.collect_key_bound_params(&method.generic_params, declared.iter());
        self.collect_fn_equality_bound_params(method);
        self.emit_generic_params_with_clone_bound_plus_display(&method.generic_params, &displayed, &defaulted);
        self.hash_key_params.clear();
        self.ord_key_params.clear();
        self.eq_bound_params.clear();
        self.hashed_params.clear();
    }
}

impl RustEmitter {
    /// Record which locals `body` reassigns, so each is declared `let mut`.
    /// Every function and method body runs this before emitting; interface
    /// bodies are emitted from here, so they run it too.
    fn seed_mutated_locals(&mut self, body: &juxc_ast::Block) {
        let mut muts = std::collections::HashSet::new();
        collect_mutated_names(body, &mut muts, &self.user_mut_methods);
        self.collect_mut_slot_locals(body, &mut muts);
        self.mutated_in_fn = muts;
    }
}
