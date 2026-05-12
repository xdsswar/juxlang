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
}

impl TypeEnv {
    /// Create a fresh environment with one (empty) root scope, no
    /// current class, and no generic parameters.
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            current_class: None,
            generic_params: HashSet::new(),
            current_package: Vec::new(),
            unqualified: HashMap::new(),
        }
    }

    /// Push a fresh scope onto the stack. Pair with [`Self::pop_scope`]
    /// whenever a syntactic block begins — `{`, the body of an `if`,
    /// the body of a loop, a switch arm.
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pop the innermost scope. Silently does nothing when only the
    /// root scope remains — preserves the never-empty invariant so
    /// callers don't have to track depth precisely.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
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

    /// Clear all in-scope generic parameters. Call when leaving a
    /// generic class/method to restore the previous (non-generic)
    /// state.
    pub fn clear_generic_params(&mut self) {
        self.generic_params.clear();
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
