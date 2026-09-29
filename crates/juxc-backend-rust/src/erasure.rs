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
//!   the checker gave the expression) where it is read out of one;
//! - a COLLECTION of one of those parameters (`Vec<T>`, `T[]`) is linked
//!   rather than boxed (ERRATA E144): the erased `Vec<JuxErased>`
//!   becomes a mirror of the typed `Vec<X>`, refreshed from it on each
//!   borrow and written back into it when a mutable borrow ends
//!   (`jux_link_unerase` / `jux_link_erase` in the prelude), so both sides
//!   hold one collection. The adapters of E143 link their arguments and
//!   results the same way.
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
    /// bound at them (`T extends Ranked<T>`, ERRATA E143).
    pub(crate) decl_params: Vec<String>,
    pub(crate) param: String,
}

/// A collection value crossing into an erased slot (ERRATA E144):
/// the slot's declared type, the value's own type, the slot's type
/// parameters (erased), and what each of them keeps when an element is
/// boxed.
#[derive(Clone)]
pub(crate) struct LinkMark {
    pub(crate) slot: TypeRef,
    pub(crate) typed: TypeRef,
    pub(crate) leaves: Vec<String>,
    pub(crate) marks: std::collections::HashMap<String, EraseMark>,
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
    /// is a `const` one (kept as it is, ERRATA E143).
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
        // A collection of the parameter (`Vec<T> acc`) is the same collection
        // on both sides, linked rather than copied (ERRATA E144).
        let decl_params: Vec<String> = callee.bounds.keys().cloned().collect();
        for (i, arg) in call.args.iter().enumerate() {
            let Some(slot) = callee.params.get(i).filter(|t| self.link_reaches(t, &callee.erased)).cloned() else {
                continue;
            };
            self.mark_link_to_erased(arg, &slot, &callee.erased, &callee.bounds, &decl_params);
        }
    }

    /// Mark `value`, flowing into the erased slot `slot` whose leaves are
    /// `erased`, to be linked into the erased collection it fills.
    fn mark_link_to_erased(
        &mut self,
        value: &Expr,
        slot: &TypeRef,
        erased: &[String],
        bounds: &std::collections::HashMap<String, Vec<TypeRef>>,
        decl_params: &[String],
    ) {
        let Some(typed) = self.expr_types.get(&crate::exprs::expr_span_of(value)).and_then(crate::analysis::ty_to_type_ref)
        else {
            return;
        };
        let marks = erased
            .iter()
            .map(|p| {
                let mark = EraseMark {
                    written: None,
                    bounds: bounds.get(p).cloned().unwrap_or_default(),
                    decl_params: decl_params.to_vec(),
                    param: p.clone(),
                };
                (p.clone(), mark)
            })
            .collect();
        self.link_on_emit.insert(
            Self::erase_key(value),
            LinkMark { slot: slot.clone(), typed, leaves: erased.to_vec(), marks },
        );
    }

    /// Emit `expr`, whose collection crosses an erasure boundary, linked to
    /// the form on the other side (ERRATA E144): into the erased
    /// slot `mark.slot` when it is marked in `link_on_emit`, out of one into
    /// its typed form when [`Self::erased_link_read`] says so. `false` when
    /// neither applies.
    pub(crate) fn emit_linked(&mut self, expr: &Expr) -> bool {
        let key = Self::erase_key(expr);
        if let Some(mark) = self.link_on_emit.get(&key).cloned() {
            if self.erasing_now.insert((key, true)) {
                let conv = self.link_closure(&mark.slot, &mark.typed, &mark.leaves, false, &LinkBox::Erase(mark.marks));
                self.w.push_str(&format!("({conv})(&("));
                self.emit_expr(expr);
                self.w.push_str("))");
                self.erasing_now.remove(&(key, true));
                return true;
            }
            return false;
        }
        if self.erasing_now.contains(&(key, false)) {
            return false;
        }
        let Some((slot, typed, leaves)) = self.erased_link_read(expr) else { return false };
        self.erasing_now.insert((key, false));
        let conv = self.link_closure(&slot, &typed, &leaves, true, &LinkBox::Erase(Default::default()));
        self.w.push_str(&format!("({conv})(&("));
        self.emit_expr(expr);
        self.w.push_str("))");
        self.erasing_now.remove(&(key, false));
        true
    }

    /// A collection read out of an erased slot: the result of a call whose
    /// declared return type holds an erased parameter in a collection, or a
    /// field of an erased class read from outside it. The slot's type, the
    /// type the checker gave the value, and the slot's erased leaves.
    fn erased_link_read(&self, expr: &Expr) -> Option<(TypeRef, TypeRef, Vec<String>)> {
        if !self.erasure_active()
            || (self.emitting_lvalue && !self.emitting_method_receiver)
            || self.operand_substitute(expr).is_some()
        {
            return None;
        }
        let (slot, leaves) = match expr {
            Expr::Call(c) => {
                let callee = self.erased_callee(c)?;
                let ret = callee.ret.clone().filter(|t| self.link_reaches(t, &callee.erased))?;
                (ret, callee.erased)
            }
            Expr::Field(f) => {
                let (class, true) = self.erased_receiver(&f.object)? else { return None };
                let (field, decl) = self.symbols.lookup_field(&class, &f.field.text)?;
                if field.is_static || !self.symbols.erasure.class(decl) {
                    return None;
                }
                let params = self.erased_class_params(decl);
                (field.ty.clone(), params)
            }
            _ => return None,
        };
        if !self.link_reaches(&slot, &leaves) {
            return None;
        }
        let typed = self.expr_types.get(&crate::exprs::expr_span_of(expr)).and_then(crate::analysis::ty_to_type_ref)?;
        Some((slot, typed, leaves))
    }

    /// Whether `f` is a field of an erased class, written from outside its
    /// generic body, whose declared type holds one of the class's type
    /// parameters in a collection: its slot type and the parameters.
    pub(crate) fn erased_link_field_slot(&self, f: &FieldExpr) -> Option<(TypeRef, Vec<String>)> {
        let (class, true) = self.erased_receiver(&f.object)? else { return None };
        let (field, decl) = self.symbols.lookup_field(&class, &f.field.text)?;
        if field.is_static || !self.symbols.erasure.class(decl) {
            return None;
        }
        let params = self.erased_class_params(decl);
        self.link_reaches(&field.ty, &params).then(|| (field.ty.clone(), params))
    }

    /// Mark a value stored into the linked field slot `f` (see
    /// [`Self::erased_link_field_slot`]).
    pub(crate) fn mark_link_field_store(&mut self, f: &FieldExpr, value: &Expr) {
        let Some((slot, params)) = self.erased_link_field_slot(f) else { return };
        let Some((class, _)) = self.erased_receiver(&f.object) else { return };
        let Some((_, decl)) = self.symbols.lookup_field(&class, &f.field.text) else { return };
        let decl = decl.to_string();
        let bounds: std::collections::HashMap<String, Vec<TypeRef>> =
            params.iter().map(|p| (p.clone(), self.erased_class_param_bounds(&decl, p))).collect();
        self.mark_link_to_erased(value, &slot, &params, &bounds, &params.clone());
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
        // A collection of the class's parameters is linked, not copied
        // (ERRATA E144).
        let bounds: std::collections::HashMap<String, Vec<TypeRef>> =
            params.iter().map(|p| (p.clone(), self.erased_class_param_bounds(&key, p))).collect();
        for (i, arg) in n.args.iter().enumerate() {
            let Some(slot) = ctor_params.get(i).filter(|t| self.link_reaches(t, &params)).cloned() else { continue };
            self.mark_link_to_erased(arg, &slot, &params, &bounds, &params);
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
            let pattern: Vec<Pos> = args.iter().map(|a| self.pos_of(a, &mark.decl_params)).collect();
            let erased_bound = erase_params(b, &mark.decl_params);
            self.w.push_str("std::rc::Rc<dyn ");
            self.emit_bound_type(&erased_bound);
            self.w.push_str("> => crate::");
            let iface = b.name.segments.last().map(|s| s.text.clone()).unwrap_or_default();
            self.w.push_str(&adapter_name(&iface, &pattern));
            self.w.push_str("::<_");
            for (j, a) in args.iter().enumerate() {
                match &pattern[j] {
                    Pos::Erased => {
                        self.w.push_str(", ");
                        let own = a.name.segments[0].text == mark.param;
                        match at {
                            Some(t) if own => self.emit_value_type_as_rust(t),
                            _ => self.w.push('_'),
                        }
                    }
                    Pos::Kept => {
                        self.w.push_str(", ");
                        let e = erase_params(a, &mark.decl_params);
                        self.emit_value_type_as_rust(&e);
                    }
                    // One argument per parameter the collection holds: the
                    // value's own type for the slot's parameter, inferred
                    // from the value's implementation for any other.
                    Pos::Linked { names, .. } => {
                        for n in names {
                            self.w.push_str(", ");
                            match at {
                                Some(t) if *n == mark.param => self.emit_value_type_as_rust(t),
                                _ => self.w.push('_'),
                            }
                        }
                    }
                }
            }
            self.w.push('>');
        }
        self.w.push(']');
    }

    /// The adapter shapes an interface needs: one per way the family's
    /// bounds name it at their parameters, each position erased (a bare
    /// parameter), kept, or linked (the parameter inside a collection).
    pub(crate) fn erased_adapter_patterns(&self, iface_bare: &str) -> Vec<Vec<Pos>> {
        let mut out: Vec<Vec<Pos>> = Vec::new();
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
                    let pattern: Vec<Pos> =
                        b.generic_args.iter().filter_map(|a| a.as_type()).map(|a| self.pos_of(a, &names)).collect();
                    if !out.contains(&pattern) {
                        out.push(pattern);
                    }
                }
            }
        }
        out
    }

    /// The adapters an erased value reaches an interface bound at its own
    /// parameters through (ERRATA E143): `I<JuxErased, ..>`,
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
    /// parameters through (`T extends Shape<T>`, ERRATA E143), made
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
            let shape = self.adapter_shape(&pattern, class_decl.name.span);
            let Some(adapter) = kind_adapter_from_forwarding(text, &params, &pattern, &name, &shape) else { continue };
            self.emit_erased_adapter_struct(&name, &shape.gens);
            self.w.push_str(&adapter);
        }
    }

    /// What an adapter of `pattern` is generic over, and how each position
    /// reads on the value's side (`view`), on the erased side (`erased`,
    /// also as a type the interface's own parameter becomes), and, for a
    /// linked one, the closures converting a collection between the two.
    fn adapter_shape(&mut self, pattern: &[Pos], span: Span) -> AdapterShape {
        let mut out = AdapterShape::default();
        for (j, pos) in pattern.iter().enumerate() {
            match pos {
                Pos::Erased | Pos::Kept => {
                    let p = format!("__JuxP{j}");
                    out.gens.push(p.clone());
                    out.pos_gens.push(vec![p.clone()]);
                    out.view.push(p.clone());
                    if matches!(pos, Pos::Erased) {
                        out.erased.push(ERASED_RUST.to_string());
                        out.erased_ty.push(Self::marker(span));
                    } else {
                        out.erased.push(p.clone());
                        out.erased_ty.push(crate::analysis::synth_iface_type_ref(&p, span));
                    }
                    out.link_in.push(None);
                    out.link_out.push(None);
                }
                Pos::Linked { shape, names, .. } => {
                    let ps: Vec<String> = (0..names.len()).map(|k| format!("__JuxP{j}_{k}")).collect();
                    out.gens.extend(ps.iter().cloned());
                    out.pos_gens.push(ps.clone());
                    let leaves: Vec<String> = (0..names.len()).map(link_leaf).collect();
                    let typed = subst_names(shape, &leaves, &ps);
                    let marker = vec![juxc_tycheck::erasure::ERASED_TYPE.to_string(); names.len()];
                    let erased = subst_names(shape, &leaves, &marker);
                    let view = self.type_text(&typed);
                    let erased_text = self.type_text(&erased);
                    let in_text = self.link_closure(shape, &typed, &leaves, true, &LinkBox::Rebox);
                    let out_text = self.link_closure(shape, &typed, &leaves, false, &LinkBox::Rebox);
                    out.view.push(view);
                    out.erased.push(erased_text);
                    out.erased_ty.push(erased);
                    out.link_in.push(Some(in_text));
                    out.link_out.push(Some(out_text));
                }
            }
        }
        out
    }

    fn emit_erased_adapter(&mut self, interface: &juxc_ast::InterfaceDecl, pattern: &[Pos]) {
        use juxc_lex::to_rust_ident;
        let iface = to_rust_ident(&interface.name.text);
        let name = adapter_name(&interface.name.text, pattern);
        let iface_params: Vec<String> = interface.generic_params.iter().map(|p| p.name.text.clone()).collect();
        let shape = self.adapter_shape(pattern, interface.name.span);
        let pgen = shape.gens.join(", ");
        self.emit_erased_adapter_struct(&name, &shape.gens);
        let view_args = shape.view.join(", ");
        let pbounds =
            shape.gens.iter().map(|p| format!("{p}: Clone + std::fmt::Debug + 'static")).collect::<Vec<_>>().join(", ");
        self.w.line(&format!(
            "impl<__JuxV: {iface}<{view_args}> + Clone + 'static, {pbounds}> {iface}<{}> for {name}<__JuxV, {pgen}> {{",
            shape.erased.join(", ")
        ));
        self.w.indent_inc();
        // The interface's own parameters read as the adapter's: erased
        // positions as `JuxErased`, kept ones as themselves, linked ones as
        // the collection of `JuxErased`.
        let mut subst: std::collections::HashMap<String, TypeRef> = std::collections::HashMap::new();
        for (j, p) in iface_params.iter().enumerate() {
            if let Some(t) = shape.erased_ty.get(j) {
                subst.insert(p.clone(), t.clone());
            }
        }
        let saved = std::mem::replace(&mut self.kind_type_subst, subst);
        let methods: Vec<juxc_ast::FnDecl> = interface
            .methods
            .iter()
            .filter(|m| !m.modifiers.iter().any(|x| matches!(x, juxc_ast::FnModifier::Static)))
            .filter(|m| m.generic_params.is_empty() || m.body.is_none())
            .cloned()
            .collect();
        let linked = pattern.iter().any(|p| matches!(p, Pos::Linked { .. }));
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
                    adapted_position(t, &iface_params, pattern)
                }
                _ => None,
            };
            self.w.push_str(" { ");
            if linked {
                self.w.push_str("let __jux_ltab = self.2.clone(); ");
            }
            match ret_pos.map(|j| (j, &pattern[j])) {
                Some((_, Pos::Erased)) => self.w.push_str("crate::jux_erased_rebox("),
                Some((j, Pos::Linked { .. })) => {
                    let conv = shape.link_out[j].clone().unwrap_or_default();
                    self.w.push_str(&format!("({conv})(&"));
                }
                _ => {}
            }
            self.w.push_str(&format!("<__JuxV as {iface}<{view_args}>>::"));
            self.w.push_str(&to_rust_ident(&m.name.text));
            self.w.push_str("(&self.0");
            for p in &m.params {
                self.w.push_str(", ");
                let pname = to_rust_ident(&p.name.text);
                match adapted_position(&p.ty, &iface_params, pattern).map(|j| (j, &pattern[j])) {
                    Some((j, Pos::Erased)) => {
                        self.w.push_str(&pname);
                        self.w.push_str(&format!(".get::<{}>()", shape.pos_gens[j][0]));
                    }
                    Some((j, Pos::Linked { .. })) => {
                        let conv = shape.link_in[j].clone().unwrap_or_default();
                        self.w.push_str(&format!("({conv})(&{pname})"));
                    }
                    _ => self.w.push_str(&pname),
                }
            }
            self.w.push(')');
            match ret_pos.map(|j| &pattern[j]) {
                Some(Pos::Erased) => self.w.push_str(", &self.2)"),
                Some(Pos::Linked { .. }) => self.w.push(')'),
                _ => {}
            }
            self.w.push_str(" }\n");
        }
        self.kind_type_subst = saved;
        self.w.indent_dec();
        self.w.line("}");
    }

    /// Where the type parameter `a` of a bound sits for the adapter: the
    /// parameter itself, a collection holding it (ERRATA E144), or
    /// neither.
    pub(crate) fn pos_of(&self, a: &TypeRef, params: &[String]) -> Pos {
        if is_bare_param(a, params) && !a.nullable {
            return Pos::Erased;
        }
        if self.link_reaches(a, params) {
            let mut names: Vec<String> = Vec::new();
            let shape = rename_leaves(a, params, &mut names);
            let key: String = juxc_tycheck::symbol_table::render_type_ref(&shape)
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            return Pos::Linked { key, shape, names };
        }
        Pos::Kept
    }

    /// Whether `t` is a collection (or a runtime-sized array) that holds one
    /// of `params`, directly or through further collections.
    pub(crate) fn link_reaches(&self, t: &TypeRef, params: &[String]) -> bool {
        if t.nullable || t.fn_shape.is_some() || t.ptr_depth > 0 {
            return false;
        }
        let leaf_or_deeper = |this: &Self, a: &TypeRef| {
            (is_bare_param(a, params) && !a.nullable) || this.link_reaches(a, params)
        };
        if let Some(shape) = &t.array_shape {
            if shape.dims.len() != 1 || shape.elem_nullable || !matches!(shape.dims[0], juxc_ast::ArrayDim::Dynamic) {
                return false;
            }
            let mut element = t.clone();
            element.array_shape = None;
            return leaf_or_deeper(self, &element);
        }
        if !self.collection_is_handle(&t.name) || !(1..=2).contains(&t.generic_args.len()) {
            return false;
        }
        t.generic_args.iter().filter_map(|a| a.as_type()).any(|a| leaf_or_deeper(self, a))
    }

    /// The Rust text of `t` as a value type.
    pub(crate) fn type_text(&mut self, t: &TypeRef) -> String {
        let mark = self.w.mark();
        self.emit_value_type_as_rust(t);
        self.w.split_off_from(mark)
    }

    /// The Rust text of the sequence inside the handle of the collection or
    /// array `t` (`std::vec::Vec<isize>` for `Vec<int>` and for `int[]`).
    fn inner_text(&mut self, t: &TypeRef) -> String {
        if t.array_shape.is_some() {
            let mut element = t.clone();
            element.array_shape = None;
            return format!("std::vec::Vec<{}>", self.type_text(&element));
        }
        let mark = self.w.mark();
        self.plain_collection_once = true;
        self.emit_value_type_as_rust(t);
        self.plain_collection_once = false;
        self.w.split_off_from(mark)
    }

    /// A closure converting one value of the linked shape `slot` between its
    /// erased form and `typed` (ERRATA E144): erased to typed when
    /// `to_typed`, typed to erased otherwise. `leaves` are the names in
    /// `slot` that the erased side holds as `JuxErased`. A collection is
    /// converted by LINKING it, so both sides share its one storage; an
    /// element that is a leaf is unboxed or boxed; anything else is the same
    /// type on both sides and is cloned (shared, for a reference).
    pub(crate) fn link_closure(
        &mut self,
        slot: &TypeRef,
        typed: &TypeRef,
        leaves: &[String],
        to_typed: bool,
        boxer: &LinkBox,
    ) -> String {
        // An adapter's closure takes its own handle on the table it reboxes
        // with; any other closure captures nothing.
        let (open, close) = match boxer {
            LinkBox::Rebox => ("{ let __jux_ltab = __jux_ltab.clone(); move ", " }"),
            LinkBox::Erase(_) => ("(move ", ")"),
        };
        if let Some(leaf) = leaf_of(slot, leaves) {
            let x = self.type_text(typed);
            if to_typed {
                return format!("{open}|__jux_le: &crate::JuxErased| __jux_le.get::<{x}>(){close}");
            }
            let boxed = match boxer {
                LinkBox::Rebox => "crate::jux_erased_rebox(::std::clone::Clone::clone(__jux_lx), &__jux_ltab)".to_string(),
                LinkBox::Erase(marks) => {
                    let mark = marks.get(&leaf).cloned().unwrap_or_default();
                    let at = typed.clone();
                    let m = self.w.mark();
                    self.w.push_str("crate::__jux_erase!((*__jux_lx), ");
                    self.emit_value_type_as_rust(&at);
                    self.emit_erased_views(&mark, Some(&at));
                    self.w.push(')');
                    self.w.split_off_from(m)
                }
            };
            return format!("{open}|__jux_lx: &{x}| {boxed}{close}");
        }
        let args = link_args(slot, typed);
        if args.is_empty() || !args.iter().any(|(s, _)| leaf_of(s, leaves).is_some() || self.link_reaches(s, leaves)) {
            return format!("{open}|__jux_ls| ::std::clone::Clone::clone(__jux_ls){close}");
        }
        let erased = subst_names(slot, leaves, &vec![juxc_tycheck::erasure::ERASED_TYPE.to_string(); leaves.len()]);
        let eh = self.type_text(&erased);
        let th = self.type_text(typed);
        let ei = self.inner_text(&erased);
        let ti = self.inner_text(typed);
        let mut elem_tt: Vec<String> = Vec::new();
        let mut elem_te: Vec<String> = Vec::new();
        for (s, t) in &args {
            elem_tt.push(self.link_closure(s, t, leaves, true, boxer));
            elem_te.push(self.link_closure(s, t, leaves, false, boxer));
        }
        let map = |elems: &[String]| {
            if elems.len() == 1 {
                format!(".map({})", elems[0])
            } else {
                format!(".map(|(__jux_lk, __jux_lw)| (({})(__jux_lk), ({})(__jux_lw)))", elems[0], elems[1])
            }
        };
        let tt = format!("{open}|__jux_lc: &{ei}| __jux_lc.iter(){}.collect::<{ti}>(){close}", map(&elem_tt));
        let te = format!("{open}|__jux_lc: &{ti}| __jux_lc.iter(){}.collect::<{ei}>(){close}", map(&elem_te));
        if to_typed {
            format!("{open}|__jux_lh: &{eh}| crate::jux_link_unerase(__jux_lh, {tt}, {te}){close}")
        } else {
            format!("{open}|__jux_lh: &{th}| crate::jux_link_erase(__jux_lh, {tt}, {te}){close}")
        }
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
        // E143).
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
        m.array_shape = t.array_shape.clone();
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
    pattern: &[Pos],
    name: &str,
    shape: &AdapterShape,
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
    let sig_view = replace_words(&sig, params, &shape.view);
    let sig_erased = replace_words(&sig, params, &shape.erased);
    // The class's own parameters, declared on the forwarding impl, become the
    // adapter's: one for an erased or kept position, one per parameter the
    // collection holds for a linked one.
    let mut rest: Vec<String> = Vec::new();
    for g in generics.iter().skip(1) {
        let head = g.split(':').next().unwrap_or(g).trim();
        match params.iter().position(|p| p == head) {
            Some(j) if matches!(pattern.get(j), Some(Pos::Linked { .. })) => {
                rest.extend(
                    shape.pos_gens[j].iter().map(|p| format!("{p}: Clone + std::fmt::Debug + 'static")),
                );
            }
            _ => rest.push(replace_words(g, params, &shape.view)),
        }
    }
    let mut out = format!(
        "impl<__JuxV: {sig_view} + Clone + 'static{}{}> {sig_erased} for {name}<__JuxV, {}> {{\n",
        if rest.is_empty() { "" } else { ", " },
        rest.join(", "),
        shape.gens.join(", ")
    );
    let linked = pattern.iter().any(|p| matches!(p, Pos::Linked { .. }));
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
                if !matches!(pattern.get(j), Some(Pos::Kept) | None) {
                    converted.push((pname.trim().to_string(), j));
                }
            }
        }
        let after = &head[pc + 1..];
        let ret = after.trim_start().strip_prefix("->").map(|r| {
            let r = r.trim();
            r.split(" where ").next().unwrap_or(r).trim().to_string()
        });
        let ret_at = ret.as_deref().and_then(|r| params.iter().position(|x| x == r));
        let new_head = replace_words(head, params, &shape.erased);
        let mut new_call =
            replace_words(call, params, &shape.view).replace("<__JuxH as", "<__JuxV as").replace("&**self", "&self.0");
        for (pname, j) in &converted {
            // `, a)` / `, a,` → the unboxed value, or the linked collection.
            let conv = match &pattern[*j] {
                Pos::Linked { .. } => format!("({})(&{pname})", shape.link_in[*j].clone().unwrap_or_default()),
                _ => format!("{pname}.get::<{}>()", shape.pos_gens[*j][0]),
            };
            for tail in [")", ","] {
                let from = format!(", {pname}{tail}");
                let to = format!(", {conv}{tail}");
                if new_call.contains(&from) {
                    new_call = new_call.replacen(&from, &to, 1);
                    break;
                }
            }
        }
        match ret_at.map(|j| (j, &pattern[j])) {
            Some((_, Pos::Erased)) => new_call = format!("crate::jux_erased_rebox({new_call}, &self.2)"),
            Some((j, Pos::Linked { .. })) => {
                new_call = format!("({})(&{new_call})", shape.link_out[j].clone().unwrap_or_default());
            }
            _ => {}
        }
        if linked {
            new_call = format!("let __jux_ltab = self.2.clone(); {new_call}");
        }
        out.push_str(&format!("    {new_head} {{ {new_call} }}\n"));
    }
    out.push_str("}\n");
    Some(out)
}

