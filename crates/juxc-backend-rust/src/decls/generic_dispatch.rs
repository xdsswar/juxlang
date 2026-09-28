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
            if inverse.iter().any(|x| x.is_none()) {
                continue;
            }
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
                ty.push_str(&inverse.iter().map(|x| to_rust_ident(x.as_deref().unwrap_or("_"))).collect::<Vec<_>>().join(", "));
                ty.push('>');
            }
            self.w.emit_indent();
            if !first {
                self.w.push_str("} else ");
            }
            first = false;
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
                // Same object, arguments seen through the subtype's own.
                self.w.push_str(&format!("crate::__jux_seen_as(__jux_c.{}", to_rust_ident(method)));
                if !method_generics.is_empty() {
                    self.w.push_str("::<");
                    self.w.push_str(&method_generics.iter().map(|g| to_rust_ident(g)).collect::<Vec<_>>().join(", "));
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
}
