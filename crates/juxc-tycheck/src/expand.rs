//! Call-sugar expansion — rewrite **named arguments** and **omitted
//! default-valued parameters** into plain positional calls.
//!
//! The checker ([`crate::check::Checker`]) records an expansion plan
//! (a [`crate::ArgSource`] per parameter slot) for every call that
//! used sugar, keyed by the call's span. This module applies those
//! plans to the AST in place: it walks every statement and expression
//! of every declaration, and at each `Expr::Call` / `Expr::NewObject`
//! whose span has a plan it re-orders the explicit arguments into
//! their parameter slots and splices in clones of the omitted
//! parameters' default expressions (§S.1.3 call-site evaluation —
//! a fresh clone per call, never a shared value).
//!
//! Running this between tycheck and the backend means every
//! downstream consumer — aliasing analysis, mutation inference,
//! emission — sees ordinary positional calls and needs zero
//! named-arg/default knowledge. The emitted Rust is the fully
//! expanded call, which also keeps it readable.
//!
//! ## Span notes
//!
//! - A spliced default expression keeps its **declaration-site**
//!   span. The checker type-checked the default at the declaration,
//!   so `expr_types` lookups for it hit those entries — consistent
//!   wherever the clone lands.
//! - Default expressions may themselves contain sugar calls; the
//!   walker recurses into spliced arguments, and the plan map carries
//!   entries for those decl-site spans, so nesting expands fully.

use std::collections::HashMap;

use juxc_ast::{
    Block, CompilationUnit, ElseBranch, Expr, FnDecl, InterpSegment, LambdaBody, Stmt,
    SwitchBody, TopLevelDecl,
};
use juxc_source::Span;

use crate::ArgSource;

/// Apply the checker's call-sugar expansion plans to every unit in
/// place. `plans` is [`crate::TypeCheckResult::call_expansions`].
/// A no-op when the map is empty (the common case — most programs
/// have no named-arg/default call sites).
pub fn apply_call_expansions(
    units: &mut [CompilationUnit],
    plans: &HashMap<Span, Vec<ArgSource>>,
) {
    if plans.is_empty() {
        return;
    }
    for unit in units {
        for item in &mut unit.items {
            expand_top_level(item, plans);
        }
    }
}

fn expand_top_level(item: &mut TopLevelDecl, plans: &HashMap<Span, Vec<ArgSource>>) {
    match item {
        TopLevelDecl::Function(f) => expand_fn(f, plans),
        TopLevelDecl::Class(c) => {
            for nested in &mut c.nested_types {
                expand_top_level(nested, plans);
            }
            for field in &mut c.fields {
                if let Some(init) = &mut field.default {
                    expand_expr(init, plans);
                }
            }
            for ctor in &mut c.constructors {
                for p in &mut ctor.params {
                    if let Some(d) = &mut p.default {
                        expand_expr(d, plans);
                    }
                }
                expand_block(&mut ctor.body, plans);
            }
            for m in &mut c.methods {
                expand_fn(m, plans);
            }
            for op in &mut c.operators {
                if let Some(body) = &mut op.body {
                    expand_block(body, plans);
                }
            }
            for b in &mut c.init_blocks {
                expand_block(b, plans);
            }
            for b in &mut c.static_init_blocks {
                expand_block(b, plans);
            }
        }
        TopLevelDecl::Enum(e) => {
            for m in &mut e.methods {
                expand_fn(m, plans);
            }
            for op in &mut e.operators {
                if let Some(body) = &mut op.body {
                    expand_block(body, plans);
                }
            }
            for c in &mut e.constants {
                if let Some(init) = &mut c.default {
                    expand_expr(init, plans);
                }
            }
        }
        TopLevelDecl::Record(r) => {
            for m in &mut r.methods {
                expand_fn(m, plans);
            }
            for op in &mut r.operators {
                if let Some(body) = &mut op.body {
                    expand_block(body, plans);
                }
            }
        }
        TopLevelDecl::Interface(i) => {
            // Default/static interface methods carry bodies.
            for m in &mut i.methods {
                expand_fn(m, plans);
            }
            for field in &mut i.fields {
                if let Some(init) = &mut field.default {
                    expand_expr(init, plans);
                }
            }
        }
        TopLevelDecl::Const(c) => expand_expr(&mut c.value, plans),
        _ => {}
    }
}

