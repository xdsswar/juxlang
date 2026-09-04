//! Last-use analysis for local bindings.
//!
//! Jux keeps Java/C#-shaped value semantics: after `take(w)` the binding `w` is
//! still yours to read. Rust does not — passing a non-`Copy` value by value
//! MOVES it, and the next read is a rustc `E0382`. Something has to bridge that,
//! and the choice shows up in both the correctness and the speed of the emitted
//! program:
//!
//! - clone at *every* by-value argument: always correct, but it turns each
//!   `String` argument into a heap allocation the source never asked for;
//! - **move on the last use, clone only where the binding is read again** —
//!   what a Rust programmer writes by hand.
//!
//! This pass computes the second. For one function body it returns the spans of
//! local reads that are *not* that local's final read; the emitter clones at
//! exactly those and moves everywhere else.
//!
//! **Which way to be wrong.** Marking a read that could have moved costs a
//! clone and still compiles; missing one is a rustc error, never a silently
//! wrong program. So the two doubtful shapes are both marked non-final:
//!
//! - a read inside a **loop** that encloses the declaration — the textually
//!   last read is re-executed on the next iteration;
//! - a read inside a **lambda body** — the closure may run any number of times,
//!   at any point after the source position where it appears.

use std::collections::{HashMap, HashSet};

use juxc_ast::{Block, ElseBranch, Expr, InterpSegment, LambdaBody, Stmt};
use juxc_source::Span;

/// The spans of local reads that are **not** the final read of that local.
///
/// Keyed by span, so the emitter can ask about the exact `Path` expression it
/// is about to emit. A name read once, outside any loop or lambda, has no entry
/// — that read owns the value and may move it.
pub(crate) fn non_final_local_uses(body: &Block) -> HashSet<Span> {
    let mut w = Walker::default();
    w.block(body);
    // The walk visits in source order, so a name's final read is the last one
    // recorded for it.
    let mut final_use: HashMap<&str, Span> = HashMap::new();
    for u in &w.uses {
        final_use.insert(u.name.as_str(), u.span);
    }
    w.uses
        .iter()
        .filter(|u| {
            u.repeats
                || u.depth > w.decl_depth.get(u.name.as_str()).copied().unwrap_or(0)
                || final_use.get(u.name.as_str()) != Some(&u.span)
        })
        .map(|u| u.span)
        .collect()
}

/// One recorded read of a single-segment name.
struct Use {
    name: String,
    span: Span,
    /// Loop nesting at the read.
    depth: u32,
    /// Inside a lambda body — runs at an unknown time, any number of times.
    repeats: bool,
}

#[derive(Default)]
struct Walker {
    uses: Vec<Use>,
    /// Loop nesting at each name's declaration, so a read can tell whether the
    /// loop around it also encloses the declaration.
    decl_depth: HashMap<String, u32>,
    depth: u32,
    in_lambda: bool,
}

impl Walker {
    fn declare(&mut self, name: &str) {
        self.decl_depth.insert(name.to_string(), self.depth);
    }

    fn block(&mut self, b: &Block) {
        for s in &b.statements {
            self.stmt(s);
        }
    }

