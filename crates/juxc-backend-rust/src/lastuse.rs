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
    let w = walk(body);
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

/// For each lambda in one function body (keyed by the lambda's span), the
/// outer bindings it captures that are **read again** after the capture, each
/// with the span of one read inside the lambda, so the caller can look its
/// type up.
///
/// Every Jux lambda lowers to a `move` closure, so a capture is a by-value use
/// of the binding. It is the final one only when all three hold:
///
/// - nothing reads the binding after the lambda, in source order;
/// - no loop around the lambda also encloses the binding's declaration (the
///   next iteration builds the closure again);
/// - the lambda is not inside another lambda (the outer closure may run many
///   times, and a `Fn` closure cannot give its own capture away).
///
/// A captured binding is an enclosing-body declaration or one of `params`;
/// names the lambda declares itself, its parameters included, are not
/// captures. Fields and functions read by bare name are neither, and are left
/// out.
pub(crate) fn captures_read_again(
    body: &Block,
    params: &HashSet<String>,
) -> HashMap<Span, Vec<(String, Span)>> {
    let w = walk(body);
    let mut out = HashMap::new();
    for site in &w.lambdas {
        let mut shared: Vec<(String, Span)> = Vec::new();
        for u in &w.uses[site.first..site.end] {
            let name = u.name.as_str();
            let binding = w.decl_depth.contains_key(name) || params.contains(name);
            if !binding
                || site.declares.contains(name)
                || shared.iter().any(|(n, _)| n == name)
            {
                continue;
            }
            let in_loop = site.depth > w.decl_depth.get(name).copied().unwrap_or(0);
            let read_later = w.uses[site.end..].iter().any(|later| later.name == name);
            if site.nested || in_loop || read_later {
                shared.push((u.name.clone(), u.span));
            }
        }
        if !shared.is_empty() {
            out.insert(site.span, shared);
        }
    }
    out
}

fn walk(body: &Block) -> Walker {
    let mut w = Walker::default();
    w.block(body);
    w
}

/// One lambda the walk passed through.
struct LambdaSite {
    span: Span,
    /// Loop nesting where the lambda is written.
    depth: u32,
    /// Written inside another lambda's body.
    nested: bool,
    /// The lambda's reads are `uses[first..end]`.
    first: usize,
    end: usize,
    /// Every name the lambda declares, its parameters included.
    declares: HashSet<String>,
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
    lambdas: Vec<LambdaSite>,
    /// The names declared by each lambda being walked, innermost last.
    lambda_declares: Vec<HashSet<String>>,
}

impl Walker {
    fn declare(&mut self, name: &str) {
        self.decl_depth.insert(name.to_string(), self.depth);
        // A name declared in a nested lambda is declared inside every lambda
        // around it too.
        for frame in &mut self.lambda_declares {
            frame.insert(name.to_string());
        }
    }

    fn block(&mut self, b: &Block) {
        for s in &b.statements {
            self.stmt(s);
        }
    }

    /// A string concat or interpolation lowers to ONE `format!`, which
    /// borrows every argument until the whole string is built. In
    /// `w + " -> " + size(w)` the textually last read of `w` is an argument
    /// to `size`, and moving it there while the macro still borrows the first
    /// `w` is rustc's E0505. So a name read more than once among
    /// `uses[first..]` keeps every one of those reads, the last included, from
    /// moving: the call gets a copy.
    fn share_within_format(&mut self, first: usize) {
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for u in &self.uses[first..] {
            *seen.entry(u.name.as_str()).or_insert(0) += 1;
        }
        let repeated: HashSet<String> =
            seen.into_iter().filter(|(_, n)| *n > 1).map(|(name, _)| name.to_string()).collect();
        for u in &mut self.uses[first..] {
            if repeated.contains(&u.name) {
                u.repeats = true;
            }
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
                let first = self.uses.len();
                self.expr(&b.left);
                self.expr(&b.right);
                if b.op == juxc_ast::BinaryOp::Add {
                    self.share_within_format(first);
                }
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
                let first = self.uses.len();
                for seg in &s.segments {
                    if let InterpSegment::Expr(inner) = seg {
                        self.expr(inner);
                    }
                }
                self.share_within_format(first);
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
            // `f(text)?` reads `text` like `f(text)` does; unwalked, the
            // read was invisible and an earlier one moved the value.
            Expr::Await(inner, _) | Expr::NotNullAssert(inner, _) | Expr::ErrorProp(inner, _) => {
                self.expr(inner)
            }
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
                let nested = self.in_lambda;
                let first = self.uses.len();
                let prev = std::mem::replace(&mut self.in_lambda, true);
                self.lambda_declares.push(HashSet::new());
                for p in &l.params {
                    self.declare(&p.name.text);
                }
                match &l.body {
                    LambdaBody::Expr(b) => self.expr(b),
                    LambdaBody::Block(blk) => self.block(blk),
                }
                self.in_lambda = prev;
                let declares = self.lambda_declares.pop().unwrap_or_default();
                self.lambdas.push(LambdaSite {
                    span: l.span,
                    depth: self.depth,
                    nested,
                    first,
                    end: self.uses.len(),
                    declares,
                });
            }
            _ => {}
        }
    }
}
