//! Which classes cross a **worker boundary**, and can therefore have their
//! refcount made atomic.
//!
//! `JUX-ASYNC-ADDENDUM.md` §18.2 lists among the transferable types "`class`
//! types whose refcount can be made atomic; the compiler upgrades the refcount
//! automatically when an instance crosses a worker boundary". This module finds
//! that set. Its members lower to `Arc<Mutex<C_Inner>>` instead of the default
//! `Rc<RefCell<C_Inner>>`, so the handle is `Send + Sync` and the object is
//! genuinely SHARED across threads — a worker's mutation is visible to the
//! caller, which is what the same code means on one thread.
//!
//! Only classes that actually cross a boundary pay for it. Atomic refcounting
//! and a mutex are real costs, and the overwhelming majority of objects in a
//! program never leave their thread.

use std::collections::{HashMap, HashSet};

use juxc_ast::{Block, ElseBranch, Expr, InterpSegment, LambdaBody, Stmt};
use juxc_source::Span;
use juxc_tycheck::Ty;

use crate::{bare_extends_adjacency, rollup_class_reps, ClassRep};

/// Bare names of the classes whose instances cross a worker boundary — captured
/// by a `Worker.spawn` closure, plus every class reachable from one through a
/// field.
///
/// The field closure is what makes the upgrade sound: `Arc<Mutex<C_Inner>>` is
/// `Send` only if `C_Inner` is, so a class field of a shared class has to be
/// upgraded with it. A class holding something that cannot be made atomic is
/// excluded; [`compute_worker_shared_blockers`] says why, so the capture stays a
/// diagnostic rather than becoming a miscompile.
pub(crate) fn compute_worker_shared_classes(
    units: &[juxc_ast::CompilationUnit],
    expr_types: &HashMap<Span, Ty>,
    symbols: &juxc_tycheck::SymbolTable,
) -> HashSet<String> {
    let blocked = worker_share_blockers(units, symbols);
    let mut out = worker_capture_class_names(units, expr_types);
    out.retain(|n| !blocked.contains_key(n));
    if out.is_empty() {
        return HashSet::new();
    }
    let fields = class_field_class_names(units);
    let mut changed = true;
    while changed {
        changed = false;
        for (owner, referenced) in &fields {
            if !out.contains(owner) {
                continue;
            }
            for r in referenced {
                if !blocked.contains_key(r) && out.insert(r.clone()) {
                    changed = true;
                }
            }
        }
    }
    // A subclass and its base share one storage layout, so the whole `extends`
    // component moves together — otherwise a child's `__parent` slice would
    // disagree with the parent's own handle shape.
    let mut reps: HashMap<String, ClassRep> = HashMap::new();
    for unit in units {
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                let rep = if out.contains(&cd.name.text) {
                    ClassRep::ArcMutex
                } else {
                    ClassRep::RcRefCell
                };
                reps.insert(cd.name.text.clone(), rep);
            }
        }
    }
    rollup_class_reps(&mut reps, &bare_extends_adjacency(units));
    reps.into_iter()
        .filter(|(n, r)| *r == ClassRep::ArcMutex && !blocked.contains_key(n))
        .map(|(n, _)| n)
        .collect()
}

/// Why each class cannot have its refcount made atomic, keyed by class name.
///
/// Delegates to [`juxc_tycheck::SymbolTable::worker_share_blocker`], the same
/// rule the `Worker.spawn` capture diagnostic applies — so what the checker
/// refuses and what the backend upgrades can never disagree.
fn worker_share_blockers(
    units: &[juxc_ast::CompilationUnit],
    symbols: &juxc_tycheck::SymbolTable,
) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    for unit in units {
        for item in &unit.items {
            if let juxc_ast::TopLevelDecl::Class(cd) = item {
                if let Some(why) = symbols.worker_share_blocker(&cd.name.text) {
                    out.insert(cd.name.text.clone(), why);
                }
            }
        }
    }
    out
}

/// Class names captured by a `Worker.spawn(…)` closure anywhere in the program.
fn worker_capture_class_names(
    units: &[juxc_ast::CompilationUnit],
    expr_types: &HashMap<Span, Ty>,
) -> HashSet<String> {
    worker_capture_seeds(units, expr_types)
        .into_iter()
        .map(|fqn| fqn.rsplit('.').next().unwrap_or(&fqn).to_string())
        .collect()
}

