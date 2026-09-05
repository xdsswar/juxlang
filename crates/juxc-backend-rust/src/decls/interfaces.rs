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

use crate::RustEmitter;
use juxc_lex::to_rust_ident;

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
            .cloned()
            .collect();
        let hooks = self.interface_hook_targets(&iface_bare);
        self.w.emit_indent();
        self.w.push_str("impl<__JuxH: ?core::marker::Sized + ");
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
        self.w.push_str(" for std::rc::Rc<__JuxH> {
");
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
                        .push_str(" -> std::pin::Pin<Box<dyn std::future::Future<Output = ");
                    self.emit_return_type_as_rust(&t);
                    self.w.push_str("> + '_>>");
                }
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
    pub(crate) fn interface_ast_by_bare(&self, bare: &str) -> Option<&juxc_ast::InterfaceDecl> {
        if let Some(d) = self.interface_asts.get(bare) {
            return Some(d);
        }
        let suffix = format!(".{bare}");
        let mut hits = self
            .interface_asts
            .iter()
            .filter(|(k, _)| k.ends_with(&suffix));
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
        // Generic params follow without bounds — the trait doesn't imply
        // `Clone` itself; implementing types pick up bounds as needed on their
        // own impls. The one exception is `Display`: a DEFAULT method body
        // lives on the trait, so a `T` value it formats needs the bound HERE,
        // where the `format!` is emitted. Same rule as a generic class's
        // inherent impl, and the same scan behind it.
        let displayed = self.interface_displayed_generic_params(interface);
        self.emit_generic_params_with_display(&interface.generic_params, &displayed);
        // `: std::fmt::Debug` supertrait — interface values lower to
        // `Rc<dyn Trait>`, which is held in `#[derive(Clone, Debug)]`
        // structs (wrapper-class fields, holders). `dyn Trait` is only
        // `Debug` if the trait requires it, so we make `Debug` a supertrait.
        // Every implementer already derives `Debug` (wrapper, value, and
        // sealed lowerings all carry `#[derive(…, Debug)]`), so the bound is
        // always satisfiable.
        self.w.push_str(": std::fmt::Debug");
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
            self.w.push_str(&to_rust_ident(&method.name.text));
            self.emit_generic_params(&method.generic_params);
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
                        .push_str(" -> std::pin::Pin<Box<dyn std::future::Future<Output = ");
                    self.emit_return_type_as_rust(t);
                    self.w.push_str("> + '_>>");
                }
            }
            // Two shapes: abstract signature (`;`) vs. default
            // body (`{ … }`). The presence of `method.body`
            // discriminates. Default bodies go through the same
            // `emit_fn_body` path as regular function bodies so
            // tail-return elision, format-arg discipline, etc. all
            // apply uniformly.
            if let Some(body) = &method.body {
                self.w.push_str(" {\n");
                self.w.indent_inc();
                // A default body for an async method IS the future the boxed
                // signature promises.
                if is_async {
                    self.w.emit_indent();
                    self.w.push_str("Box::pin(async move {
");
                    self.w.indent_inc();
                }
                // `&self` in the interface trait method maps to
                // the Rust `self` keyword as the implicit
                // receiver; set the alias so `this` in the body
                // emits correctly.
                let prev_alias = self.this_alias.take();
                self.this_alias = Some("self".to_string());
                // Track the enclosing interface so a bare-name
                // method call inside the default body (Java rule:
                // `foo()` ≡ `self.foo()` when `foo` is declared on
                // the same interface) rewrites correctly in
                // `emit_call`.
                let prev_iface = self.enclosing_interface.take();
                self.enclosing_interface = Some(interface.name.text.clone());
                let saved_return = self.current_return_type.take();
                self.current_return_type = Some(method.return_type.clone());
                self.emit_fn_body_at(body, &method.return_type);
                self.current_return_type = saved_return;
                self.enclosing_interface = prev_iface;
                self.this_alias = prev_alias;
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
            self.emit_generic_params(&method.generic_params);
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
            self.emit_field_type_as_rust(&juxc_tycheck::resolved_field_type(field));
            self.w.push_str(" = ");
            if let Some(init) = &field.default {
                self.emit_expr(init);
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
}