fn expand_fn(f: &mut FnDecl, plans: &HashMap<Span, Vec<ArgSource>>) {
    // Param defaults can contain sugar calls of their own.
    for p in &mut f.params {
        if let Some(d) = &mut p.default {
            expand_expr(d, plans);
        }
    }
    if let Some(body) = &mut f.body {
        expand_block(body, plans);
    }
}

fn expand_block(block: &mut Block, plans: &HashMap<Span, Vec<ArgSource>>) {
    for stmt in &mut block.statements {
        expand_stmt(stmt, plans);
    }
}

fn expand_stmt(stmt: &mut Stmt, plans: &HashMap<Span, Vec<ArgSource>>) {
    match stmt {
        Stmt::Expr(e) => expand_expr(e, plans),
        Stmt::Return(e) => {
            if let Some(e) = e {
                expand_expr(e, plans);
            }
        }
        Stmt::VarDecl(v) => {
            if let Some(init) = &mut v.init {
                expand_expr(init, plans);
            }
        }
        Stmt::If(i) => expand_if(i, plans),
        Stmt::While(w) => {
            expand_expr(&mut w.condition, plans);
            expand_block(&mut w.body, plans);
        }
        Stmt::DoWhile(d) => {
            expand_block(&mut d.body, plans);
            expand_expr(&mut d.condition, plans);
        }
        Stmt::ForEach(f) => {
            expand_expr(&mut f.iter, plans);
            expand_block(&mut f.body, plans);
        }
        Stmt::Assign(a) => {
            expand_expr(&mut a.target, plans);
            expand_expr(&mut a.value, plans);
        }
        Stmt::Labeled { stmt, .. } => expand_stmt(stmt, plans),
        Stmt::SuperCall(args, _) => {
            for a in args {
                expand_expr(a, plans);
            }
        }
        Stmt::Throw(e, _) => expand_expr(e, plans),
        Stmt::Try(t) => {
            expand_block(&mut t.body, plans);
            for c in &mut t.catches {
                expand_block(&mut c.body, plans);
            }
            if let Some(f) = &mut t.finally {
                expand_block(f, plans);
            }
        }
        Stmt::Unsafe(b) => expand_block(b, plans),
        Stmt::ForC(f) => {
            if let Some(init) = &mut f.init {
                expand_stmt(init, plans);
            }
            if let Some(cond) = &mut f.cond {
                expand_expr(cond, plans);
            }
            if let Some(update) = &mut f.update {
                expand_stmt(update, plans);
            }
            expand_block(&mut f.body, plans);
        }
        Stmt::Break(..) | Stmt::Continue(..) => {}
    }
}

fn expand_if(i: &mut juxc_ast::IfStmt, plans: &HashMap<Span, Vec<ArgSource>>) {
    expand_expr(&mut i.condition, plans);
    expand_block(&mut i.then_block, plans);
    if let Some(else_branch) = &mut i.else_branch {
        match &mut **else_branch {
            ElseBranch::If(elif) => expand_if(elif, plans),
            ElseBranch::Block(b) => expand_block(b, plans),
        }
    }
}