    fn loop_body(&mut self, b: &Block) {
        self.depth += 1;
        self.block(b);
        self.depth -= 1;
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(e) => self.expr(e),
            Stmt::Return(Some(e), _) => self.expr(e),
            Stmt::VarDecl(v) => {
                // The initializer runs BEFORE the name exists, so a read of the
                // same spelling there belongs to an outer binding.
                if let Some(init) = &v.init {
                    self.expr(init);
                }
                self.declare(&v.name.text);
            }
            Stmt::Assign(a) => {
                self.expr(&a.value);
                self.expr(&a.target);
            }
            Stmt::If(i) => {
                self.expr(&i.condition);
                self.block(&i.then_block);
                match i.else_branch.as_deref() {
                    Some(ElseBranch::Block(b)) => self.block(b),
                    Some(ElseBranch::If(inner)) => self.stmt(&Stmt::If(inner.clone())),
                    None => {}
                }
            }
            Stmt::While(wl) => {
                self.expr(&wl.condition);
                self.loop_body(&wl.body);
            }
            Stmt::DoWhile(d) => {
                self.loop_body(&d.body);
                self.expr(&d.condition);
            }
            Stmt::ForC(f) => {
                if let Some(init) = &f.init {
                    self.stmt(init);
                }
                self.depth += 1;
                if let Some(c) = &f.cond {
                    self.expr(c);
                }
                if let Some(u) = &f.update {
                    self.stmt(u);
                }
                self.block(&f.body);
                self.depth -= 1;
            }
            Stmt::ForEach(f) => {
                self.expr(&f.iter);
                self.depth += 1;
                self.declare(&f.var_name.text);
                self.block(&f.body);
                self.depth -= 1;
            }
            Stmt::Try(t) => {
                self.block(&t.body);
                for c in &t.catches {
                    self.declare(&c.name.text);
                    self.block(&c.body);
                }
                if let Some(f) = &t.finally {
                    self.block(f);
                }
            }
            Stmt::Labeled { stmt, .. } => self.stmt(stmt),
            Stmt::SuperCall(args, _) => {
                for a in args {
                    self.expr(a);
                }
            }
            Stmt::Throw(e, _) => self.expr(e),
            Stmt::Unsafe(b) => self.block(b),
            _ => {}
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Path(qn) => {
                if qn.segments.len() == 1 {
                    self.uses.push(Use {
                        name: qn.segments[0].text.clone(),
                        span: qn.span,
                        depth: self.depth,
                        repeats: self.in_lambda,
                    });
                }
            }
            Expr::Call(c) => {
                self.expr(&c.callee);
                for a in &c.args {
                    self.expr(a);
                }
            }
            Expr::NewObject(n) => {
                for a in &n.args {
                    self.expr(a);
                }
            }
            Expr::NewArrayLit(n) => {
                for el in &n.elements {
                    self.expr(el);
                }
            }
            Expr::NewArray(n) => {
                self.expr(&n.size);
                for inner in &n.inner_sizes {
                    self.expr(inner);
                }
            }
            Expr::Binary(b) => {
                self.expr(&b.left);
                self.expr(&b.right);
            }
            Expr::Unary(u) => self.expr(&u.operand),
            Expr::Range(r) => {
                self.expr(&r.start);
                self.expr(&r.end);
            }
            Expr::Cast(c) => self.expr(&c.value),
            Expr::TypeTest(t) => self.expr(&t.value),
            Expr::Index(i) => {
                self.expr(&i.array);
                self.expr(&i.index);
            }
            Expr::Field(f) => self.expr(&f.object),
            Expr::InterpString(s) => {
                for seg in &s.segments {
                    if let InterpSegment::Expr(inner) = seg {
                        self.expr(inner);
                    }
                }
            }
            Expr::Elvis(el) => {
                self.expr(&el.value);
                self.expr(&el.fallback);
            }
            Expr::Ternary(t) => {
                self.expr(&t.condition);
                self.expr(&t.then_branch);
                self.expr(&t.else_branch);
            }
            Expr::Await(inner, _) | Expr::NotNullAssert(inner, _) => self.expr(inner),
            Expr::Switch(sw) => {
                self.expr(&sw.scrutinee);
                for arm in &sw.arms {
                    if let Some(g) = &arm.guard {
                        self.expr(g);
                    }
                    match &arm.body {
                        juxc_ast::SwitchBody::Expr(e) => self.expr(e),
                        juxc_ast::SwitchBody::Block(b) => self.block(b),
                    }
                }
            }
            Expr::Lambda(l) => {
                let prev = std::mem::replace(&mut self.in_lambda, true);
                for p in &l.params {
                    self.declare(&p.name.text);
                }
                match &l.body {
                    LambdaBody::Expr(b) => self.expr(b),
                    LambdaBody::Block(blk) => self.block(blk),
                }
                self.in_lambda = prev;
            }
            _ => {}
        }
    }
}
