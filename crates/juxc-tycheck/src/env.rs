//! Phase B of the type checker — the **local type environment**.
//!
//! A [`TypeEnv`] is a scope-stack of `name → Ty` maps used by Phase C
//! (expression inference) to look up variable types. Scopes nest as
//! the walker descends into nested blocks (`if`, `while`, `for-each`,
//! switch arms), and pop back off when the walker leaves.
//!
//! Beyond the local-binding stack, [`TypeEnv`] also carries:
//!
//! - `current_class`: the name of the enclosing class while walking a
//!   method body, so `Expr::This` can resolve to a [`crate::Ty::User`].
//! - `generic_params`: the set of generic-parameter names currently in
//!   scope, so a `TypeRef` mentioning `T` inside `class Box<T>` lowers
//!   to [`crate::Ty::Param`] rather than [`crate::Ty::Unknown`].
//!
//! All state mutation happens through the methods on [`TypeEnv`] —
//! direct field access is supported for the two non-stack pieces (so
//! callers can borrow them shared-immutably while iterating), but the
//! scope stack itself is private.

use std::collections::{HashMap, HashSet};

use crate::ty::Ty;

/// Local type environment built up by the Phase B walker. See module
/// docs for the high-level picture.
///
/// **Invariant**: the scope stack is never empty. [`Self::pop_scope`]
/// is a no-op when only the root scope remains.
#[derive(Debug, Default)]
pub struct TypeEnv {
    /// Stack of nested scopes. Innermost on top. The root entry is the
    /// function/method's parameter scope.
    scopes: Vec<HashMap<String, Ty>>,
    /// Raw-pointer depth of the names in the matching scope of [`Self::scopes`]
    /// (`int*` is 1, `int**` is 2). `Ty` erases `ptr_depth`, and the `unsafe`
    /// gate on `p[i]` / `p + n` (§L.6.2) needs to know which names are
    /// pointers. A name absent here is not one.
    ptr_depths: Vec<HashMap<String, u8>>,
    /// Names bound as `ref` (JUX-MISSING-DEFS §M.13), one set per scope,
    /// parallel to `scopes`. A `ref` binding's TYPE is the plain `T`, so
    /// nothing in the type tells a caller that the name is a shared cell:
    /// the `Worker.spawn` gate (E0702) needs to know, because an `Rc` cell
    /// cannot cross a thread.
    ref_binds: Vec<HashSet<String>>,
    /// The names in the matching scope that are pointers to `void`. `Ty`
    /// lowers `void` to `Unknown`, so this is how `void*` stays distinct from
    /// a typed pointer (§L.6.1a).
    void_bases: Vec<HashSet<String>>,
    /// The names in the matching scope that were DECLARED with a fixed-size
    /// array type (`int[3] a`, `int[N] xs`), and whether each is a parameter.
    /// `Ty` says `T[N]` for a `var a = new int[3]` too, but only a declared
    /// `T[N]` has fixed storage, which is what JUX-LANG-V1 §5.5 cares about
    /// when the array is handed to a `T[]` slot (E0468).
    fixed_arrays: Vec<HashMap<String, bool>>,
    /// Name of the class whose method body we're currently inside —
    /// `None` at top level, `Some("Foo")` while walking `class Foo`'s
    /// method bodies. Drives `Expr::This` inference. Stored as the
    /// **fully-qualified name** so cross-package lookups work; e.g.
    /// `Some("a.lib.Foo")` for a class in package `a.lib`.
    pub current_class: Option<String>,
    /// Generic-parameter names currently in scope. Includes the
    /// surrounding class/record's params **and** the current method's
    /// params (if any). Cleared between methods.
    pub generic_params: HashSet<String>,
    /// The declared bounds of the generic parameters in [`Self::generic_params`]
    /// that have any (`<T extends Auto>` → `T -> [Auto]`).
    pub generic_bounds: HashMap<String, Vec<juxc_ast::TypeRef>>,
    /// Dotted package path of the unit currently being checked —
    /// e.g. `["a", "lib"]` for `package a.lib;`. Empty for the
    /// crate-root (no-package) case. Drives the bare-name → FQN
    /// resolution rule "look in the current package first".
    pub current_package: Vec<String>,
    /// Bare-name → FQN map built from the unit's `package` and
    /// `import` declarations. Populated once at the start of each
    /// unit's tycheck and consulted by `ty_from_ref` when a single-
    /// segment type reference is encountered. Empty when no package
    /// or imports apply (top-level single-unit builds).
    pub unqualified: HashMap<String, String>,
    /// Names of `weak` parameters (§M.14.3) in the current function/method.
    /// A weak param's `lookup` type is its class `T` (for `.get()` typing and
    /// the E0456 bare-read gate), but it is physically a `Weak<…>` handle — so
    /// `name.get()` infers to `T?` and a bare read of `name` is rejected.
    /// Repopulated per body; not scoped (params live for the whole body).
    pub weak_names: HashSet<String>,
    /// When a MEMBER signature is lowered from outside its declaration (a
    /// call to `scene.render()` typing its `Pixmap` return), the index of the
    /// unit that declared it: a bare name there means what that unit's own
    /// imports say (`import rust.tiny_skia.Pixmap;` in `Scene.jux`), not what
    /// the caller's do.
    pub declaring_unit: Option<usize>,
}