/// Every class a `Worker.spawn(…)` closure takes across, by the name its type
/// carries (the FQN when the class lives in a package).
///
/// Two sources. A captured value's type, walked through `T?`, `T[]` and type
/// arguments, because a `Vec<Job>` capture hands the worker the `Job`s in it.
/// And the enclosing class, when the closure reaches `this`: explicitly, or
/// through a bare name that is one of the class's own fields, properties or
/// methods (Java's `total` for `this.total`). Missing the second made a method
/// that spawned work over its own fields send `Rc<RefCell<…>>` to a thread.
fn worker_capture_seeds(
    units: &[juxc_ast::CompilationUnit],
    expr_types: &HashMap<Span, Ty>,
) -> HashSet<String> {
    let mut out: HashSet<String> = HashSet::new();
    for_each_worker_lambda(units, &mut |owner, l| {
        let params: HashSet<&str> = l.params.iter().map(|p| p.name.text.as_str()).collect();
        let mut reaches_this = false;
        // Resolve each capture's type as we see it: the walker hands out
        // borrows valid only inside the callback, so nothing is collected.
        let mut visit = |inner: &Expr| match inner {
            Expr::This(_) => reaches_this = true,
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = qn.segments[0].text.as_str();
                if params.contains(name) {
                    return;
                }
                if let Some(ty) = expr_types.get(&qn.span) {
                    ty_class_names(ty, &mut out);
                }
                if let Some((_, cd)) = owner {
                    let member = cd.fields.iter().any(|f| f.name.text == name)
                        || cd.properties.iter().any(|p| p.name.text == name)
                        || cd.methods.iter().any(|m| m.name.text == name);
                    if member {
                        reaches_this = true;
                    }
                }
            }
            _ => {}
        };
        match &l.body {
            LambdaBody::Expr(b) => walk_expr(b, &mut visit),
            LambdaBody::Block(b) => walk_block(b, &mut visit),
        }
        if reaches_this {
            if let Some((pkg, cd)) = owner {
                out.insert(if pkg.is_empty() {
                    cd.name.text.clone()
                } else {
                    format!("{pkg}.{}", cd.name.text)
                });
            }
        }
    });
    out
}

/// Every user type name `ty` mentions, through `T?`, `T[]` and type arguments.
/// Names that are not classes are filtered out later against the class table.
fn ty_class_names(ty: &Ty, out: &mut HashSet<String>) {
    match ty {
        Ty::Nullable(inner) => ty_class_names(inner, out),
        Ty::Array { element, .. } => ty_class_names(element, out),
        Ty::User { name, generic_args } => {
            out.insert(name.clone());
            for a in generic_args {
                ty_class_names(a, out);
            }
        }
        _ => {}
    }
}

/// Where a worker closure sits: its package and class, or `None` in a free
/// function.
type LambdaOwner<'a> = Option<(&'a str, &'a juxc_ast::ClassDecl)>;