/// How one argument of a bound at the family's parameters is seen by the
/// adapter that carries the bound for an erased value.
#[derive(Clone, Debug)]
pub(crate) enum Pos {
    /// The parameter itself: unboxed on the way in, reboxed on the way out.
    Erased,
    /// No parameter, or one held inside a Jux class, which is erased with
    /// the family: the same Rust type on both sides.
    Kept,
    /// The parameter inside a collection (`Vec<T>`, ERRATA E144):
    /// the erased side and the value's side are two Rust types for one
    /// collection, linked to one storage. `shape` is the argument with the
    /// parameters it holds renamed `__JuxL0`, `__JuxL1`, … in order of first
    /// appearance, `names` are the parameters they were, and `key` names the
    /// shape in the adapter's name.
    Linked { key: String, shape: TypeRef, names: Vec<String> },
}

impl PartialEq for Pos {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Pos::Erased, Pos::Erased) | (Pos::Kept, Pos::Kept) => true,
            (Pos::Linked { key: a, .. }, Pos::Linked { key: b, .. }) => a == b,
            _ => false,
        }
    }
}

/// See [`RustEmitter::adapter_shape`].
#[derive(Default)]
pub(crate) struct AdapterShape {
    /// Every type parameter of the adapter, after `__JuxV`.
    pub(crate) gens: Vec<String>,
    /// The adapter's type parameters, per position.
    pub(crate) pos_gens: Vec<Vec<String>>,
    /// Each position as the value's own implementation reads it.
    pub(crate) view: Vec<String>,
    /// Each position as the erased code reads it.
    pub(crate) erased: Vec<String>,
    /// The same, as a type.
    pub(crate) erased_ty: Vec<TypeRef>,
    /// For a linked position, the closure taking the erased collection to
    /// the value's, and the one taking it back.
    pub(crate) link_in: Vec<Option<String>>,
    pub(crate) link_out: Vec<Option<String>>,
}

