//! Observable-property emission (§P — JUX-OBSERVABLE-PROPERTIES-ADDENDUM).
//!
//! Every writable `{ get; set; }` property of a wrapper class is
//! observable: the class's inner struct grows two lazily-allocated
//! observer vectors per property (full-shape and invalidation-shape)
//! plus a binding keep-alive slot, the wrapper impl grows the
//! attach/detach/clear/size/fire helpers, and the synthesized
//! `__set_<X>` setter fires observers after its body runs (an early
//! `return` skips the fire — exactly the W0973 semantics).
//!
//! Storage model (per property `X` of Jux type `T`):
//!
//! ```text
//! __obs_X_full: Option<Vec<crate::JuxObserver<dyn Fn(T, T)>>>,  // shapes 1+2
//! __obs_X_inv:  Option<Vec<crate::JuxObserver<dyn Fn()>>>,      // shape 3
//! __bind_X:     Option<crate::JuxObserver<dyn Fn(T, T)>>,       // bind() keep-alive
//! ```
//!
//! `JuxObserver` (emitted in the prelude) is Weak for NAMED observer
//! variables (§P.2.3 — the owner's field is the strong ref; owner
//! drops → observer silently stops firing and is pruned on the next
//! fire) and Strong for INLINE lambdas (nothing else would hold them).
//! `__bind_X` is stored through the same enum (always Strong) purely
//! so the inner struct's `#[derive(Debug)]` keeps resolving — a bare
//! `Rc<dyn Fn>` has no `Debug` impl.

use juxc_ast::{ClassDecl, PropertyDecl};

use crate::RustEmitter;

/// The properties of `class_decl` that get observer infrastructure:
/// non-static, with BOTH accessors (a getter for old/new capture, a
/// setter to fire from). Computed (get-only) property observation is
/// a §P.1.5 follow-up — it needs dependency tracking.
pub(crate) fn observable_props(class_decl: &ClassDecl) -> Vec<&PropertyDecl> {
    class_decl
        .properties
        .iter()
        .filter(|p| !p.is_static && p.getter.is_some() && p.setter.is_some())
        .collect()
}

/// True when the property's lowered type supports `!=` change
/// detection (rustc `PartialEq`). Primitives and `String` (nullable
/// included) compare; user classes lower to `Rc<RefCell<…>>` wrappers
/// without a derived `PartialEq`, so their setters fire on every
/// write instead.
pub(crate) fn property_ty_is_comparable(ty: &juxc_ast::TypeRef) -> bool {
    if ty.fn_shape.is_some() || ty.array_shape.is_some() || ty.ptr_depth > 0 {
        return false;
    }
    if ty.name.segments.len() != 1 {
        return false;
    }
    matches!(
        ty.name.segments[0].text.as_str(),
        "bool" | "byte" | "ubyte" | "short" | "ushort" | "int" | "uint" | "long" | "ulong"
            | "float" | "double" | "char" | "String"
    )
}

impl RustEmitter {
    /// Look up a class AST by bare name (exact key, or unique
    /// `…\.<bare>` FQN suffix) — `class_asts` is keyed by FQN.
    pub(crate) fn class_ast_by_bare(&self, bare: &str) -> Option<&ClassDecl> {
        if let Some(cd) = self.class_asts.get(bare) {
            return Some(cd);
        }
        let suffix = format!(".{bare}");
        let mut found = None;
        for (fqn, cd) in &self.class_asts {
            if fqn.ends_with(&suffix) {
                if found.is_some() {
                    return None; // ambiguous — caller falls back
                }
                found = Some(cd);
            }
        }
        found
    }

    /// True when `bare` names a class whose property `prop` is
    /// observable (drives the `.observers` / `bind` routing).
    pub(crate) fn class_has_observable_prop(&self, bare: &str, prop: &str) -> bool {
        self.class_ast_by_bare(bare)
            .map(|cd| {
                observable_props(cd)
                    .iter()
                    .any(|p| p.name.text == prop)
            })
            .unwrap_or(false)
    }

