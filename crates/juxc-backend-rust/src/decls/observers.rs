//! Observable-property emission (§P — JUX-OBSERVABLE-PROPERTIES-ADDENDUM).
//!
//! Every writable `{ get; set; }` property of a wrapper class is
//! observable: the class's inner struct grows two lazily-allocated
//! observer vectors per property (full-shape and invalidation-shape)
//! plus a binding keep-alive slot, the wrapper impl grows the
//! attach/detach/clear/size/fire helpers, and the synthesized
//! `__set_<X>` setter fires observers after its body runs (an early
//! `return` skips the fire — exactly the W0973 semantics).
//!
//! Storage model (per property `X` of Jux type `T`):
//!
//! ```text
//! __obs_X_full: Option<Vec<crate::JuxObserver<dyn Fn(T, T)>>>,  // shapes 1+2
//! __obs_X_inv:  Option<Vec<crate::JuxObserver<dyn Fn()>>>,      // shape 3
//! __bind_X:     Option<crate::JuxObserver<dyn Fn(T, T)>>,       // bind() keep-alive
//! ```
//!
//! `JuxObserver` (emitted in the prelude) is Weak for NAMED observer
//! variables (§P.2.3 — the owner's field is the strong ref; owner
//! drops → observer silently stops firing and is pruned on the next
//! fire) and Strong for INLINE lambdas (nothing else would hold them).
//! `__bind_X` is stored through the same enum (always Strong) purely
//! so the inner struct's `#[derive(Debug)]` keeps resolving — a bare
//! `Rc<dyn Fn>` has no `Debug` impl.

use juxc_ast::{AccessorBody, ClassDecl, Expr, PropertyDecl, Stmt};

use crate::RustEmitter;

/// The properties of `class_decl` that get the FULL observer
/// infrastructure: non-static, with BOTH accessors (a getter for
/// old/new capture, a setter to fire from). Computed (get-only)
/// properties are handled separately — see
/// [`computed_observable_props`].
pub(crate) fn observable_props(class_decl: &ClassDecl) -> Vec<&PropertyDecl> {
    class_decl
        .properties
        .iter()
        .filter(|p| !p.is_static && p.getter.is_some() && p.setter.is_some())
        .collect()
}

/// Static observable properties (P7): class-scoped `{ get; set; }`.
/// Their observer storage is `thread_local!` (observers are `Rc`
/// closures — `!Send` — so they can't live in the `LazyLock<Mutex>`
/// the backing value uses); observers attached on one thread fire
/// only for sets made on that same thread. Phase-1 limitation,
/// consistent with observers being task-local everywhere else.
pub(crate) fn static_observable_props(class_decl: &ClassDecl) -> Vec<&PropertyDecl> {
    class_decl
        .properties
        .iter()
        .filter(|p| p.is_static && p.getter.is_some() && p.setter.is_some())
        .collect()
}

/// Computed properties (§P.1.5): non-static, get-only, with a REAL
/// getter body (expression or block) deriving the value from other
/// state. These get observer storage + fire helpers but NO `__bind_X`
/// slot (nothing can drive a value that has no setter) — their
/// observers fire from the setters of the settable properties the
/// getter body reads (see [`computed_prop_deps`]).
///
/// Auto get-only properties (`{ get; }`) are excluded: their backing
/// value is fixed after construction, so there is no change to
/// observe.
pub(crate) fn computed_observable_props(class_decl: &ClassDecl) -> Vec<&PropertyDecl> {
    class_decl
        .properties
        .iter()
        .filter(|p| {
            !p.is_static
                && p.setter.is_none()
                && matches!(
                    p.getter.as_ref().map(|g| &g.body),
                    Some(AccessorBody::Expr(_)) | Some(AccessorBody::Block(_))
                )
        })
        .collect()
}

/// §P.1.5 dependency extraction: the names of `class_decl`'s SETTABLE
/// observable properties that `computed`'s getter body reads — either
/// as a bare name (`First`) or through an explicit `this.First`.
/// Each named setter gains a re-fire bracket for this computed
/// property (recompute after the set, fire on change).
///
/// Over-approximation is harmless (the re-fire is change-guarded);
/// a missed read would lose fires, so the walker recurses through
/// every expression-carrying shape.
pub(crate) fn computed_prop_deps(class_decl: &ClassDecl, computed: &PropertyDecl) -> Vec<String> {
    let settable: Vec<&str> = observable_props(class_decl)
        .iter()
        .map(|p| p.name.text.as_str())
        .collect();
    let mut deps = Vec::new();
    match computed.getter.as_ref().map(|g| &g.body) {
        Some(AccessorBody::Expr(e)) => walk_expr_deps(e, &settable, &mut deps),
        Some(AccessorBody::Block(b)) => walk_block_deps(b, &settable, &mut deps),
        _ => {}
    }
    deps
}

/// Record `name` as a dependency when it names a settable observable
/// property (dedup-preserving order).
fn note_dep(name: &str, settable: &[&str], out: &mut Vec<String>) {
    if settable.iter().any(|s| *s == name) && !out.iter().any(|d| d == name) {
        out.push(name.to_string());
    }
}

/// Dependency walk over a statement (block-bodied getters).
fn walk_stmt_deps(stmt: &Stmt, settable: &[&str], out: &mut Vec<String>) {
    match stmt {
        Stmt::Expr(e) | Stmt::Throw(e, _) => walk_expr_deps(e, settable, out),
        Stmt::Return(Some(e)) => walk_expr_deps(e, settable, out),
        Stmt::Return(None) | Stmt::Break(..) | Stmt::Continue(..) => {}
        Stmt::VarDecl(v) => {
            if let Some(init) = &v.init {
                walk_expr_deps(init, settable, out);
            }
        }
        Stmt::If(i) => walk_if_deps(i, settable, out),
        Stmt::While(w) => {
            walk_expr_deps(&w.condition, settable, out);
            walk_block_deps(&w.body, settable, out);
        }
        Stmt::DoWhile(d) => {
            walk_block_deps(&d.body, settable, out);
            walk_expr_deps(&d.condition, settable, out);
        }
        Stmt::ForEach(f) => {
            walk_expr_deps(&f.iter, settable, out);
            walk_block_deps(&f.body, settable, out);
        }
        Stmt::ForC(f) => {
            if let Some(init) = &f.init {
                walk_stmt_deps(init, settable, out);
            }
            if let Some(cond) = &f.cond {
                walk_expr_deps(cond, settable, out);
            }
            if let Some(update) = &f.update {
                walk_stmt_deps(update, settable, out);
            }
            walk_block_deps(&f.body, settable, out);
        }
        Stmt::Assign(a) => {
            // Only the RHS is a READ; an assignment target inside a
            // getter body can't be a property of `this` anyway
            // (get-only context).
            walk_expr_deps(&a.value, settable, out);
        }
        Stmt::Try(t) => {
            walk_block_deps(&t.body, settable, out);
            for c in &t.catches {
                walk_block_deps(&c.body, settable, out);
            }
            if let Some(f) = &t.finally {
                walk_block_deps(f, settable, out);
            }
        }
        Stmt::Unsafe(b) => walk_block_deps(b, settable, out),
        Stmt::SuperCall(args, _) => {
            for a in args {
                walk_expr_deps(a, settable, out);
            }
        }
        _ => {}
    }
}

/// Dependency walk over an `if` chain.
fn walk_if_deps(i: &juxc_ast::IfStmt, settable: &[&str], out: &mut Vec<String>) {
    walk_expr_deps(&i.condition, settable, out);
    walk_block_deps(&i.then_block, settable, out);
    if let Some(eb) = &i.else_branch {
        match &**eb {
            juxc_ast::ElseBranch::If(inner) => walk_if_deps(inner, settable, out),
            juxc_ast::ElseBranch::Block(b) => walk_block_deps(b, settable, out),
        }
    }
}

