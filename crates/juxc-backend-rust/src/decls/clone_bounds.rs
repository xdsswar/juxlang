//! Writing the `Clone + Debug` bounds a generic declaration's members need
//! (GAPS.md gap 2, ERRATA E118 and E120).
//!
//! Every generic declaration used to lower each parameter with the literal
//! bound `Clone + std::fmt::Debug + 'static`, so `Cell<File>` over
//! `class Cell<T> { T value; }` did not compile: `std::fs::File` is not
//! `Clone`. The bound MOVES instead, from the declaration to the members that
//! need it. Which parameters move and what each member needs back is decided
//! once, by the checker (`juxc_tycheck::clone_needs`), and stored on the
//! symbol table: the checker reports a use that needs a bound its type argument
//! lacks (`E0457`), and this module writes exactly the clauses that table
//! says, so the two cannot disagree.
//!
//! While a relaxed declaration is being emitted its relaxed parameters are
//! the [`RelaxedScope`]. Every header written in that scope (struct heads,
//! the inherent impl, trait declarations and impls, the identity impls)
//! carries `'static` alone for them (records and enums also keep `Debug`, see
//! `CloneNeeds::keeps_debug`), and every function item states
//! `where T: Clone + std::fmt::Debug` for the relaxed parameters its member
//! needs ([`RustEmitter::relaxed_where`]). A function item this module does
//! not know states every relaxed parameter, which is the old baseline moved
//! onto that one item: never unsound, only less permissive.

use std::collections::{HashMap, HashSet};

use juxc_ast::TypeRef;
use juxc_source::Span;

use crate::RustEmitter;

/// The relaxed parameters of the declaration being emitted.
#[derive(Debug, Clone, Default)]
pub(crate) struct RelaxedScope {
    /// FQN of the declaration.
    pub(crate) fqn: String,
    /// Its relaxed parameters, in declaration order.
    pub(crate) order: Vec<String>,
    /// The same, as a set, for the header emitters.
    pub(crate) set: HashSet<String>,
    /// A record or enum: its relaxed parameters keep `Debug` on the headers.
    pub(crate) keep_debug: bool,
}

impl RustEmitter {
    /// The relaxed parameters of declaration `fqn`, in declaration order.
    pub(crate) fn relaxed_class_params(&self, fqn: &str) -> Vec<String> {
        self.symbols.clone_needs.relaxed_params(fqn).to_vec()
    }

    /// Enter declaration `fqn`'s relaxed scope, returning the scope it
    /// replaces (restore it with [`Self::leave_relaxed_scope`]). `None`, or a
    /// declaration with nothing relaxed, enters the empty scope: every header
    /// then carries the full baseline, as before gap 2.
    pub(crate) fn enter_relaxed_scope(&mut self, fqn: Option<&str>) -> RelaxedScope {
        let next = match fqn {
            Some(fqn) => {
                let order = self.relaxed_class_params(fqn);
                RelaxedScope {
                    fqn: fqn.to_string(),
                    set: order.iter().cloned().collect(),
                    keep_debug: self.symbols.clone_needs.keeps_debug(fqn),
                    order,
                }
            }
            None => RelaxedScope::default(),
        };
        std::mem::replace(&mut self.relaxed_scope, next)
    }

    /// Restore the scope [`Self::enter_relaxed_scope`] replaced.
    pub(crate) fn leave_relaxed_scope(&mut self, prev: RelaxedScope) {
        self.relaxed_scope = prev;
    }

    /// Run `f` with no parameter relaxed: the items it writes carry the full
    /// baseline (a lifted static function, whose callers name no instance).
    pub(crate) fn with_baseline_scope(&mut self, f: impl FnOnce(&mut Self)) {
        let prev = self.enter_relaxed_scope(None);
        f(self);
        self.leave_relaxed_scope(prev);
    }

    /// The `where` clause (with its leading space, or empty) for a function
    /// item emitting the member declared at `span`.
    ///
    /// The member's needs are in its OWNER's vocabulary. When the owner is the
    /// declaration being emitted they apply as they are. When it is another
    /// one (an inherited method copied into a subclass, a parent's `Kind`
    /// member implemented by a child, an interface member implemented by a
    /// class), `subst` maps the owner's parameters into this declaration's
    /// types: a parameter passed bare carries its need across, one bound to
    /// anything else needs nothing here (a concrete type meets or fails the
    /// bound where it is written, which the checker reports, and a parameter
    /// nested in a type argument keeps the baseline). A member that hands
    /// `this` on and is copied out of its owner needs every relaxed parameter.
    /// An unknown span needs every relaxed parameter.
    pub(crate) fn relaxed_where(&self, span: Span, subst: Option<&HashMap<String, TypeRef>>) -> String {
        Self::where_text(self.relaxed_where_parts(span, subst))
    }