    /// Emit the per-property observer storage fields into the inner
    /// struct (multi-line, writer already at field depth).
    pub(crate) fn emit_observer_inner_fields(&mut self, class_decl: &ClassDecl) {
        for prop in observable_props(class_decl) {
            let x = &prop.name.text;
            // Full-shape observers: `(old, now)` lambdas (and the
            // shape-2 adapter wrappers).
            self.w.emit_indent();
            self.w.push_str(&format!("pub(crate) __obs_{x}_full: Option<Vec<crate::JuxObserver<dyn Fn("));
            self.emit_type_as_rust(&prop.ty);
            self.w.push_str(", ");
            self.emit_type_as_rust(&prop.ty);
            self.w.push_str(")>>>,\n");
            // Invalidation observers: `()` lambdas.
            self.w.emit_indent();
            self.w
                .push_str(&format!("pub(crate) __obs_{x}_inv: Option<Vec<crate::JuxObserver<dyn Fn()>>>,\n"));
            // `bind()` keep-alive: the binding closure attached (weakly)
            // to the SOURCE property lives here on the bound TARGET, so
            // the binding dies with the target and `unbind()` is just
            // `= None`.
            self.w.emit_indent();
            self.w.push_str(&format!("pub(crate) __bind_{x}: Option<crate::JuxObserver<dyn Fn("));
            self.emit_type_as_rust(&prop.ty);
            self.w.push_str(", ");
            self.emit_type_as_rust(&prop.ty);
            self.w.push_str(")>>,\n");
        }
    }

    /// Append the observer fields' `None` seeds to a SINGLE-LINE inner
    /// struct literal (simple-ctor / synthetic-default shapes).
    /// `first` mirrors the caller's separator state.
    pub(crate) fn emit_observer_field_inits_inline(
        &mut self,
        class_decl: &ClassDecl,
        first: &mut bool,
    ) {
        for prop in observable_props(class_decl) {
            let x = &prop.name.text;
            let sep = if *first { " " } else { ", " };
            *first = false;
            self.w.push_str(&format!(
                "{sep}__obs_{x}_full: None, __obs_{x}_inv: None, __bind_{x}: None"
            ));
        }
    }

    /// Append the observer fields' `None` seeds to a MULTI-LINE inner
    /// struct literal (the `__self`-builder ctor shape) — one line per
    /// property, writer already at field depth.
    pub(crate) fn emit_observer_field_inits_lines(&mut self, class_decl: &ClassDecl) {
        for prop in observable_props(class_decl) {
            let x = &prop.name.text;
            self.w.emit_indent();
            self.w.push_str(&format!(
                "__obs_{x}_full: None, __obs_{x}_inv: None, __bind_{x}: None,\n"
            ));
        }
    }

