//! Which type parameters of a generic class may go without `Clone + Debug`
//! (GAPS.md gap 2, ERRATA E1XX-PHASE3).
//!
//! Every generic declaration used to lower each parameter with the literal
//! bound `Clone + std::fmt::Debug + 'static`, so `Cell<File>` over
//! `class Cell<T> { T value; }` did not compile: `std::fs::File` is not
//! `Clone`. The bound is load-bearing almost everywhere a body touches a `T`
//! (a field read auto-`.clone()`s, a return copies, the renderer picks its
//! tier against the declared bounds), so it cannot simply be dropped. What
//! this pass does instead is MOVE it, for the classes where that is provably
//! enough:
//!
//! - The struct heads, the handle, the inherent `impl` header and the identity
//!   impls (`Display`, `Debug`, `JuxIdentity`, `PartialEq`/`Eq`/`Hash`) carry
//!   only `'static` (plus the user's own bounds and the key/equality/default
//!   bounds §T.2.1 already infers) for a RELAXED parameter.
//! - Each constructor and method of the class states `where T: Clone +
//!   std::fmt::Debug` for exactly the relaxed parameters its signature or body
//!   touches. A member that touches none states nothing, so it is callable
//!   with a type argument that has neither trait.
//!
//! Every other impl for the class (its `Kind` marker, bound-position
//! accessors, lifted statics) keeps the full baseline, so a delegating body
//! there still resolves to the inherent method: that method's `where` clause
//! is satisfied in the baseline scope.
//!
//! What "touches" means is deliberately conservative, see
//! [`RustEmitter::relaxed_member_needs`]. Which classes and parameters qualify
//! at all is [`RustEmitter::compute_relaxed_class_params`].

use std::collections::{HashMap, HashSet};

use juxc_ast::{Block, ClassDecl, Expr, Stmt, TypeRef};
use juxc_source::Span;

use crate::RustEmitter;

/// The relaxed parameters of the class whose inherent impl is being emitted,
/// and what each of its members needs back.
#[derive(Debug, Clone, Default)]
pub(crate) struct RelaxedClass {
    /// Relaxed parameters, in declaration order.
    pub(crate) params: Vec<String>,
    /// A member's span (constructor or method) to the relaxed parameters its
    /// `where` clause names. A member absent from the map needs all of them.
    pub(crate) member_needs: HashMap<Span, Vec<String>>,
}

impl RelaxedClass {
    /// The `where` clause a member with span `span` carries, including the
    /// leading space, or the empty string.
    pub(crate) fn where_clause(&self, span: Span) -> String {
        let needs: &[String] = self.member_needs.get(&span).map_or(&self.params, |v| v.as_slice());
        if needs.is_empty() {
            return String::new();
        }
        let parts: Vec<String> =
            needs.iter().map(|p| format!("{}: Clone + std::fmt::Debug", juxc_lex::to_rust_ident(p))).collect();
        format!(" where {}", parts.join(", "))
    }
}

/// Whether `ty`'s written form mentions the type name `p` anywhere (its head,
/// a generic argument, a function shape). Conservative by construction: it
/// reads the derived `Debug` of the tree, where every name is an `Ident` with
/// `text: "<name>"`.
pub(crate) fn type_ref_mentions(ty: &TypeRef, p: &str) -> bool {
    format!("{ty:?}").contains(&format!("text: \"{p}\""))
}

/// Whether the recorded checker type `ty` mentions the parameter `p`.
fn ty_mentions(ty: &juxc_tycheck::Ty, p: &str) -> bool {
    let s = format!("{ty:?}");
    s.contains(&format!("Param(\"{p}\")")) || s.contains(&format!("name: \"{p}\""))
}

/// `ty` is the bare parameter `p` (or `p?`): a slot that holds the value and
/// asks nothing of its type.
fn is_bare_param(ty: &TypeRef, p: &str) -> bool {
    ty.generic_args.is_empty()
        && ty.array_shape.is_none()
        && ty.fn_shape.is_none()
        && ty.name.segments.len() == 1
        && ty.name.segments[0].text == p
}

impl RustEmitter {
    /// Relaxed parameters per class FQN, computed once over every class the
    /// emitter knows (see [`Self::compute_relaxed_class_params`]).
    pub(crate) fn relaxed_class_params(&mut self, fqn: &str) -> Vec<String> {
        if self.relaxed_params_by_fqn.is_none() {
            let computed = self.compute_relaxed_class_params();
            self.relaxed_params_by_fqn = Some(computed);
        }
        self.relaxed_params_by_fqn
            .as_ref()
            .and_then(|m| m.get(fqn))
            .cloned()
            .unwrap_or_default()
    }