impl TypeEnv {
    /// Create a fresh environment with one (empty) root scope, no
    /// current class, and no generic parameters.
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            ptr_depths: vec![HashMap::new()],
            ref_binds: vec![HashSet::new()],
            void_bases: vec![HashSet::new()],
            fixed_arrays: vec![HashMap::new()],
            current_class: None,
            generic_params: HashSet::new(),
            generic_bounds: HashMap::new(),
            current_package: Vec::new(),
            unqualified: HashMap::new(),
            weak_names: HashSet::new(),
            declaring_unit: None,
        }
    }

    /// Push a fresh scope onto the stack. Pair with [`Self::pop_scope`]
    /// whenever a syntactic block begins — `{`, the body of an `if`,
    /// the body of a loop, a switch arm.
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.ptr_depths.push(HashMap::new());
        self.ref_binds.push(HashSet::new());
        self.void_bases.push(HashSet::new());
        self.fixed_arrays.push(HashMap::new());
    }

    /// Pop the innermost scope. Silently does nothing when only the
    /// root scope remains — preserves the never-empty invariant so
    /// callers don't have to track depth precisely.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
            self.ptr_depths.pop();
            self.ref_binds.pop();
            self.void_bases.pop();
            self.fixed_arrays.pop();
        }
    }

    /// Declare `name` with type `ty` in the **innermost** scope. If a
    /// binding with the same name already lives in that scope it is
    /// overwritten — the type checker treats redeclaration at the same
    /// scope as the user's intent (shadowing diagnostics come later).
    pub fn declare(&mut self, name: &str, ty: Ty) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_string(), ty);
        }
        // A new declaration is not a pointer until `declare_pointer` says so,
        // so a non-pointer that shadows a pointer is not mistaken for one.
        if let Some(top) = self.ptr_depths.last_mut() {
            top.remove(name);
        }
        if let Some(top) = self.void_bases.last_mut() {
            top.remove(name);
        }
        if let Some(top) = self.fixed_arrays.last_mut() {
            top.remove(name);
        }
    }

    /// Record that `name`, just declared in the innermost scope, was declared
    /// with a fixed-size array type; `is_param` tells a parameter from a local.
    pub fn declare_fixed_array(&mut self, name: &str, is_param: bool) {
        if let Some(top) = self.fixed_arrays.last_mut() {
            top.insert(name.to_string(), is_param);
        }
    }

    /// For the binding `name` resolves to: `Some(is_param)` when it was
    /// declared `T[N]`, `None` otherwise.
    pub fn fixed_array(&self, name: &str) -> Option<bool> {
        for (i, scope) in self.scopes.iter().enumerate().rev() {
            if scope.contains_key(name) {
                return self.fixed_arrays.get(i).and_then(|f| f.get(name)).copied();
            }
        }
        None
    }

    /// Record that `name`, just declared in the innermost scope, is a pointer
    /// to `void` (`void*`, `void**`), whose pointee has no type.
    pub fn declare_void_base(&mut self, name: &str) {
        if let Some(top) = self.void_bases.last_mut() {
            top.insert(name.to_string());
        }
    }

    /// Whether the binding `name` resolves to is a pointer to `void`.
    pub fn is_void_base(&self, name: &str) -> bool {
        for (i, scope) in self.scopes.iter().enumerate().rev() {
            if scope.contains_key(name) {
                return self.void_bases.get(i).is_some_and(|v| v.contains(name));
            }
        }
        false
    }

    /// Record that `name`, just declared in the innermost scope, is a raw
    /// pointer of `depth` levels. A depth of 0 records nothing.
    pub fn declare_pointer(&mut self, name: &str, depth: u8) {
        if depth == 0 {
            return;
        }
        if let Some(top) = self.ptr_depths.last_mut() {
            top.insert(name.to_string(), depth);
        }
    }

    /// Record that `name` is a `ref` binding in the current scope.
    pub fn declare_ref_binding(&mut self, name: &str) {
        if let Some(top) = self.ref_binds.last_mut() {
            top.insert(name.to_string());
        }
    }

    /// Whether `name` resolves to a `ref` binding (§M.13): a local, parameter
    /// or field declared `ref`, whose slot is a shared cell.
    pub fn is_ref_binding(&self, name: &str) -> bool {
        for (i, scope) in self.scopes.iter().enumerate().rev() {
            if scope.contains_key(name) {
                return self.ref_binds.get(i).is_some_and(|s| s.contains(name));
            }
        }
        false
    }

    /// The raw-pointer depth of the binding `name` resolves to, or 0 when it
    /// is not a pointer (or not bound).
    pub fn pointer_depth(&self, name: &str) -> u8 {
        for (i, scope) in self.scopes.iter().enumerate().rev() {
            if scope.contains_key(name) {
                return self.ptr_depths.get(i).and_then(|d| d.get(name)).copied().unwrap_or(0);
            }
        }
        0
    }

    /// Look up `name` from innermost scope outward. Returns `None` when
    /// the name isn't bound anywhere — caller decides how to react.
    pub fn lookup(&self, name: &str) -> Option<&Ty> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty);
            }
        }
        None
    }

    /// The type `name` was DECLARED with, when an inner scope has since
    /// rebound it to a refinement (a null check, a type test, a guard clause
    /// narrows a name by declaring it again with the narrower type). `None`
    /// when the name is bound once, or not at all: nothing is refined.
    pub fn declared_type_under_refinement(&self, name: &str) -> Option<&Ty> {
        let mut bindings = self.scopes.iter().filter_map(|scope| scope.get(name));
        let outermost = bindings.next()?;
        let innermost = bindings.next_back()?;
        (outermost != innermost).then_some(outermost)
    }

    /// Enter a class context. Replaces any previous `current_class`.
    pub fn set_class(&mut self, name: &str) {
        self.current_class = Some(name.to_string());
    }

    /// Leave the current class context. Idempotent — calling with no
    /// class set is fine.
    pub fn clear_class(&mut self) {
        self.current_class = None;
    }

    /// Register a generic parameter as in-scope. Names are stored as
    /// owned `String`s so the env can outlive the AST that produced
    /// them — useful for borrowing the env across method bodies.
    pub fn add_generic_param(&mut self, name: &str) {
        self.generic_params.insert(name.to_string());
    }

    /// Bring a generic parameter into scope together with its declared
    /// bounds (`<T extends Auto>`), so a member used on a `T` can be checked
    /// against what the bound provides.
    pub fn add_generic_param_bounded(&mut self, name: &str, bounds: &[juxc_ast::TypeRef]) {
        self.generic_params.insert(name.to_string());
        if bounds.is_empty() {
            self.generic_bounds.remove(name);
        } else {
            self.generic_bounds.insert(name.to_string(), bounds.to_vec());
        }
    }

    /// Clear all in-scope generic parameters. Call when leaving a
    /// generic class/method to restore the previous (non-generic)
    /// state.
    pub fn clear_generic_params(&mut self) {
        self.generic_params.clear();
        self.generic_bounds.clear();
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::{Primitive, Ty};

    /// A fresh env starts with one scope, no class, no generic params.
    #[test]
    fn new_env_is_empty_with_one_scope() {
        let env = TypeEnv::new();
        assert_eq!(env.scopes.len(), 1);
        assert!(env.current_class.is_none());
        assert!(env.generic_params.is_empty());
        assert!(env.lookup("x").is_none());
    }

    /// declare → lookup round-trips in the same scope.
    #[test]
    fn declare_then_lookup_returns_ty() {
        let mut env = TypeEnv::new();
        env.declare("x", Ty::Primitive(Primitive::Int));
        assert_eq!(env.lookup("x"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// Inner scope shadows outer; pop restores the outer binding.
    #[test]
    fn inner_scope_shadows_outer() {
        let mut env = TypeEnv::new();
        env.declare("x", Ty::Primitive(Primitive::Int));
        env.push_scope();
        env.declare("x", Ty::String);
        assert_eq!(env.lookup("x"), Some(&Ty::String));
        env.pop_scope();
        assert_eq!(env.lookup("x"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// pop_scope is a no-op once we're back at the root.
    #[test]
    fn pop_scope_preserves_root() {
        let mut env = TypeEnv::new();
        env.pop_scope();
        env.pop_scope();
        assert_eq!(env.scopes.len(), 1);
    }

    /// set_class / clear_class round-trip.
    #[test]
    fn set_and_clear_class() {
        let mut env = TypeEnv::new();
        env.set_class("Foo");
        assert_eq!(env.current_class.as_deref(), Some("Foo"));
        env.clear_class();
        assert!(env.current_class.is_none());
    }

    /// add_generic_param tracks the name; clear empties the set.
    #[test]
    fn generic_params_track_and_clear() {
        let mut env = TypeEnv::new();
        env.add_generic_param("T");
        env.add_generic_param("U");
        assert!(env.generic_params.contains("T"));
        assert!(env.generic_params.contains("U"));
        env.clear_generic_params();
        assert!(env.generic_params.is_empty());
    }
}
