//! Dispatch of a method with type parameters of its own through a supertype
//! value (ERRATA E136; the design is in `juxc_tycheck::generic_dispatch`).
//!
//! A supertype value is `Rc<dyn Trait>`. A trait object cannot carry a
//! generic method, so the trait declares it `where Self: Sized` (every
//! concrete type still implements it, and the trait stays usable as `dyn`),
//! and the handle's own impl of the trait, `impl Trait for Rc<H>`, answers it
//! by asking the object what it is (`JuxDynAny`, which does go through the
//! vtable) and calling that concrete type's method:
//!
//! ```text
//! fn accept<R: ..>(&self, v: Rc<dyn Visitor<R>>) -> R where .. {
//!     let __jux_any = crate::JuxDynAny::__jux_dyn_any(&**self);
//!     if let Some(__jux_c) = __jux_any.downcast_ref::<Num>() { <Num as Expr>::accept(__jux_c, v) }
//!     else if let Some(__jux_c) = __jux_any.downcast_ref::<Add>() { <Add as Expr>::accept(__jux_c, v) }
//!     else { panic!(..) }
//! }
//! ```
//!
//! Each branch is the Rust compiler's monomorphization of that type's method
//! for the call's own type arguments, so no set of type arguments has to be
//! closed. The set of TYPES is closed because the whole program is compiled
//! together. A subtype that instantiates a generic supertype with arguments of
//! its own (`IntLeaf extends Tree<int>` behind a `Tree<T>`) is reached with its
//! arguments and result converted through `__jux_seen_as`, which is the identity
//! at run time: the value IS an `IntLeaf`, so `T` is `int`.

use crate::RustEmitter;
use juxc_lex::to_rust_ident;

impl RustEmitter {
    /// Whether `bare` (an interface or class) declares a method, other than a
    /// `static` or `private` one, with type parameters of its own.
    pub(crate) fn declares_generic_virtual(&self, fqn: &str) -> bool {
        juxc_tycheck::generic_dispatch::generic_virtual_owners(&self.symbols).contains_key(fqn)
    }