    /// Which type parameters of which classes drop the baseline from their
    /// headers.
    ///
    /// A class qualifies when it is a generic `Rc<RefCell>` handle class that
    /// stands alone: no `extends`, not extended, no `implements`, not abstract,
    /// and none of the members whose lowering adds impls or helpers that call
    /// back into the inherent impl (properties, operators, instance
    /// initializer blocks, a `drop` body, whose `Drop` impl must repeat the
    /// struct's bounds exactly). A worker-shared (atomic) class does not.
    ///
    /// Within a qualifying class a parameter is relaxed when every instance
    /// field mentioning it holds it bare (`T value`, `T? value`) or passes it
    /// bare to another class in the same position a relaxed parameter holds
    /// there. That last rule is a greatest fixpoint: start with every
    /// parameter relaxed and drop one whenever a field forwards it to a slot
    /// that is not, until nothing changes. A field of any other shape
    /// (`List<T>`, `T[]`, `(T) -> void`, an interface over `T`) keeps the
    /// parameter on the baseline, since that type's own declaration asks for
    /// it.
    pub(crate) fn compute_relaxed_class_params(&self) -> HashMap<String, Vec<String>> {
        let mut cand: HashMap<String, Vec<String>> = HashMap::new();
        for (fqn, cd) in &self.class_asts {
            if !self.class_may_relax(fqn, cd) {
                continue;
            }
            // A parameter a bound passes as a type argument (`K extends
            // Comparable<K>`) stays on the baseline: the bound's interface
            // declares its own parameter with it, and the header has to be
            // well-formed.
            let in_bound_args = |p: &str| {
                cd.generic_params.iter().flat_map(|q| q.bounds.iter()).any(|b| type_ref_mentions(b, p) && !is_bare_param(b, p))
            };
            let params: Vec<String> = cd
                .generic_params
                .iter()
                .filter(|p| !p.is_const() && !in_bound_args(&p.name.text))
                .map(|p| p.name.text.clone())
                .collect();
            if !params.is_empty() {
                cand.insert(fqn.clone(), params);
            }
        }
        loop {
            let mut changed = false;
            let fqns: Vec<String> = cand.keys().cloned().collect();
            for fqn in fqns {
                let Some(cd) = self.class_asts.get(&fqn) else { continue };
                let params = cand.get(&fqn).cloned().unwrap_or_default();
                let mut keep = Vec::new();
                for p in params {
                    let ok = cd
                        .fields
                        .iter()
                        .filter(|f| !f.is_static)
                        .all(|f| Self::field_type_allows(&juxc_tycheck::resolved_field_type(f), &p, &cand, &self.class_asts));
                    if ok {
                        keep.push(p);
                    } else {
                        changed = true;
                    }
                }
                if keep.is_empty() {
                    cand.remove(&fqn);
                } else {
                    cand.insert(fqn, keep);
                }
            }
            if !changed {
                break;
            }
        }
        cand
    }

    /// Whether the class `fqn` can take relaxed headers at all.
    fn class_may_relax(&self, fqn: &str, cd: &ClassDecl) -> bool {
        let bare = crate::backend_fqn::fqn_bare(fqn);
        !cd.generic_params.is_empty()
            && self.wrapper_classes.contains(fqn)
            && self.refcell_classes.contains(fqn)
            && !self.box_classes.contains(fqn)
            && !self.sync_class_fqns.contains(fqn)
            && !self.sync_classes.contains(bare)
            && cd.extends.is_none()
            && cd.implements.is_empty()
            && !cd.is_abstract
            && cd.properties.is_empty()
            && cd.operators.is_empty()
            && cd.init_blocks.is_empty()
            && cd.drop_blocks.is_empty()
            && !cd.fields.iter().any(|f| f.is_weak || f.is_ref)
            && !self
                .class_asts
                .iter()
                .any(|(k, c)| k != fqn && c.extends.as_ref().and_then(|t| t.name.segments.last()).is_some_and(|s| s.text == bare))
    }