/// Call `sink` for every `Worker.spawn(…)` closure in the program, with the
/// package and class declaration it sits in (`None` in a free function).
fn for_each_worker_lambda(
    units: &[juxc_ast::CompilationUnit],
    sink: &mut dyn FnMut(LambdaOwner<'_>, &juxc_ast::LambdaExpr),
) {
    let mut in_body = |owner: LambdaOwner<'_>, b: &Block| {
        walk_block(b, &mut |e| {
            let Expr::Call(c) = e else { return };
            if !is_worker_spawn_callee(&c.callee) {
                return;
            }
            if let Some(Expr::Lambda(l)) = c.args.first() {
                sink(owner, l);
            }
        });
    };
    for unit in units {
        let pkg = unit
            .package
            .as_ref()
            .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
            .unwrap_or_default();
        for item in &unit.items {
            match item {
                juxc_ast::TopLevelDecl::Function(f) => {
                    if let Some(b) = &f.body {
                        in_body(None, b);
                    }
                }
                juxc_ast::TopLevelDecl::Class(cd) => {
                    let owner = Some((pkg.as_str(), cd));
                    for m in &cd.methods {
                        if let Some(b) = &m.body {
                            in_body(owner, b);
                        }
                    }
                    for ctor in &cd.constructors {
                        in_body(owner, &ctor.body);
                    }
                    for op in &cd.operators {
                        if let Some(b) = &op.body {
                            in_body(owner, b);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// The fully-qualified names of the classes that cross a worker boundary.
///
/// The same closure as [`compute_worker_shared_classes`] -- capture seeds, then
/// every class a shared class's fields reach -- with each class named by FQN
/// rather than bare name. The bare set drives the class-representation system,
/// which is keyed by bare name throughout; this one is for the questions where
/// two same-named classes in different packages must not be confused, the
/// first being "is the class I am emitting worker-shared?".
///
/// `units[i]` and `symbols.units[i]` describe the same compilation unit (the
/// driver builds both from one list), which is how a field type's bare head is
/// resolved in the package and imports of the unit that wrote it.
pub(crate) fn compute_worker_shared_class_fqns(
    units: &[juxc_ast::CompilationUnit],
    expr_types: &HashMap<Span, Ty>,
    symbols: &juxc_tycheck::SymbolTable,
) -> HashSet<String> {
    // Seeds: a capture's checked type is already a full name, and so is the
    // enclosing class a closure reaches through `this`. Only classes count.
    let mut out = worker_capture_seeds(units, expr_types);
    out.retain(|fqn| {
        symbols.classes.get(fqn).is_some_and(|c| !c.is_external)
            && symbols.worker_share_blocker(fqn.rsplit('.').next().unwrap_or(fqn)).is_none()
    });
    if out.is_empty() {
        return out;
    }

    // Edges: each class's FQN to the FQNs its fields name, resolved in the
    // declaring unit's own context.
    let resolve = |idx: usize, head: &str| -> Option<String> {
        let ctx = symbols.units.get(idx);
        if let Some(fqn) = ctx.and_then(|c| c.unqualified.get(head)) {
            if symbols.classes.contains_key(fqn) {
                return Some(fqn.clone());
            }
        }
        if let Some(ctx) = ctx {
            if !ctx.package.is_empty() {
                let cand = format!("{}.{}", ctx.package.join("."), head);
                if symbols.classes.contains_key(&cand) {
                    return Some(cand);
                }
            }
        }
        symbols.classes.contains_key(head).then(|| head.to_string())
    };
    let mut edges: HashMap<String, HashSet<String>> = HashMap::new();
    for (idx, unit) in units.iter().enumerate() {
        let pkg = unit
            .package
            .as_ref()
            .map(|p| p.name.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("."))
            .unwrap_or_default();
        for item in &unit.items {
            let juxc_ast::TopLevelDecl::Class(cd) = item else { continue };
            let owner = if pkg.is_empty() {
                cd.name.text.clone()
            } else {
                format!("{pkg}.{}", cd.name.text)
            };
            let entry = edges.entry(owner).or_default();
            for f in &cd.fields {
                let Some(ty) = &f.ty else { continue };
                let mut heads: Vec<&str> =
                    ty.name.segments.last().map(|s| s.text.as_str()).into_iter().collect();
                for a in &ty.generic_args {
                    if let Some(t) = a.as_type() {
                        heads.extend(t.name.segments.last().map(|s| s.text.as_str()));
                    }
                }
                for h in heads {
                    if let Some(fqn) = resolve(idx, h) {
                        entry.insert(fqn);
                    }
                }
            }
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for (owner, referenced) in &edges {
            if !out.contains(owner) {
                continue;
            }
            for r in referenced {
                let bare = r.rsplit('.').next().unwrap_or(r);
                if symbols.worker_share_blocker(bare).is_none() && out.insert(r.clone()) {
                    changed = true;
                }
            }
        }
    }
    out
}

/// `Worker.spawn` — the one call form that starts another OS thread (§18.2).
pub(crate) fn is_worker_spawn_callee(callee: &Expr) -> bool {
    let Expr::Field(f) = callee else { return false };
    f.field.text == "spawn"
        && matches!(
            f.object.as_ref(),
            Expr::Path(qn)
                if qn.segments.last().map(|s| s.text.as_str()) == Some("Worker")
        )
}

/// For each class, the classes its fields name — the edges the shared-class
/// closure walks. A `Vec<Job>` field reaches `Job` just as a `Job` field does.
fn class_field_class_names(
    units: &[juxc_ast::CompilationUnit],
) -> HashMap<String, HashSet<String>> {
    let classes: HashSet<&str> = units
        .iter()
        .flat_map(|u| u.items.iter())
        .filter_map(|i| match i {
            juxc_ast::TopLevelDecl::Class(cd) => Some(cd.name.text.as_str()),
            _ => None,
        })
        .collect();
    let mut out: HashMap<String, HashSet<String>> = HashMap::new();
    for unit in units {
        for item in &unit.items {
            let juxc_ast::TopLevelDecl::Class(cd) = item else { continue };
            let entry = out.entry(cd.name.text.clone()).or_default();
            for f in &cd.fields {
                let Some(ty) = &f.ty else { continue };
                let mut heads: Vec<&str> =
                    ty.name.segments.last().map(|s| s.text.as_str()).into_iter().collect();
                for a in &ty.generic_args {
                    if let Some(t) = a.as_type() {
                        heads.extend(t.name.segments.last().map(|s| s.text.as_str()));
                    }
                }
                for h in heads {
                    if classes.contains(h) {
                        entry.insert(h.to_string());
                    }
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Expression walk
// ---------------------------------------------------------------------------

fn walk_block(b: &Block, sink: &mut dyn FnMut(&Expr)) {
    for s in &b.statements {
        walk_stmt(s, sink);
    }
}

pub(crate) fn walk_stmt(s: &Stmt, sink: &mut dyn FnMut(&Expr)) {
    match s {
        Stmt::Expr(e) | Stmt::Return(Some(e), _) | Stmt::Throw(e, _) => walk_expr(e, sink),
        Stmt::VarDecl(v) => {
            if let Some(init) = &v.init {
                walk_expr(init, sink);
            }
        }
        Stmt::Assign(a) => {
            walk_expr(&a.target, sink);
            walk_expr(&a.value, sink);
        }
        Stmt::If(i) => {
            walk_expr(&i.condition, sink);
            walk_block(&i.then_block, sink);
            match i.else_branch.as_deref() {
                Some(ElseBranch::Block(b)) => walk_block(b, sink),
                Some(ElseBranch::If(inner)) => walk_stmt(&Stmt::If(inner.clone()), sink),
                None => {}
            }
        }
        Stmt::While(w) => {
            walk_expr(&w.condition, sink);
            walk_block(&w.body, sink);
        }
        Stmt::DoWhile(d) => {
            walk_block(&d.body, sink);
            walk_expr(&d.condition, sink);
        }
        Stmt::ForEach(f) => {
            walk_expr(&f.iter, sink);
            walk_block(&f.body, sink);
        }
        Stmt::ForC(f) => {
            if let Some(init) = &f.init {
                walk_stmt(init, sink);
            }
            if let Some(c) = &f.cond {
                walk_expr(c, sink);
            }
            if let Some(u) = &f.update {
                walk_stmt(u, sink);
            }
            walk_block(&f.body, sink);
        }
        Stmt::Try(t) => {
            walk_block(&t.body, sink);
            for c in &t.catches {
                walk_block(&c.body, sink);
            }
            if let Some(f) = &t.finally {
                walk_block(f, sink);
            }
        }
        Stmt::Block(b) | Stmt::Unsafe(b) => walk_block(b, sink),
        Stmt::Labeled { stmt, .. } => walk_stmt(stmt, sink),
        Stmt::SuperCall(args, _) => {
            for a in args {
                walk_expr(a, sink);
            }
        }
        _ => {}
    }
}

pub(crate) fn walk_expr(e: &Expr, sink: &mut dyn FnMut(&Expr)) {
    sink(e);
    match e {
        Expr::Call(c) => {
            walk_expr(&c.callee, sink);
            for a in &c.args {
                walk_expr(a, sink);
            }
        }
        Expr::NewObject(n) => {
            for a in &n.args {
                walk_expr(a, sink);
            }
        }
        Expr::NewArrayLit(n) => {
            for el in &n.elements {
                walk_expr(el, sink);
            }
        }
        Expr::NewArray(n) => {
            walk_expr(&n.size, sink);
            for inner in &n.inner_sizes {
                walk_expr(inner, sink);
            }
        }
        Expr::Binary(b) => {
            walk_expr(&b.left, sink);
            walk_expr(&b.right, sink);
        }
        Expr::Unary(u) => walk_expr(&u.operand, sink),
        Expr::Range(r) => {
            walk_expr(&r.start, sink);
            walk_expr(&r.end, sink);
        }
        Expr::Cast(c) => walk_expr(&c.value, sink),
        Expr::TypeTest(t) => walk_expr(&t.value, sink),
        Expr::Index(i) => {
            walk_expr(&i.array, sink);
            walk_expr(&i.index, sink);
        }
        Expr::Field(f) => walk_expr(&f.object, sink),
        Expr::InterpString(s) => {
            for seg in &s.segments {
                if let InterpSegment::Expr(inner) = seg {
                    walk_expr(inner, sink);
                }
            }
        }
        Expr::Elvis(el) => {
            walk_expr(&el.value, sink);
            walk_expr(&el.fallback, sink);
        }
        Expr::Ternary(t) => {
            walk_expr(&t.condition, sink);
            walk_expr(&t.then_branch, sink);
            walk_expr(&t.else_branch, sink);
        }
        Expr::Await(inner, _) | Expr::NotNullAssert(inner, _) => walk_expr(inner, sink),
        Expr::Switch(sw) => {
            walk_expr(&sw.scrutinee, sink);
            for arm in &sw.arms {
                if let Some(g) = &arm.guard {
                    walk_expr(g, sink);
                }
                match &arm.body {
                    juxc_ast::SwitchBody::Expr(b) => walk_expr(b, sink),
                    juxc_ast::SwitchBody::Block(b) => walk_block(b, sink),
                }
            }
        }
        Expr::Lambda(l) => match &l.body {
            LambdaBody::Expr(b) => walk_expr(b, sink),
            LambdaBody::Block(b) => walk_block(b, sink),
        },
        _ => {}
    }
}
