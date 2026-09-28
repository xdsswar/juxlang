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

    /// Whether `args` name the erased class `fqn` at its own parameters inside
    /// its own body: the generic code, not a use of it.
    fn in_own_generic_body(&self, fqn: &str, args: &[TypeRef]) -> bool {
        let bare = fqn.rsplit('.').next().unwrap_or(fqn);
        let enclosing = self.enclosing_class.as_deref().map(|c| c.rsplit('.').next().unwrap_or(c));
        if enclosing != Some(bare) {
            return false;
        }
        let params = self.erased_class_params(fqn);
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
        let mut out = ty.clone();
        out.generic_args = args.iter().map(|a| GenericArg::Type(Self::marker(a.span))).collect();
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
            self.erase_on_emit.insert(Self::erase_key(arg), written);
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
            self.erase_on_emit.insert(Self::erase_key(arg), written);
        }
    }

    /// Emit the boxing of `expr` into an erased slot (`at` is the type the
    /// value is taken at). An optional value keeps its `null`: the value
    /// inside is boxed.
    pub(crate) fn emit_erased_box(&mut self, expr: &Expr, at: Option<TypeRef>) {
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
            }
            self.w.push_str("))");
            return;
        }
        self.w.push_str("crate::__jux_erase!(");
        self.emit_expr(expr);
        if let Some(t) = at {
            self.w.push_str(", ");
            self.emit_value_type_as_rust(&t);
        }
        self.w.push(')');
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
    /// The declared parameter types.
    params: Vec<TypeRef>,
    /// The declared return type.
    ret: Option<TypeRef>,
    /// The function or method itself is erased (its own type arguments).
    fn_erased: bool,
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