    /// Whether a field of type `ty` lets parameter `p` stay relaxed.
    fn field_type_allows(
        ty: &TypeRef,
        p: &str,
        cand: &HashMap<String, Vec<String>>,
        class_asts: &HashMap<String, ClassDecl>,
    ) -> bool {
        if is_bare_param(ty, p) || !type_ref_mentions(ty, p) {
            return true;
        }
        if ty.array_shape.is_some() || ty.fn_shape.is_some() {
            return false;
        }
        // Another class, reached by bare name. Two classes of the name make
        // the answer unknowable here, so the parameter stays on the baseline.
        let Some(head) = ty.name.segments.last() else { return false };
        let owners: Vec<&String> =
            class_asts.keys().filter(|k| crate::backend_fqn::fqn_bare(k) == head.text).collect();
        let [owner] = owners.as_slice() else { return false };
        let Some(owner_decl) = class_asts.get(*owner) else { return false };
        let owner_params: Vec<&str> =
            owner_decl.generic_params.iter().filter(|q| !q.is_const()).map(|q| q.name.text.as_str()).collect();
        let relaxed = cand.get(*owner).cloned().unwrap_or_default();
        ty.generic_args.iter().enumerate().all(|(i, arg)| match arg.as_type() {
            Some(a) if !type_ref_mentions(a, p) => true,
            Some(a) if is_bare_param(a, p) => owner_params.get(i).is_some_and(|q| relaxed.iter().any(|r| r == q)),
            _ => false,
        })
    }


    /// The relaxed parameters each constructor and method of `cd` needs back
    /// as a `where` clause.
    ///
    /// A member needs parameter `T` when `T` reaches it as a VALUE or in a
    /// type the member cannot write without the bound:
    ///
    /// - a parameter or return type mentioning `T`, other than the bare `T` /
    ///   `T?` of a parameter (holding a value asks nothing of its type) and a
    ///   relaxed class over `T` (`Cell<T>`, see below);
    /// - the checker's recorded type of any expression in the body, a field or
    ///   parameter the body names, or the declared type of a local, a loop
    ///   binder or a lambda parameter, under the same two exemptions minus the
    ///   bare one (reading a `T` by value is what `Clone` is for);
    /// - a catch clause, a cast, a type test, a `new`, a `new T[n]` or an
    ///   explicit call type argument mentioning `T` at all;
    /// - a method called on a receiver whose type mentions `T`: the callee's
    ///   own `where` clause is not consulted, so it is assumed to need `T`;
    /// - for a constructor, the same for every instance field initializer
    ///   (they run in `new_inner`).
    ///
    /// A handle of a relaxed class over `T` (`Cell<T>` where `Cell`'s
    /// parameter is relaxed) passes through like a bare `T` slot does: its
    /// type is well-formed without the bound and copying the handle is an
    /// `Rc` bump.
    ///
    /// One more exemption: in a CONSTRUCTOR on the fast path (nothing but
    /// field stores), the same-named store `this.f = f;` of a parameter the
    /// body names nowhere else lowers to the `C_Inner { f }` shorthand, which
    /// moves the value and clones nothing. (Any other spelling, and a
    /// method's store, clones the parameter first, so a setter over `T` needs
    /// `T`.)
    ///
    /// A member needs EVERY relaxed parameter when its body names `this`
    /// other than to reach a field, uses `super`, calls a method of the class
    /// unqualified, or takes a method reference: the callee may state a
    /// `where` clause of its own, and passing `this` on hands the class to a
    /// context whose impls carry the baseline. So does an abstract member,
    /// and so does the constructor synthesized for a class that declares none
    /// (it is absent from the map).
    pub(crate) fn relaxed_member_needs(&self, cd: &ClassDecl, params: &[String]) -> HashMap<Span, Vec<String>> {
        let fields: HashMap<&str, TypeRef> = cd
            .fields
            .iter()
            .filter(|f| !f.is_static)
            .map(|f| (f.name.text.as_str(), juxc_tycheck::resolved_field_type(f)))
            .collect();
        let method_names: HashSet<&str> = cd.methods.iter().map(|m| m.name.text.as_str()).collect();
        let ctx = Ctx { emitter: self, params, fields: &fields, methods: &method_names };
        let mut out = HashMap::new();

        // Field initializers feed every constructor.
        let mut init_needs: HashSet<String> = HashSet::new();
        for f in cd.fields.iter().filter(|f| !f.is_static) {
            if let Some(d) = &f.default {
                let mut acc = Walk::new(&ctx, HashMap::new());
                juxc_ast::visit::for_each_node_in(d, &mut |n| acc.node(n));
                init_needs.extend(acc.needs);
            }
        }

        for ctor in &cd.constructors {
            let own = ctor.params.iter().map(|p| (p.name.text.as_str(), &p.ty)).collect();
            let mut acc = Walk::new(&ctx, own);
            // Only the fast path builds the literal straight from the
            // parameters, and only a parameter named once is moved rather
            // than cloned for a second use.
            acc.stores_move = crate::analysis::extract_simple_ctor_inits(ctor, cd)
                .is_some_and(|s| s.side_effects.is_empty() && s.super_args.is_none());
            let mut uses: HashMap<String, usize> = HashMap::new();
            crate::exprs::collect_bare_names_block(&ctor.body, &mut |n| *uses.entry(n.to_string()).or_default() += 1);
            acc.moved_once = uses.into_iter().filter(|(_, k)| *k == 1).map(|(n, _)| n).collect();
            for p in &ctor.params {
                acc.signature_type(&p.ty, true);
            }
            acc.block(&ctor.body);
            let mut needs = acc.needs;
            needs.extend(init_needs.iter().cloned());
            out.insert(ctor.span, ordered(params, &needs));
        }
        for m in &cd.methods {
            let own = m.params.iter().map(|p| (p.name.text.as_str(), &p.ty)).collect();
            let mut acc = Walk::new(&ctx, own);
            for p in &m.params {
                acc.signature_type(&p.ty, true);
            }
            match &m.return_type {
                juxc_ast::ReturnType::Void => {}
                juxc_ast::ReturnType::Type(t) | juxc_ast::ReturnType::AsyncType(t) => acc.signature_type(t, false),
            }
            // A method's own type parameter bounded by a class parameter
            // (`<U extends T>`) expands to that parameter's bounds.
            for gp in &m.generic_params {
                for b in &gp.bounds {
                    acc.strict_type(b);
                }
            }
            match &m.body {
                Some(b) => acc.block(b),
                None => acc.all(),
            }
            out.insert(m.span, ordered(params, &acc.needs));
        }
        out
    }