    /// Emit the per-property observer helper methods inside the
    /// wrapper's `impl` block: attach (full/inv), detach (full/inv),
    /// clear, size, and the fire routine the setter epilogue calls.
    pub(crate) fn emit_observer_helper_methods(&mut self, class_decl: &ClassDecl) {
        // Per-class handle helpers for the §P.4 binding machinery —
        // the wrapper's tuple field is module-private, so bind sites
        // in OTHER packages go through these instead of `.0`:
        // `__jux_weak` hands out a non-owning handle for the binding
        // closure (capturing the wrapper strongly would cycle), and
        // `__jux_from_inner` rebuilds a wrapper from the upgraded
        // handle inside the closure.
        if !observable_props(class_decl).is_empty() {
            let name = class_decl.name.text.clone();
            // Thread the class's generic args onto the inner type
            // (`Box_Inner<T>` for `class Box<T>`).
            let gargs = {
                let mark = self.w.len();
                self.emit_generic_params_as_args(&class_decl.generic_params);
                self.w.split_off_from(mark)
            };
            self.w.indent_inc();
            self.w.line(&format!(
                "pub fn __jux_weak(&self) -> std::rc::Weak<std::cell::RefCell<{name}_Inner{gargs}>> {{ std::rc::Rc::downgrade(&self.0) }}"
            ));
            self.w.line(&format!(
                "pub fn __jux_from_inner(rc: std::rc::Rc<std::cell::RefCell<{name}_Inner{gargs}>>) -> Self {{ Self(rc) }}"
            ));
            self.w.indent_dec();
            self.w.newline();
        }
        for prop in observable_props(class_decl) {
            let x = prop.name.text.clone();
            // Render the property's Rust type once for reuse in the
            // signatures below.
            let t = self.render_type_to_string(&prop.ty);

            self.w.indent_inc();

            // -- attach --------------------------------------------------
            self.w
                .line(&format!("pub fn __obs_{x}_attach_full(&self, o: crate::JuxObserver<dyn Fn({t}, {t})>) {{"));
            self.w.indent_inc();
            self.w.line(&format!(
                "self.0.borrow_mut().__obs_{x}_full.get_or_insert_with(Vec::new).push(o);"
            ));
            self.w.indent_dec();
            self.w.line("}");
            self.w
                .line(&format!("pub fn __obs_{x}_attach_inv(&self, o: crate::JuxObserver<dyn Fn()>) {{"));
            self.w.indent_inc();
            self.w.line(&format!(
                "self.0.borrow_mut().__obs_{x}_inv.get_or_insert_with(Vec::new).push(o);"
            ));
            self.w.indent_dec();
            self.w.line("}");

            // -- detach (pointer identity against the named Rc) ----------
            self.w.line(&format!(
                "pub fn __obs_{x}_detach_full(&self, t: &std::rc::Rc<dyn Fn({t}, {t})>) {{"
            ));
            self.w.indent_inc();
            self.w.line(&format!(
                "if let Some(v) = self.0.borrow_mut().__obs_{x}_full.as_mut() {{ v.retain(|o| !o.is_for(t)); }}"
            ));
            self.w.indent_dec();
            self.w.line("}");
            self.w.line(&format!(
                "pub fn __obs_{x}_detach_inv(&self, t: &std::rc::Rc<dyn Fn()>) {{"
            ));
            self.w.indent_inc();
            self.w.line(&format!(
                "if let Some(v) = self.0.borrow_mut().__obs_{x}_inv.as_mut() {{ v.retain(|o| !o.is_for(t)); }}"
            ));
            self.w.indent_dec();
            self.w.line("}");

            // -- clear / size ---------------------------------------------
            // `clear` releases the storage entirely (§P.3.2); `size`
            // counts LIVE observers without allocating (§P.3.3).
            self.w.line(&format!("pub fn __obs_{x}_clear(&self) {{"));
            self.w.indent_inc();
            self.w.line("let mut s = self.0.borrow_mut();");
            self.w.line(&format!("s.__obs_{x}_full = None;"));
            self.w.line(&format!("s.__obs_{x}_inv = None;"));
            self.w.indent_dec();
            self.w.line("}");
            self.w.line(&format!("pub fn __obs_{x}_size(&self) -> isize {{"));
            self.w.indent_inc();
            self.w.line("let s = self.0.borrow();");
            self.w.line(&format!(
                "let full = s.__obs_{x}_full.as_ref().map(|v| v.iter().filter(|o| o.upgrade().is_some()).count()).unwrap_or(0);"
            ));
            self.w.line(&format!(
                "let inv = s.__obs_{x}_inv.as_ref().map(|v| v.iter().filter(|o| o.upgrade().is_some()).count()).unwrap_or(0);"
            ));
            self.w.line("(full + inv) as isize");
            self.w.indent_dec();
            self.w.line("}");

            // -- bind keep-alive store (§P.4) ------------------------------
            // `Some(Strong(f))` while a binding drives this property;
            // `None` after `unbind()` (the weak ref left at the source
            // dies and is pruned on its next fire).
            self.w.line(&format!(
                "pub fn __bind_{x}_store(&self, f: Option<crate::JuxObserver<dyn Fn({t}, {t})>>) {{"
            ));
            self.w.indent_inc();
            self.w.line(&format!("self.0.borrow_mut().__bind_{x} = f;"));
            self.w.indent_dec();
            self.w.line("}");

            // -- fire ------------------------------------------------------
            // Invalidation observers first, then full observers
            // (§P.3.4), pruning dead weak refs on the way. Each vec is
            // take()n out of the cell before iterating so a re-entrant
            // set inside an observer body can't double-borrow (a nested
            // fire sees None and no-ops — change-driven recursion
            // terminates).
            self.w.line(&format!(
                "pub fn __obs_{x}_fire(&self, old: &{t}, now: &{t}) {{"
            ));
            self.w.indent_inc();
            self.w.line(&format!("let inv = self.0.borrow_mut().__obs_{x}_inv.take();"));
            self.w.line("if let Some(mut v) = inv {");
            self.w.indent_inc();
            self.w.line(
                "v.retain(|o| match o.upgrade() { Some(f) => { f(); true } None => false });",
            );
            self.w.line("let mut s = self.0.borrow_mut();");
            self.w.line(&format!(
                "if let Some(cur) = s.__obs_{x}_inv.take() {{ v.extend(cur); }}"
            ));
            self.w.line(&format!("s.__obs_{x}_inv = Some(v);"));
            self.w.indent_dec();
            self.w.line("}");
            self.w.line(&format!("let full = self.0.borrow_mut().__obs_{x}_full.take();"));
            self.w.line("if let Some(mut v) = full {");
            self.w.indent_inc();
            self.w.line(
                "v.retain(|o| match o.upgrade() { Some(f) => { f(old.clone(), now.clone()); true } None => false });",
            );
            self.w.line("let mut s = self.0.borrow_mut();");
            self.w.line(&format!(
                "if let Some(cur) = s.__obs_{x}_full.take() {{ v.extend(cur); }}"
            ));
            self.w.line(&format!("s.__obs_{x}_full = Some(v);"));
            self.w.indent_dec();
            self.w.line("}");
            self.w.indent_dec();
            self.w.line("}");

            self.w.indent_dec();
            self.w.newline();
        }
    }

