//! Which names a block assigns to.
//!
//! Null-test narrowing (JUX-LANG-V1 §7.10) makes a `T?` binding read as plain
//! `T` where the flow has proved it non-null, and an assignment to the binding
//! drops that. The type checker and the backend both have to decide which
//! bindings that covers, and they have to agree exactly -- a disagreement
//! there is a rustc error on a program Jux accepted. So they ask this one
//! function rather than each carrying its own idea of "assigned".
//!
//! The walk is deliberately narrow: assignment TARGETS, and nothing else. A
//! mutating method call (`xs.push(1)`) does not rebind `xs` and so cannot
//! reintroduce null.

use std::collections::HashSet;

use juxc_ast::{Block, ElseBranch, Expr, Stmt};

/// Every name REBOUND anywhere in `block`, including in blocks nested inside
/// it.
///
/// Only a whole-binding assignment counts. `a.f = v` and `a[i] = v` write
/// through `a` without rebinding it, so neither can put a null back into `a`
/// and neither belongs here -- counting them would refuse to narrow the
/// commonest shape there is, a guard clause followed by writes through the
/// thing it guarded.
pub fn names_assigned_in(block: &Block) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_block(block, &mut out);
    out
}

fn walk_block(block: &Block, out: &mut HashSet<String>) {
    for stmt in &block.statements {
        walk_stmt(stmt, out);
    }
}

fn walk_stmt(stmt: &Stmt, out: &mut HashSet<String>) {
    match stmt {
        Stmt::Assign(a) => {
            if let Some(name) = place_base(&a.target) {
                out.insert(name);
            }
        }
        Stmt::If(i) => {
            walk_block(&i.then_block, out);
            match i.else_branch.as_deref() {
                Some(ElseBranch::Block(b)) => walk_block(b, out),
                Some(ElseBranch::If(inner)) => walk_stmt(&Stmt::If(inner.clone()), out),
                None => {}
            }
        }
        Stmt::While(w) => walk_block(&w.body, out),
        Stmt::DoWhile(d) => walk_block(&d.body, out),
        Stmt::ForEach(f) => walk_block(&f.body, out),
        Stmt::ForC(f) => {
            if let Some(init) = f.init.as_deref() {
                walk_stmt(init, out);
            }
            if let Some(upd) = f.update.as_deref() {
                walk_stmt(upd, out);
            }
            walk_block(&f.body, out);
        }
        Stmt::Block(b) | Stmt::Unsafe(b) => walk_block(b, out),
        Stmt::Labeled { stmt, .. } => walk_stmt(stmt, out),
        Stmt::Try(t) => {
            walk_block(&t.body, out);
            for c in &t.catches {
                walk_block(&c.body, out);
            }
            if let Some(fin) = &t.finally {
                walk_block(fin, out);
            }
        }
        _ => {}
    }
}

/// The name a place REBINDS: `x` for `x = v`, and nothing for `x.f = v` or
/// `x[i] = v`, which write through `x` and leave the binding itself alone.
fn place_base(e: &Expr) -> Option<String> {
    match e {
        Expr::Path(qn) if qn.segments.len() == 1 => Some(qn.segments[0].text.clone()),
        _ => None,
    }
}