    /// The relaxed parameters of the class a checker type names, when it
    /// names exactly one relaxed class.
    fn relaxed_params_of_named(&self, name: &str) -> Option<(Vec<String>, Vec<String>)> {
        let map = self.relaxed_params_by_fqn.as_ref()?;
        let bare = name.rsplit('.').next().unwrap_or(name);
        let fqn = if self.class_asts.contains_key(name) {
            name.to_string()
        } else {
            let owners: Vec<&String> =
                self.class_asts.keys().filter(|k| crate::backend_fqn::fqn_bare(k) == bare).collect();
            let [owner] = owners.as_slice() else { return None };
            (*owner).clone()
        };
        let relaxed = map.get(&fqn)?.clone();
        let decl = self.class_asts.get(&fqn)?;
        let declared = decl.generic_params.iter().filter(|q| !q.is_const()).map(|q| q.name.text.clone()).collect();
        Some((declared, relaxed))
    }

    /// `ty` mentions `p` only as the handle of a relaxed class over it.
    fn ty_is_relaxed_handle(&self, ty: &juxc_tycheck::Ty, p: &str) -> bool {
        match ty {
            juxc_tycheck::Ty::Nullable(inner) => self.ty_is_relaxed_handle(inner, p),
            juxc_tycheck::Ty::User { name, generic_args } if !generic_args.is_empty() => {
                let Some((declared, relaxed)) = self.relaxed_params_of_named(name) else { return false };
                generic_args.iter().enumerate().all(|(i, a)| match a {
                    a if !ty_mentions(a, p) => true,
                    juxc_tycheck::Ty::Param(n) if n == p => {
                        declared.get(i).is_some_and(|q| relaxed.iter().any(|r| r == q))
                    }
                    _ => false,
                })
            }
            _ => false,
        }
    }
}

/// `needs` in the declaration order of `params`.
fn ordered(params: &[String], needs: &HashSet<String>) -> Vec<String> {
    params.iter().filter(|p| needs.contains(*p)).cloned().collect()
}

/// What every walk over one class shares.
struct Ctx<'a> {
    emitter: &'a RustEmitter,
    params: &'a [String],
    fields: &'a HashMap<&'a str, TypeRef>,
    methods: &'a HashSet<&'a str>,
}

