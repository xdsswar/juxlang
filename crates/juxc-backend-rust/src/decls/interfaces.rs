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

impl RustEmitter {
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
        self.w.push_str(&interface.name.text);
        // Generic params follow without bounds — the trait doesn't
        // imply `Clone` itself; implementing types pick up bounds as
        // needed on their own impls.
        self.emit_generic_params(&interface.generic_params);
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
            // `async T` interface methods become `async fn` on the
            // trait. Rust supports async fns in traits since 1.75
            // (stabilized RFC 3185), so this lowers directly without
            // needing `#[async_trait]` shims. The default-body and
            // signature-only paths both honor the prefix the same way.
            if matches!(method.return_type, ReturnType::AsyncType(_)) {
                self.w.push_str("async fn ");
            } else {
                self.w.push_str("fn ");
            }
            self.w.push_str(&method.name.text);
            self.emit_generic_params(&method.generic_params);
            // `&mut self` — Java semantics make every `this` mutable,
            // and an implementing class's method body may need to
            // write `this.field` to satisfy its concrete behavior.
            // Emitting the trait method as `&mut self` keeps the
            // implementer's signature consistent so method
            // resolution doesn't fall through to the trait method
            // and recurse. The mutation analyzer (see
            // `collect_user_mut_methods` + `body_writes_to_this`)
            // is extended to follow the cascade: callers of trait
            // methods on `this.field` get promoted to `&mut self`
            // too.
            self.w.push_str("(&mut self");
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
                ReturnType::AsyncType(t) => {
                    // `async T` → `async fn (...) -> T`. The keyword
                    // was already prepended above.
                    self.w.push_str(" -> ");
                    self.emit_return_type_as_rust(t);
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
                self.w.indent_dec();
                self.w.line("}");
            } else {
                self.w.push_str(";\n");
            }
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();

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
            self.w.push_str(&interface.name.text);
            self.w.push('_');
            self.w.push_str(&method.name.text);
            self.emit_generic_params(&method.generic_params);
            self.w.push('(');
            for (i, param) in method.params.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
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
            self.w.push_str(&interface.name.text);
            self.w.push('_');
            self.w.push_str(&field.name.text);
            self.w.push_str(": ");
            self.emitting_const_context = true;
            self.emit_field_type_as_rust(&field.ty);
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
