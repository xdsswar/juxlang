//! A free function named as a VALUE (§M.8: a function is a first-class value
//! of its function type).
//!
//! A Jux function type `(int) -> int` lowers to `Rc<dyn Fn(isize) -> isize>`,
//! the same representation every lambda gets. A bare Rust fn item is not that
//! type, so `apply(twice, 3)` passed `twice` straight through and rustc
//! rejected it (E0308). The name is boxed instead:
//!
//! ```rust,ignore
//! apply(std::rc::Rc::new(twice), 3)
//! ```
//!
//! `Rc<fn item>` coerces to `Rc<dyn Fn(..)>` wherever the target type is known
//! (an argument, a typed local, a field, a return), so the plain name is all
//! the value needs. A function whose signature differs from its function type
//! (a parameter lowered to `&mut T` by the C6 by-reference rule) is wrapped in
//! a forwarding closure that lends each such argument:
//!
//! ```rust,ignore
//! std::rc::Rc::new(move |mut __a0: Grid| fill(&mut __a0))
//! ```
//!
//! A foreign `impl Fn(..)` parameter (§G.3) takes the fn item itself, with no
//! `Rc`, exactly as a lambda there is emitted bare.

use juxc_lex::to_rust_ident;

use crate::RustEmitter;

impl RustEmitter {
    /// Emit the bare name `name` as a function VALUE when it names a Jux free
    /// function and nothing closer (a parameter, a local, a pointer local)
    /// shadows it. Returns `false`, writing nothing, for every other name and
    /// for a callee, which `emit_call` resolves on its own.
    pub(crate) fn emit_free_fn_value(&mut self, name: &str) -> bool {
        if self.emitting_call_callee || self.emitting_lvalue {
            return false;
        }
        let shadowed = self.current_fn_params.contains(name)
            || self.pointer_locals.contains_key(name)
            || self.local_types.iter().any(|s| s.contains_key(name));
        if shadowed {
            return false;
        }
        // The same resolution a call makes: an import or same-package name
        // through the unit's table first, then a unique bare name.
        let sig = self
            .current_unit_idx
            .and_then(|i| self.symbols.units.get(i))
            .and_then(|ctx| ctx.unqualified.get(name))
            .and_then(|fqn| self.symbols.functions.get(fqn))
            .or_else(|| self.symbols.lookup_function(name).map(|(_, f)| f));
        let Some(sig) = sig else {
            return false;
        };
        // Only a Jux function with a body has the plain Rust signature this
        // relies on. A foreign stub (`rust_path`, a C `native` function) keeps
        // its own calling convention, and a generic one has no type arguments
        // to name here; both keep the old spelling.
        if sig.body.is_none()
            || sig.rust_path.is_some()
            || sig.is_extern_c
            || !sig.generic_params.is_empty()
        {
            return false;
        }
        // `ref` / `out` / varargs parameters change the Rust signature in ways
        // a function type cannot express; tycheck decides whether such a
        // function is a value at all, and the name is left as written.
        if sig
            .params
            .iter()
            .any(|p| p.is_ref || p.is_mut_ref || p.is_out || p.is_shared_ref || p.is_varargs)
        {
            return false;
        }
        let ident = to_rust_ident(name);
        // A foreign `impl Fn(..)` slot takes the fn item itself (§G.3).
        if std::mem::take(&mut self.lambda_bare_target) {
            self.w.push_str(&ident);
            return true;
        }
        let byref = self.byref_params.get(&format!("fn::{name}")).cloned().unwrap_or_default();
        if byref.is_empty() {
            self.w.push_str("std::rc::Rc::new(");
            self.w.push_str(&ident);
            self.w.push(')');
            return true;
        }
        // Some parameter is `&mut T` in the Rust signature: forward through a
        // closure that takes the value and lends it.
        let params = sig.params.clone();
        self.w.push_str("std::rc::Rc::new(move |");
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            if byref.contains(&i) {
                self.w.push_str("mut ");
            }
            self.w.push_str(&format!("__a{i}: "));
            self.emit_value_type_as_rust(&p.ty);
        }
        self.w.push_str("| ");
        self.w.push_str(&ident);
        self.w.push('(');
        for i in 0..params.len() {
            if i > 0 {
                self.w.push_str(", ");
            }
            if byref.contains(&i) {
                self.w.push_str("&mut ");
            }
            self.w.push_str(&format!("__a{i}"));
        }
        self.w.push_str("))");
        true
    }
}
