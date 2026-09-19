//! A lambda that becomes a single-method interface's anonymous implementation
//! (LANG-V1 §7.9.1) still reads the object around it the way a lambda does
//! (§7.9): a bare field, a bare method call or `this` inside it means the
//! ENCLOSING object's, captured by reference.
//!
//! An anonymous implementation is a struct of its own, so inside its method
//! `self` is that struct, not the enclosing object. This module rewrites the
//! body before it is emitted: every read of the enclosing object goes through
//! one captured handle, `__jux_outer`,
//!
//! ```text
//! seen.push(s)   ->  __jux_outer.seen.push(s)
//! total()        ->  __jux_outer.total()
//! this           ->  __jux_outer
//! ```
//!
//! and the anonymous-class capture machinery then stores `__jux_outer` as a
//! field of the struct like any other captured local. A name the lambda
//! declares itself (a parameter, a local, a loop variable) shadows the
//! enclosing member of the same name and is left alone.

use std::collections::HashSet;

use juxc_ast::{Block, ElseBranch, Expr, Ident, InterpSegment, QualifiedName, Stmt};

/// The captured handle's name. The `__jux_` prefix keeps it out of every
/// user namespace.
pub(crate) const OUTER: &str = "__jux_outer";

/// What a lambda body may read of the enclosing object.
pub(crate) struct OuterMembers<'a> {
    /// Instance fields and properties of the enclosing class (and its
    /// ancestors).
    pub fields: &'a HashSet<String>,
    /// Instance methods of the enclosing class (and its ancestors).
    pub methods: &'a HashSet<String>,
}

/// Rewrite `block` so every read of the enclosing object goes through
/// [`OUTER`]. `shadow` holds the names the lambda binds itself. Returns
/// whether anything was rewritten, so a lambda that reads nothing of the
/// object keeps its capture-less form.
pub(crate) fn rewrite_block(block: &mut Block, members: &OuterMembers<'_>, shadow: &HashSet<String>) -> bool {
    let mut shadow = shadow.clone();
    collect_declared_names(block, &mut shadow);
    let mut changed = false;
    for s in &mut block.statements {
        changed |= rewrite_stmt(s, members, &shadow);
    }
    changed
}

/// Every name a block declares at any depth: locals, loop variables and catch
/// binders. Collected up front, so a name is shadowed for the whole body --
/// conservative (a use BEFORE the declaration is left alone too), never wrong
/// in the other direction.
fn collect_declared_names(block: &Block, out: &mut HashSet<String>) {
    for s in &block.statements {
        collect_declared_in_stmt(s, out);
    }
}

fn collect_declared_in_stmt(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::VarDecl(v) => {
            out.insert(v.name.text.clone());
        }
        Stmt::If(i) => collect_declared_in_if(i, out),
        Stmt::While(w) => collect_declared_names(&w.body, out),
        Stmt::DoWhile(d) => collect_declared_names(&d.body, out),
        Stmt::ForEach(f) => {
            out.insert(f.var_name.text.clone());
            collect_declared_names(&f.body, out);
        }
        Stmt::ForC(f) => {
            if let Some(init) = &f.init {
                collect_declared_in_stmt(init, out);
            }
            collect_declared_names(&f.body, out);
        }
        Stmt::Block(b) | Stmt::Unsafe(b) => collect_declared_names(b, out),
        Stmt::Try(t) => {
            collect_declared_names(&t.body, out);
            for c in &t.catches {
                out.insert(c.name.text.clone());
                collect_declared_names(&c.body, out);
            }
            if let Some(f) = &t.finally {
                collect_declared_names(f, out);
            }
        }
        Stmt::Labeled { stmt, .. } => collect_declared_in_stmt(stmt, out),
        _ => {}
    }
}

