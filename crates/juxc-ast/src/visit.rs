//! A structural walk over every expression and statement in a body.
//!
//! One visitor, shared by the passes that need to find a kind of node
//! wherever it appears: the backend's bound analysis (`new T[n]` needs
//! `T: Default`) and the checker's capture test for a lambda given where a
//! function pointer is expected. The matches list every variant with no
//! catch-all, so adding an expression or statement kind does not compile
//! until someone decides how it is walked. The older hand-written walkers in
//! the backend's `analysis` each cover the shapes their own question needed,
//! and each missed some.

use crate::{Block, ElseBranch, Expr, Stmt};

/// One node the walk reaches.
#[derive(Clone, Copy)]
pub enum Node<'a> {
    /// An expression, reached before the expressions inside it.
    Expr(&'a Expr),
    /// A statement, reached before the statements and expressions inside it.
    Stmt(&'a Stmt),
}

/// Call `f` on every statement and expression in `block`, recursively:
/// nested blocks, lambda bodies, anonymous-class members, switch arms and loop
/// headers included.
pub fn for_each_node(block: &Block, f: &mut dyn FnMut(Node<'_>)) {
    walk_block(block, f);
}

/// Call `f` on every expression in `block`, recursively (see
/// [`for_each_node`]).
pub fn for_each_expr(block: &Block, f: &mut dyn FnMut(&Expr)) {
    walk_block(block, &mut |n| {
        if let Node::Expr(e) = n {
            f(e);
        }
    });
}

/// [`for_each_expr`] over one expression and everything inside it.
pub fn for_each_expr_in(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    walk_expr(e, &mut |n| {
        if let Node::Expr(e) = n {
            f(e);
        }
    });
}

/// [`for_each_node`] over one expression and everything inside it.
pub fn for_each_node_in(e: &Expr, f: &mut dyn FnMut(Node<'_>)) {
    walk_expr(e, f);
}

fn walk_block(block: &Block, f: &mut dyn FnMut(Node<'_>)) {
    for st in &block.statements {
        walk_stmt(st, f);
    }
}

fn walk_expr(e: &Expr, f: &mut dyn FnMut(Node<'_>)) {
    f(Node::Expr(e));
    match e {
        Expr::Literal(_)
        | Expr::Path(_)
        | Expr::This(_)
        | Expr::Super(_)
        | Expr::MethodRef(_) => {}
        Expr::Out(inner, _)
        | Expr::TypeOf(inner, _)
        | Expr::Await(inner, _)
        | Expr::ErrorProp(inner, _)
        | Expr::NotNullAssert(inner, _)
        | Expr::Throw(inner, _) => walk_expr(inner, f),
        Expr::Call(c) => {
            walk_expr(&c.callee, f);
            for a in &c.args {
                walk_expr(a, f);
            }
        }
        Expr::Binary(b) => {
            walk_expr(&b.left, f);
            walk_expr(&b.right, f);
        }
        Expr::Unary(u) => walk_expr(&u.operand, f),
        Expr::Range(r) => {
            walk_expr(&r.start, f);
            if let Some(step) = &r.step {
                walk_expr(step, f);
            }
            walk_expr(&r.end, f);
        }
        Expr::Cast(c) => walk_expr(&c.value, f),
        Expr::SizeOf(sz) => walk_expr(&sz.operand, f),
        Expr::NewArray(n) => {
            walk_expr(&n.size, f);
            for inner in &n.inner_sizes {
                walk_expr(inner, f);
            }
        }
        Expr::NewArrayLit(n) => {
            for el in &n.elements {
                walk_expr(el, f);
            }
        }
        Expr::Index(i) => {
            walk_expr(&i.array, f);
            walk_expr(&i.index, f);
        }
        Expr::Field(fe) => walk_expr(&fe.object, f),
        Expr::InterpString(sx) => {
            for seg in &sx.segments {
                if let crate::InterpSegment::Expr(inner) = seg {
                    walk_expr(inner, f);
                }
            }
        }
        Expr::TypeTest(t) => walk_expr(&t.value, f),
        Expr::NewObject(n) => {
            for a in &n.args {
                walk_expr(a, f);
            }
            if let Some(body) = &n.anonymous_body {
                for b in &body.init_blocks {
                    walk_block(b, f);
                }
                for m in &body.methods {
                    if let Some(b) = &m.body {
                        walk_block(b, f);
                    }
                }
            }
        }
        Expr::Switch(sw) => {
            walk_expr(&sw.scrutinee, f);
            for arm in &sw.arms {
                if let Some(guard) = &arm.guard {
                    walk_expr(guard, f);
                }
                match &arm.body {
                    crate::SwitchBody::Expr(inner) => walk_expr(inner, f),
                    crate::SwitchBody::Block(b) => walk_block(b, f),
                }
            }
        }
        Expr::Lambda(l) => match &l.body {
            crate::LambdaBody::Expr(inner) => walk_expr(inner, f),
            crate::LambdaBody::Block(b) => walk_block(b, f),
        },
        Expr::Elvis(el) => {
            walk_expr(&el.value, f);
            walk_expr(&el.fallback, f);
        }
        Expr::Ternary(t) => {
            walk_expr(&t.condition, f);
            walk_expr(&t.then_branch, f);
            walk_expr(&t.else_branch, f);
        }
        Expr::TupleLit(items, _) => {
            for item in items {
                walk_expr(item, f);
            }
        }
        Expr::TryExpr(t) => walk_try(t, f),
        Expr::IncDec(i) => walk_expr(&i.target, f),
    }
}

fn walk_try(t: &crate::TryStmt, f: &mut dyn FnMut(Node<'_>)) {
    walk_block(&t.body, f);
    for c in &t.catches {
        walk_block(&c.body, f);
    }
    if let Some(fin) = &t.finally {
        walk_block(fin, f);
    }
}

fn walk_stmt(st: &Stmt, f: &mut dyn FnMut(Node<'_>)) {
    f(Node::Stmt(st));
    match st {
        Stmt::Expr(e) | Stmt::Throw(e, _) | Stmt::Yield(e, _) => walk_expr(e, f),
        Stmt::Return(value, _) => {
            if let Some(e) = value {
                walk_expr(e, f);
            }
        }
        Stmt::VarDecl(v) => {
            if let Some(init) = &v.init {
                walk_expr(init, f);
            }
        }
        Stmt::If(i) => {
            walk_expr(&i.condition, f);
            walk_block(&i.then_block, f);
            match i.else_branch.as_deref() {
                Some(ElseBranch::Block(b)) => walk_block(b, f),
                Some(ElseBranch::If(inner)) => walk_stmt(&Stmt::If(inner.clone()), f),
                None => {}
            }
        }
        Stmt::While(w) => {
            walk_expr(&w.condition, f);
            walk_block(&w.body, f);
        }
        Stmt::DoWhile(d) => {
            walk_block(&d.body, f);
            walk_expr(&d.condition, f);
        }
        Stmt::ForEach(fe) => {
            walk_expr(&fe.iter, f);
            walk_block(&fe.body, f);
        }
        Stmt::Assign(a) => {
            walk_expr(&a.target, f);
            walk_expr(&a.value, f);
        }
        Stmt::Break(..) | Stmt::Continue(..) => {}
        Stmt::Labeled { stmt, .. } => walk_stmt(stmt, f),
        Stmt::SuperCall(args, _) => {
            for a in args {
                walk_expr(a, f);
            }
        }
        Stmt::Try(t) => walk_try(t, f),
        Stmt::Unsafe(b) | Stmt::Block(b) => walk_block(b, f),
        Stmt::IfCfg(c) => {
            walk_block(&c.then_block, f);
            if let Some(b) = &c.else_block {
                walk_block(b, f);
            }
        }
        Stmt::ForC(fc) => {
            if let Some(init) = &fc.init {
                walk_stmt(init, f);
            }
            if let Some(cond) = &fc.cond {
                walk_expr(cond, f);
            }
            if let Some(update) = &fc.update {
                walk_stmt(update, f);
            }
            walk_block(&fc.body, f);
        }
    }
}