/// How a linked collection's leaf is boxed on its way to the erased side: with
/// the table of the value an adapter wraps (`__jux_ltab`), or on its own with
/// the bounds its parameter keeps.
pub(crate) enum LinkBox {
    Rebox,
    Erase(std::collections::HashMap<String, EraseMark>),
}

/// The name a linked shape's `k`-th parameter is renamed to.
fn link_leaf(k: usize) -> String {
    format!("__JuxL{k}")
}

/// `t` with each bare name `from[k]` replaced by `to[k]`, its nullability
/// and array shape kept.
fn subst_names(t: &TypeRef, from: &[String], to: &[String]) -> TypeRef {
    if t.generic_args.is_empty() && t.name.segments.len() == 1 {
        if let Some(k) = from.iter().position(|f| *f == t.name.segments[0].text) {
            let mut m = crate::analysis::synth_iface_type_ref(&to[k], t.span);
            m.nullable = t.nullable;
            m.array_shape = t.array_shape.clone();
            return m;
        }
    }
    let mut out = t.clone();
    out.generic_args = t
        .generic_args
        .iter()
        .map(|a| match a.as_type() {
            Some(x) => GenericArg::Type(subst_names(x, from, to)),
            None => a.clone(),
        })
        .collect();
    out
}

/// `t` with each of `params` it names renamed `__JuxL<k>`, in order of first
/// appearance; `names` collects the parameters in that order.
fn rename_leaves(t: &TypeRef, params: &[String], names: &mut Vec<String>) -> TypeRef {
    if t.generic_args.is_empty() && t.name.segments.len() == 1 && params.contains(&t.name.segments[0].text) {
        let name = t.name.segments[0].text.clone();
        let k = match names.iter().position(|n| *n == name) {
            Some(k) => k,
            None => {
                names.push(name);
                names.len() - 1
            }
        };
        let mut m = crate::analysis::synth_iface_type_ref(&link_leaf(k), t.span);
        m.nullable = t.nullable;
        m.array_shape = t.array_shape.clone();
        return m;
    }
    let mut out = t.clone();
    out.generic_args = t
        .generic_args
        .iter()
        .map(|a| match a.as_type() {
            Some(x) => GenericArg::Type(rename_leaves(x, params, names)),
            None => a.clone(),
        })
        .collect();
    out
}

