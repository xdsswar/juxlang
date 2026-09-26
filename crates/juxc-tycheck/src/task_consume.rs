//! **E0707 (§18.1.4)**: a task is used after something consumed it.
//!
//! A task yields its result once. Awaiting it, calling `blockingGet()` on it,
//! or handing it to `Task.all` / `any` / `race` / `allSettled` takes it, and
//! the emitted `JuxTask` is moved there; a later use of the same local was a
//! rustc `E0382` ("use of moved value") about a crate the programmer never
//! wrote. This walk finds those uses first.
//!
//! It runs after a body has been checked, over the same AST, and reads each
//! name's type out of the checker's per-span type map, so only a local whose
//! type is `Task<T>` is tracked. The walk is flow-ordered the way rustc's move
//! check is: a task consumed on any branch is consumed after the branch, a
//! reassignment (`t = spawn(...)`) re-arms the local, and a declaration of the
//! same name shadows it. A consumption inside a lambda consumes the local at
//! the lambda, because the closure has to own the task to await it.
//!
//! What it does not do is follow a loop's back edge: a task consumed inside a
//! loop body is not reported against its own next turn. rustc still catches
//! that case, so the walk errs towards saying nothing.

use std::collections::HashMap;

use juxc_ast::{Block, ElseBranch, Expr, LambdaBody, Stmt, SwitchBody};
use juxc_diagnostics::{code, Diagnostic};
use juxc_source::Span;

use crate::ty::Ty;

/// The task statics that take their task arguments (§18.1.4).
const CONSUMING_STATICS: &[&str] = &["all", "any", "race", "allSettled"];

/// Report every use of a task local after it was consumed, in `body`.
pub(crate) fn check_body(body: &Block, types: &HashMap<Span, Ty>, diagnostics: &mut Vec<Diagnostic>) {
    let mut walk = Walk { types, consumed: HashMap::new(), diagnostics };
    walk.block(body);
}