/// Dependency walk over a block.
fn walk_block_deps(b: &juxc_ast::Block, settable: &[&str], out: &mut Vec<String>) {
    for s in &b.statements {
        walk_stmt_deps(s, settable, out);
    }
}

/// Dependency walk over an expression — notes bare-name and
/// `this.<name>` reads of settable properties and recurses through
/// every expression-carrying variant.
fn walk_expr_deps(e: &Expr, settable: &[&str], out: &mut Vec<String>) {
    match e {
        // `typeof` never evaluates — its operand reads nothing.
        Expr::TypeOf(..) => {}
        Expr::Path(q) => {
            if q.segments.len() == 1 {
                note_dep(&q.segments[0].text, settable, out);
            }
        }
        Expr::Field(f) => {
            if matches!(&*f.object, Expr::This(_)) {
                note_dep(&f.field.text, settable, out);
            }
            walk_expr_deps(&f.object, settable, out);
        }
        Expr::Literal(_) | Expr::This(_) | Expr::Super(_) | Expr::MethodRef(_) => {}
        Expr::Out(inner, _)
        | Expr::Await(inner, _)
        | Expr::ErrorProp(inner, _)
        | Expr::NotNullAssert(inner, _) => walk_expr_deps(inner, settable, out),
        Expr::Call(c) => {
            walk_expr_deps(&c.callee, settable, out);
            for a in &c.args {
                walk_expr_deps(a, settable, out);
            }
        }
        Expr::Binary(b) => {
            walk_expr_deps(&b.left, settable, out);
            walk_expr_deps(&b.right, settable, out);
        }
        Expr::Unary(u) => walk_expr_deps(&u.operand, settable, out),
        Expr::Range(r) => {
            walk_expr_deps(&r.start, settable, out);
            walk_expr_deps(&r.end, settable, out);
        }
        Expr::Cast(c) => walk_expr_deps(&c.value, settable, out),
        Expr::TypeTest(t) => walk_expr_deps(&t.value, settable, out),
        Expr::SizeOf(s) => walk_expr_deps(&s.operand, settable, out),
        Expr::NewArray(n) => {
            walk_expr_deps(&n.size, settable, out);
            for inner in &n.inner_sizes {
                walk_expr_deps(inner, settable, out);
            }
        }
        Expr::NewArrayLit(n) => {
            for el in &n.elements {
                walk_expr_deps(el, settable, out);
            }
        }
        Expr::NewObject(n) => {
            for a in &n.args {
                walk_expr_deps(a, settable, out);
            }
        }
        Expr::Index(i) => {
            walk_expr_deps(&i.array, settable, out);
            walk_expr_deps(&i.index, settable, out);
        }
        // `++place` / `place++` reads (and writes) the place — walk it
        // so a settable property stepped via `++` is noted as a dep.
        Expr::IncDec(i) => walk_expr_deps(&i.target, settable, out),
        Expr::InterpString(s) => {
            for seg in &s.segments {
                if let juxc_ast::InterpSegment::Expr(inner) = seg {
                    walk_expr_deps(inner, settable, out);
                }
            }
        }
        Expr::Elvis(el) => {
            walk_expr_deps(&el.value, settable, out);
            walk_expr_deps(&el.fallback, settable, out);
        }
        Expr::Ternary(t) => {
            walk_expr_deps(&t.condition, settable, out);
            walk_expr_deps(&t.then_branch, settable, out);
            walk_expr_deps(&t.else_branch, settable, out);
        }
        Expr::Switch(sw) => {
            walk_expr_deps(&sw.scrutinee, settable, out);
            for arm in &sw.arms {
                if let Some(g) = &arm.guard {
                    walk_expr_deps(g, settable, out);
                }
                match &arm.body {
                    juxc_ast::SwitchBody::Expr(e) => walk_expr_deps(e, settable, out),
                    juxc_ast::SwitchBody::Block(b) => walk_block_deps(b, settable, out),
                }
            }
        }
        Expr::Lambda(l) => match &l.body {
            juxc_ast::LambdaBody::Expr(e) => walk_expr_deps(e, settable, out),
            juxc_ast::LambdaBody::Block(b) => walk_block_deps(b, settable, out),
        },
        Expr::TupleLit(elems, _) => {
            for el in elems {
                walk_expr_deps(el, settable, out);
            }
        }
        Expr::TryExpr(t) => {
            walk_block_deps(&t.body, settable, out);
            for c in &t.catches {
                walk_block_deps(&c.body, settable, out);
            }
            if let Some(f) = &t.finally {
                walk_block_deps(f, settable, out);
            }
        }
    }
}

/// True when the property's lowered type supports `!=` change
/// detection (rustc `PartialEq`). Primitives and `String` (nullable
/// included) compare; user classes lower to `Rc<RefCell<…>>` wrappers
/// without a derived `PartialEq`, so their setters fire on every
/// write instead.
pub(crate) fn property_ty_is_comparable(ty: &juxc_ast::TypeRef) -> bool {
    if ty.fn_shape.is_some() || ty.array_shape.is_some() || ty.ptr_depth > 0 {
        return false;
    }
    if ty.name.segments.len() != 1 {
        return false;
    }
    matches!(
        ty.name.segments[0].text.as_str(),
        "bool" | "byte" | "ubyte" | "short" | "ushort" | "int" | "uint" | "long" | "ulong"
            | "float" | "double" | "char" | "String"
    )
}

impl RustEmitter {
    /// Look up a class AST by bare name (exact key, or unique
    /// `…\.<bare>` FQN suffix) — `class_asts` is keyed by FQN.
    pub(crate) fn class_ast_by_bare(&self, bare: &str) -> Option<&ClassDecl> {
        if let Some(cd) = self.class_asts.get(bare) {
            return Some(cd);
        }
        let suffix = format!(".{bare}");
        let mut found = None;
        for (fqn, cd) in &self.class_asts {
            if fqn.ends_with(&suffix) {
                if found.is_some() {
                    return None; // ambiguous — caller falls back
                }
                found = Some(cd);
            }
        }
        found
    }