/// The leaf `slot` is, when it is one of `leaves` itself.
fn leaf_of(slot: &TypeRef, leaves: &[String]) -> Option<String> {
    (slot.generic_args.is_empty()
        && slot.array_shape.is_none()
        && !slot.nullable
        && slot.name.segments.len() == 1
        && leaves.contains(&slot.name.segments[0].text))
    .then(|| slot.name.segments[0].text.clone())
}

/// The element types of the collection or array `slot`, paired with the
/// same of `typed`.
fn link_args(slot: &TypeRef, typed: &TypeRef) -> Vec<(TypeRef, TypeRef)> {
    if slot.array_shape.is_some() {
        if typed.array_shape.is_none() {
            return Vec::new();
        }
        let (mut s, mut t) = (slot.clone(), typed.clone());
        s.array_shape = None;
        t.array_shape = None;
        return vec![(s, t)];
    }
    let s: Vec<&TypeRef> = slot.generic_args.iter().filter_map(|a| a.as_type()).collect();
    let t: Vec<&TypeRef> = typed.generic_args.iter().filter_map(|a| a.as_type()).collect();
    if s.len() != t.len() {
        return Vec::new();
    }
    s.into_iter().zip(t).map(|(a, b)| (a.clone(), b.clone())).collect()
}

/// The adapter's name for an interface and a shape (`E` erased, `K` kept,
/// `L<shape>_` linked).
fn adapter_name(iface: &str, pattern: &[Pos]) -> String {
    let shape: String = pattern
        .iter()
        .map(|p| match p {
            Pos::Erased => "E".to_string(),
            Pos::Kept => "K".to_string(),
            Pos::Linked { key, .. } => format!("L{key}_"),
        })
        .collect();
    format!("__JuxAdapt_{iface}_{shape}")
}

/// The position of the interface's parameters `t` is, bare, when the adapter
/// converts it (erased or linked).
fn adapted_position(t: &TypeRef, iface_params: &[String], pattern: &[Pos]) -> Option<usize> {
    if t.nullable || !t.generic_args.is_empty() || t.name.segments.len() != 1 || t.array_shape.is_some() {
        return None;
    }
    let j = iface_params.iter().position(|p| *p == t.name.segments[0].text)?;
    (!matches!(pattern.get(j), Some(Pos::Kept) | None)).then_some(j)
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
