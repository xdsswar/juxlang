//! Lowering the erased part of a program (ERRATA E141; the plan is
//! `juxc_tycheck::erasure`).
//!
//! The functions, methods and classes on a polymorphic-recursion cycle are
//! instantiated at one type, `crate::JuxErased`, for every type parameter:
//!
//! - a use of an erased class (`Nested<Pair<T>>`, `new Flat<T>(x)`) names it
//!   at `JuxErased`, except inside the class's own body at its own
//!   parameters, which is the generic code every instantiation runs;
//! - a call of an erased function or method names its type arguments as
//!   `JuxErased`;
//! - a value is boxed (`__jux_erase!`) where it fills a slot declared as one
//!   of those type parameters, and unboxed (`.get::<X>()`, `X` being the type
//!   the checker gave the expression) where it is read out of one.
//!
//! Boxing a value that already is a `JuxErased` keeps it, and unboxing at
//! `JuxErased` gives it back: generic code, whose `T` may be either, boxes
//! and unboxes without knowing which.

use juxc_ast::{CallExpr, Expr, FieldExpr, GenericArg, TypeRef};
use juxc_source::Span;

use crate::RustEmitter;

/// What a value boxed into an erased slot is taken as: the slot's type
/// where the site wrote it, and the bounds of the slot's type parameter,
/// each kept as its dispatch object (ERRATA E142).
#[derive(Clone, Default)]
pub(crate) struct EraseMark {
    pub(crate) written: Option<TypeRef>,
    pub(crate) bounds: Vec<TypeRef>,
    /// The declaration's type parameters, and the one the slot is, for a
    /// bound at them (`T extends Ranked<T>`, ERRATA E1XX-GAP39i).
    pub(crate) decl_params: Vec<String>,
    pub(crate) param: String,
}

/// The key an expression is marked by for boxing: its span, or, for one
/// with no span of its own (a literal), the node itself.
pub(crate) type EraseKey = (Span, usize);

/// The Rust path of the erased type.
pub(crate) const ERASED_RUST: &str = "crate::JuxErased";

impl RustEmitter {
    /// The key `expr` is marked by (see [`EraseKey`]).
    pub(crate) fn erase_key(expr: &Expr) -> EraseKey {
        let span = crate::exprs::expr_span_of(expr);
        if span == Span::DUMMY {
            (span, expr as *const Expr as usize)
        } else {
            (span, 0)
        }
    }

    /// Whether any part of the program is erased.
    pub(crate) fn erasure_active(&self) -> bool {
        !self.symbols.erasure.classes.is_empty() || !self.symbols.erasure.fns.is_empty()
    }

    /// The marker type the checker's instantiation sets use.
    pub(crate) fn is_erased_marker(ty: &TypeRef) -> bool {
        ty.generic_args.is_empty()
            && ty.name.segments.len() == 1
            && ty.name.segments[0].text == juxc_tycheck::erasure::ERASED_TYPE
    }

    fn marker(span: Span) -> TypeRef {
        crate::analysis::synth_iface_type_ref(juxc_tycheck::erasure::ERASED_TYPE, span)
    }

    /// The erased class, record or interface `name` (bare or FQN) means.
    pub(crate) fn erased_class_key(&self, name: &str) -> Option<String> {
        let erased = &self.symbols.erasure.classes;
        if erased.is_empty() {
            return None;
        }
        if erased.contains(name) {
            return Some(name.to_string());
        }
        let resolved = self.resolve_bare_type_fqn(name);
        if let Some(fqn) = resolved.as_ref().filter(|f| erased.contains(*f)) {
            return Some(fqn.clone());
        }
        if resolved.is_some() {
            return None;
        }
        let bare = name.rsplit('.').next().unwrap_or(name);
        erased.iter().find(|k| k.rsplit('.').next() == Some(bare)).cloned()
    }

    /// The type parameter names of the class, record or interface `fqn`.
    fn erased_class_params(&self, fqn: &str) -> Vec<String> {
        let ps = self
            .symbols
            .classes
            .get(fqn)
            .map(|c| &c.generic_params)
            .or_else(|| self.symbols.records.get(fqn).map(|r| &r.generic_params))
            .or_else(|| self.symbols.interfaces.get(fqn).map(|i| &i.generic_params));
        ps.map(|ps| ps.iter().filter(|p| !p.is_const()).map(|p| p.name.text.clone()).collect()).unwrap_or_default()
    }

    /// Every parameter of the class, record or interface `fqn`, and whether it
    /// is a `const` one (kept as it is, ERRATA E1XX-GAP39i).
    fn erased_class_all_params(&self, fqn: &str) -> Vec<(String, bool)> {
        let ps = self
            .symbols
            .classes
            .get(fqn)
            .map(|c| &c.generic_params)
            .or_else(|| self.symbols.records.get(fqn).map(|r| &r.generic_params))
            .or_else(|| self.symbols.interfaces.get(fqn).map(|i| &i.generic_params));
        ps.map(|ps| ps.iter().map(|p| (p.name.text.clone(), p.is_const())).collect()).unwrap_or_default()
    }

    /// Whether `args` name the erased class `fqn` at its own parameters inside
    /// its own body: the generic code, not a use of it.
    fn in_own_generic_body(&self, fqn: &str, args: &[TypeRef]) -> bool {
        let bare = fqn.rsplit('.').next().unwrap_or(fqn);
        let enclosing = self.enclosing_class.as_deref().map(|c| c.rsplit('.').next().unwrap_or(c));
        if enclosing != Some(bare) {
            return false;
        }
        let params: Vec<String> = self.erased_class_all_params(fqn).into_iter().map(|(n, _)| n).collect();
        params.len() == args.len()
            && params.iter().zip(args).all(|(p, a)| {
                a.generic_args.is_empty() && a.name.segments.len() == 1 && a.name.segments[0].text == *p && !a.nullable
            })
    }