struct Walk<'a> {
    types: &'a HashMap<Span, Ty>,
    /// Task locals already consumed on some path to here, with where.
    consumed: HashMap<String, Span>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl Walk<'_> {
    /// The local a bare path names, when the checker typed it as a task.
    fn task_local<'e>(&self, e: &'e Expr) -> Option<&'e str> {
        let Expr::Path(qn) = e else { return None };
        if qn.segments.len() != 1 {
            return None;
        }
        match self.types.get(&qn.span) {
            Some(Ty::User { name, .. }) if name == juxc_ast::TASK_SENTINEL => Some(qn.segments[0].text.as_str()),
            _ => None,
        }
    }

    /// Walk `e` as a use, then mark the task it names consumed at `at`.
    fn consume(&mut self, e: &Expr, at: Span) {
        self.expr(e);
        if let Some(name) = self.task_local(e) {
            self.consumed.insert(name.to_string(), at);
        }
    }

    /// Run `f` from the current state and union what it consumed back in:
    /// what a branch might have consumed is consumed after it.
    fn branch(&mut self, f: impl FnOnce(&mut Self)) -> HashMap<String, Span> {
        let before = self.consumed.clone();
        f(self);
        std::mem::replace(&mut self.consumed, before)
    }

    fn merge(&mut self, other: HashMap<String, Span>) {
        for (name, at) in other {
            self.consumed.entry(name).or_insert(at);
        }
    }

    fn block(&mut self, b: &Block) {
        // A name declared in the block shadows the outer one for the rest of
        // the block only; afterwards the outer local's state is back.
        let before = self.consumed.clone();
        let mut declared: Vec<&str> = Vec::new();
        for stmt in &b.statements {
            if let Stmt::VarDecl(v) = stmt {
                declared.push(&v.name.text);
            }
            self.stmt(stmt);
        }
        for name in declared {
            match before.get(name) {
                Some(at) => self.consumed.insert(name.to_string(), *at),
                None => self.consumed.remove(name),
            };
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(e) | Stmt::Yield(e, _) | Stmt::Throw(e, _) => self.expr(e),
            Stmt::Return(Some(e), _) => self.expr(e),
            Stmt::VarDecl(v) => {
                if let Some(init) = &v.init {
                    self.expr(init);
                }
                self.consumed.remove(&v.name.text);
            }
            Stmt::Assign(a) => {
                self.expr(&a.value);
                match &a.target {
                    Expr::Path(qn) if qn.segments.len() == 1 && a.op.is_none() => {
                        self.consumed.remove(&qn.segments[0].text);
                    }
                    target => self.expr(target),
                }
            }
            Stmt::If(i) => self.if_stmt(i),
            Stmt::While(w) => {
                self.expr(&w.condition);
                let body = self.branch(|me| me.block(&w.body));
                self.merge(body);
            }
            Stmt::DoWhile(d) => {
                self.block(&d.body);
                self.expr(&d.condition);
            }
            Stmt::ForEach(f) => {
                self.expr(&f.iter);
                let name = f.var_name.text.clone();
                let outer = self.consumed.remove(&name);
                let mut body = self.branch(|me| me.block(&f.body));
                body.remove(&name);
                if let Some(at) = outer {
                    self.consumed.insert(name, at);
                }
                self.merge(body);
            }
            Stmt::ForC(f) => {
                let loop_state = self.branch(|me| {
                    if let Some(init) = &f.init {
                        me.stmt(init);
                    }
                    if let Some(cond) = &f.cond {
                        me.expr(cond);
                    }
                    me.block(&f.body);
                    if let Some(update) = &f.update {
                        me.stmt(update);
                    }
                });
                self.merge(loop_state);
            }
            Stmt::Try(t) => self.try_stmt(t),
            Stmt::Block(b) | Stmt::Unsafe(b) => self.block(b),
            Stmt::Labeled { stmt, .. } => self.stmt(stmt),
            Stmt::SuperCall(args, _) => {
                for a in args {
                    self.expr(a);
                }
            }
            _ => {}
        }
    }

    fn if_stmt(&mut self, i: &juxc_ast::IfStmt) {
        self.expr(&i.condition);
        let then = self.branch(|me| me.block(&i.then_block));
        let other = self.branch(|me| match i.else_branch.as_deref() {
            Some(ElseBranch::If(inner)) => me.if_stmt(inner),
            Some(ElseBranch::Block(b)) => me.block(b),
            None => {}
        });
        self.merge(then);
        self.merge(other);
    }

    fn try_stmt(&mut self, t: &juxc_ast::TryStmt) {
        let body = self.branch(|me| me.block(&t.body));
        let mut after = body.clone();
        for c in &t.catches {
            // A handler can run after any part of the body did.
            let saved = std::mem::replace(&mut self.consumed, body.clone());
            let outer = self.consumed.remove(&c.name.text);
            self.block(&c.body);
            if let Some(at) = outer {
                self.consumed.entry(c.name.text.clone()).or_insert(at);
            }
            let handled = std::mem::replace(&mut self.consumed, saved);
            for (name, at) in handled {
                after.entry(name).or_insert(at);
            }
        }
        self.merge(after);
        if let Some(fin) = &t.finally {
            self.block(fin);
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Path(qn) => {
                if let Some(name) = self.task_local(e) {
                    if let Some(at) = self.consumed.remove(name) {
                        self.report(name, qn.span, at);
                    }
                }
            }
            Expr::Await(inner, span) => self.consume(inner, *span),
            Expr::Call(c) => self.call(c),
            Expr::Lambda(l) => {
                // Uses inside the body are uses; what the body consumes, the
                // closure had to own, so it is consumed at the lambda.
                let shadowed: Vec<(String, Option<Span>)> =
                    l.params.iter().map(|p| (p.name.text.clone(), self.consumed.remove(&p.name.text))).collect();
                let before: Vec<String> = self.consumed.keys().cloned().collect();
                match &l.body {
                    LambdaBody::Expr(body) => self.expr(body),
                    LambdaBody::Block(body) => self.block(body),
                }
                for (name, at) in shadowed {
                    self.consumed.remove(&name);
                    if let Some(at) = at {
                        self.consumed.insert(name, at);
                    }
                }
                for at in self.consumed.iter_mut().filter(|(n, _)| !before.contains(n)).map(|(_, at)| at) {
                    *at = l.span;
                }
            }
            Expr::Binary(b) => {
                self.expr(&b.left);
                self.expr(&b.right);
            }
            Expr::Unary(u) => self.expr(&u.operand),
            Expr::Field(f) => self.expr(&f.object),
            Expr::Index(i) => {
                self.expr(&i.array);
                self.expr(&i.index);
            }
            Expr::Cast(c) => self.expr(&c.value),
            Expr::TypeTest(t) => self.expr(&t.value),
            Expr::Ternary(t) => {
                self.expr(&t.condition);
                let a = self.branch(|me| me.expr(&t.then_branch));
                let b = self.branch(|me| me.expr(&t.else_branch));
                self.merge(a);
                self.merge(b);
            }
            Expr::Elvis(el) => {
                self.expr(&el.value);
                let fallback = self.branch(|me| me.expr(&el.fallback));
                self.merge(fallback);
            }
            Expr::InterpString(s) => {
                for seg in &s.segments {
                    if let juxc_ast::InterpSegment::Expr(inner) = seg {
                        self.expr(inner);
                    }
                }
            }
            Expr::NewObject(n) => {
                for a in &n.args {
                    self.expr(a);
                }
            }
            Expr::TupleLit(items, _) => {
                for i in items {
                    self.expr(i);
                }
            }
            Expr::NewArrayLit(n) => {
                for i in &n.elements {
                    self.expr(i);
                }
            }
            Expr::Switch(sw) => {
                self.expr(&sw.scrutinee);
                let mut after = HashMap::new();
                for arm in &sw.arms {
                    let taken = self.branch(|me| {
                        if let Some(g) = &arm.guard {
                            me.expr(g);
                        }
                        match &arm.body {
                            SwitchBody::Expr(e) => me.expr(e),
                            SwitchBody::Block(b) => me.block(b),
                        }
                    });
                    for (name, at) in taken {
                        after.entry(name).or_insert(at);
                    }
                }
                self.merge(after);
            }
            Expr::Throw(inner, _)
            | Expr::ErrorProp(inner, _)
            | Expr::NotNullAssert(inner, _)
            | Expr::Out(inner, _) => self.expr(inner),
            Expr::IncDec(i) => self.expr(&i.target),
            Expr::Range(r) => {
                self.expr(&r.start);
                self.expr(&r.end);
            }
            Expr::TryExpr(t) => self.try_stmt(t),
            _ => {}
        }
    }

    fn call(&mut self, c: &juxc_ast::CallExpr) {
        if let Expr::Field(f) = c.callee.as_ref() {
            let method = f.field.text.as_str();
            // `t.blockingGet()` takes the task.
            if method == "blockingGet" && c.args.is_empty() && self.task_local(&f.object).is_some() {
                self.consume(&f.object, c.span);
                return;
            }
            // `Task.all(a, b)` and its siblings take every task argument.
            if let Expr::Path(qn) = f.object.as_ref() {
                if qn.segments.len() == 1 && qn.segments[0].text == "Task" && CONSUMING_STATICS.contains(&method) {
                    for a in &c.args {
                        self.consume(a, c.span);
                    }
                    return;
                }
            }
        }
        self.expr(&c.callee);
        for a in &c.args {
            self.expr(a);
        }
    }

    fn report(&mut self, name: &str, use_span: Span, consumed_at: Span) {
        self.diagnostics.push(
            Diagnostic::error(
                code::Code::E0707_TaskAlreadyConsumed,
                format!(
                    "task `{name}` was already consumed: awaiting a task, `blockingGet()`, or passing \
                     it to `Task.all`/`any`/`race`/`allSettled` takes its result, and a task has \
                     only one (§18.1.4)",
                ),
            )
            .with_span(use_span)
            .with_label(consumed_at, format!("`{name}` consumed here"))
            .with_help(format!("keep the result the first use gives, or spawn `{name}` again")),
        );
    }
}