    /// The body `{ ... }` of `method` on the handle impl of `base_fqn`'s trait.
    ///
    /// `trait_args` is the trait's argument list as written in this impl
    /// (`<T>` or empty) and `trait_path` the trait's name, so each identity
    /// branch can call `<C<..> as Trait<..>>::method`. `base_params` are the
    /// trait's parameter names in this impl's vocabulary.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_generic_dispatch_body(
        &mut self,
        base_fqn: &str,
        trait_path: &str,
        trait_args: &str,
        base_params: &[String],
        method: &str,
        method_generics: &[String],
        params: &[String],
        default_twin: bool,
    ) {
        let mut implementers = juxc_tycheck::generic_dispatch::concrete_implementers(&self.symbols, base_fqn);
        // A method with a default body is dispatched only to the types that
        // write their own; every other value, whatever it is (a class, a
        // record, or a type of the runtime's), runs the default, through the
        // `__jux_default_<m>` twin the trait keeps for it.
        if default_twin {
            implementers.retain(|sub| {
                // Written by the class or a class it extends; the interface's
                // own default does not count.
                let mut cur = self.symbols.classes.get(sub);
                let mut hops = 0;
                while let Some(c) = cur {
                    if c.methods.get(method).is_some_and(|m| !m.is_abstract) {
                        return true;
                    }
                    hops += 1;
                    if hops > 64 {
                        break;
                    }
                    cur = c.extends_fqn.as_deref().and_then(|p| self.symbols.classes.get(p));
                }
                self.symbols.records.get(sub).is_some_and(|r| r.methods.contains_key(method))
            });
        }
        self.w.push_str("{\n");
        self.w.indent_inc();
        self.w.line("let __jux_any = crate::JuxDynAny::__jux_dyn_any(&**self);");
        let mut first = true;
        for sub in &implementers {
            // A subtype whose parameters the supertype does not fix is
            // refused by the checker (E0438); it cannot appear here.
            let Some(args) = juxc_tycheck::generic_dispatch::supertype_args(&self.symbols, sub, base_fqn) else {
                continue;
            };
            let sub_params: Vec<String> = self
                .symbols
                .classes
                .get(sub)
                .map(|c| &c.generic_params)
                .or_else(|| self.symbols.records.get(sub).map(|r| &r.generic_params))
                .map(|ps| ps.iter().filter(|p| !p.is_const()).map(|p| p.name.text.clone()).collect())
                .unwrap_or_default();
            // Each of the subtype's parameters, spelled in the base's
            // vocabulary: the base parameter at the position it fills.
            let mut inverse: Vec<Option<String>> = vec![None; sub_params.len()];
            for (arg, bp) in args.iter().zip(base_params) {
                if arg.generic_args.is_empty() && arg.name.segments.len() == 1 {
                    if let Some(i) = sub_params.iter().position(|p| *p == arg.name.segments[0].text) {
                        if inverse[i].is_none() {
                            inverse[i] = Some(bp.clone());
                        }
                    }
                }
            }
            // A parameter the supertype does not fix (`U` in `Weird<T, U>
            // extends Tree<T>`) is filled with each argument the program
            // builds the subtype with (`juxc_tycheck::instantiations`,
            // ERRATA E1XX-GAP39c): one branch per distinct one.
            let fills: Vec<Vec<Option<String>>> = if inverse.iter().any(|x| x.is_none()) {
                let insts = self.symbols.instantiations.classes.get(sub).cloned().unwrap_or_default();
                let mut out: Vec<Vec<Option<String>>> = Vec::new();
                for inst in insts {
                    let mut row = Vec::new();
                    for (i, slot) in inverse.iter().enumerate() {
                        if slot.is_some() {
                            row.push(None);
                            continue;
                        }
                        let Some(ty) = inst.get(i).and_then(crate::analysis::ty_to_type_ref) else {
                            row.clear();
                            break;
                        };
                        let mark = self.w.mark();
                        self.emit_generic_arg_type_as_rust(&juxc_ast::GenericArg::Type(ty));
                        row.push(Some(self.w.split_off_from(mark)));
                    }
                    if row.len() == inverse.len() && !out.contains(&row) {
                        out.push(row);
                    }
                }
                out
            } else {
                vec![vec![None; inverse.len()]]
            };
            for fill in fills {
                let spelled: Vec<String> = inverse
                    .iter()
                    .zip(&fill)
                    .map(|(inv, f)| match (inv, f) {
                        (Some(p), _) => to_rust_ident(p),
                        (None, Some(t)) => t.clone(),
                        (None, None) => "_".to_string(),
                    })
                    .collect();
                self.emit_dispatch_branch(
                    sub, &args, &sub_params, &spelled, base_fqn, trait_path, trait_args, base_params, method,
                    method_generics, params, &mut first,
                );
            }
        }
        self.emit_dispatch_tail(first, method, params, default_twin);
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_dispatch_branch(
        &mut self,
        sub: &str,
        args: &[juxc_ast::TypeRef],
        sub_params: &[String],
        spelled: &[String],
        base_fqn: &str,
        trait_path: &str,
        trait_args: &str,
        base_params: &[String],
        method: &str,
        method_generics: &[String],
        params: &[String],
        first: &mut bool,
    ) {
        {
            let inverse: Vec<Option<String>> = spelled.iter().map(|s| Some(s.clone())).collect();
            let _ = &inverse;
            // Identity: the subtype passes its own parameters straight
            // through, one per base parameter, so it implements THIS trait
            // at THESE arguments.
            let identity = args.len() == base_params.len()
                && args.iter().all(|a| {
                    a.generic_args.is_empty()
                        && a.name.segments.len() == 1
                        && sub_params.contains(&a.name.segments[0].text)
                });
            let mut ty = self.rust_path_for_type_fqn(sub);
            if !inverse.is_empty() {
                ty.push('<');
                ty.push_str(&spelled.join(", "));
                ty.push('>');
            }
            self.w.emit_indent();
            if !*first {
                self.w.push_str("} else ");
            }
            *first = false;
            self.w.push_str(&format!("if let Some(__jux_c) = __jux_any.downcast_ref::<{ty}>() {{\n"));
            self.w.indent_inc();
            self.w.emit_indent();
            if identity {
                self.w.push_str(&format!("<{ty} as {trait_path}{trait_args}>::{}(__jux_c", to_rust_ident(method)));
                for p in params {
                    self.w.push_str(", ");
                    self.w.push_str(p);
                }
                self.w.push_str(")\n");
            } else {
                // Same object, arguments seen through the subtype's own. A
                // method whose type parameter the subtype bounds by the type
                // it fixes (`<V extends Pet>` for `<V extends K>`) is reached
                // through its `__jux_via_` twin, which states the bound as
                // `Into<K>` in the trait's terms (ERRATA E1XX-GAP39c).
                let via = self.via_bound_names(base_fqn, sub, method);
                let name = if via.is_empty() {
                    to_rust_ident(method)
                } else {
                    format!("__jux_via_{}", to_rust_ident(method))
                };
                self.w.push_str(&format!("crate::__jux_seen_as(__jux_c.{name}"));
                if !method_generics.is_empty() {
                    self.w.push_str("::<");
                    let all: Vec<String> = via
                        .iter()
                        .chain(method_generics.iter())
                        .map(|g| to_rust_ident(g))
                        .collect();
                    self.w.push_str(&all.join(", "));
                    self.w.push('>');
                }
                self.w.push('(');
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.w.push_str(&format!("crate::__jux_seen_as({p})"));
                }
                self.w.push_str("))\n");
            }
            self.w.indent_dec();
        }
    }

    /// The last arm of a dispatch chain: the default body, or a failure no
    /// program the checker accepts can reach.
    fn emit_dispatch_tail(&mut self, first: bool, method: &str, params: &[String], default_twin: bool) {
        self.w.emit_indent();
        if first {
            self.w.push_str("{\n");
        } else {
            self.w.push_str("} else {\n");
        }
        self.w.indent_inc();
        if default_twin {
            self.w.emit_indent();
            self.w.push_str(&format!("self.__jux_default_{method}({})\n", params.join(", ")));
        } else {
            self.w.line(&format!(
                "panic!(\"`{method}` was called on a value of no class the program declares\")"
            ));
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.emit_indent();
        self.w.push_str("}");
    }

    /// `method`'s own type parameters, with the bounds a method's get, as a
    /// `<..>` list (empty for a non-generic method).
    pub(crate) fn emit_method_own_generics(&mut self, generics: &[juxc_ast::TypeParam]) {
        if generics.is_empty() {
            return;
        }
        let none = std::collections::HashSet::new();
        self.emit_generic_params_with_bounds(generics, &none);
    }

    /// The names of `params`, for [`Self::emit_generic_dispatch_body`].
    pub(crate) fn names_of(params: &[juxc_ast::TypeParam]) -> Vec<String> {
        params.iter().filter(|p| !p.is_const()).map(|p| p.name.text.clone()).collect()
    }

    /// `generics` (a method's own type parameters, declared on `class_bare`)
    /// with the bound at every position
    /// [`juxc_tycheck::generic_dispatch::overridden_into_positions`] names
    /// rewritten to `Into<bound>`, and the original bound of each rewritten
    /// parameter (ERRATA E1XX-GAP39c). A supertype's `<V extends K>` is
    /// `V: Into<K>` on its trait; the override's `<V extends Pet>` becomes
    /// `V: Into<Pet>`, the trait's bound at `K = Pet`, so the impl asks
    /// exactly what the trait does. Inside the body a `V` is converted to its
    /// bound where a member of it is used ([`Self::collect_into_receiver_spans`]).
    pub(crate) fn lowered_method_generics(
        &self,
        class_bare: &str,
        method: &str,
        generics: &[juxc_ast::TypeParam],
    ) -> (Vec<juxc_ast::TypeParam>, Vec<(String, juxc_ast::TypeRef)>) {
        if generics.is_empty() {
            return (generics.to_vec(), Vec::new());
        }
        let fqn = self
            .resolve_bare_class_fqn(class_bare)
            .unwrap_or_else(|| class_bare.to_string());
        let positions = juxc_tycheck::generic_dispatch::overridden_into_positions(&self.symbols, &fqn, method);
        let mut out = generics.to_vec();
        let mut originals = Vec::new();
        for i in positions {
            let Some(p) = out.get_mut(i) else { continue };
            if p.bounds.len() != 1 {
                continue;
            }
            let b = p.bounds[0].clone();
            // A bound that names a type parameter is E135's `Into` already.
            let is_param = b.generic_args.is_empty()
                && b.name.segments.len() == 1
                && (self.current_type_params.contains(b.name.segments[0].text.as_str())
                    || generics.iter().any(|g| g.name.text == b.name.segments[0].text)
                    || self
                        .lookup_class_by_bare_or_fqn(&fqn)
                        .is_some_and(|c| c.generic_params.iter().any(|g| g.name.text == b.name.segments[0].text)));
            if is_param {
                continue;
            }
            originals.push((p.name.text.clone(), b.clone()));
            let span = b.span;
            p.bounds = vec![juxc_ast::TypeRef {
                name: juxc_ast::QualifiedName {
                    segments: vec![juxc_ast::Ident { text: "Into".to_string(), span }],
                    span,
                },
                generic_args: vec![juxc_ast::GenericArg::Type(b)],
                nullable: false,
                array_shape: None,
                fn_shape: None,
                ptr_depth: 0,
                span,
            }];
        }
        (out, originals)
    }

    /// The own type parameters of a class `Kind` trait member, as the trait,
    /// its impls and the handle's forwarding all state them (ERRATA
    /// E1XX-GAP39c): a bound naming a parameter of the declaring class
    /// (`<V extends K>` in `class Shelf<K>`) is `V: Into<K>`, as on an
    /// interface; a subclass's impl reads it through its `K` (`Into<Pet>`),
    /// which is what the subclass's own override states.
    pub(crate) fn kind_member_generics(&self, sig: &juxc_tycheck::symbol_table::MethodSig) -> Vec<juxc_ast::TypeParam> {
        if sig.generic_params.is_empty() {
            return Vec::new();
        }
        let owner_params: Vec<String> = self
            .symbols
            .classes
            .values()
            .find(|c| c.methods.values().any(|m| m.span == sig.span))
            .map(|c| c.generic_params.iter().map(|p| p.name.text.clone()).collect())
            .unwrap_or_default();
        let mut out = crate::decls::classes::bounds_into_outer_params(&sig.generic_params, &owner_params);
        // Read through the impl's own vocabulary here, at the type level, so
        // `Into<K>` at `K = Pet` spells `Pet` as a value (`Rc<dyn PetKind>`).
        if !self.kind_type_subst.is_empty() {
            for p in &mut out {
                p.bounds = p
                    .bounds
                    .iter()
                    .map(|b| crate::decls::classes::substitute_type_ref(b, &self.kind_type_subst))
                    .collect();
            }
        }
        out
    }

    /// `impl From<C> for Rc<dyn I>` for every concrete non-generic class that
    /// implements the non-generic interface `iface_bare`: the conversion a
    /// `V: Into<I>` bound asks of a `V` (ERRATA E1XX-GAP39c).
    pub(crate) fn emit_iface_from_impls(&mut self, iface_bare: &str, iface_generic: bool) {
        if iface_generic {
            return;
        }
        let Some((iface_fqn, _)) = self.lookup_interface_by_bare_or_fqn(iface_bare).map(|(k, v)| (k.to_string(), v.clone()))
        else {
            return;
        };
        let mut subs: Vec<String> = juxc_tycheck::generic_dispatch::concrete_implementers(&self.symbols, &iface_fqn)
            .into_iter()
            .filter(|s| {
                self.symbols.classes.get(s).is_some_and(|c| c.generic_params.is_empty() && !c.is_struct)
                    || self.symbols.records.get(s).is_some_and(|r| r.generic_params.is_empty())
            })
            .collect();
        subs.sort();
        for sub in subs {
            let path = self.rust_path_for_type_fqn(&sub);
            self.w.line(&format!(
                "impl From<{path}> for std::rc::Rc<dyn {}> {{ fn from(__v: {path}) -> Self {{ std::rc::Rc::new(__v) }} }}",
                to_rust_ident(iface_bare)
            ));
        }
    }

    /// For `sub`'s `method` reached from `base_fqn`'s trait: the base's type
    /// parameter each of `sub`'s `Into`-lowered method parameters stands
    /// for, in order (`[K]` for `<V extends Pet>` overriding `<V extends K>`).
    /// Empty when `sub` lowers none, so the method is called as it is.
    pub(crate) fn via_bound_names(&self, base_fqn: &str, sub: &str, method: &str) -> Vec<String> {
        let sub_bare = crate::backend_fqn::fqn_bare(sub).to_string();
        let Some((sub_sig, _)) = self.symbols.lookup_method(sub, method) else { return Vec::new() };
        let (_, originals) = self.lowered_method_generics(&sub_bare, method, &sub_sig.generic_params);
        if originals.is_empty() {
            return Vec::new();
        }
        let base_sig = self
            .symbols
            .interfaces
            .get(base_fqn)
            .and_then(|i| i.methods.get(method))
            .or_else(|| self.symbols.classes.get(base_fqn).and_then(|c| c.methods.get(method)));
        originals
            .iter()
            .map(|(p, _)| {
                let pos = sub_sig.generic_params.iter().position(|g| g.name.text == *p).unwrap_or(0);
                base_sig
                    .and_then(|s| s.generic_params.get(pos))
                    .and_then(|g| g.bounds.first())
                    .and_then(|b| b.name.segments.last())
                    .map(|s| s.text.clone())
                    .unwrap_or_else(|| "_".to_string())
            })
            .collect()
    }

    /// The `__jux_via_<m>` twin of a method whose type parameters were
    /// lowered to `Into<C>`: the same body, generic over the type each bound
    /// stands for in a supertype's trait (`__JuxK0` for `K`), bounded
    /// `Into<__JuxK0>`, and converting a `V` to `C` by way of it (the two are
    /// the same type whenever the twin is called). The dispatch through a
    /// supertype that fixes `K` calls this, since there `V: Into<Pet>` cannot
    /// be shown, only `V: Into<K>` (ERRATA E1XX-GAP39c).
    pub(crate) fn via_twin(method: &juxc_ast::FnDecl, originals: &[(String, juxc_ast::TypeRef)]) -> (juxc_ast::FnDecl, Vec<(String, String)>) {
        let mut m = method.clone();
        m.name.text = format!("__jux_via_{}", method.name.text);
        let mut via = Vec::new();
        let mut extra = Vec::new();
        for (i, (p, _)) in originals.iter().enumerate() {
            let k = format!("__JuxK{i}");
            via.push((p.clone(), k.clone()));
            let mut tp = method.generic_params.iter().find(|g| g.name.text == *p).cloned().unwrap_or_else(|| method.generic_params[0].clone());
            tp.name.text = k.clone();
            tp.bounds = Vec::new();
            extra.push(tp);
            if let Some(g) = m.generic_params.iter_mut().find(|g| g.name.text == *p) {
                let span = g.name.span;
                let ident = |t: &str| juxc_ast::Ident { text: t.to_string(), span };
                let kref = juxc_ast::TypeRef {
                    name: juxc_ast::QualifiedName { segments: vec![ident(&k)], span },
                    generic_args: Vec::new(),
                    nullable: false,
                    array_shape: None,
                    fn_shape: None,
                    ptr_depth: 0,
                    span,
                };
                g.bounds = vec![juxc_ast::TypeRef {
                    name: juxc_ast::QualifiedName { segments: vec![ident("Into")], span },
                    generic_args: vec![juxc_ast::GenericArg::Type(kref)],
                    nullable: false,
                    array_shape: None,
                    fn_shape: None,
                    ptr_depth: 0,
                    span,
                }];
            }
        }
        extra.extend(m.generic_params);
        m.generic_params = extra;
        (m, via)
    }

    /// The spans, in `body`, of every expression that is the receiver of a
    /// member access (`v.name()`, `v.legs`) and has the type of one of the
    /// `into` parameters, with the bound it converts to: that is the type
    /// that carries the member (ERRATA E1XX-GAP39c).
    pub(crate) fn collect_into_receiver_spans(
        &self,
        body: &juxc_ast::Block,
        into: &[(String, juxc_ast::TypeRef)],
    ) -> std::collections::HashMap<juxc_source::Span, juxc_ast::TypeRef> {
        let mut out = std::collections::HashMap::new();
        if into.is_empty() {
            return out;
        }
        juxc_ast::visit::for_each_expr(body, &mut |e| {
            if let juxc_ast::Expr::Field(f) = e {
                let span = crate::exprs::expr_span_of(&f.object);
                if let Some(juxc_tycheck::Ty::Param(p)) = self.expr_types.get(&span) {
                    if let Some((_, b)) = into.iter().find(|(n, _)| n == p) {
                        out.insert(span, b.clone());
                    }
                }
            }
        });
        out
    }
}