    /// `ty` as it is written at a use of an erased class: every type argument
    /// `JuxErased`. `None` when `ty` is not such a use.
    pub(crate) fn erased_type_ref(&self, ty: &TypeRef) -> Option<TypeRef> {
        if ty.generic_args.is_empty() || ty.fn_shape.is_some() || !self.erasure_active() {
            return None;
        }
        let name = ty.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
        let key = self.erased_class_key(&name)?;
        let args: Vec<TypeRef> = ty.generic_args.iter().filter_map(|a| a.as_type().cloned()).collect();
        if args.len() != ty.generic_args.len() || args.iter().all(Self::is_erased_marker) {
            return None;
        }
        if self.in_own_generic_body(&key, &args) {
            return None;
        }
        // A `const` argument is kept: a `const` parameter is not erased.
        let consts: Vec<bool> = self.erased_class_all_params(&key).into_iter().map(|(_, c)| c).collect();
        if args.iter().enumerate().all(|(i, a)| consts.get(i).copied().unwrap_or(false) || Self::is_erased_marker(a)) {
            return None;
        }
        let mut out = ty.clone();
        out.generic_args = args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                if consts.get(i).copied().unwrap_or(false) {
                    GenericArg::Type(a.clone())
                } else {
                    GenericArg::Type(Self::marker(a.span))
                }
            })
            .collect();
        Some(out)
    }

    /// The erased function or method a call reaches, with the names of the
    /// type parameters its declaration erases, and whether its receiver is
    /// an erased class seen from outside (so the class's parameters are
    /// erased too).
    fn erased_callee(&self, call: &CallExpr) -> Option<ErasedCallee> {
        if !self.erasure_active() {
            return None;
        }
        match call.callee.as_ref() {
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = &qn.segments[0].text;
                if let Some((fqn, sig)) = self.lookup_function_here(name) {
                    if !self.symbols.erasure.function(fqn) {
                        return None;
                    }
                    return Some(ErasedCallee {
                        erased: sig.generic_params.iter().map(|p| p.name.text.clone()).collect(),
                        own: sig.generic_params.iter().map(|p| p.name.text.clone()).collect(),
                        params: sig.params.iter().map(|p| p.ty.clone()).collect(),
                        ret: match &sig.return_type {
                            juxc_ast::ReturnType::Type(t) => Some(t.clone()),
                            _ => None,
                        },
                        fn_erased: true,
                        bounds: sig.generic_params.iter().map(|p| (p.name.text.clone(), p.bounds.clone())).collect(),
                    });
                }
                // A method of the enclosing class, called on `this`.
                let class = self.enclosing_class.clone()?;
                let fqn = self.resolve_bare_class_fqn(&class).unwrap_or(class);
                let (sig, decl) = self.symbols.lookup_method(&fqn, name)?;
                if !self.symbols.erasure.method(decl, name) {
                    return None;
                }
                Some(ErasedCallee {
                    erased: sig.generic_params.iter().map(|p| p.name.text.clone()).collect(),
                    own: sig.generic_params.iter().map(|p| p.name.text.clone()).collect(),
                    params: sig.params.iter().map(|p| p.ty.clone()).collect(),
                    ret: match &sig.return_type {
                        juxc_ast::ReturnType::Type(t) => Some(t.clone()),
                        _ => None,
                    },
                    fn_erased: true,
                    bounds: sig.generic_params.iter().map(|p| (p.name.text.clone(), p.bounds.clone())).collect(),
                })
            }
            Expr::Field(f) => {
                let (class, from_outside) = self.erased_receiver(&f.object)?;
                let (sig, decl) = self.symbols.lookup_method(&class, &f.field.text)?;
                let method_erased = self.symbols.erasure.method(decl, &f.field.text);
                let class_erased = from_outside && self.symbols.erasure.class(decl);
                if !method_erased && !class_erased {
                    return None;
                }
                let mut erased: Vec<String> = Vec::new();
                if method_erased {
                    erased.extend(sig.generic_params.iter().map(|p| p.name.text.clone()));
                }
                if class_erased {
                    erased.extend(self.erased_class_params(decl));
                }
                Some(ErasedCallee {
                    erased,
                    own: sig.generic_params.iter().map(|p| p.name.text.clone()).collect(),
                    params: sig.params.iter().map(|p| p.ty.clone()).collect(),
                    ret: match &sig.return_type {
                        juxc_ast::ReturnType::Type(t) => Some(t.clone()),
                        _ => None,
                    },
                    fn_erased: method_erased,
                    bounds: {
                        let mut b: std::collections::HashMap<String, Vec<TypeRef>> =
                            sig.generic_params.iter().map(|p| (p.name.text.clone(), p.bounds.clone())).collect();
                        if class_erased {
                            let ps = self.symbols.classes.get(decl).map(|c| c.generic_params.clone()).unwrap_or_default();
                            b.extend(ps.into_iter().map(|p| (p.name.text.clone(), p.bounds)));
                        }
                        b
                    },
                })
            }
            _ => None,
        }
    }

    /// The class of a receiver, and whether it is an erased class seen from
    /// outside its own generic body.
    fn erased_receiver(&self, recv: &Expr) -> Option<(String, bool)> {
        if matches!(recv, Expr::This(_) | Expr::Super(_)) {
            let class = self.enclosing_class.clone()?;
            return Some((self.resolve_bare_class_fqn(&class).unwrap_or(class), false));
        }
        let ty = self.expr_types.get(&crate::exprs::expr_span_of(recv))?;
        let ty = match ty {
            juxc_tycheck::Ty::Nullable(inner) => inner.as_ref(),
            other => other,
        };
        let juxc_tycheck::Ty::User { name, generic_args } = ty else { return None };
        let outside = match self.erased_class_key(name) {
            Some(key) => {
                let args: Vec<TypeRef> = generic_args.iter().filter_map(crate::analysis::ty_to_type_ref).collect();
                !self.in_own_generic_body(&key, &args)
            }
            None => false,
        };
        Some((name.clone(), outside))
    }

    /// Whether the explicit type arguments of `call` are erased.
    pub(crate) fn call_type_args_erased(&self, call: &CallExpr) -> bool {
        self.erased_callee(call).is_some_and(|c| c.fn_erased)
    }

    /// Mark the arguments of `call` that fill an erased slot, so `emit_expr`
    /// boxes them.
    pub(crate) fn mark_erased_call_args(&mut self, call: &CallExpr) {
        let Some(callee) = self.erased_callee(call) else { return };
        for (i, arg) in call.args.iter().enumerate() {
            let Some(t) = callee.params.get(i).filter(|t| is_bare_param(t, &callee.erased)) else { continue };
            // What the slot holds here, when the call says (`f<int>(7)`).
            let written = if callee.fn_erased {
                callee
                    .own
                    .iter()
                    .position(|p| *p == t.name.segments[0].text)
                    .and_then(|k| call.explicit_generic_args.get(k).cloned())
            } else {
                None
            };
            let bounds = callee.bounds.get(&t.name.segments[0].text).cloned().unwrap_or_default();
            let decl_params: Vec<String> = callee.bounds.keys().cloned().collect();
            let param = t.name.segments[0].text.clone();
            self.erase_on_emit.insert(Self::erase_key(arg), EraseMark { written, bounds, decl_params, param });
        }
    }

    /// Mark the arguments of `new C<..>(..)` that fill a slot declared as one
    /// of an erased class's type parameters.
    pub(crate) fn mark_erased_ctor_args(&mut self, n: &juxc_ast::NewObjectExpr) {
        if !self.erasure_active() || n.anonymous_body.is_some() {
            return;
        }
        let name = n.class_name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
        let Some(key) = self.erased_class_key(&name) else { return };
        if !n.generic_args.is_empty() && self.in_own_generic_body(&key, &n.generic_args) {
            return;
        }
        if n.generic_args.is_empty()
            && self.enclosing_class.as_deref().map(|c| c.rsplit('.').next().unwrap_or(c)) == key.rsplit('.').next()
        {
            // `new C(..)` inferred inside C's own body: its own parameters.
            return;
        }
        let params = self.erased_class_params(&key);
        let ctor_params: Vec<TypeRef> = self
            .symbols
            .classes
            .get(&key)
            .and_then(|c| {
                let pick = self.symbols.ctor_selections.get(&n.span).copied();
                pick.and_then(|k| c.constructors.get(k))
                    .or_else(|| c.constructors.iter().find(|ct| ct.params.len() == n.args.len()))
                    .map(|ct| ct.params.iter().map(|p| p.ty.clone()).collect())
            })
            .or_else(|| self.symbols.records.get(&key).map(|r| r.components.iter().map(|c| c.ty.clone()).collect()))
            .unwrap_or_default();
        for (i, arg) in n.args.iter().enumerate() {
            let Some(t) = ctor_params.get(i).filter(|t| is_bare_param(t, &params)) else { continue };
            // What the slot holds here, when the `new` says (`new Box<int>(7)`).
            let written = params
                .iter()
                .position(|p| *p == t.name.segments[0].text)
                .and_then(|k| n.generic_args.get(k).cloned());
            let bounds = self.erased_class_param_bounds(&key, &t.name.segments[0].text);
            let param = t.name.segments[0].text.clone();
            self.erase_on_emit
                .insert(Self::erase_key(arg), EraseMark { written, bounds, decl_params: params.clone(), param });
        }
    }

    /// Emit the boxing of `expr` into an erased slot (`at` is the type the
    /// value is taken at). An optional value keeps its `null`: the value
    /// inside is boxed.
    pub(crate) fn emit_erased_box(&mut self, expr: &Expr, at: Option<TypeRef>, mark: &EraseMark) {
        if matches!(expr, Expr::Literal(juxc_ast::Literal::Null)) {
            self.emit_expr(expr);
            return;
        }
        let nullable = at.as_ref().is_some_and(|t| t.nullable)
            || matches!(
                self.expr_types.get(&crate::exprs::expr_span_of(expr)),
                Some(juxc_tycheck::Ty::Nullable(_))
            );
        if nullable {
            self.w.push('(');
            self.emit_expr(expr);
            self.w.push_str(").map(|__jux_ev| crate::__jux_erase!(__jux_ev");
            if let Some(mut t) = at {
                t.nullable = false;
                self.w.push_str(", ");
                self.emit_value_type_as_rust(&t);
                self.emit_erased_views(mark, Some(&t));
            }
            self.w.push_str("))");
            return;
        }
        self.w.push_str("crate::__jux_erase!(");
        self.emit_expr(expr);
        match at {
            Some(t) => {
                self.w.push_str(", ");
                self.emit_value_type_as_rust(&t);
                self.emit_erased_views(mark, Some(&t));
            }
            None if !mark.bounds.is_empty() => {
                self.w.push_str(", _");
                self.emit_erased_views(mark, None);
            }
            None => {}
        }
        self.w.push(')');
    }

    /// `; Rc<dyn B>, ..`: the dispatch objects a bounded erased value keeps,
    /// one per bound and per supertrait of one.
    fn emit_erased_views(&mut self, mark: &EraseMark, at: Option<&TypeRef>) {
        // A bound at the declaration's parameters (`Ranked<T>`) is kept
        // through an adapter; any other bound is the value itself.
        let (adapted, plain): (Vec<TypeRef>, Vec<TypeRef>) =
            mark.bounds.iter().cloned().partition(|b| names_any(b, &mark.decl_params));
        let views = self.erased_view_traits(&plain);
        if views.is_empty() && adapted.is_empty() {
            return;
        }
        if adapted.is_empty() {
            self.w.push_str("; ");
            for (i, v) in views.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.w.push_str("std::rc::Rc<dyn ");
                self.emit_bound_type(v);
                self.w.push('>');
            }
            return;
        }
        self.w.push_str("; [");
        for (i, v) in views.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str("std::rc::Rc<dyn ");
            self.emit_bound_type(v);
            self.w.push('>');
        }
        self.w.push_str("]; [");
        for (i, b) in adapted.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            let args: Vec<TypeRef> = b.generic_args.iter().filter_map(|a| a.as_type().cloned()).collect();
            let pattern: Vec<bool> = args.iter().map(|a| is_bare_param(a, &mark.decl_params) && !a.nullable).collect();
            let erased_bound = erase_params(b, &mark.decl_params);
            self.w.push_str("std::rc::Rc<dyn ");
            self.emit_bound_type(&erased_bound);
            self.w.push_str("> => crate::");
            let iface = b.name.segments.last().map(|s| s.text.clone()).unwrap_or_default();
            self.w.push_str(&adapter_name(&iface, &pattern));
            self.w.push_str("::<_");
            for (j, a) in args.iter().enumerate() {
                self.w.push_str(", ");
                if pattern[j] {
                    let own = a.name.segments[0].text == mark.param;
                    match at {
                        Some(t) if own => self.emit_value_type_as_rust(t),
                        _ => self.w.push('_'),
                    }
                } else {
                    let e = erase_params(a, &mark.decl_params);
                    self.emit_value_type_as_rust(&e);
                }
            }
            self.w.push('>');
        }
        self.w.push(']');
    }

    /// The adapter shapes an interface needs: one per way the family's
    /// bounds name it at their parameters, each position erased (a bare
    /// parameter) or kept.
    pub(crate) fn erased_adapter_patterns(&self, iface_bare: &str) -> Vec<Vec<bool>> {
        let mut out: Vec<Vec<bool>> = Vec::new();
        if !self.erasure_active() {
            return out;
        }
        let e = &self.symbols.erasure;
        let mut decls: Vec<Vec<juxc_ast::TypeParam>> = Vec::new();
        for c in &e.classes {
            if let Some(ps) = self
                .symbols
                .classes
                .get(c)
                .map(|x| x.generic_params.clone())
                .or_else(|| self.symbols.records.get(c).map(|r| r.generic_params.clone()))
            {
                decls.push(ps);
            }
        }
        for k in &e.fns {
            let mut ps = if let Some(f) = k.strip_prefix("fn:") {
                self.symbols.functions.get(f).map(|s| s.generic_params.clone()).unwrap_or_default()
            } else if let Some((class, m)) = k.strip_prefix("method:").and_then(|m| m.rsplit_once('.')) {
                self.symbols.lookup_method(class, m).map(|(s, _)| s.generic_params.clone()).unwrap_or_default()
            } else {
                Vec::new()
            };
            if let Some((class, _)) = k.strip_prefix("method:").and_then(|m| m.rsplit_once('.')) {
                ps.extend(self.symbols.classes.get(class).map(|c| c.generic_params.clone()).unwrap_or_default());
            }
            decls.push(ps);
        }
        for ps in decls {
            let names: Vec<String> = ps.iter().map(|p| p.name.text.clone()).collect();
            for p in &ps {
                for b in &p.bounds {
                    if b.name.segments.last().map(|s| s.text.as_str()) != Some(iface_bare) || !names_any(b, &names) {
                        continue;
                    }
                    let pattern: Vec<bool> = b
                        .generic_args
                        .iter()
                        .filter_map(|a| a.as_type())
                        .map(|a| is_bare_param(a, &names) && !a.nullable)
                        .collect();
                    if !out.contains(&pattern) {
                        out.push(pattern);
                    }
                }
            }
        }
        out
    }

    /// The adapters an erased value reaches an interface bound at its own
    /// parameters through (ERRATA E1XX-GAP39i): `I<JuxErased, ..>`,
    /// implemented by wrapping the value's own `I<X, ..>`. An erased argument
    /// is unboxed to `X` on the way in, and a result at an erased position is
    /// boxed on the way out with the value's own table when it is of the
    /// value's type, so it keeps its bounds.
    pub(crate) fn emit_erased_adapters(&mut self, interface: &juxc_ast::InterfaceDecl) {
        let patterns = self.erased_adapter_patterns(&interface.name.text);
        for pattern in patterns {
            self.emit_erased_adapter(interface, &pattern);
        }
    }

    /// An adapter's struct, and what every trait an adapter implements asks
    /// of it: it prints, and is the object, as the value it wraps.
    fn emit_erased_adapter_struct(&mut self, name: &str, ps: &[String]) {
        let pgen = ps.join(", ");
        let phantom = format!("({})", ps.iter().map(|p| format!("{p},")).collect::<String>());
        self.w.line(&format!(
            "pub struct {name}<__JuxV, {pgen}>(pub __JuxV, pub std::marker::PhantomData<{phantom}>, pub std::rc::Rc<crate::JuxErasedVtable>);"
        ));
        for (tr, body) in [
            ("std::fmt::Debug", "fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { std::fmt::Debug::fmt(&self.0, f) }"),
            ("std::fmt::Display", "fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { std::fmt::Display::fmt(&self.0, f) }"),
            ("crate::JuxIdentity", "fn __jux_identity(&self) -> *const () { crate::JuxIdentity::__jux_identity(&self.0) }"),
        ] {
            self.w.line(&format!("impl<__JuxV: {tr}, {pgen}> {tr} for {name}<__JuxV, {pgen}> {{ {body} }}"));
        }
    }

    /// The adapters an erased value reaches a class bound at its own
    /// parameters through (`T extends Shape<T>`, ERRATA E1XX-GAP39i), made
    /// from the class's `Kind` forwarding implementation (`text`): the same
    /// members, erased positions unboxed on the way in and reboxed on the
    /// way out. `None` when a member is not in the one-line forwarding form
    /// (a method with type parameters of its own).
    pub(crate) fn erased_kind_adapters(&mut self, class_decl: &juxc_ast::ClassDecl, text: &str) {
        let patterns = self.erased_adapter_patterns(&class_decl.name.text);
        let params: Vec<String> = class_decl.generic_params.iter().map(|p| p.name.text.clone()).collect();
        for pattern in patterns {
            if pattern.len() != params.len() {
                continue;
            }
            let name = adapter_name(&class_decl.name.text, &pattern);
            let ps: Vec<String> = (0..pattern.len()).map(|j| format!("__JuxP{j}")).collect();
            let Some(adapter) = kind_adapter_from_forwarding(text, &params, &pattern, &name, &ps) else { continue };
            self.emit_erased_adapter_struct(&name, &ps);
            self.w.push_str(&adapter);
        }
    }

    fn emit_erased_adapter(&mut self, interface: &juxc_ast::InterfaceDecl, pattern: &[bool]) {
        use juxc_lex::to_rust_ident;
        let iface = to_rust_ident(&interface.name.text);
        let name = adapter_name(&interface.name.text, pattern);
        let ps: Vec<String> = (0..pattern.len()).map(|j| format!("__JuxP{j}")).collect();
        let iface_params: Vec<String> = interface.generic_params.iter().map(|p| p.name.text.clone()).collect();
        let pgen = ps.join(", ");
        self.emit_erased_adapter_struct(&name, &ps);
        let view_args = ps.join(", ");
        let erased_args: Vec<String> = pattern
            .iter()
            .enumerate()
            .map(|(j, e)| if *e { crate::erasure::ERASED_RUST.to_string() } else { ps[j].clone() })
            .collect();
        let pbounds = ps.iter().map(|p| format!("{p}: Clone + std::fmt::Debug + 'static")).collect::<Vec<_>>().join(", ");
        self.w.line(&format!(
            "impl<__JuxV: {iface}<{view_args}> + Clone + 'static, {pbounds}> {iface}<{}> for {name}<__JuxV, {pgen}> {{",
            erased_args.join(", ")
        ));
        self.w.indent_inc();
        // The interface's own parameters read as the adapter's: erased
        // positions as `JuxErased`, kept ones as themselves.
        let mut subst: std::collections::HashMap<String, TypeRef> = std::collections::HashMap::new();
        for (j, p) in iface_params.iter().enumerate() {
            let t = if pattern.get(j).copied().unwrap_or(false) {
                Self::marker(interface.name.span)
            } else {
                crate::analysis::synth_iface_type_ref(&ps[j], interface.name.span)
            };
            subst.insert(p.clone(), t);
        }
        let saved = std::mem::replace(&mut self.kind_type_subst, subst);
        let methods: Vec<juxc_ast::FnDecl> = interface
            .methods
            .iter()
            .filter(|m| !m.modifiers.iter().any(|x| matches!(x, juxc_ast::FnModifier::Static)))
            .filter(|m| m.generic_params.is_empty() || m.body.is_none())
            .cloned()
            .collect();
        for m in &methods {
            self.w.emit_indent();
            self.w.push_str("fn ");
            self.w.push_str(&to_rust_ident(&m.name.text));
            if !m.generic_params.is_empty() {
                let own = crate::decls::classes::bounds_into_outer_params(&m.generic_params, &iface_params);
                self.emit_method_own_generics(&own);
            }
            self.w.push_str("(&self");
            for p in &m.params {
                self.w.push_str(", ");
                self.w.push_str(&to_rust_ident(&p.name.text));
                self.w.push_str(": ");
                self.emit_value_type_as_rust(&p.ty);
            }
            self.w.push(')');
            let ret_pos = match &m.return_type {
                juxc_ast::ReturnType::Type(t) => {
                    self.w.push_str(" -> ");
                    self.emit_return_type_as_rust(t);
                    erased_position(t, &iface_params, pattern)
                }
                _ => None,
            };
            self.w.push_str(" { ");
            if ret_pos.is_some() {
                self.w.push_str("crate::jux_erased_rebox(");
            }
            self.w.push_str(&format!("<__JuxV as {iface}<{view_args}>>::"));
            self.w.push_str(&to_rust_ident(&m.name.text));
            self.w.push_str("(&self.0");
            for p in &m.params {
                self.w.push_str(", ");
                self.w.push_str(&to_rust_ident(&p.name.text));
                if let Some(j) = erased_position(&p.ty, &iface_params, pattern) {
                    self.w.push_str(&format!(".get::<{}>()", ps[j]));
                }
            }
            self.w.push(')');
            if ret_pos.is_some() {
                self.w.push_str(", &self.2)");
            }
            self.w.push_str(" }\n");
        }
        self.kind_type_subst = saved;
        self.w.indent_dec();
        self.w.line("}");
    }

    /// `bounds` and every Jux supertrait of each (an interface's `extends`, a
    /// class's ancestors and the interfaces it implements), each once.
    pub(crate) fn erased_view_traits(&self, bounds: &[TypeRef]) -> Vec<TypeRef> {
        let mut out: Vec<TypeRef> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut stack: Vec<TypeRef> = bounds.to_vec();
        while let Some(b) = stack.pop() {
            let bare = b.name.segments.last().map(|s| s.text.clone()).unwrap_or_default();
            if !seen.insert(bare.clone()) {
                continue;
            }
            if let Some((_, iface)) = self.lookup_interface_by_bare_or_fqn(&bare) {
                stack.extend(iface.extends.iter().filter(|t| t.generic_args.is_empty()).cloned());
            } else if let Some(fqn) = self.resolve_bare_class_fqn(&bare) {
                if let Some(c) = self.symbols.classes.get(&fqn) {
                    if let Some(p) = &c.extends_fqn {
                        let pb = p.rsplit('.').next().unwrap_or(p);
                        stack.push(crate::analysis::synth_iface_type_ref(pb, b.span));
                    }
                    stack.extend(c.implements.iter().filter(|t| t.generic_args.is_empty()).cloned());
                }
            }
            out.push(b);
        }
        out
    }

    /// The bounds of the type parameter `param` of the erased class `key`.
    fn erased_class_param_bounds(&self, key: &str, param: &str) -> Vec<TypeRef> {
        self.symbols
            .classes
            .get(key)
            .map(|c| &c.generic_params)
            .or_else(|| self.symbols.records.get(key).map(|r| &r.generic_params))
            .and_then(|ps| ps.iter().find(|p| p.name.text == param))
            .map(|p| p.bounds.clone())
            .unwrap_or_default()
    }

    /// The bounds of the type parameter an erased field slot is declared as.
    pub(crate) fn erased_field_bounds(&self, f: &FieldExpr) -> Vec<TypeRef> {
        let Some((class, _)) = self.erased_receiver(&f.object) else { return Vec::new() };
        let Some((field, decl)) = self.symbols.lookup_field(&class, &f.field.text) else { return Vec::new() };
        let Some(p) = field.ty.name.segments.first().map(|s| s.text.clone()) else { return Vec::new() };
        let decl = decl.to_string();
        self.erased_class_param_bounds(&decl, &p)
    }

    /// Whether the trait named `bare` (an interface, or a class's `Kind`) is
    /// a bound an erased type parameter has, or a supertrait of one: the
    /// erased type implements it by dispatching through the object it keeps
    /// (ERRATA E142).
    pub(crate) fn erased_bound_trait(&self, bare: &str) -> bool {
        if !self.erasure_active() {
            return false;
        }
        let mut bounds: Vec<TypeRef> = Vec::new();
        let e = &self.symbols.erasure;
        for c in &e.classes {
            let ps = self
                .symbols
                .classes
                .get(c)
                .map(|x| x.generic_params.clone())
                .or_else(|| self.symbols.records.get(c).map(|r| r.generic_params.clone()))
                .or_else(|| self.symbols.interfaces.get(c).map(|i| i.generic_params.clone()))
                .unwrap_or_default();
            bounds.extend(ps.into_iter().flat_map(|p| p.bounds));
        }
        for k in &e.fns {
            let ps = if let Some(f) = k.strip_prefix("fn:") {
                self.symbols.functions.get(f).map(|s| s.generic_params.clone()).unwrap_or_default()
            } else if let Some((class, m)) = k.strip_prefix("method:").and_then(|m| m.rsplit_once('.')) {
                self.symbols.lookup_method(class, m).map(|(s, _)| s.generic_params.clone()).unwrap_or_default()
            } else {
                Vec::new()
            };
            bounds.extend(ps.into_iter().flat_map(|p| p.bounds));
        }
        self.erased_view_traits(&bounds)
            .iter()
            .any(|t| t.name.segments.last().is_some_and(|s| s.text == bare))
    }

    /// The erased type's implementation of a trait, made from the `Rc`
    /// forwarding implementation of it (`text`): the same members, each
    /// forwarding to the dispatch object the erased value keeps for the trait
    /// instead of to the value behind the `Rc`.
    pub(crate) fn erased_twin_of_forwarding_impl(text: &str) -> Option<String> {
        let start = text.find("impl<__JuxH:")?;
        let head_end = text[start..].find(" for std::rc::Rc<__JuxH> {")? + start;
        // `impl<...>` generics, bracket-matched.
        let gen_open = start + "impl".len();
        let mut depth = 0i32;
        let mut gen_close = None;
        for (i, ch) in text[gen_open..head_end].char_indices() {
            match ch {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        gen_close = Some(gen_open + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let gen_close = gen_close?;
        let generics = &text[gen_open + 1..gen_close];
        let sig = text[gen_close + 1..head_end].trim();
        // Drop the first parameter, `__JuxH: ?Sized + ..`.
        let mut depth = 0i32;
        let mut rest = "";
        for (i, ch) in generics.char_indices() {
            match ch {
                '<' | '(' => depth += 1,
                '>' | ')' => depth -= 1,
                ',' if depth == 0 => {
                    rest = generics[i + 1..].trim();
                    break;
                }
                _ => {}
            }
        }
        // A trait whose parameter is bounded by the trait itself (`Shape<T
        // extends Shape<T>>`): a generic implementation for the erased type
        // would ask itself to hold (rustc E0275), so it implements the one
        // instantiation erasure uses, at the erased type (ERRATA
        // E1XX-GAP39i).
        let head_name = sig.split('<').next().unwrap_or(sig).trim().to_string();
        let rest_parts = split_top(rest);
        let self_bounded = rest_parts.iter().any(|g| {
            g.split_once(':').is_some_and(|(_, b)| replace_words(b, std::slice::from_ref(&head_name), &["\u{0}".to_string()]).contains('\u{0}'))
        });
        let (sig, rest, body_text) = if self_bounded {
            let names: Vec<String> = rest_parts
                .iter()
                .filter_map(|g| g.split_once(':').map(|(n, _)| n.trim().to_string()))
                .collect();
            let erased: Vec<String> = names.iter().map(|_| crate::erasure::ERASED_RUST.to_string()).collect();
            let body = &text[head_end + " for std::rc::Rc<__JuxH> {".len()..];
            (replace_words(sig, &names, &erased), String::new(), replace_words(body, &names, &erased))
        } else {
            (sig.to_string(), rest.to_string(), text[head_end + " for std::rc::Rc<__JuxH> {".len()..].to_string())
        };
        let view = format!("*self.jux_view::<std::rc::Rc<dyn {sig}>>()");
        let body = body_text.replace("**self", &view).replace("__JuxH", &format!("dyn {sig}"));
        let indent = &text[..start];
        let head = if rest.is_empty() {
            format!("{indent}impl {sig} for crate::JuxErased {{")
        } else {
            format!("{indent}impl<{rest}> {sig} for crate::JuxErased {{")
        };
        Some(format!("{head}{body}"))
    }

    /// Emit the unboxing of `expr`, read out of an erased slot, as `target`.
    pub(crate) fn emit_erased_unbox(&mut self, expr: &Expr, target: &TypeRef) {
        if target.nullable {
            let mut inner = target.clone();
            inner.nullable = false;
            self.w.push('(');
            self.emit_expr(expr);
            self.w.push_str(").as_ref().map(|__jux_ev| __jux_ev.get::<");
            self.emit_value_type_as_rust(&inner);
            self.w.push_str(">())");
            return;
        }
        self.w.push('(');
        self.emit_expr(expr);
        self.w.push_str(").get::<");
        self.emit_value_type_as_rust(target);
        self.w.push_str(">()");
    }

    /// The Rust type a value boxed into an erased slot is taken at: the
    /// slot's own type where the call site wrote it, else the type the
    /// checker gave the value, else a literal's own.
    pub(crate) fn erased_box_type(&self, expr: &Expr, written: Option<&TypeRef>) -> Option<TypeRef> {
        if let Some(t) = written {
            return Some(t.clone());
        }
        let span = crate::exprs::expr_span_of(expr);
        if span != Span::DUMMY {
            if let Some(t) = self.expr_types.get(&span).and_then(crate::analysis::ty_to_type_ref) {
                return Some(t);
            }
        }
        let name = match expr {
            Expr::Literal(juxc_ast::Literal::Int(i)) => match i.kind {
                None => "int",
                Some(_) => return None,
            },
            Expr::Literal(juxc_ast::Literal::Float(_)) => "double",
            Expr::Literal(juxc_ast::Literal::Bool(_)) => "bool",
            Expr::Literal(juxc_ast::Literal::Char(_)) => "char",
            Expr::Literal(juxc_ast::Literal::String(_)) => "String",
            _ => return None,
        };
        Some(crate::analysis::synth_iface_type_ref(name, span))
    }

    /// The type an expression read out of an erased slot comes back as:
    /// a field of an erased class read from outside, or the result of a call
    /// whose declared return type is an erased type parameter.
    pub(crate) fn erased_read_target(&self, expr: &Expr) -> Option<TypeRef> {
        if !self.erasure_active() || self.emitting_lvalue {
            return None;
        }
        // Read back from the temporary an operator chain bound it to, which
        // holds the value already unboxed.
        if self.operand_substitute(expr).is_some() {
            return None;
        }
        let span = crate::exprs::expr_span_of(expr);
        let is_slot = match expr {
            Expr::Call(c) => self
                .erased_callee(c)
                .is_some_and(|callee| callee.ret.as_ref().is_some_and(|t| is_bare_param(t, &callee.erased))),
            Expr::Field(f) => self.erased_field_slot(f),
            _ => false,
        };
        if !is_slot {
            return None;
        }
        let ty = self.expr_types.get(&span)?;
        crate::analysis::ty_to_type_ref(ty)
    }

    /// Whether `f` is a field of an erased class, read or written from
    /// outside its generic body, whose declared type is one of the class's
    /// type parameters.
    pub(crate) fn erased_field_slot(&self, f: &FieldExpr) -> bool {
        let Some((class, true)) = self.erased_receiver(&f.object) else { return false };
        let Some((field, decl)) = self.symbols.lookup_field(&class, &f.field.text) else { return false };
        !field.is_static && self.symbols.erasure.class(decl) && is_bare_param(&field.ty, &self.erased_class_params(decl))
    }
}

/// What an erased call's declaration says.
struct ErasedCallee {
    /// The type parameter names erased at this call.
    erased: Vec<String>,
    /// The function's or method's own type parameters, in order.
    own: Vec<String>,
    /// Each erased type parameter's bounds.
    bounds: std::collections::HashMap<String, Vec<TypeRef>>,
    /// The declared parameter types.
    params: Vec<TypeRef>,
    /// The declared return type.
    ret: Option<TypeRef>,
    /// The function or method itself is erased (its own type arguments).
    fn_erased: bool,
}

/// Whether `t` names one of `params` anywhere in it.
fn names_any(t: &TypeRef, params: &[String]) -> bool {
    (t.name.segments.len() == 1 && params.contains(&t.name.segments[0].text))
        || t.generic_args.iter().filter_map(|a| a.as_type()).any(|a| names_any(a, params))
}

/// `t` with each of `params` read as the erased type.
fn erase_params(t: &TypeRef, params: &[String]) -> TypeRef {
    if t.generic_args.is_empty() && t.name.segments.len() == 1 && params.contains(&t.name.segments[0].text) {
        let mut m = crate::analysis::synth_iface_type_ref(juxc_tycheck::erasure::ERASED_TYPE, t.span);
        m.nullable = t.nullable;
        return m;
    }
    let mut out = t.clone();
    out.generic_args = t
        .generic_args
        .iter()
        .map(|a| match a.as_type() {
            Some(x) => GenericArg::Type(erase_params(x, params)),
            None => a.clone(),
        })
        .collect();
    out
}

/// Replace each whole-word occurrence of `from[i]` in `s` with `to[i]`.
fn replace_words(s: &str, from: &[String], to: &[String]) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes: Vec<char> = s.chars().collect();
    let mut i = 0;
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    while i < bytes.len() {
        if is_word(bytes[i]) && (i == 0 || !is_word(bytes[i - 1])) {
            let mut j = i;
            while j < bytes.len() && is_word(bytes[j]) {
                j += 1;
            }
            let word: String = bytes[i..j].iter().collect();
            match from.iter().position(|f| *f == word) {
                Some(k) => out.push_str(&to[k]),
                None => out.push_str(&word),
            }
            i = j;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// Split `s` at its top-level commas.
fn split_top(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// See [`RustEmitter::erased_kind_adapters`].
fn kind_adapter_from_forwarding(
    text: &str,
    params: &[String],
    pattern: &[bool],
    name: &str,
    ps: &[String],
) -> Option<String> {
    let start = text.find("impl<__JuxH:")?;
    let head_end = text[start..].find(" for std::rc::Rc<__JuxH> {")? + start;
    let gen_open = start + "impl".len();
    let mut depth = 0i32;
    let mut gen_close = None;
    for (i, ch) in text[gen_open..head_end].char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    gen_close = Some(gen_open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let gen_close = gen_close?;
    let generics = split_top(&text[gen_open + 1..gen_close]);
    let sig = text[gen_close + 1..head_end].trim().to_string();
    let erased: Vec<String> = pattern
        .iter()
        .enumerate()
        .map(|(j, e)| if *e { crate::erasure::ERASED_RUST.to_string() } else { ps[j].clone() })
        .collect();
    let sig_view = replace_words(&sig, params, ps);
    let sig_erased = replace_words(&sig, params, &erased);
    let rest: Vec<String> = generics.iter().skip(1).map(|g| replace_words(g, params, ps)).collect();
    let mut out = format!(
        "impl<__JuxV: {sig_view} + Clone + 'static{}{}> {sig_erased} for {name}<__JuxV, {}> {{\n",
        if rest.is_empty() { "" } else { ", " },
        rest.join(", "),
        ps.join(", ")
    );
    let body = &text[head_end + " for std::rc::Rc<__JuxH> {".len()..];
    for line in body.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if l == "}" {
            break;
        }
        if !l.starts_with("fn ") || !l.ends_with('}') {
            return None;
        }
        let split = l.find(" { <__JuxH as ")?;
        let head = &l[..split];
        let call = &l[split + 3..l.len() - 1].trim().to_string();
        // Parameters, from `(&self, a: T, b: U)`.
        let po = head.find("(&self")?;
        let mut d = 0i32;
        let mut pc = None;
        for (i, ch) in head[po..].char_indices() {
            match ch {
                '(' | '<' => d += 1,
                ')' | '>' => {
                    d -= 1;
                    if d == 0 && ch == ')' {
                        pc = Some(po + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let pc = pc?;
        let plist = split_top(&head[po + 1..pc]);
        let mut converted: Vec<(String, usize)> = Vec::new();
        for p in plist.iter().skip(1) {
            let (pname, pty) = p.split_once(':')?;
            if let Some(j) = params.iter().position(|x| x == pty.trim()) {
                if pattern.get(j).copied().unwrap_or(false) {
                    converted.push((pname.trim().to_string(), j));
                }
            }
        }
        let after = &head[pc + 1..];
        let ret = after.trim_start().strip_prefix("->").map(|r| {
            let r = r.trim();
            r.split(" where ").next().unwrap_or(r).trim().to_string()
        });
        let ret_erased = ret
            .as_deref()
            .and_then(|r| params.iter().position(|x| x == r))
            .is_some_and(|j| pattern.get(j).copied().unwrap_or(false));
        let new_head = replace_words(head, params, &erased);
        let mut new_call = replace_words(call, params, ps).replace("<__JuxH as", "<__JuxV as").replace("&**self", "&self.0");
        for (pname, j) in &converted {
            // `, a)` / `, a,` → the unboxed value.
            for tail in [")", ","] {
                let from = format!(", {pname}{tail}");
                let to = format!(", {pname}.get::<{}>(){tail}", ps[*j]);
                if new_call.contains(&from) {
                    new_call = new_call.replacen(&from, &to, 1);
                    break;
                }
            }
        }
        if ret_erased {
            new_call = format!("crate::jux_erased_rebox({new_call}, &self.2)");
        }
        out.push_str(&format!("    {new_head} {{ {new_call} }}\n"));
    }
    out.push_str("}\n");
    Some(out)
}

/// The adapter's name for an interface and a shape (`E` erased, `K` kept).
fn adapter_name(iface: &str, pattern: &[bool]) -> String {
    let shape: String = pattern.iter().map(|e| if *e { 'E' } else { 'K' }).collect();
    format!("__JuxAdapt_{iface}_{shape}")
}

/// The erased position of the interface's parameters `t` is, bare.
fn erased_position(t: &TypeRef, iface_params: &[String], pattern: &[bool]) -> Option<usize> {
    if t.nullable || !t.generic_args.is_empty() || t.name.segments.len() != 1 || t.array_shape.is_some() {
        return None;
    }
    let j = iface_params.iter().position(|p| *p == t.name.segments[0].text)?;
    pattern.get(j).copied().unwrap_or(false).then_some(j)
}

/// Whether `t` is one of `params`, bare or nullable (`T`, `T?`; not `T[]`,
/// `Vec<T>`).
fn is_bare_param(t: &TypeRef, params: &[String]) -> bool {
    t.array_shape.is_none()
        && t.fn_shape.is_none()
        && t.ptr_depth == 0
        && t.generic_args.is_empty()
        && t.name.segments.len() == 1
        && params.contains(&t.name.segments[0].text)
}