fn expand_expr(expr: &mut Expr, plans: &HashMap<Span, Vec<ArgSource>>) {
    match expr {
        // `out <place>` (§M.4) — recurse into the place.
        Expr::Out(inner, _) => expand_expr(inner, plans),
        Expr::Call(c) => {
            // Apply this call's plan FIRST (it re-orders/splices the
            // argument vector), then recurse into the result so
            // spliced defaults containing sugar calls expand too.
            if let Some(plan) = plans.get(&c.span) {
                splice_args(&mut c.args, &mut c.arg_names, plan);
            }
            expand_expr(&mut c.callee, plans);
            for a in &mut c.args {
                expand_expr(a, plans);
            }
        }
        Expr::NewObject(n) => {
            if let Some(plan) = plans.get(&n.span) {
                splice_args(&mut n.args, &mut n.arg_names, plan);
            }
            for a in &mut n.args {
                expand_expr(a, plans);
            }
            if let Some(body) = &mut n.anonymous_body {
                for m in &mut body.methods {
                    expand_fn(m, plans);
                }
                for b in &mut body.init_blocks {
                    expand_block(b, plans);
                }
            }
        }
        Expr::Binary(b) => {
            expand_expr(&mut b.left, plans);
            expand_expr(&mut b.right, plans);
        }
        Expr::Unary(u) => expand_expr(&mut u.operand, plans),
        Expr::Range(r) => {
            expand_expr(&mut r.start, plans);
            expand_expr(&mut r.end, plans);
            if let Some(s) = &mut r.step {
                expand_expr(s, plans);
            }
        }
        Expr::Cast(c) => expand_expr(&mut c.value, plans),
        Expr::SizeOf(s) => expand_expr(&mut s.operand, plans),
        Expr::NewArray(n) => expand_expr(&mut n.size, plans),
        Expr::NewArrayLit(n) => {
            for e in &mut n.elements {
                expand_expr(e, plans);
            }
        }
        Expr::Index(i) => {
            expand_expr(&mut i.array, plans);
            expand_expr(&mut i.index, plans);
        }
        Expr::Field(f) => expand_expr(&mut f.object, plans),
        Expr::InterpString(s) => {
            for seg in &mut s.segments {
                if let InterpSegment::Expr(e) = seg {
                    expand_expr(e, plans);
                }
            }
        }
        Expr::TypeTest(t) => expand_expr(&mut t.value, plans),
        Expr::Switch(s) => {
            expand_expr(&mut s.scrutinee, plans);
            for arm in &mut s.arms {
                if let Some(g) = &mut arm.guard {
                    expand_expr(g, plans);
                }
                match &mut arm.body {
                    SwitchBody::Expr(e) => expand_expr(e, plans),
                    SwitchBody::Block(b) => expand_block(b, plans),
                }
            }
        }
        Expr::Lambda(l) => match &mut l.body {
            LambdaBody::Expr(e) => expand_expr(e, plans),
            LambdaBody::Block(b) => expand_block(b, plans),
        },
        Expr::Elvis(e) => {
            expand_expr(&mut e.value, plans);
            expand_expr(&mut e.fallback, plans);
        }
        Expr::Ternary(t) => {
            expand_expr(&mut t.condition, plans);
            expand_expr(&mut t.then_branch, plans);
            expand_expr(&mut t.else_branch, plans);
        }
        Expr::Await(e, _) => expand_expr(e, plans),
        Expr::NotNullAssert(e, _) => expand_expr(e, plans),
        Expr::TupleLit(elems, _) => {
            for e in elems {
                expand_expr(e, plans);
            }
        }
        Expr::ErrorProp(inner, _) => expand_expr(inner, plans),
        Expr::TryExpr(t) => {
            expand_block(&mut t.body, plans);
            for c in &mut t.catches {
                expand_block(&mut c.body, plans);
            }
            if let Some(f) = &mut t.finally {
                expand_block(f, plans);
            }
        }
        Expr::Literal(_)
        | Expr::Path(_)
        | Expr::This(_)
        | Expr::Super(_)
        | Expr::MethodRef(_) => {}
    }
}

/// Rebuild a call's argument vector from its expansion plan:
/// `Explicit(i)` slots take the original `args[i]` (moving it),
/// `Default(e)` slots take a fresh clone of the declaration's
/// default expression. Labels are cleared — the result is a plain
/// positional call.
fn splice_args(args: &mut Vec<Expr>, arg_names: &mut Vec<Option<juxc_ast::Ident>>, plan: &[ArgSource]) {
    // Move the originals out, leaving placeholders we never read twice
    // (the checker guarantees each Explicit index appears once).
    let mut originals: Vec<Option<Expr>> = args.drain(..).map(Some).collect();
    let mut rebuilt: Vec<Expr> = Vec::with_capacity(plan.len());
    for source in plan {
        match source {
            ArgSource::Explicit(i) => {
                if let Some(slot) = originals.get_mut(*i) {
                    if let Some(e) = slot.take() {
                        rebuilt.push(e);
                    }
                }
            }
            ArgSource::Default(e) => rebuilt.push(e.clone()),
            // Variadic slot — pack the listed args into a synthesized
            // dynamic-array literal of the element type (§E.1.2.1).
            // Span: the first packed arg's (or DUMMY for an empty
            // pack) — synthesized nodes must not alias the call's own
            // span in the span-keyed type map.
            ArgSource::Variadic { element_type, indices } => {
                let mut elements = Vec::with_capacity(indices.len());
                for &i in indices {
                    if let Some(slot) = originals.get_mut(i) {
                        if let Some(e) = slot.take() {
                            elements.push(e);
                        }
                    }
                }
                let span = elements
                    .first()
                    .map(crate::check::expr_span_pub)
                    .unwrap_or(juxc_source::Span::DUMMY);
                rebuilt.push(Expr::NewArrayLit(juxc_ast::NewArrayLitExpr {
                    element_type: element_type.clone(),
                    elements,
                    fixed: false,
                    span,
                }));
            }
        }
    }
    *args = rebuilt;
    arg_names.clear();
}
