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

use crate::{rollup_class_reps, ClassRep};

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
    rollup_class_reps(&mut reps, units);
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
    let mut out: HashSet<String> = HashSet::new();
    walk_unit_exprs(units, &mut |e| {
        let Expr::Call(c) = e else { return };
        if !callee_is_worker_spawn(&c.callee) {
            return;
        }
        let Some(Expr::Lambda(l)) = c.args.first() else { return };
        let params: HashSet<&str> = l.params.iter().map(|p| p.name.text.as_str()).collect();
        // Resolve each capture's type as we see it: the walker hands out
        // borrows valid only inside the callback, so nothing is collected.
        let mut visit = |inner: &Expr| {
            let Expr::Path(qn) = inner else { return };
            if qn.segments.len() != 1 || params.contains(qn.segments[0].text.as_str()) {
                return;
            }
            if let Some(Ty::User { name, .. }) = peel(expr_types.get(&qn.span)) {
                out.insert(name.rsplit('.').next().unwrap_or(name).to_string());
            }
        };
        match &l.body {
            LambdaBody::Expr(b) => walk_expr(b, &mut visit),
            LambdaBody::Block(b) => walk_block(b, &mut visit),
        }
    });
    out
}

/// `Worker.spawn` — the one call form that starts another OS thread (§18.2).
fn callee_is_worker_spawn(callee: &Expr) -> bool {
    let Expr::Field(f) = callee else { return false };
    f.field.text == "spawn"
        && matches!(
            f.object.as_ref(),
            Expr::Path(qn)
                if qn.segments.last().map(|s| s.text.as_str()) == Some("Worker")
        )
}

/// Strip `T?` / `T[]` down to the type that decides shareability.
fn peel(ty: Option<&Ty>) -> Option<&Ty> {
    match ty? {
        Ty::Nullable(inner) => peel(Some(inner)),
        Ty::Array { element, .. } => peel(Some(element)),
        other => Some(other),
    }
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

/// Visit every expression in every body of the program — free functions, class
/// methods, constructors and operator overloads.
fn walk_unit_exprs(units: &[juxc_ast::CompilationUnit], sink: &mut dyn FnMut(&Expr)) {
    for unit in units {
        for item in &unit.items {
            match item {
                juxc_ast::TopLevelDecl::Function(f) => {
                    if let Some(b) = &f.body {
                        walk_block(b, sink);
                    }
                }
                juxc_ast::TopLevelDecl::Class(cd) => {
                    for m in &cd.methods {
                        if let Some(b) = &m.body {
                            walk_block(b, sink);
                        }
                    }
                    for ctor in &cd.constructors {
                        walk_block(&ctor.body, sink);
                    }
                    for op in &cd.operators {
                        if let Some(b) = &op.body {
                            walk_block(b, sink);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

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
        Stmt::Unsafe(b) => walk_block(b, sink),
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
