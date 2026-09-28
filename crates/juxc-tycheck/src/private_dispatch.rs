//! Private members reached through a class's dispatch value (ERRATA
//! E139).
//!
//! A value typed as a class with subclasses lowers to its dispatch trait
//! object, which reaches the object's members only through the trait: a
//! non-private field through a getter and setter, a non-private method as a
//! trait method. A private member has no slot there. Yet Java lets code in
//! the class's own body reach a private member through any value of the
//! class, and an anonymous class written in the class does exactly that
//! with the enclosing object it holds (`hits += x` in a listener built by an
//! abstract `Widget`, ERRATA E138).
//!
//! This pass finds each such access in the checked program and gives the
//! member a hidden, non-private stand-in on the declaring class: a property
//! `__jux_priv_<Class>_<field>` over the field, a method
//! `__jux_priv_<Class>_<method>` that calls the method. Being non-private,
//! each lands on the dispatch trait like any member, is implemented by the
//! class and reached from each subclass through what it inherits. The
//! access is renamed to the stand-in and the program is checked again. It
//! runs only when the program is compiled, so an editor never sees the
//! hidden names.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};

use juxc_ast::{CompilationUnit, Expr};
use juxc_source::Span;

use crate::{SymbolTable, Ty, TypeCheckResult};

/// The prefix of every hidden stand-in's name.
pub const PRIVATE_PREFIX: &str = "__jux_priv_";

/// One private member reached through a dispatch value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PrivateAccess {
    /// The FQN of the class that declares it.
    pub class: String,
    /// The member's name.
    pub member: String,
    /// A method rather than a field.
    pub is_method: bool,
}

impl PrivateAccess {
    /// The stand-in's name: `__jux_priv_<Class>_<member>`.
    pub fn hidden_name(&self) -> String {
        hidden_name(&self.class, &self.member)
    }
}

fn hidden_name(class: &str, member: &str) -> String {
    let bare = class.rsplit('.').next().unwrap_or(class);
    format!("{PRIVATE_PREFIX}{bare}_{member}")
}

/// What the renaming walk needs, and what it found.
pub(crate) struct PrivateDispatch<'a> {
    symbols: &'a SymbolTable,
    expr_types: &'a HashMap<Span, Ty>,
    poly: HashSet<String>,
    pub(crate) found: RefCell<BTreeSet<PrivateAccess>>,
}

impl PrivateDispatch<'_> {
    /// The stand-in `f`'s member is renamed to, when `f` reaches a private
    /// member through a dispatch value. As a call's callee it names a method
    /// first; anywhere else, only a field.
    pub(crate) fn rename(&self, f: &juxc_ast::FieldExpr, as_callee: bool) -> Option<String> {
        if matches!(f.object.as_ref(), Expr::This(_) | Expr::Super(_)) {
            return None;
        }
        let ty = self.expr_types.get(&crate::check::expr_span_pub(&f.object))?;
        let ty = match ty {
            Ty::Nullable(inner) => inner.as_ref(),
            other => other,
        };
        let Ty::User { name, .. } = ty else { return None };
        let bare = name.rsplit('.').next().unwrap_or(name);
        if !self.poly.contains(bare) {
            return None;
        }
        let member = &f.field.text;
        let method = if as_callee { self.symbols.lookup_method(name, member) } else { None };
        let access = if let (None, Some((field, decl))) = (&method, self.symbols.lookup_field(name, member)) {
            if field.is_static || !matches!(field.visibility, juxc_ast::Visibility::Private) {
                return None;
            }
            PrivateAccess { class: decl.to_string(), member: member.clone(), is_method: false }
        } else if let Some((method, decl)) = method {
            // An overloaded method: the overload this call picked decides
            // (ERRATA E1XX-GAP39f). Each private overload has a stand-in of
            // its own, all under the one hidden name, so the stand-ins are an
            // overload group of their own and the call picks among them as it
            // picked here.
            let group = self.symbols.merged_method_overloads(name, member);
            let picked = self
                .symbols
                .method_selections
                .get(&f.span)
                .and_then(|&k| group.get(k).cloned())
                .unwrap_or_else(|| method.clone());
            if picked.is_static || !matches!(picked.visibility, juxc_ast::Visibility::Private) {
                return None;
            }
            PrivateAccess { class: decl.to_string(), member: member.clone(), is_method: true }
        } else {
            return None;
        };
        let hidden = access.hidden_name();
        self.found.borrow_mut().insert(access);
        Some(hidden)
    }
}

/// Rename every private member reached through a dispatch value to its
/// stand-in, and return the stand-ins the program needs (the caller declares
/// them and checks the program again). Empty when there are none.
pub fn rename_accesses(units: &mut [CompilationUnit], typed: &TypeCheckResult) -> Vec<PrivateAccess> {
    let walk = PrivateDispatch {
        symbols: &typed.symbols,
        expr_types: &typed.expr_types,
        poly: crate::symbol_table::polymorphic_base_bare_names(&typed.symbols),
        found: RefCell::new(BTreeSet::new()),
    };
    if walk.poly.is_empty() {
        return Vec::new();
    }
    crate::expand::rename_private_dispatch(units, &walk);
    let found = walk.found.into_inner();
    found.into_iter().collect()
}