    /// [`Self::relaxed_where`] as its separate predicates, for an item that
    /// states `where` predicates of its own to merge them with.
    pub(crate) fn relaxed_where_parts(&self, span: Span, subst: Option<&HashMap<String, TypeRef>>) -> Vec<String> {
        let scope = &self.relaxed_scope;
        if scope.order.is_empty() {
            return Vec::new();
        }
        // Safe level (gap 34, sweep C): the table's own answer for a member it
        // does not know, every relaxed parameter, as far as the program's
        // instantiations allow: a parameter every one of them binds to a type
        // that is surely `Clone + Debug`. The test hook `clone` reverts the
        // table's entry for the member to "needs nothing" (the shape gap 2's
        // bound moves had before E120), which only this answer overrides.
        // Not for a member that implements or overrides another's: its
        // trait's declaration states the clause the table gives that other
        // member, and an impl may not ask for more (rustc E0276).
        if self.safe_at(span) && !self.member_overrides_another(span) {
            let allowed = self.clone_debug_params_allowed(&scope.fqn, &scope.order);
            let mut wanted: HashSet<String> = allowed.into_iter().collect();
            if !self.broken_by(crate::BreakKind::Clone, span) {
                wanted.extend(self.relaxed_member_params(span, subst));
            }
            return Self::predicates(scope.order.iter().filter(|p| wanted.contains(*p)));
        }
        if self.broken_by(crate::BreakKind::Clone, span) {
            return Vec::new();
        }
        let wanted = self.relaxed_member_params(span, subst);
        Self::predicates(scope.order.iter().filter(|p| wanted.contains(*p)))
    }

    /// The relaxed parameters the table says the member at `span` states.
    fn relaxed_member_params(&self, span: Span, subst: Option<&HashMap<String, TypeRef>>) -> HashSet<String> {
        let scope = &self.relaxed_scope;
        match self.symbols.clone_needs.member(span) {
            None => scope.set.clone(),
            Some(n) if n.owner == scope.fqn => n.params.iter().cloned().collect(),
            Some(n) => {
                if n.touches_self {
                    scope.set.clone()
                } else {
                    let mut out = HashSet::new();
                    for p in &n.params {
                        let Some(to) = subst.and_then(|s| s.get(p)) else { continue };
                        if let Some(q) = scope.order.iter().find(|q| juxc_tycheck::clone_needs::is_bare_param(to, q)) {
                            out.insert(q.clone());
                        }
                    }
                    out
                }
            }
        }
    }

    /// Whether the member declared at `span` is an interface's own method (a
    /// default body, a trait item), or implements or overrides a method of an
    /// interface or ancestor its owner has: a member whose `where` clause a
    /// trait's declaration fixes.
    fn member_overrides_another(&self, span: Span) -> bool {
        let Some(member) = self.symbols.clone_needs.member(span) else { return true };
        let name = member.name.as_str();
        if self.symbols.interfaces.contains_key(&member.owner) {
            return true;
        }
        let mut ifaces: Vec<String> = Vec::new();
        let mut class = self.symbols.classes.get(&member.owner);
        let mut first = true;
        for _ in 0..64 {
            let Some(c) = class else { break };
            if !first && c.methods.contains_key(name) {
                return true;
            }
            first = false;
            for t in &c.implements {
                if let Some(seg) = t.name.segments.last() {
                    ifaces.push(seg.text.clone());
                }
            }
            class = c.extends_fqn.as_ref().and_then(|p| self.symbols.classes.get(p));
        }
        if let Some(r) = self.symbols.records.get(&member.owner) {
            for t in &r.implements {
                if let Some(seg) = t.name.segments.last() {
                    ifaces.push(seg.text.clone());
                }
            }
        }
        let mut seen: HashSet<String> = HashSet::new();
        while let Some(i) = ifaces.pop() {
            if !seen.insert(i.clone()) {
                continue;
            }
            let Some((_, sig)) = self.lookup_interface_by_bare_or_fqn(&i) else { continue };
            if sig.methods.contains_key(name) {
                return true;
            }
            for t in &sig.extends {
                if let Some(seg) = t.name.segments.last() {
                    ifaces.push(seg.text.clone());
                }
            }
        }
        false
    }

