//! A collection crossing into or out of a worker-shared class (ERRATA
//! E145).
//!
//! A class whose instances cross a worker boundary is `Send`: its handle is
//! the atomic `JuxSync`, and the collections it holds are stored INLINE in the
//! object, protected by the object's own lock (JUX-LANG-V1 §6.5.1, the one
//! place a collection does not alias). Everywhere else a collection is the
//! single-threaded shared handle. The two representations meet wherever a
//! collection passes between code of such a class and any other code: a call
//! of its method or constructor, a read or write of its field.
//!
//! Each crossing converts, and a conversion is a copy: out of the object, the
//! inline collection becomes a new handle; into it, the handle's contents are
//! copied into the inline slot. A copy that is written is a write lost, and
//! [`crate::worker::worker_copy_diagnostics`] refuses that (`E0702`), so a
//! copy the program keeps is only ever read, where it cannot be told from the
//! original. Before this, every such crossing reached rustc as a type error.

use juxc_ast::{Expr, NewObjectExpr};
use juxc_tycheck::Ty;

use crate::RustEmitter;

impl RustEmitter {
    /// Whether the code being emitted belongs to a worker-shared class.
    pub(crate) fn in_sync_class(&self) -> bool {
        self.enclosing_class_fqn().is_some_and(|c| self.sync_class_fqns.contains(&c))
    }

    /// The fully-qualified class `name` names, if it is a program class.
    fn sync_fqn_of(&self, name: &str) -> Option<String> {
        if self.symbols.classes.contains_key(name) {
            return Some(name.to_string());
        }
        self.resolve_bare_class_fqn(name.rsplit('.').next().unwrap_or(name))
    }

    /// Whether `name` is a worker-shared class.
    pub(crate) fn class_is_sync(&self, name: &str) -> bool {
        self.sync_fqn_of(name).is_some_and(|f| self.sync_class_fqns.contains(&f))
    }

    /// Whether `ty` is a collection (a type the single-threaded handle holds
    /// anywhere but inside a worker-shared class).
    fn crossing_ty(&self, ty: Option<&Ty>) -> bool {
        match ty {
            Some(Ty::User { name, .. }) => self.symbols.is_rust_collection(name) && !self.name_is_user_type(name),
            _ => false,
        }
    }

    /// Which side a receiver's class is on: `Some(true)` for a worker-shared
    /// class, `Some(false)` for any other, `None` for `this` (no crossing).
    fn receiver_side(&self, recv: &Expr) -> Option<bool> {
        if matches!(recv, Expr::This(_) | Expr::Super(_)) {
            return None;
        }
        if let Expr::Path(qn) = recv {
            // A static call, `C.m(..)`.
            if let Some(fqn) = self.path_resolves_to_class_in_emit(qn) {
                return Some(self.sync_class_fqns.contains(&fqn));
            }
        }
        match self.receiver_ty_of(recv).map(crate::exprs::field::strip_nullable) {
            Some(Ty::User { name, .. }) => Some(self.class_is_sync(&name)),
            _ => Some(false),
        }
    }

    /// Whether the field read `obj.f` is stored inline in a worker-shared
    /// object, from code that is not that class's own: its collection is a
    /// plain one, reached through the object, never a handle.
    pub(crate) fn field_is_sync_inline(&self, e: &Expr) -> bool {
        if self.sync_class_fqns.is_empty() {
            return false;
        }
        let Expr::Field(f) = e else { return false };
        if !self.crossing_ty(self.expr_types.get(&f.span)) {
            return false;
        }
        match self.receiver_side(&f.object) {
            Some(true) => true,
            Some(false) => false,
            None => self.in_sync_class(),
        }
    }