    /// Render a Jux type to its Rust spelling as a `String` (for use
    /// inside `format!`-built signatures) by emitting into a scratch
    /// writer.
    pub(crate) fn render_type_to_string(&mut self, ty: &juxc_ast::TypeRef) -> String {
        let mark = self.w.len();
        self.emit_type_as_rust(ty);
        self.w.split_off_from(mark)
    }

    /// Emit the Rust type of an `observer<T>` variable for a known
    /// lambda arity: `Rc<dyn Fn()>` (invalidation), `Rc<dyn Fn(T, T)>`
    /// (full), or `Rc<dyn Fn(String, T, T)>` (full + property
    /// reference — Phase 1 passes the firing property's NAME).
    pub(crate) fn emit_observer_var_type(&mut self, ty: &juxc_ast::TypeRef, arity: usize) {
        self.w.push_str("std::rc::Rc<dyn Fn(");
        match arity {
            0 => {}
            3 => {
                self.w.push_str("String, ");
                self.emit_observer_payload_ty(ty);
                self.w.push_str(", ");
                self.emit_observer_payload_ty(ty);
            }
            _ => {
                self.emit_observer_payload_ty(ty);
                self.w.push_str(", ");
                self.emit_observer_payload_ty(ty);
            }
        }
        self.w.push_str(")>");
    }