    /// The parameters of declaration `fqn` (out of `params`) that every
    /// instantiation the program builds binds to a type that is surely `Clone
    /// + Debug`: the ones a safe member may state without refusing a caller.
    /// A declaration with no recorded instantiation (one reached only through
    /// a subclass's `extends`, say) states nothing more.
    fn clone_debug_params_allowed(&self, fqn: &str, params: &[String]) -> Vec<String> {
        let declared: Vec<String> = self
            .symbols
            .classes
            .get(fqn)
            .map(|c| c.generic_params.iter().map(|p| p.name.text.clone()).collect())
            .or_else(|| self.symbols.records.get(fqn).map(|r| r.generic_params.iter().map(|p| p.name.text.clone()).collect()))
            .or_else(|| self.symbols.enums.get(fqn).map(|e| e.generic_params.iter().map(|p| p.name.text.clone()).collect()))
            .unwrap_or_default();
        let insts = self.symbols.instantiations.classes.get(fqn).cloned().unwrap_or_default();
        params
            .iter()
            .filter(|p| {
                let Some(i) = declared.iter().position(|d| d == *p) else { return false };
                !insts.is_empty() && insts.iter().all(|args| args.get(i).is_some_and(|t| self.surely_clone_debug(t)))
            })
            .cloned()
            .collect()
    }

    /// The clause for a member of a `Kind` trait (its declaration, a
    /// delegating impl, the `Rc` forwarding impl). Inside an ancestor's impl
    /// the member's needs are read through the ancestor-to-this-class map
    /// (`kind_type_subst`). `__jux_share` rebuilds the handle, which asks
    /// nothing of any parameter.
    pub(crate) fn kind_member_where(&self, name: &str, sig: &juxc_tycheck::symbol_table::MethodSig) -> String {
        if name == "__jux_share" {
            return String::new();
        }
        let subst = (!self.kind_type_subst.is_empty()).then_some(&self.kind_type_subst);
        let mut parts = self.relaxed_where_parts(sig.span, subst);
        parts.extend(self.kind_member_into_parts(sig));
        Self::where_text(parts)
    }

    /// `V: Into<K>` for a member of a class declared `<K, V extends K>` that
    /// takes a `V` (ERRATA E136): its body may store the `V` where a
    /// `K` goes, and the trait member, every impl of it and the handle's
    /// forwarding all have to say so, since the class's own method does.
    /// Spelled in the vocabulary of the impl being written.
    fn kind_member_into_parts(&self, sig: &juxc_tycheck::symbol_table::MethodSig) -> Vec<String> {
        let Some(owner) = self
            .symbols
            .classes
            .values()
            .find(|c| c.methods.values().any(|m| m.span == sig.span))
        else {
            return Vec::new();
        };
        let names: Vec<&str> = owner.generic_params.iter().map(|p| p.name.text.as_str()).collect();
        let spell = |n: &str| -> Option<String> {
            match self.kind_type_subst.get(n) {
                None => Some(juxc_lex::to_rust_ident(n)),
                Some(t)
                    if t.generic_args.is_empty()
                        && t.name.segments.len() == 1
                        && self.current_type_params.contains(t.name.segments[0].text.as_str()) =>
                {
                    Some(juxc_lex::to_rust_ident(&t.name.segments[0].text))
                }
                // Fixed to a concrete type by a subclass: nothing to say.
                Some(_) => None,
            }
        };
        let mut out = Vec::new();
        for p in &owner.generic_params {
            let takes = sig.params.iter().any(|q| {
                q.ty.generic_args.is_empty() && q.ty.name.segments.len() == 1 && q.ty.name.segments[0].text == p.name.text
            });
            if !takes {
                continue;
            }
            for b in &p.bounds {
                if !b.generic_args.is_empty() || b.name.segments.len() != 1 {
                    continue;
                }
                let k = b.name.segments[0].text.as_str();
                if k == p.name.text || !names.contains(&k) {
                    continue;
                }
                if let (Some(v), Some(k)) = (spell(&p.name.text), spell(k)) {
                    out.push(format!("{v}: Into<{k}>"));
                }
            }
        }
        out
    }