    /// Emit `expr` converted across the boundary when it crosses one: `true`
    /// when it did (see the module documentation).
    pub(crate) fn emit_sync_boundary(&mut self, expr: &Expr) -> bool {
        let key = Self::erase_key(expr);
        if let Some(to_plain) = self.sync_args.get(&key).copied() {
            if self.sync_now.insert(key) {
                self.emit_sync_conversion(expr, to_plain);
                self.sync_now.remove(&key);
                return true;
            }
            return false;
        }
        if self.sync_now.contains(&key) {
            return false;
        }
        let here = self.in_sync_class();
        let ty = self.expr_types.get(&crate::exprs::expr_span_of(expr)).cloned();
        let crossing_result = match expr {
            Expr::Call(c) => {
                let side = match c.callee.as_ref() {
                    Expr::Field(f) => self.receiver_side(&f.object),
                    // A function, or a method of the enclosing class.
                    Expr::Path(qn) if qn.segments.len() == 1 => {
                        if self.lookup_function_here(&qn.segments[0].text).is_some() {
                            Some(false)
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                if let Some(side) = side {
                    self.mark_sync_call_args(c, side, here);
                }
                side.is_some_and(|s| s != here)
            }
            Expr::NewObject(n) => {
                let name = n.class_name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".");
                let side = self.class_is_sync(&name);
                if side != here {
                    self.mark_sync_ctor_args(n, side);
                }
                false
            }
            Expr::Field(f) => {
                !self.emitting_method_receiver
                    && !self.emitting_lvalue
                    && match self.receiver_side(&f.object) {
                        Some(side) => side != here,
                        None => false,
                    }
            }
            _ => false,
        };
        if !crossing_result || !self.crossing_ty(ty.as_ref()) {
            return false;
        }
        self.sync_now.insert(key);
        self.emit_sync_conversion(expr, here);
        self.sync_now.remove(&key);
        true
    }

    /// `expr` as the plain collection (`to_plain`) or as a new handle.
    fn emit_sync_conversion(&mut self, expr: &Expr, to_plain: bool) {
        if to_plain {
            // Bound in a block of its own, so the guard the copy is read
            // through is gone before the call the copy is handed to runs.
            self.w.push_str("{ let __jux_copy = (");
            self.emit_expr(expr);
            self.w.push_str(").borrow().clone(); __jux_copy }");
        } else {
            self.w.push_str("crate::jux_arr(");
            self.emit_expr(expr);
            self.w.push(')');
        }
    }

    /// Mark the collection arguments of a call into a method on the other
    /// side of the boundary.
    fn mark_sync_call_args(&mut self, c: &juxc_ast::CallExpr, callee_sync: bool, here: bool) {
        if callee_sync == here {
            return;
        }
        for arg in &c.args {
            let ty = self.expr_types.get(&crate::exprs::expr_span_of(arg)).cloned();
            if self.crossing_ty(ty.as_ref()) {
                self.sync_args.insert(Self::erase_key(arg), callee_sync);
            }
        }
    }

    /// The same for a constructor.
    fn mark_sync_ctor_args(&mut self, n: &NewObjectExpr, callee_sync: bool) {
        for arg in &n.args {
            let ty = self.expr_types.get(&crate::exprs::expr_span_of(arg)).cloned();
            if self.crossing_ty(ty.as_ref()) {
                self.sync_args.insert(Self::erase_key(arg), callee_sync);
            }
        }
    }

    /// Mark the value stored into `obj.f`, when the field is on the other
    /// side of the boundary.
    pub(crate) fn mark_sync_field_store(&mut self, target: &Expr, value: &Expr) {
        if self.sync_class_fqns.is_empty() {
            return;
        }
        let Expr::Field(f) = target else { return };
        let here = self.in_sync_class();
        let Some(side) = self.receiver_side(&f.object) else { return };
        if side == here {
            return;
        }
        let ty = self.expr_types.get(&crate::exprs::expr_span_of(value)).cloned();
        if self.crossing_ty(ty.as_ref()) {
            self.sync_args.insert(Self::erase_key(value), side);
        }
    }
}