fn collect_declared_in_if(i: &juxc_ast::IfStmt, out: &mut HashSet<String>) {
    collect_declared_names(&i.then_block, out);
    if let Some(eb) = i.else_branch.as_deref() {
        match eb {
            ElseBranch::Block(b) => collect_declared_names(b, out),
            ElseBranch::If(elif) => collect_declared_in_if(elif, out),
        }
    }
}

/// The captured handle as an expression. It carries no span: the checker's
/// span-keyed types describe the ORIGINAL expression at that position (`seen`,
/// a `Vec`), so a reused span would make the handle look like that type. With
/// none, its type comes from the name, bound to the enclosing class.
fn outer_path(_at: juxc_source::Span) -> Expr {
    let span = juxc_source::Span::DUMMY;
    Expr::Path(QualifiedName { segments: vec![Ident { text: OUTER.to_string(), span }], span })
}

fn outer_field(name: &Ident, span: juxc_source::Span) -> Expr {
    Expr::Field(juxc_ast::FieldExpr {
        object: Box::new(outer_path(span)),
        field: name.clone(),
        safe: false,
        span,
    })
}

fn rewrite_stmt(s: &mut Stmt, m: &OuterMembers<'_>, shadow: &HashSet<String>) -> bool {
    let mut changed = false;
    let expr = |e: &mut Expr| rewrite_expr(e, m, shadow);
    match s {
        Stmt::Expr(e) | Stmt::Yield(e, _) | Stmt::Throw(e, _) => changed |= expr(e),
        Stmt::Return(Some(e), _) => changed |= expr(e),
        Stmt::VarDecl(v) => {
            if let Some(init) = &mut v.init {
                changed |= expr(init);
            }
        }
        Stmt::Assign(a) => {
            changed |= expr(&mut a.target);
            changed |= expr(&mut a.value);
        }
        Stmt::If(i) => changed |= rewrite_if(i, m, shadow),
        Stmt::While(w) => {
            changed |= expr(&mut w.condition);
            changed |= rewrite_block(&mut w.body, m, shadow);
        }
        Stmt::DoWhile(d) => {
            changed |= rewrite_block(&mut d.body, m, shadow);
            changed |= rewrite_expr(&mut d.condition, m, shadow);
        }
        Stmt::ForEach(f) => {
            changed |= expr(&mut f.iter);
            changed |= rewrite_block(&mut f.body, m, shadow);
        }
        Stmt::ForC(f) => {
            if let Some(init) = &mut f.init {
                changed |= rewrite_stmt(init, m, shadow);
            }
            if let Some(c) = &mut f.cond {
                changed |= rewrite_expr(c, m, shadow);
            }
            if let Some(u) = &mut f.update {
                changed |= rewrite_stmt(u, m, shadow);
            }
            changed |= rewrite_block(&mut f.body, m, shadow);
        }
        Stmt::Block(b) | Stmt::Unsafe(b) => changed |= rewrite_block(b, m, shadow),
        Stmt::Try(t) => {
            changed |= rewrite_block(&mut t.body, m, shadow);
            for c in &mut t.catches {
                changed |= rewrite_block(&mut c.body, m, shadow);
            }
            if let Some(f) = &mut t.finally {
                changed |= rewrite_block(f, m, shadow);
            }
        }
        Stmt::Labeled { stmt, .. } => changed |= rewrite_stmt(stmt, m, shadow),
        _ => {}
    }
    changed
}

fn rewrite_if(i: &mut juxc_ast::IfStmt, m: &OuterMembers<'_>, shadow: &HashSet<String>) -> bool {
    let mut changed = rewrite_expr(&mut i.condition, m, shadow);
    changed |= rewrite_block(&mut i.then_block, m, shadow);
    if let Some(eb) = i.else_branch.as_deref_mut() {
        changed |= match eb {
            ElseBranch::Block(b) => rewrite_block(b, m, shadow),
            ElseBranch::If(elif) => rewrite_if(elif, m, shadow),
        };
    }
    changed
}