    /// The clause for a function that returns a COPY of a value of type `ty`
    /// (a field accessor's getter): every relaxed parameter `ty` mentions,
    /// except as the handle of a relaxed class or interface, which copies by
    /// refcount. `ty` is read through `kind_type_subst` when one is set.
    pub(crate) fn value_copy_where(&self, ty: &TypeRef) -> String {
        let ty = if self.kind_type_subst.is_empty() {
            ty.clone()
        } else {
            Self::subst_type_ref(ty, &self.kind_type_subst)
        };
        let wanted: Vec<&String> = self
            .relaxed_scope
            .order
            .iter()
            .filter(|p| {
                juxc_tycheck::clone_needs::type_ref_mentions(&ty, p) && !self.is_relaxed_handle_type(&ty, p)
            })
            .collect();
        Self::where_text(Self::predicates(wanted.into_iter()))
    }

    /// Whether `ty` is the handle of a relaxed class or interface that holds
    /// `p` only as a bare argument in a relaxed position (`Cell<T>`): copying
    /// it is a refcount bump.
    pub(crate) fn is_relaxed_handle_type(&self, ty: &TypeRef, p: &str) -> bool {
        if juxc_tycheck::clone_needs::is_bare_param(ty, p) || ty.array_shape.is_some() || ty.fn_shape.is_some() {
            return false;
        }
        let Some(head) = ty.name.segments.last() else { return false };
        let class = self
            .resolve_bare_class_fqn(&head.text)
            .and_then(|fqn| self.symbols.classes.get(&fqn).map(|c| (fqn, c)))
            .filter(|(_, c)| !c.is_external && !c.is_struct)
            .map(|(fqn, c)| (fqn, c.generic_params.clone()));
        let target = class.or_else(|| {
            self.lookup_interface_by_bare_or_fqn(&head.text)
                .filter(|(_, i)| !i.is_external)
                .map(|(f, i)| (f.to_string(), i.generic_params.clone()))
        });
        let Some((fqn, params)) = target else { return false };
        ty.generic_args.iter().enumerate().all(|(i, a)| match a.as_type() {
            Some(t) if !juxc_tycheck::clone_needs::type_ref_mentions(t, p) => true,
            Some(t) if juxc_tycheck::clone_needs::is_bare_param(t, p) => {
                params.get(i).is_some_and(|q| self.symbols.clone_needs.is_relaxed(&fqn, &q.name.text))
            }
            _ => false,
        })
    }

    /// The map from the parameters of the ancestor of `class_decl` that
    /// declares the member at `span` (an operator or method) to the types
    /// `class_decl` passes for them, composed down the `extends` chain the way
    /// the inherited-member copies compose it. `None` when no ancestor
    /// declares it.
    pub(crate) fn ancestor_member_subst(
        &self,
        class_decl: &juxc_ast::ClassDecl,
        span: Span,
    ) -> Option<HashMap<String, TypeRef>> {
        let mut subst: HashMap<String, TypeRef> = HashMap::new();
        let mut cursor = class_decl.extends.clone();
        for _ in 0..64 {
            let parent_ref = cursor?;
            let parent = parent_ref.name.segments.last().and_then(|s| self.class_ast_named(&s.text))?;
            let mut next = HashMap::new();
            for (param, arg) in parent.generic_params.iter().zip(parent_ref.generic_args.iter()) {
                if let juxc_ast::GenericArg::Type(t) = arg {
                    next.insert(param.name.text.clone(), crate::decls::classes::substitute_type_ref(t, &subst));
                }
            }
            subst = next;
            let declares = parent.operators.iter().any(|o| o.span == span)
                || parent.methods.iter().any(|m| m.span == span);
            if declares {
                return Some(subst);
            }
            cursor = parent.extends.clone();
        }
        None
    }

    /// The clause naming every relaxed parameter in scope: for an impl whose
    /// body copies values of the declaration wholesale (the slicing upcast of
    /// a non-polymorphic parent), and for a conditional trait impl.
    pub(crate) fn relaxed_where_all(&self) -> String {
        Self::where_text(Self::predicates(self.relaxed_scope.order.iter()))
    }

    fn predicates<'s>(params: impl Iterator<Item = &'s String>) -> Vec<String> {
        params.map(|p| format!("{}: Clone + std::fmt::Debug", juxc_lex::to_rust_ident(p))).collect()
    }

    fn where_text(parts: Vec<String>) -> String {
        if parts.is_empty() {
            String::new()
        } else {
            format!(" where {}", parts.join(", "))
        }
    }
}