/// One member's walk: the relaxed parameters it has been seen to need.
struct Walk<'a> {
    ctx: &'a Ctx<'a>,
    /// The member's own parameters and their declared types.
    own_params: HashMap<&'a str, &'a TypeRef>,
    /// Whether `this.f = p;` moves `p` rather than cloning it. True in a
    /// constructor, whose stores build the `C_Inner` literal; a method's
    /// store of a parameter clones it.
    stores_move: bool,
    /// Names the constructor body mentions exactly once.
    moved_once: HashSet<String>,
    /// Spans the plain-store exemption has cleared.
    exempt: HashSet<Span>,
    needs: HashSet<String>,
}

impl<'a> Walk<'a> {
    fn new(ctx: &'a Ctx<'a>, own_params: HashMap<&'a str, &'a TypeRef>) -> Self {
        Walk {
            ctx,
            own_params,
            stores_move: false,
            moved_once: HashSet::new(),
            exempt: HashSet::new(),
            needs: HashSet::new(),
        }
    }

    fn all(&mut self) {
        self.needs.extend(self.ctx.params.iter().cloned());
    }

    /// A type the member spells in a position that asks for the bound
    /// whatever its shape (a cast, a `new`, a type argument).
    fn strict_type(&mut self, ty: &TypeRef) {
        for p in self.ctx.params {
            if type_ref_mentions(ty, p) {
                self.needs.insert(p.clone());
            }
        }
    }

    /// Whether the written type `ty` is the handle of a relaxed class over
    /// `p` (see [`RustEmitter::compute_relaxed_class_params`]).
    fn relaxed_handle(&self, ty: &TypeRef, p: &str) -> bool {
        let emitter = self.ctx.emitter;
        let Some(map) = emitter.relaxed_params_by_fqn.as_ref() else { return false };
        !is_bare_param(ty, p) && RustEmitter::field_type_allows(ty, p, map, &emitter.class_asts)
    }

    /// The type of a value the member reads: a bare parameter needs the
    /// bound, a relaxed handle over it does not.
    fn value_type(&mut self, ty: &TypeRef) {
        for p in self.ctx.params {
            if type_ref_mentions(ty, p) && !self.relaxed_handle(ty, p) {
                self.needs.insert(p.clone());
            }
        }
    }

    /// A signature type: a bare parameter in a parameter slot is free, and
    /// so is a relaxed handle anywhere.
    fn signature_type(&mut self, ty: &TypeRef, param_slot: bool) {
        for p in self.ctx.params {
            if param_slot && is_bare_param(ty, p) {
                continue;
            }
            if type_ref_mentions(ty, p) && !self.relaxed_handle(ty, p) {
                self.needs.insert(p.clone());
            }
        }
    }

    fn block(&mut self, b: &Block) {
        juxc_ast::visit::for_each_node(b, &mut |n| self.node(n));
    }

    /// Whether a store target is `this.f` or a bare `f` no parameter shadows.
    fn store_target_field(&self, target: &Expr) -> bool {
        match target {
            Expr::Field(fe) => {
                matches!(&*fe.object, Expr::This(_)) && self.ctx.fields.contains_key(fe.field.text.as_str())
            }
            Expr::Path(qn) => {
                qn.segments.len() == 1
                    && self.ctx.fields.contains_key(qn.segments[0].text.as_str())
                    && !self.own_params.contains_key(qn.segments[0].text.as_str())
            }
            _ => false,
        }
    }

    /// The declared type of a name the body reads bare: a parameter of the
    /// member, else an instance field of the class.
    fn named_type(&self, name: &str) -> Option<TypeRef> {
        match self.own_params.get(name) {
            Some(t) => Some((*t).clone()),
            None => self.ctx.fields.get(name).cloned(),
        }
    }

    /// Whether the value of `e` has a type mentioning `p`, by its recorded
    /// type or, for a name or `this.f`, its declared one.
    fn value_mentions(&self, e: &Expr, p: &str) -> bool {
        let recorded = self
            .ctx
            .emitter
            .expr_types
            .get(&crate::exprs::expr_span_of(e))
            .is_some_and(|t| ty_mentions(t, p));
        let declared = match e {
            Expr::Path(qn) if qn.segments.len() == 1 => {
                self.named_type(&qn.segments[0].text).is_some_and(|t| type_ref_mentions(&t, p))
            }
            Expr::Field(fe) => {
                self.ctx.fields.get(fe.field.text.as_str()).is_some_and(|t| type_ref_mentions(t, p))
            }
            Expr::This(_) => true,
            _ => false,
        };
        recorded || declared
    }