fn rewrite_expr(e: &mut Expr, m: &OuterMembers<'_>, shadow: &HashSet<String>) -> bool {
    match e {
        Expr::This(span) => {
            *e = outer_path(*span);
            true
        }
        Expr::Path(qn) if qn.segments.len() == 1 => {
            let name = &qn.segments[0];
            if shadow.contains(&name.text) || !m.fields.contains(&name.text) {
                return false;
            }
            let replaced = outer_field(name, qn.span);
            *e = replaced;
            true
        }
        Expr::Call(c) => {
            let mut changed = false;
            let callee_is_outer_method = matches!(&*c.callee, Expr::Path(qn)
                if qn.segments.len() == 1
                    && !shadow.contains(&qn.segments[0].text)
                    && m.methods.contains(&qn.segments[0].text));
            if callee_is_outer_method {
                if let Expr::Path(qn) = &*c.callee {
                    let replaced = outer_field(&qn.segments[0], qn.span);
                    *c.callee = replaced;
                    changed = true;
                }
            } else {
                changed |= rewrite_expr(&mut c.callee, m, shadow);
            }
            for a in &mut c.args {
                changed |= rewrite_expr(a, m, shadow);
            }
            changed
        }
        Expr::Field(f) => rewrite_expr(&mut f.object, m, shadow),
        Expr::Binary(b) => rewrite_expr(&mut b.left, m, shadow) | rewrite_expr(&mut b.right, m, shadow),
        Expr::Unary(u) => rewrite_expr(&mut u.operand, m, shadow),
        Expr::Index(i) => rewrite_expr(&mut i.array, m, shadow) | rewrite_expr(&mut i.index, m, shadow),
        Expr::Ternary(t) => {
            rewrite_expr(&mut t.condition, m, shadow)
                | rewrite_expr(&mut t.then_branch, m, shadow)
                | rewrite_expr(&mut t.else_branch, m, shadow)
        }
        Expr::Cast(c) => rewrite_expr(&mut c.value, m, shadow),
        Expr::TypeTest(t) => rewrite_expr(&mut t.value, m, shadow),
        Expr::Elvis(el) => rewrite_expr(&mut el.value, m, shadow) | rewrite_expr(&mut el.fallback, m, shadow),
        Expr::Range(r) => rewrite_expr(&mut r.start, m, shadow) | rewrite_expr(&mut r.end, m, shadow),
        Expr::Await(inner, _) | Expr::NotNullAssert(inner, _) | Expr::ErrorProp(inner, _) | Expr::Throw(inner, _) => {
            rewrite_expr(inner, m, shadow)
        }
        Expr::NewObject(n) if n.anonymous_body.is_none() => {
            let mut changed = false;
            for a in &mut n.args {
                changed |= rewrite_expr(a, m, shadow);
            }
            changed
        }
        Expr::NewArrayLit(n) => {
            let mut changed = false;
            for el in &mut n.elements {
                changed |= rewrite_expr(el, m, shadow);
            }
            changed
        }
        Expr::TupleLit(items, _) => {
            let mut changed = false;
            for it in items {
                changed |= rewrite_expr(it, m, shadow);
            }
            changed
        }
        Expr::InterpString(s) => {
            let mut changed = false;
            for seg in &mut s.segments {
                if let InterpSegment::Expr(inner) = seg {
                    changed |= rewrite_expr(inner, m, shadow);
                }
            }
            changed
        }
        // A nested lambda reads the same object; its own parameters shadow.
        Expr::Lambda(l) => {
            let mut inner_shadow = shadow.clone();
            inner_shadow.extend(l.params.iter().map(|p| p.name.text.clone()));
            match &mut l.body {
                juxc_ast::LambdaBody::Expr(b) => rewrite_expr(b, m, &inner_shadow),
                juxc_ast::LambdaBody::Block(b) => rewrite_block(b, m, &inner_shadow),
            }
        }
        _ => false,
    }
}