    /// The `T` of `observer<T>`, emitted as a Rust value type.
    fn emit_observer_payload_ty(&mut self, ty: &juxc_ast::TypeRef) {
        if let Some(juxc_ast::GenericArg::Type(t)) = ty.generic_args.first() {
            self.emit_type_as_rust(t);
        } else {
            self.w.push_str("()");
        }
    }

    /// Resolve the expression `.observers` (or `.bind` …) hangs off to
    /// the observable property it names. Returns `(receiver, prop)`
    /// where `receiver = None` means the enclosing instance. Handles
    /// the post-desugar shapes:
    ///
    /// - `obj.X` — `X` is an observable property of `obj`'s class;
    /// - `this.__prop_X` / `__prop_X` — the constructor rewrite routed
    ///   an auto-property access to its backing field;
    /// - bare `X` — a property of the enclosing class.
    pub(crate) fn resolve_observable_prop<'a>(
        &self,
        e: &'a juxc_ast::Expr,
    ) -> Option<(Option<&'a juxc_ast::Expr>, String, String)> {
        use juxc_ast::Expr;
        match e {
            Expr::Field(f) => {
                let name = f.field.text.as_str();
                let prop = name.strip_prefix("__prop_").unwrap_or(name);
                let class = if matches!(*f.object, Expr::This(_)) {
                    self.enclosing_class.clone()
                } else {
                    self.receiver_class_bare(&f.object)
                }?;
                if self.class_has_observable_prop(&class, prop) {
                    Some((Some(&f.object), prop.to_string(), class))
                } else {
                    None
                }
            }
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = qn.segments[0].text.as_str();
                let prop = name.strip_prefix("__prop_").unwrap_or(name);
                let class = self.enclosing_class.clone()?;
                if self.class_has_observable_prop(&class, prop) {
                    Some((None, prop.to_string(), class))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// True when the resolved receiver is the enclosing instance INSIDE
    /// a constructor body — there `this` is the bare inner struct
    /// (`__self`), which has the storage fields but not the wrapper's
    /// helper methods, so the observer ops emit field-direct.
    fn observers_receiver_is_ctor_inner(&self, recv: Option<&juxc_ast::Expr>) -> bool {
        let this_is_inner = self.this_alias.as_deref() == Some("__self");
        match recv {
            None => this_is_inner,
            Some(juxc_ast::Expr::This(_)) => this_is_inner,
            _ => false,
        }
    }

    /// Emit the receiver prefix for an observer operation — `self`,
    /// the aliased `this`, or the receiver expression.
    fn emit_observers_receiver(&mut self, recv: Option<&juxc_ast::Expr>) {
        match recv {
            None => {
                let alias = self.this_alias.clone().unwrap_or_else(|| "self".to_string());
                self.w.push_str(&alias);
            }
            Some(e) => self.emit_expr(e),
        }
    }

    /// Lambda arity of an `.observers.attach/detach` argument — an
    /// inline lambda counts its params; a named observer variable
    /// (path or field) looks up the recorded `observer<T>` shape;
    /// anything else defaults to the full 2-arg shape.
    fn observer_arg_arity(&self, arg: &juxc_ast::Expr) -> usize {
        use juxc_ast::Expr;
        match arg {
            Expr::Lambda(l) => l.params.len(),
            Expr::Path(qn) if qn.segments.len() == 1 => self
                .observer_shapes
                .get(qn.segments[0].text.as_str())
                .copied()
                .unwrap_or(2),
            Expr::Field(f) => self
                .observer_shapes
                .get(f.field.text.as_str())
                .copied()
                .unwrap_or(2),
            _ => 2,
        }
    }

    /// Emit one `<prop>.observers.attach(arg)` / `.detach(arg)` call.
    /// `recv`/`prop` come from [`Self::resolve_observable_prop`].
    pub(crate) fn emit_observers_call(
        &mut self,
        recv: Option<&juxc_ast::Expr>,
        prop: &str,
        class: &str,
        op: &str,
        call: &juxc_ast::CallExpr,
    ) {
        let Some(arg) = call.args.first() else {
            // Malformed — keep the output compiling.
            self.w.push_str("()");
            return;
        };
        let arity = self.observer_arg_arity(arg);
        let vec_kind = if arity == 0 { "inv" } else { "full" };
        let ctor_inner = self.observers_receiver_is_ctor_inner(recv);
        // The stored element's full type — the let temp below needs the
        // annotation so an inline closure unsize-coerces to `dyn Fn`.
        let elem_ty = if arity == 0 {
            "crate::JuxObserver<dyn Fn()>".to_string()
        } else {
            let t = self
                .prop_rust_ty(class, prop)
                .unwrap_or_else(|| "()".to_string());
            format!("crate::JuxObserver<dyn Fn({t}, {t})>")
        };
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = false;
        // The observer value is hoisted into a statement-scoped temp
        // BEFORE the storage call: reading a named observer field goes
        // through `self.0.borrow()`, and that guard temporary would
        // otherwise live to the end of the whole call statement —
        // colliding with the attach/detach `borrow_mut()` (runtime
        // `RefCell already borrowed`).
        if op == "attach" {
            self.w.push_str(&format!("{{ let __jux_o: {elem_ty} = "));
            self.emit_observer_value(prop, arg, arity);
            self.w.push_str("; ");
            if ctor_inner {
                // Field-direct: `__self.__obs_X_full.get_or_insert_with(Vec::new).push(o)`.
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(
                    ".__obs_{prop}_{vec_kind}.get_or_insert_with(Vec::new).push(__jux_o)"
                ));
            } else {
                self.emit_observers_receiver(recv);
                self.w
                    .push_str(&format!(".__obs_{prop}_attach_{vec_kind}(__jux_o)"));
            }
            self.w.push_str("; }");
        } else {
            // detach — pointer-identity against the named observer's Rc.
            self.w.push_str("{ let __jux_t = (");
            self.emit_expr(arg);
            self.w.push_str("); ");
            if ctor_inner {
                self.w.push_str("if let Some(v) = ");
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(
                    ".__obs_{prop}_{vec_kind}.as_mut() {{ v.retain(|o| !o.is_for(&__jux_t)); }}"
                ));
            } else {
                self.emit_observers_receiver(recv);
                self.w
                    .push_str(&format!(".__obs_{prop}_detach_{vec_kind}(&__jux_t)"));
            }
            self.w.push_str("; }");
        }
        self.emitting_format_arg = prev;
    }

    /// Emit the `crate::JuxObserver<…>` value an attach stores:
    ///
    /// - inline lambda → `Strong(<lambda>)` — nothing else would keep
    ///   it alive;
    /// - named observer variable → `Weak(Rc::downgrade(&<var>))` —
    ///   §P.2.3 weak-by-default, the owner's field is the strong ref;
    /// - shape 2 (3-arg, property reference) → a Strong adapter closure
    ///   holding the named observer WEAKLY and prepending the property
    ///   name. (The adapter itself out-lives a dropped owner as a tiny
    ///   no-op until the next prune — a known Phase-1 wrinkle.)
    fn emit_observer_value(&mut self, prop: &str, arg: &juxc_ast::Expr, arity: usize) {
        let inline = matches!(arg, juxc_ast::Expr::Lambda(_));
        if arity == 3 {
            if inline {
                self.w.push_str("{ let __jux_f3 = ");
                self.emit_expr(arg);
                self.w.push_str(&format!(
                    "; crate::JuxObserver::Strong(std::rc::Rc::new(move |__jux_o, __jux_n| {{ __jux_f3(\"{prop}\".to_string(), __jux_o, __jux_n); }})) }}"
                ));
            } else {
                self.w.push_str("{ let __jux_w = std::rc::Rc::downgrade(&(");
                self.emit_expr(arg);
                self.w.push_str(&format!(
                    ")); crate::JuxObserver::Strong(std::rc::Rc::new(move |__jux_o, __jux_n| {{ if let Some(__jux_f) = __jux_w.upgrade() {{ __jux_f(\"{prop}\".to_string(), __jux_o, __jux_n); }} }})) }}"
                ));
            }
            return;
        }
        if inline {
            self.w.push_str("crate::JuxObserver::Strong(");
            self.emit_expr(arg);
            self.w.push(')');
        } else {
            self.w
                .push_str("crate::JuxObserver::Weak(std::rc::Rc::downgrade(&(");
            self.emit_expr(arg);
            self.w.push_str(")))");
        }
    }

    /// Emit the receiver of a binding operation as an owned wrapper
    /// handle binding (`let __jux_bX = (<recv>).clone();`). The clone
    /// is the cheap `Rc` refcount bump — and keeps a `Path` receiver
    /// from being moved out of its local.
    fn emit_bind_recv_binding(&mut self, var: &str, recv: Option<&juxc_ast::Expr>) {
        self.w.push_str(&format!("let {var} = ("));
        self.emit_observers_receiver(recv);
        self.w.push_str(").clone(); ");
    }

    /// The Rust path of a class's wrapper type at the CURRENT emission
    /// site — crate-rooted for cross-package classes, bare otherwise.
    fn class_rust_path(&mut self, bare: &str) -> String {
        let local = self.symbols.classes.contains_key(bare);
        if !local {
            if let Some(fqn) = self.symbols.find_fqn_by_bare(bare) {
                if fqn.contains('.') {
                    return format!(
                        "crate::{}",
                        fqn.split('.').collect::<Vec<_>>().join("::")
                    );
                }
            }
        }
        bare.to_string()
    }

    /// The declared Jux type of class `class`'s property `prop`,
    /// rendered as a Rust type string.
    fn prop_rust_ty(&mut self, class: &str, prop: &str) -> Option<String> {
        let ty = self
            .class_ast_by_bare(class)?
            .properties
            .iter()
            .find(|p| p.name.text == prop)?
            .ty
            .clone();
        Some(self.render_type_to_string(&ty))
    }

    /// Emit `target.X.bind(source.Y)` (§P.4.2) / `bindBidirectional`
    /// (§P.4.3). One direction wires:
    ///
    /// 1. an immediate sync (`target.__set_X(source.Y())`),
    /// 2. a closure that re-sets the target on every source fire —
    ///    holding the target WEAKLY (a strong capture would leak the
    ///    target through its own keep-alive slot),
    /// 3. the closure stored strongly in the target's `__bind_X` slot
    ///    (so the binding lives exactly as long as the target), and
    /// 4. a weak attach of that closure on the source property.
    ///
    /// Bidirectional wires both directions; the take-during-fire
    /// re-entrancy guard in `__obs_*_fire` (plus `!=` change detection
    /// for comparable types) breaks the update cycle.
    pub(crate) fn emit_bind(
        &mut self,
        target: (Option<&juxc_ast::Expr>, &str, &str),
        source: (Option<&juxc_ast::Expr>, &str, &str),
        bidirectional: bool,
    ) {
        let (t_recv, t_prop, t_class) = target;
        let (s_recv, s_prop, s_class) = source;
        if self.observers_receiver_is_ctor_inner(t_recv)
            || self.observers_receiver_is_ctor_inner(s_recv)
        {
            // The enclosing instance is still a bare inner struct in a
            // constructor body — no wrapper handle exists to bind to.
            self.w.push_str(
                "compile_error!(\"jux: bind() on a property of `this` inside a constructor is not supported yet — bind from a method instead\")",
            );
            return;
        }
        let s_ty = self
            .prop_rust_ty(s_class, s_prop)
            .unwrap_or_else(|| "()".to_string());
        let t_ty = self
            .prop_rust_ty(t_class, t_prop)
            .unwrap_or_else(|| "()".to_string());
        let t_path = self.class_rust_path(t_class);
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = false;
        self.w.push_str("{ ");
        self.emit_bind_recv_binding("__jux_bt", t_recv);
        self.emit_bind_recv_binding("__jux_bs", s_recv);
        // 1 — initial sync: target takes the source's current value.
        self.w
            .push_str(&format!("__jux_bt.__set_{t_prop}(__jux_bs.{s_prop}()); "));
        // 2/3/4 — source → target direction.
        self.w.push_str(&format!(
            "let __jux_tw = __jux_bt.__jux_weak(); \
             let __jux_f: std::rc::Rc<dyn Fn({s_ty}, {s_ty})> = std::rc::Rc::new(move |_, __jux_n| {{ \
             if let Some(__jux_rc) = __jux_tw.upgrade() {{ {t_path}::__jux_from_inner(__jux_rc).__set_{t_prop}(__jux_n); }} }}); \
             __jux_bt.__bind_{t_prop}_store(Some(crate::JuxObserver::Strong(__jux_f.clone()))); \
             __jux_bs.__obs_{s_prop}_attach_full(crate::JuxObserver::Weak(std::rc::Rc::downgrade(&__jux_f))); "
        ));
        if bidirectional {
            // target → source direction, same wiring mirrored.
            let s_path = self.class_rust_path(s_class);
            self.w.push_str(&format!(
                "let __jux_sw = __jux_bs.__jux_weak(); \
                 let __jux_g: std::rc::Rc<dyn Fn({t_ty}, {t_ty})> = std::rc::Rc::new(move |_, __jux_n| {{ \
                 if let Some(__jux_rc) = __jux_sw.upgrade() {{ {s_path}::__jux_from_inner(__jux_rc).__set_{s_prop}(__jux_n); }} }}); \
                 __jux_bs.__bind_{s_prop}_store(Some(crate::JuxObserver::Strong(__jux_g.clone()))); \
                 __jux_bt.__obs_{t_prop}_attach_full(crate::JuxObserver::Weak(std::rc::Rc::downgrade(&__jux_g))); "
            ));
        }
        self.w.push('}');
        self.emitting_format_arg = prev;
    }

    /// Emit `prop.unbind()` (§P.4.4) — drop the keep-alive; the weak
    /// observer left at the source dies with it and is pruned on the
    /// source's next fire. Safe when not bound (`None` → `None`).
    pub(crate) fn emit_unbind(&mut self, recv: Option<&juxc_ast::Expr>, prop: &str) {
        if self.observers_receiver_is_ctor_inner(recv) {
            self.emit_observers_receiver(recv);
            self.w.push_str(&format!(".__bind_{prop} = None"));
            return;
        }
        self.emit_observers_receiver(recv);
        self.w.push_str(&format!(".__bind_{prop}_store(None)"));
    }

    /// Emit a `.observers.clear` / `.observers.size` read (§P.3.2 —
    /// command accessors, no parentheses in Jux).
    pub(crate) fn emit_observers_command(
        &mut self,
        recv: Option<&juxc_ast::Expr>,
        prop: &str,
        op: &str,
    ) {
        let ctor_inner = self.observers_receiver_is_ctor_inner(recv);
        if op == "clear" {
            if ctor_inner {
                self.w.push_str("{ ");
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(".__obs_{prop}_full = None; "));
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(".__obs_{prop}_inv = None; }}"));
            } else {
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(".__obs_{prop}_clear()"));
            }
        } else {
            // size
            if ctor_inner {
                self.w.push_str("((");
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(
                    ".__obs_{prop}_full.as_ref().map(|v| v.iter().filter(|o| o.upgrade().is_some()).count()).unwrap_or(0) + "
                ));
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(
                    ".__obs_{prop}_inv.as_ref().map(|v| v.iter().filter(|o| o.upgrade().is_some()).count()).unwrap_or(0)) as isize)"
                ));
            } else {
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(".__obs_{prop}_size()"));
            }
        }
    }
}