    /// True when `bare` names a class whose property `prop` is
    /// observable (drives the `.observers` / `bind` routing).
    /// Computed properties count (§P.1.5 — they can be observed and
    /// can be a binding SOURCE; binding INTO one is rejected upstream
    /// since there is no setter). INHERITED observable properties
    /// count too (Java semantics — a `B extends A` object observes
    /// `A`'s properties through its own handle); the subclass wrapper
    /// carries depth-aware helper methods reaching the ancestor's
    /// storage slice, so emission is uniform either way.
    pub(crate) fn class_has_observable_prop(&self, bare: &str, prop: &str) -> bool {
        let mut cursor = self.class_ast_by_bare(bare);
        let mut depth = 0usize;
        while let Some(cd) = cursor {
            if depth > 64 {
                return false;
            }
            if observable_props(cd).iter().any(|p| p.name.text == prop)
                || computed_observable_props(cd).iter().any(|p| p.name.text == prop)
            {
                return true;
            }
            cursor = cd
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.first())
                .and_then(|seg| self.class_ast_by_bare(&seg.text));
            depth += 1;
        }
        false
    }

    /// The observable properties a subclass INHERITS from its
    /// `extends` chain — `(prop, settable, depth)` where `depth` is
    /// the number of `__parent` hops from the subclass's inner struct
    /// to the slice that owns the storage. Properties shadowed by a
    /// closer declaration (own or nearer ancestor) are skipped.
    pub(crate) fn inherited_observable_props(
        &self,
        class_decl: &ClassDecl,
    ) -> Vec<(PropertyDecl, bool, usize)> {
        let mut out = Vec::new();
        let mut seen: std::collections::HashSet<String> = class_decl
            .properties
            .iter()
            .map(|p| p.name.text.clone())
            .collect();
        let mut depth = 1usize;
        let mut cursor = class_decl
            .extends
            .as_ref()
            .and_then(|t| t.name.segments.first().map(|s| s.text.clone()));
        while let Some(bare) = cursor {
            if depth > 64 {
                break;
            }
            let Some(parent) = self.class_ast_by_bare(&bare) else { break };
            for p in observable_props(parent) {
                if seen.insert(p.name.text.clone()) {
                    out.push((p.clone(), true, depth));
                }
            }
            for p in computed_observable_props(parent) {
                if seen.insert(p.name.text.clone()) {
                    out.push((p.clone(), false, depth));
                }
            }
            cursor = parent
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.first().map(|s| s.text.clone()));
            depth += 1;
        }
        out
    }

    /// True when `bare` names a class with a STATIC observable
    /// property `prop` (P7 routing — `Config.Level.observers.…`).
    pub(crate) fn class_has_static_observable_prop(&self, bare: &str, prop: &str) -> bool {
        self.class_ast_by_bare(bare)
            .map(|cd| {
                static_observable_props(cd)
                    .iter()
                    .any(|p| p.name.text == prop)
            })
            .unwrap_or(false)
    }

    /// Resolve the expression `.observers` hangs off to a STATIC
    /// observable property — `(class bare name, prop name)`. Handles
    /// the post-desugar shapes:
    ///
    /// - `Config.Level` as a Field over a class-naming path;
    /// - `Config.Level` parsed whole as a multi-segment Path;
    /// - bare `Level` inside the declaring class's own methods.
    pub(crate) fn resolve_static_observable_prop(
        &self,
        e: &juxc_ast::Expr,
    ) -> Option<(String, String)> {
        use juxc_ast::Expr;
        match e {
            Expr::Field(f) => {
                let name = f.field.text.as_str();
                let prop = name.strip_prefix("__prop_").unwrap_or(name);
                if let Expr::Path(qn) = &*f.object {
                    let bare = qn.segments.last()?.text.as_str();
                    if self.class_has_static_observable_prop(bare, prop) {
                        return Some((bare.to_string(), prop.to_string()));
                    }
                }
                None
            }
            Expr::Path(qn) if qn.segments.len() >= 2 => {
                let prop = qn.segments.last()?.text.as_str();
                let bare = qn.segments[qn.segments.len() - 2].text.as_str();
                if self.class_has_static_observable_prop(bare, prop) {
                    Some((bare.to_string(), prop.to_string()))
                } else {
                    None
                }
            }
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = qn.segments[0].text.as_str();
                let prop = name.strip_prefix("__prop_").unwrap_or(name);
                let class = self.enclosing_class.clone()?;
                if self.class_has_static_observable_prop(&class, prop) {
                    Some((class, prop.to_string()))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Emit the per-property observer storage fields into the inner
    /// struct (multi-line, writer already at field depth).
    pub(crate) fn emit_observer_inner_fields(&mut self, class_decl: &ClassDecl) {
        // Settable props: observer vectors + the bind slot. Computed
        // props (§P.1.5): observer vectors only — nothing can bind
        // INTO a value with no setter.
        let props: Vec<(&PropertyDecl, bool)> = observable_props(class_decl)
            .into_iter()
            .map(|p| (p, true))
            .chain(computed_observable_props(class_decl).into_iter().map(|p| (p, false)))
            .collect();
        for (prop, settable) in props {
            let x = prop.name.text.clone();
            // Full-shape observers: `(old, now)` lambdas (and the
            // shape-2 adapter wrappers).
            self.w.emit_indent();
            self.w.push_str(&format!("pub(crate) __obs_{x}_full: Option<Vec<crate::JuxObserver<dyn Fn("));
            self.emit_type_as_rust(&prop.ty);
            self.w.push_str(", ");
            self.emit_type_as_rust(&prop.ty);
            self.w.push_str(")>>>,\n");
            // Invalidation observers: `()` lambdas.
            self.w.emit_indent();
            self.w
                .push_str(&format!("pub(crate) __obs_{x}_inv: Option<Vec<crate::JuxObserver<dyn Fn()>>>,\n"));
            if !settable {
                continue;
            }
            // `bind()` keep-alive: the binding closure attached (weakly)
            // to the SOURCE property lives here on the bound TARGET,
            // PAIRED with the binding's shared kill token (P4) — a
            // bidirectional binding gives BOTH slots the same token,
            // so `unbind()` from either side deactivates both
            // directions at once — plus the bidirectionality flag the
            // E0973 guard consults (only ONE-WAY targets refuse
            // direct sets).
            self.w.emit_indent();
            self.w.push_str(&format!("pub(crate) __bind_{x}: Option<(crate::JuxObserver<dyn Fn("));
            self.emit_type_as_rust(&prop.ty);
            self.w.push_str(", ");
            self.emit_type_as_rust(&prop.ty);
            self.w
                .push_str(")>, std::rc::Rc<std::cell::Cell<bool>>, bool)>,\n");
        }
    }

    /// Append the observer fields' `None` seeds to a SINGLE-LINE inner
    /// struct literal (simple-ctor / synthetic-default shapes).
    /// `first` mirrors the caller's separator state.
    pub(crate) fn emit_observer_field_inits_inline(
        &mut self,
        class_decl: &ClassDecl,
        first: &mut bool,
    ) {
        for prop in observable_props(class_decl) {
            let x = &prop.name.text;
            let sep = if *first { " " } else { ", " };
            *first = false;
            self.w.push_str(&format!(
                "{sep}__obs_{x}_full: None, __obs_{x}_inv: None, __bind_{x}: None"
            ));
        }
        for prop in computed_observable_props(class_decl) {
            let x = &prop.name.text;
            let sep = if *first { " " } else { ", " };
            *first = false;
            self.w
                .push_str(&format!("{sep}__obs_{x}_full: None, __obs_{x}_inv: None"));
        }
    }

    /// Append the observer fields' `None` seeds to a MULTI-LINE inner
    /// struct literal (the `__self`-builder ctor shape) — one line per
    /// property, writer already at field depth.
    pub(crate) fn emit_observer_field_inits_lines(&mut self, class_decl: &ClassDecl) {
        for prop in observable_props(class_decl) {
            let x = &prop.name.text;
            self.w.emit_indent();
            self.w.push_str(&format!(
                "__obs_{x}_full: None, __obs_{x}_inv: None, __bind_{x}: None,\n"
            ));
        }
        for prop in computed_observable_props(class_decl) {
            let x = &prop.name.text;
            self.w.emit_indent();
            self.w
                .push_str(&format!("__obs_{x}_full: None, __obs_{x}_inv: None,\n"));
        }
    }

    /// Emit the per-property observer helper methods inside the
    /// wrapper's `impl` block: attach (full/inv), detach (full/inv),
    /// clear, size, and the fire routine the setter epilogue calls.
    pub(crate) fn emit_observer_helper_methods(&mut self, class_decl: &ClassDecl) {
        // Per-class handle helpers for the §P.4 binding machinery —
        // the wrapper's tuple field is module-private, so bind sites
        // in OTHER packages go through these instead of `.0`:
        // `__jux_weak` hands out a non-owning handle for the binding
        // closure (capturing the wrapper strongly would cycle), and
        // `__jux_from_inner` rebuilds a wrapper from the upgraded
        // handle inside the closure.
        let inherited = self.inherited_observable_props(class_decl);
        if !observable_props(class_decl).is_empty()
            || !computed_observable_props(class_decl).is_empty()
            || !inherited.is_empty()
        {
            let name = class_decl.name.text.clone();
            // Thread the class's generic args onto the inner type
            // (`Box_Inner<T>` for `class Box<T>`).
            let gargs = {
                let mark = self.w.len();
                self.emit_generic_params_as_args(&class_decl.generic_params);
                self.w.split_off_from(mark)
            };
            self.w.indent_inc();
            self.w.line(&format!(
                "pub fn __jux_weak(&self) -> std::rc::Weak<std::cell::RefCell<{name}_Inner{gargs}>> {{ std::rc::Rc::downgrade(&self.0) }}"
            ));
            self.w.line(&format!(
                "pub fn __jux_from_inner(rc: std::rc::Rc<std::cell::RefCell<{name}_Inner{gargs}>>) -> Self {{ Self(rc) }}"
            ));
            self.w.indent_dec();
            self.w.newline();
        }
        // Settable props get the full helper set; computed props
        // (§P.1.5) get everything EXCEPT the bind keep-alive store.
        // INHERITED observable props (Java semantics) get the same
        // helpers with `__parent` hops to the ancestor's storage
        // slice — `depth` counts the hops (0 = own storage).
        let props: Vec<(PropertyDecl, bool, usize)> = observable_props(class_decl)
            .into_iter()
            .map(|p| (p.clone(), true, 0usize))
            .chain(
                computed_observable_props(class_decl)
                    .into_iter()
                    .map(|p| (p.clone(), false, 0usize)),
            )
            .chain(inherited)
            .collect();
        for (prop, settable, depth) in props {
            let x = prop.name.text.clone();
            // `__parent` hop prefix from this wrapper's inner struct to
            // the slice that owns the storage fields.
            let h = "__parent.".repeat(depth);
            // Render the property's Rust type once for reuse in the
            // signatures below.
            let t = self.render_type_to_string(&prop.ty);

            self.w.indent_inc();

            // -- attach --------------------------------------------------
            self.w
                .line(&format!("pub fn __obs_{x}_attach_full(&self, o: crate::JuxObserver<dyn Fn({t}, {t})>) {{"));
            self.w.indent_inc();
            self.w.line(&format!(
                "self.0.borrow_mut().{h}__obs_{x}_full.get_or_insert_with(Vec::new).push(o);"
            ));
            self.w.indent_dec();
            self.w.line("}");
            self.w
                .line(&format!("pub fn __obs_{x}_attach_inv(&self, o: crate::JuxObserver<dyn Fn()>) {{"));
            self.w.indent_inc();
            self.w.line(&format!(
                "self.0.borrow_mut().{h}__obs_{x}_inv.get_or_insert_with(Vec::new).push(o);"
            ));
            self.w.indent_dec();
            self.w.line("}");

            // -- detach (pointer identity against the named Rc) ----------
            self.w.line(&format!(
                "pub fn __obs_{x}_detach_full(&self, t: &std::rc::Rc<dyn Fn({t}, {t})>) {{"
            ));
            self.w.indent_inc();
            self.w.line(&format!(
                "if let Some(v) = self.0.borrow_mut().{h}__obs_{x}_full.as_mut() {{ v.retain(|o| !o.is_for(t)); }}"
            ));
            self.w.indent_dec();
            self.w.line("}");
            self.w.line(&format!(
                "pub fn __obs_{x}_detach_inv(&self, t: &std::rc::Rc<dyn Fn()>) {{"
            ));
            self.w.indent_inc();
            self.w.line(&format!(
                "if let Some(v) = self.0.borrow_mut().{h}__obs_{x}_inv.as_mut() {{ v.retain(|o| !o.is_for(t)); }}"
            ));
            self.w.indent_dec();
            self.w.line("}");

            // -- clear / size ---------------------------------------------
            // `clear` releases the storage entirely (§P.3.2); `size`
            // counts LIVE observers without allocating (§P.3.3).
            self.w.line(&format!("pub fn __obs_{x}_clear(&self) {{"));
            self.w.indent_inc();
            self.w.line("let mut s = self.0.borrow_mut();");
            self.w.line(&format!("s.{h}__obs_{x}_full = None;"));
            self.w.line(&format!("s.{h}__obs_{x}_inv = None;"));
            self.w.indent_dec();
            self.w.line("}");
            self.w.line(&format!("pub fn __obs_{x}_size(&self) -> isize {{"));
            self.w.indent_inc();
            self.w.line("let s = self.0.borrow();");
            self.w.line(&format!(
                "let full = s.{h}__obs_{x}_full.as_ref().map(|v| v.iter().filter(|o| o.upgrade().is_some()).count()).unwrap_or(0);"
            ));
            self.w.line(&format!(
                "let inv = s.{h}__obs_{x}_inv.as_ref().map(|v| v.iter().filter(|o| o.upgrade().is_some()).count()).unwrap_or(0);"
            ));
            self.w.line("(full + inv) as isize");
            self.w.indent_dec();
            self.w.line("}");

            // -- bind keep-alive store (§P.4) ------------------------------
            // `Some((Strong(f), kill_token, bidi))` while a binding
            // drives this property. Storing — whether a fresh binding
            // or `None` from `unbind()` — DEACTIVATES the previous
            // binding's token first (P4): a bidirectional peer's
            // closure shares the token, so both directions die from
            // either side's unbind/rebind. Computed props have no
            // bind slot (nothing can drive a setterless value).
            if settable {
                self.w.line(&format!(
                    "pub fn __bind_{x}_store(&self, f: Option<(crate::JuxObserver<dyn Fn({t}, {t})>, std::rc::Rc<std::cell::Cell<bool>>, bool)>) {{"
                ));
                self.w.indent_inc();
                self.w.line(&format!(
                    "let __jux_old = std::mem::replace(&mut self.0.borrow_mut().{h}__bind_{x}, f);"
                ));
                self.w
                    .line("if let Some((_, __jux_a, _)) = __jux_old { __jux_a.set(false); }");
                self.w.indent_dec();
                self.w.line("}");
            }

            // -- fire ------------------------------------------------------
            // Invalidation observers first, then full observers
            // (§P.3.4), pruning dead weak refs on the way. Each vec is
            // take()n out of the cell before iterating so a re-entrant
            // set inside an observer body can't double-borrow (a nested
            // fire sees None and no-ops — change-driven recursion
            // terminates).
            self.w.line(&format!(
                "pub fn __obs_{x}_fire(&self, old: &{t}, now: &{t}) {{"
            ));
            self.w.indent_inc();
            self.w.line(&format!("let inv = self.0.borrow_mut().{h}__obs_{x}_inv.take();"));
            self.w.line("if let Some(mut v) = inv {");
            self.w.indent_inc();
            self.w.line(
                "v.retain(|o| match o.upgrade() { Some(f) => { f(); true } None => false });",
            );
            self.w.line("let mut s = self.0.borrow_mut();");
            self.w.line(&format!(
                "if let Some(cur) = s.{h}__obs_{x}_inv.take() {{ v.extend(cur); }}"
            ));
            self.w.line(&format!("s.{h}__obs_{x}_inv = Some(v);"));
            self.w.indent_dec();
            self.w.line("}");
            self.w.line(&format!("let full = self.0.borrow_mut().{h}__obs_{x}_full.take();"));
            self.w.line("if let Some(mut v) = full {");
            self.w.indent_inc();
            self.w.line(
                "v.retain(|o| match o.upgrade() { Some(f) => { f(old.clone(), now.clone()); true } None => false });",
            );
            self.w.line("let mut s = self.0.borrow_mut();");
            self.w.line(&format!(
                "if let Some(cur) = s.{h}__obs_{x}_full.take() {{ v.extend(cur); }}"
            ));
            self.w.line(&format!("s.{h}__obs_{x}_full = Some(v);"));
            self.w.indent_dec();
            self.w.line("}");
            self.w.indent_dec();
            self.w.line("}");

            self.w.indent_dec();
            self.w.newline();
        }
    }

    /// Render a Jux type to its Rust spelling as a `String` (for use
    /// inside `format!`-built signatures) by emitting into a scratch
    /// writer.
    pub(crate) fn render_type_to_string(&mut self, ty: &juxc_ast::TypeRef) -> String {
        let mark = self.w.len();
        self.emit_type_as_rust(ty);
        self.w.split_off_from(mark)
    }

    /// Emit the Rust type of an `observer<T>` variable for a known
    /// lambda arity: `Rc<dyn Fn()>` (invalidation), `Rc<dyn Fn(T, T)>`
    /// (full), or `Rc<dyn Fn(String, T, T)>` (full + property
    /// reference — Phase 1 passes the firing property's NAME).
    pub(crate) fn emit_observer_var_type(&mut self, ty: &juxc_ast::TypeRef, arity: usize) {
        self.w.push_str("std::rc::Rc<dyn Fn(");
        match arity {
            0 => {}
            3 => {
                self.w.push_str("String, ");
                self.emit_observer_payload_ty(ty);
                self.w.push_str(", ");
                self.emit_observer_payload_ty(ty);
            }
            _ => {
                self.emit_observer_payload_ty(ty);
                self.w.push_str(", ");
                self.emit_observer_payload_ty(ty);
            }
        }
        self.w.push_str(")>");
    }

    /// The `T` of `observer<T>`, emitted as a Rust value type.
    fn emit_observer_payload_ty(&mut self, ty: &juxc_ast::TypeRef) {
        if let Some(juxc_ast::GenericArg::Type(t)) = ty.generic_args.first() {
            self.emit_type_as_rust(t);
        } else {
            self.w.push_str("()");
        }
    }

    /// Resolve the expression `.observers` (or `.bind` …) hangs off to
    /// the observable property it names. Returns `(receiver, prop)`
    /// where `receiver = None` means the enclosing instance. Handles
    /// the post-desugar shapes:
    ///
    /// - `obj.X` — `X` is an observable property of `obj`'s class;
    /// - `this.__prop_X` / `__prop_X` — the constructor rewrite routed
    ///   an auto-property access to its backing field;
    /// - bare `X` — a property of the enclosing class.
    pub(crate) fn resolve_observable_prop<'a>(
        &self,
        e: &'a juxc_ast::Expr,
    ) -> Option<(Option<&'a juxc_ast::Expr>, String, String)> {
        use juxc_ast::Expr;
        match e {
            Expr::Field(f) => {
                let name = f.field.text.as_str();
                let prop = name.strip_prefix("__prop_").unwrap_or(name);
                let class = if matches!(*f.object, Expr::This(_)) {
                    self.enclosing_class.clone()
                } else {
                    self.receiver_class_bare(&f.object)
                }?;
                if self.class_has_observable_prop(&class, prop) {
                    Some((Some(&f.object), prop.to_string(), class))
                } else {
                    None
                }
            }
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = qn.segments[0].text.as_str();
                let prop = name.strip_prefix("__prop_").unwrap_or(name);
                let class = self.enclosing_class.clone()?;
                if self.class_has_observable_prop(&class, prop) {
                    Some((None, prop.to_string(), class))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// True when the resolved receiver is the enclosing instance INSIDE
    /// a constructor body — there `this` is the bare inner struct
    /// (`__self`), which has the storage fields but not the wrapper's
    /// helper methods, so the observer ops emit field-direct.
    fn observers_receiver_is_ctor_inner(&self, recv: Option<&juxc_ast::Expr>) -> bool {
        let this_is_inner = self.this_alias.as_deref() == Some("__self");
        match recv {
            None => this_is_inner,
            Some(juxc_ast::Expr::This(_)) => this_is_inner,
            _ => false,
        }
    }

    /// Emit the receiver prefix for an observer operation — `self`,
    /// the aliased `this`, or the receiver expression.
    fn emit_observers_receiver(&mut self, recv: Option<&juxc_ast::Expr>) {
        match recv {
            None => {
                let alias = self.this_alias.clone().unwrap_or_else(|| "self".to_string());
                self.w.push_str(&alias);
            }
            Some(e) => self.emit_expr(e),
        }
    }

    /// Lambda arity of an `.observers.attach/detach` argument — an
    /// inline lambda counts its params; a named observer variable
    /// (path or field) looks up the recorded `observer<T>` shape;
    /// anything else defaults to the full 2-arg shape.
    fn observer_arg_arity(&self, arg: &juxc_ast::Expr) -> usize {
        use juxc_ast::Expr;
        match arg {
            Expr::Lambda(l) => l.params.len(),
            Expr::Path(qn) if qn.segments.len() == 1 => self
                .observer_shapes
                .get(qn.segments[0].text.as_str())
                .copied()
                .unwrap_or(2),
            Expr::Field(f) => self
                .observer_shapes
                .get(f.field.text.as_str())
                .copied()
                .unwrap_or(2),
            _ => 2,
        }
    }

    /// Emit one `<prop>.observers.attach(arg)` / `.detach(arg)` call.
    /// `recv`/`prop` come from [`Self::resolve_observable_prop`].
    pub(crate) fn emit_observers_call(
        &mut self,
        recv: Option<&juxc_ast::Expr>,
        prop: &str,
        class: &str,
        op: &str,
        call: &juxc_ast::CallExpr,
    ) {
        let Some(arg) = call.args.first() else {
            // Malformed — keep the output compiling.
            self.w.push_str("()");
            return;
        };
        let arity = self.observer_arg_arity(arg);
        let vec_kind = if arity == 0 { "inv" } else { "full" };
        let ctor_inner = self.observers_receiver_is_ctor_inner(recv);
        // The stored element's full type — the let temp below needs the
        // annotation so an inline closure unsize-coerces to `dyn Fn`.
        let elem_ty = if arity == 0 {
            "crate::JuxObserver<dyn Fn()>".to_string()
        } else {
            let t = self
                .prop_rust_ty(class, prop)
                .unwrap_or_else(|| "()".to_string());
            format!("crate::JuxObserver<dyn Fn({t}, {t})>")
        };
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = false;
        // The observer value is hoisted into a statement-scoped temp
        // BEFORE the storage call: reading a named observer field goes
        // through `self.0.borrow()`, and that guard temporary would
        // otherwise live to the end of the whole call statement —
        // colliding with the attach/detach `borrow_mut()` (runtime
        // `RefCell already borrowed`).
        if op == "attach" {
            self.w.push_str(&format!("{{ let __jux_o: {elem_ty} = "));
            self.emit_observer_value(prop, arg, arity);
            self.w.push_str("; ");
            if ctor_inner {
                // Field-direct: `__self.__obs_X_full.get_or_insert_with(Vec::new).push(o)`.
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(
                    ".__obs_{prop}_{vec_kind}.get_or_insert_with(Vec::new).push(__jux_o)"
                ));
            } else {
                self.emit_observers_receiver(recv);
                self.w
                    .push_str(&format!(".__obs_{prop}_attach_{vec_kind}(__jux_o)"));
            }
            self.w.push_str("; }");
        } else {
            // detach — pointer-identity against the named observer's Rc.
            self.w.push_str("{ let __jux_t = (");
            self.emit_expr(arg);
            self.w.push_str("); ");
            if ctor_inner {
                self.w.push_str("if let Some(v) = ");
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(
                    ".__obs_{prop}_{vec_kind}.as_mut() {{ v.retain(|o| !o.is_for(&__jux_t)); }}"
                ));
            } else {
                self.emit_observers_receiver(recv);
                self.w
                    .push_str(&format!(".__obs_{prop}_detach_{vec_kind}(&__jux_t)"));
            }
            self.w.push_str("; }");
        }
        self.emitting_format_arg = prev;
    }

    /// Emit the `crate::JuxObserver<…>` value an attach stores:
    ///
    /// - inline lambda → `Strong(<lambda>)` — nothing else would keep
    ///   it alive;
    /// - named observer variable → `Weak(Rc::downgrade(&<var>))` —
    ///   §P.2.3 weak-by-default, the owner's field is the strong ref;
    /// - shape 2 (3-arg, property reference) → a Strong adapter closure
    ///   holding the named observer WEAKLY and prepending the property
    ///   name. (The adapter itself out-lives a dropped owner as a tiny
    ///   no-op until the next prune — a known Phase-1 wrinkle.)
    fn emit_observer_value(&mut self, prop: &str, arg: &juxc_ast::Expr, arity: usize) {
        let inline = matches!(arg, juxc_ast::Expr::Lambda(_));
        if arity == 3 {
            if inline {
                self.w.push_str("{ let __jux_f3 = ");
                self.emit_expr(arg);
                self.w.push_str(&format!(
                    "; crate::JuxObserver::Strong(std::rc::Rc::new(move |__jux_o, __jux_n| {{ __jux_f3(\"{prop}\".to_string(), __jux_o, __jux_n); }})) }}"
                ));
            } else {
                // P5: the adapter is Strong (nothing else keeps it
                // alive) but its REAL observer is the named variable
                // behind `__jux_w`. When that owner dies, the adapter
                // flips its shared `dead` flag on the next fire, and
                // the fire loop's `upgrade()` (None for a dead
                // StrongGuarded) prunes the adapter itself — no
                // permanent leak.
                self.w.push_str("{ let __jux_w = std::rc::Rc::downgrade(&(");
                self.emit_expr(arg);
                self.w.push_str(&format!(
                    ")); let __jux_dead = std::rc::Rc::new(std::cell::Cell::new(false)); let __jux_dead_c = __jux_dead.clone(); crate::JuxObserver::StrongGuarded(std::rc::Rc::new(move |__jux_o, __jux_n| {{ match __jux_w.upgrade() {{ Some(__jux_f) => {{ __jux_f(\"{prop}\".to_string(), __jux_o, __jux_n); }} None => {{ __jux_dead_c.set(true); }} }} }}), __jux_dead) }}"
                ));
            }
            return;
        }
        if inline {
            self.w.push_str("crate::JuxObserver::Strong(");
            self.emit_expr(arg);
            self.w.push(')');
        } else {
            self.w
                .push_str("crate::JuxObserver::Weak(std::rc::Rc::downgrade(&(");
            self.emit_expr(arg);
            self.w.push_str(")))");
        }
    }

    /// Emit the receiver of a binding operation as an owned wrapper
    /// handle binding (`let __jux_bX = (<recv>).clone();`). The clone
    /// is the cheap `Rc` refcount bump — and keeps a `Path` receiver
    /// from being moved out of its local.
    fn emit_bind_recv_binding(&mut self, var: &str, recv: Option<&juxc_ast::Expr>) {
        self.w.push_str(&format!("let {var} = ("));
        self.emit_observers_receiver(recv);
        self.w.push_str(").clone(); ");
    }

    /// The Rust path of a class's wrapper type at the CURRENT emission
    /// site — crate-rooted for cross-package classes, bare otherwise.
    fn class_rust_path(&mut self, bare: &str) -> String {
        let local = self.symbols.classes.contains_key(bare);
        if !local {
            if let Some(fqn) = self.symbols.find_fqn_by_bare(bare) {
                if fqn.contains('.') {
                    return format!(
                        "crate::{}",
                        fqn.split('.').collect::<Vec<_>>().join("::")
                    );
                }
            }
        }
        bare.to_string()
    }

    /// The declared Jux type of class `class`'s property `prop`,
    /// rendered as a Rust type string. Walks the `extends` chain so
    /// inherited properties resolve (Java semantics).
    fn prop_rust_ty(&mut self, class: &str, prop: &str) -> Option<String> {
        let mut bare = class.to_string();
        for _ in 0..64 {
            let cd = self.class_ast_by_bare(&bare)?;
            if let Some(p) = cd.properties.iter().find(|p| p.name.text == prop) {
                let ty = p.ty.clone();
                return Some(self.render_type_to_string(&ty));
            }
            bare = cd
                .extends
                .as_ref()
                .and_then(|t| t.name.segments.first().map(|s| s.text.clone()))?;
        }
        None
    }

    /// Emit `target.X.bind(source.Y)` (§P.4.2) / `bindBidirectional`
    /// (§P.4.3). One direction wires:
    ///
    /// 1. an immediate sync (`target.__set_X(source.Y())`),
    /// 2. a closure that re-sets the target on every source fire —
    ///    holding the target WEAKLY (a strong capture would leak the
    ///    target through its own keep-alive slot),
    /// 3. the closure stored strongly in the target's `__bind_X` slot
    ///    (so the binding lives exactly as long as the target), and
    /// 4. a weak attach of that closure on the source property.
    ///
    /// Bidirectional wires both directions; the take-during-fire
    /// re-entrancy guard in `__obs_*_fire` (plus `!=` change detection
    /// for comparable types) breaks the update cycle.
    pub(crate) fn emit_bind(
        &mut self,
        target: (Option<&juxc_ast::Expr>, &str, &str),
        source: (Option<&juxc_ast::Expr>, &str, &str),
        bidirectional: bool,
    ) {
        let (t_recv, t_prop, t_class) = target;
        let (s_recv, s_prop, s_class) = source;
        if self.observers_receiver_is_ctor_inner(t_recv)
            || self.observers_receiver_is_ctor_inner(s_recv)
        {
            // P6 (§P.9): the enclosing instance is still a bare inner
            // struct in the constructor body — no wrapper handle
            // exists for the binding closures to hold. Defer the
            // whole bind to the public `new`, which replays it right
            // after `Self(Rc::new(RefCell::new(__self)))` wraps the
            // instance. A `this`-shaped receiver is normalized to
            // `None` so the replay resolves it through the
            // then-current `this_alias` (the wrapper handle).
            let norm = |r: Option<&juxc_ast::Expr>| -> Option<juxc_ast::Expr> {
                match r {
                    None | Some(juxc_ast::Expr::This(_)) => None,
                    Some(e) => Some(e.clone()),
                }
            };
            self.pending_ctor_binds.push(crate::PendingCtorBind {
                target: (norm(t_recv), t_prop.to_string(), t_class.to_string()),
                source: (norm(s_recv), s_prop.to_string(), s_class.to_string()),
                bidirectional,
            });
            self.w.push_str("()");
            return;
        }
        let s_ty = self
            .prop_rust_ty(s_class, s_prop)
            .unwrap_or_else(|| "()".to_string());
        let t_ty = self
            .prop_rust_ty(t_class, t_prop)
            .unwrap_or_else(|| "()".to_string());
        let t_path = self.class_rust_path(t_class);
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = false;
        self.w.push_str("{ ");
        self.emit_bind_recv_binding("__jux_bt", t_recv);
        self.emit_bind_recv_binding("__jux_bs", s_recv);
        // 1 — initial sync: target takes the source's current value
        // through the RAW setter (a binding write is not a "direct
        // assignment" — P2's E0973 guard must not fire on it).
        self.w
            .push_str(&format!("__jux_bt.__set_{t_prop}_raw(__jux_bs.{s_prop}()); "));
        // Shared kill token (P4): both directions check it, either
        // side's unbind/rebind deactivates it. The store's third slot
        // records bidirectionality — a BIDIRECTIONAL binding leaves
        // direct sets legal on either side (JavaFX semantics), so the
        // P2/E0973 guard only fires for one-way targets.
        let bidi = if bidirectional { "true" } else { "false" };
        self.w.push_str(
            "let __jux_alive = std::rc::Rc::new(std::cell::Cell::new(true)); ",
        );
        // 2/3/4 — source → target direction.
        self.w.push_str(&format!(
            "let __jux_tw = __jux_bt.__jux_weak(); \
             let __jux_alive_f = __jux_alive.clone(); \
             let __jux_f: std::rc::Rc<dyn Fn({s_ty}, {s_ty})> = std::rc::Rc::new(move |_, __jux_n| {{ \
             if !__jux_alive_f.get() {{ return; }} \
             if let Some(__jux_rc) = __jux_tw.upgrade() {{ {t_path}::__jux_from_inner(__jux_rc).__set_{t_prop}_raw(__jux_n); }} }}); \
             __jux_bt.__bind_{t_prop}_store(Some((crate::JuxObserver::Strong(__jux_f.clone()), __jux_alive.clone(), {bidi}))); \
             __jux_bs.__obs_{s_prop}_attach_full(crate::JuxObserver::Weak(std::rc::Rc::downgrade(&__jux_f))); "
        ));
        if bidirectional {
            // target → source direction, same wiring mirrored — and
            // the SAME kill token, so one unbind breaks both (P4).
            let s_path = self.class_rust_path(s_class);
            self.w.push_str(&format!(
                "let __jux_sw = __jux_bs.__jux_weak(); \
                 let __jux_alive_g = __jux_alive.clone(); \
                 let __jux_g: std::rc::Rc<dyn Fn({t_ty}, {t_ty})> = std::rc::Rc::new(move |_, __jux_n| {{ \
                 if !__jux_alive_g.get() {{ return; }} \
                 if let Some(__jux_rc) = __jux_sw.upgrade() {{ {s_path}::__jux_from_inner(__jux_rc).__set_{s_prop}_raw(__jux_n); }} }}); \
                 __jux_bs.__bind_{s_prop}_store(Some((crate::JuxObserver::Strong(__jux_g.clone()), __jux_alive.clone(), true))); \
                 __jux_bt.__obs_{t_prop}_attach_full(crate::JuxObserver::Weak(std::rc::Rc::downgrade(&__jux_g))); "
            ));
        }
        self.w.push('}');
        self.emitting_format_arg = prev;
    }

    /// Emit `prop.unbind()` (§P.4.4) — drop the keep-alive; the weak
    /// observer left at the source dies with it and is pruned on the
    /// source's next fire. Safe when not bound (`None` → `None`).
    pub(crate) fn emit_unbind(&mut self, recv: Option<&juxc_ast::Expr>, prop: &str) {
        if self.observers_receiver_is_ctor_inner(recv) {
            self.emit_observers_receiver(recv);
            self.w.push_str(&format!(".__bind_{prop} = None"));
            return;
        }
        self.emit_observers_receiver(recv);
        self.w.push_str(&format!(".__bind_{prop}_store(None)"));
    }

    /// Emit a `.observers.clear` / `.observers.size` read (§P.3.2 —
    /// command accessors, no parentheses in Jux).
    pub(crate) fn emit_observers_command(
        &mut self,
        recv: Option<&juxc_ast::Expr>,
        prop: &str,
        op: &str,
    ) {
        let ctor_inner = self.observers_receiver_is_ctor_inner(recv);
        if op == "clear" {
            if ctor_inner {
                self.w.push_str("{ ");
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(".__obs_{prop}_full = None; "));
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(".__obs_{prop}_inv = None; }}"));
            } else {
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(".__obs_{prop}_clear()"));
            }
        } else {
            // size
            if ctor_inner {
                self.w.push_str("((");
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(
                    ".__obs_{prop}_full.as_ref().map(|v| v.iter().filter(|o| o.upgrade().is_some()).count()).unwrap_or(0) + "
                ));
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(
                    ".__obs_{prop}_inv.as_ref().map(|v| v.iter().filter(|o| o.upgrade().is_some()).count()).unwrap_or(0)) as isize)"
                ));
            } else {
                self.emit_observers_receiver(recv);
                self.w.push_str(&format!(".__obs_{prop}_size()"));
            }
        }
    }

    /// P7: module-scope `thread_local!` observer storage for a class's
    /// STATIC observable properties. Emitted next to the
    /// `LazyLock<Mutex>` value backing — one full vec + one
    /// invalidation vec per property. `thread_local` because observers
    /// are `Rc` closures (`!Send`); see [`static_observable_props`].
    pub(crate) fn emit_static_observer_storage(&mut self, class_decl: &ClassDecl) {
        let props = static_observable_props(class_decl);
        if props.is_empty() {
            return;
        }
        let class = class_decl.name.text.clone();
        self.w.line("thread_local! {");
        self.w.indent_inc();
        for prop in static_observable_props(class_decl) {
            let x = prop.name.text.clone();
            let t = self.render_type_to_string(&prop.ty);
            self.w.line(&format!(
                "pub static {class}___obs_{x}_full: std::cell::RefCell<Vec<crate::JuxObserver<dyn Fn({t}, {t})>>> = std::cell::RefCell::new(Vec::new());"
            ));
            self.w.line(&format!(
                "pub static {class}___obs_{x}_inv: std::cell::RefCell<Vec<crate::JuxObserver<dyn Fn()>>> = std::cell::RefCell::new(Vec::new());"
            ));
        }
        self.w.indent_dec();
        self.w.line("}");
    }

    /// P7: the attach/detach/clear/size/fire helpers for STATIC
    /// observable properties — associated functions (no receiver)
    /// over the module-scope `thread_local!` storage. The fire
    /// routine keeps the instance semantics: vectors are taken out of
    /// the cell during the pass (a re-entrant static set inside an
    /// observer body sees an empty list and no-ops; the setter's
    /// quiescence loop fires the follow-up transition), and dead weak
    /// observers are pruned on the way.
    pub(crate) fn emit_static_observer_helper_methods(&mut self, class_decl: &ClassDecl) {
        let class = class_decl.name.text.clone();
        let raw: Vec<(String, juxc_ast::TypeRef)> = static_observable_props(class_decl)
            .iter()
            .map(|p| (p.name.text.clone(), p.ty.clone()))
            .collect();
        for (x, ty) in raw {
            let t = self.render_type_to_string(&ty);
            self.w.indent_inc();

            // -- attach --------------------------------------------------
            self.w.line(&format!(
                "pub fn __obs_{x}_attach_full(o: crate::JuxObserver<dyn Fn({t}, {t})>) {{ {class}___obs_{x}_full.with(|v| v.borrow_mut().push(o)); }}"
            ));
            self.w.line(&format!(
                "pub fn __obs_{x}_attach_inv(o: crate::JuxObserver<dyn Fn()>) {{ {class}___obs_{x}_inv.with(|v| v.borrow_mut().push(o)); }}"
            ));

            // -- detach (pointer identity) --------------------------------
            self.w.line(&format!(
                "pub fn __obs_{x}_detach_full(t: &std::rc::Rc<dyn Fn({t}, {t})>) {{ {class}___obs_{x}_full.with(|v| v.borrow_mut().retain(|o| !o.is_for(t))); }}"
            ));
            self.w.line(&format!(
                "pub fn __obs_{x}_detach_inv(t: &std::rc::Rc<dyn Fn()>) {{ {class}___obs_{x}_inv.with(|v| v.borrow_mut().retain(|o| !o.is_for(t))); }}"
            ));

            // -- clear / size ---------------------------------------------
            self.w.line(&format!(
                "pub fn __obs_{x}_clear() {{ {class}___obs_{x}_full.with(|v| v.borrow_mut().clear()); {class}___obs_{x}_inv.with(|v| v.borrow_mut().clear()); }}"
            ));
            self.w.line(&format!("pub fn __obs_{x}_size() -> isize {{"));
            self.w.indent_inc();
            self.w.line(&format!(
                "let full = {class}___obs_{x}_full.with(|v| v.borrow().iter().filter(|o| o.upgrade().is_some()).count());"
            ));
            self.w.line(&format!(
                "let inv = {class}___obs_{x}_inv.with(|v| v.borrow().iter().filter(|o| o.upgrade().is_some()).count());"
            ));
            self.w.line("(full + inv) as isize");
            self.w.indent_dec();
            self.w.line("}");

            // -- fire ------------------------------------------------------
            self.w
                .line(&format!("pub fn __obs_{x}_fire(old: &{t}, now: &{t}) {{"));
            self.w.indent_inc();
            self.w.line(&format!(
                "let mut v = {class}___obs_{x}_inv.with(|c| std::mem::take(&mut *c.borrow_mut()));"
            ));
            self.w.line(
                "v.retain(|o| match o.upgrade() { Some(f) => { f(); true } None => false });",
            );
            self.w.line(&format!(
                "{class}___obs_{x}_inv.with(|c| {{ let mut cur = c.borrow_mut(); v.extend(std::mem::take(&mut *cur)); *cur = v; }});"
            ));
            self.w.line(&format!(
                "let mut v = {class}___obs_{x}_full.with(|c| std::mem::take(&mut *c.borrow_mut()));"
            ));
            self.w.line(
                "v.retain(|o| match o.upgrade() { Some(f) => { f(old.clone(), now.clone()); true } None => false });",
            );
            self.w.line(&format!(
                "{class}___obs_{x}_full.with(|c| {{ let mut cur = c.borrow_mut(); v.extend(std::mem::take(&mut *cur)); *cur = v; }});"
            ));
            self.w.indent_dec();
            self.w.line("}");

            self.w.indent_dec();
            self.w.newline();
        }
    }

    /// P7: emit `Config.Level.observers.attach(o)` / `.detach(o)` —
    /// routed to the class's static observer helpers.
    pub(crate) fn emit_static_observers_call(
        &mut self,
        class: &str,
        prop: &str,
        op: &str,
        call: &juxc_ast::CallExpr,
    ) {
        let Some(arg) = call.args.first() else {
            self.w.push_str("()");
            return;
        };
        let arity = self.observer_arg_arity(arg);
        let vec_kind = if arity == 0 { "inv" } else { "full" };
        let elem_ty = if arity == 0 {
            "crate::JuxObserver<dyn Fn()>".to_string()
        } else {
            let t = self
                .prop_rust_ty(class, prop)
                .unwrap_or_else(|| "()".to_string());
            format!("crate::JuxObserver<dyn Fn({t}, {t})>")
        };
        let path = self.class_rust_path(class);
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = false;
        if op == "attach" {
            self.w.push_str(&format!("{{ let __jux_o: {elem_ty} = "));
            self.emit_observer_value(prop, arg, arity);
            self.w
                .push_str(&format!("; {path}::__obs_{prop}_attach_{vec_kind}(__jux_o); }}"));
        } else {
            self.w.push_str("{ let __jux_t = (");
            self.emit_expr(arg);
            self.w
                .push_str(&format!("); {path}::__obs_{prop}_detach_{vec_kind}(&__jux_t); }}"));
        }
        self.emitting_format_arg = prev;
    }

    /// P7: emit `Config.Level.observers.clear` / `.size`.
    pub(crate) fn emit_static_observers_command(&mut self, class: &str, prop: &str, op: &str) {
        let path = self.class_rust_path(class);
        if op == "clear" {
            self.w.push_str(&format!("{path}::__obs_{prop}_clear()"));
        } else {
            self.w.push_str(&format!("{path}::__obs_{prop}_size()"));
        }
    }

    /// Non-rendering check: does `class_bare`'s Kind trait carry
    /// observer-helper signatures? (Wrapper class with own observable
    /// or computed props.)
    pub(crate) fn class_has_kind_observer_props(&self, class_bare: &str) -> bool {
        self.wrapper_classes.contains(class_bare)
            && self
                .class_ast_by_bare(class_bare)
                .map(|cd| {
                    !observable_props(cd).is_empty()
                        || !computed_observable_props(cd).is_empty()
                })
                .unwrap_or(false)
    }

    /// The observable props (settable + computed) of `class_bare`'s
    /// OWN declaration, as `(name, rust_ty)` — the set surfaced on the
    /// class's `Kind` trait so observer operations dispatch through a
    /// base-typed (`Rc<dyn …Kind>`) reference (Java semantics).
    /// Inherited props ride the supertrait chain, so only own props
    /// appear here. Empty for non-wrapper classes (no helper targets).
    fn kind_trait_observer_props(&mut self, class_bare: &str) -> Vec<(String, String)> {
        if !self.wrapper_classes.contains(class_bare) {
            return Vec::new();
        }
        let raw: Vec<(String, juxc_ast::TypeRef)> = match self.class_ast_by_bare(class_bare) {
            Some(cd) => observable_props(cd)
                .into_iter()
                .chain(computed_observable_props(cd))
                .map(|p| (p.name.text.clone(), p.ty.clone()))
                .collect(),
            None => Vec::new(),
        };
        raw.into_iter()
            .map(|(n, ty)| {
                let t = self.render_type_to_string(&ty);
                (n, t)
            })
            .collect()
    }

    /// Emit the observer-helper SIGNATURES into a polymorphic base's
    /// `Kind` trait body, so `A a = new B(); a.p.observers.attach(…)`
    /// dispatches through the trait object.
    pub(crate) fn emit_observer_trait_sigs(&mut self, class_bare: &str) {
        for (x, t) in self.kind_trait_observer_props(class_bare) {
            self.w.line(&format!(
                "fn __obs_{x}_attach_full(&self, o: crate::JuxObserver<dyn Fn({t}, {t})>);"
            ));
            self.w.line(&format!(
                "fn __obs_{x}_attach_inv(&self, o: crate::JuxObserver<dyn Fn()>);"
            ));
            self.w.line(&format!(
                "fn __obs_{x}_detach_full(&self, t: &std::rc::Rc<dyn Fn({t}, {t})>);"
            ));
            self.w.line(&format!(
                "fn __obs_{x}_detach_inv(&self, t: &std::rc::Rc<dyn Fn()>);"
            ));
            self.w.line(&format!("fn __obs_{x}_clear(&self);"));
            self.w.line(&format!("fn __obs_{x}_size(&self) -> isize;"));
        }
    }

    /// Emit the delegating observer-helper BODIES into an
    /// `impl <owner>Kind for <implementor>` block. The implementor's
    /// inherent helpers (depth-aware for inherited props) do the work.
    pub(crate) fn emit_observer_impl_methods(
        &mut self,
        owner_bare: &str,
        implementor_bare: &str,
    ) {
        for (x, t) in self.kind_trait_observer_props(owner_bare) {
            self.w.line(&format!(
                "fn __obs_{x}_attach_full(&self, o: crate::JuxObserver<dyn Fn({t}, {t})>) {{ {implementor_bare}::__obs_{x}_attach_full(self, o) }}"
            ));
            self.w.line(&format!(
                "fn __obs_{x}_attach_inv(&self, o: crate::JuxObserver<dyn Fn()>) {{ {implementor_bare}::__obs_{x}_attach_inv(self, o) }}"
            ));
            self.w.line(&format!(
                "fn __obs_{x}_detach_full(&self, t: &std::rc::Rc<dyn Fn({t}, {t})>) {{ {implementor_bare}::__obs_{x}_detach_full(self, t) }}"
            ));
            self.w.line(&format!(
                "fn __obs_{x}_detach_inv(&self, t: &std::rc::Rc<dyn Fn()>) {{ {implementor_bare}::__obs_{x}_detach_inv(self, t) }}"
            ));
            self.w.line(&format!(
                "fn __obs_{x}_clear(&self) {{ {implementor_bare}::__obs_{x}_clear(self) }}"
            ));
            self.w.line(&format!(
                "fn __obs_{x}_size(&self) -> isize {{ {implementor_bare}::__obs_{x}_size(self) }}"
            ));
        }
    }
}