    fn node(&mut self, n: juxc_ast::visit::Node<'_>) {
        match n {
            juxc_ast::visit::Node::Stmt(st) => self.stmt(st),
            juxc_ast::visit::Node::Expr(e) => self.expr(e),
        }
    }

    fn stmt(&mut self, st: &Stmt) {
        match st {
            Stmt::Assign(a) if a.op.is_none() => {
                // Only the same-named store (`this.f = f;`) lowers to the
                // `C_Inner { f }` shorthand; any other spelling is emitted
                // through the value path, which clones a parameter.
                let target_name = match &a.target {
                    Expr::Field(fe) => Some(fe.field.text.as_str()),
                    Expr::Path(qn) if qn.segments.len() == 1 => Some(qn.segments[0].text.as_str()),
                    _ => None,
                };
                let value_is_param = matches!(&a.value, Expr::Path(qn)
                    if qn.segments.len() == 1
                        && target_name == Some(qn.segments[0].text.as_str())
                        && self.own_params.contains_key(qn.segments[0].text.as_str())
                        && self.moved_once.contains(&qn.segments[0].text));
                if self.stores_move && value_is_param && self.store_target_field(&a.target) {
                    self.exempt.insert(crate::exprs::expr_span_of(&a.target));
                    self.exempt.insert(crate::exprs::expr_span_of(&a.value));
                    if let Expr::Field(fe) = &a.target {
                        self.exempt.insert(crate::exprs::expr_span_of(&fe.object));
                    }
                }
            }
            Stmt::VarDecl(v) => {
                if let Some(t) = &v.ty {
                    self.value_type(t);
                }
            }
            Stmt::ForEach(f) => {
                if let Some(t) = &f.var_type {
                    self.value_type(t);
                }
            }
            Stmt::Try(t) => {
                for c in &t.catches {
                    self.strict_type(&c.ty);
                    for alt in &c.alt_tys {
                        self.strict_type(alt);
                    }
                }
            }
            Stmt::SuperCall(..) => self.all(),
            _ => {}
        }
    }

    fn expr(&mut self, e: &Expr) {
        let span = crate::exprs::expr_span_of(e);
        if self.exempt.contains(&span) {
            return;
        }
        let emitter = self.ctx.emitter;
        if let Some(ty) = emitter.expr_types.get(&span) {
            for p in self.ctx.params {
                if ty_mentions(ty, p) && !emitter.ty_is_relaxed_handle(ty, p) {
                    self.needs.insert(p.clone());
                }
            }
        }
        match e {
            Expr::This(_) | Expr::Super(_) | Expr::MethodRef(_) => self.all(),
            Expr::Field(fe) => {
                if matches!(&*fe.object, Expr::This(_)) && self.ctx.fields.contains_key(fe.field.text.as_str()) {
                    self.exempt.insert(crate::exprs::expr_span_of(&fe.object));
                }
                if let Some(t) = self.ctx.fields.get(fe.field.text.as_str()).cloned() {
                    self.value_type(&t);
                }
            }
            Expr::Path(qn) if qn.segments.len() == 1 => {
                if let Some(t) = self.named_type(&qn.segments[0].text) {
                    self.value_type(&t);
                }
            }
            Expr::Call(c) => {
                for t in &c.explicit_generic_args {
                    self.strict_type(t);
                }
                match &*c.callee {
                    Expr::Path(qn) if qn.segments.len() == 1 && self.ctx.methods.contains(qn.segments[0].text.as_str()) => {
                        self.all();
                    }
                    // A method on a receiver over `T` may state a `where`
                    // clause of its own.
                    Expr::Field(fe) => {
                        for p in self.ctx.params {
                            if self.value_mentions(&fe.object, p) {
                                self.needs.insert(p.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            Expr::Cast(c) => self.strict_type(&c.ty),
            Expr::TypeTest(t) => self.strict_type(&t.ty),
            Expr::NewArray(n) => self.strict_type(&n.element_type),
            Expr::NewObject(n) => {
                for t in &n.generic_args {
                    self.strict_type(t);
                }
                if let Some(head) = n.class_name.segments.last() {
                    if self.ctx.params.contains(&head.text) {
                        self.needs.insert(head.text.clone());
                    }
                }
            }
            Expr::Lambda(l) => {
                for p in &l.params {
                    if let Some(t) = &p.ty {
                        self.value_type(t);
                    }
                }
            }
            _ => {}
        }
    }
}
