//! Free-function pre-passes and lvalue/expression helpers used by the
//! action-focused emit modules.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original free functions.

use std::collections::HashSet;

use juxc_ast::{
    Block, CompilationUnit, ElseBranch, Expr, GenericArg, Ident, Literal, QualifiedName, Stmt,
    TopLevelDecl, TypeParam, TypeRef, WildcardBound,
};
use juxc_source::Span;

/// Walk a function body and collect every name that appears as the
/// target of an [`AssignStmt`].
///
/// This is a one-shot mutation analysis used by the emitter to decide
/// whether a `var` should lower to `let mut` (target of an assignment
/// somewhere in the body) or plain `let` (never reassigned). It walks
/// nested blocks — `if`/`else`/`while` bodies — but stops at function
/// boundaries (a nested function decl would have its own pass, and we
/// don't have nested functions yet anyway).
pub(crate) fn collect_mutated_names(
    block: &Block,
    out: &mut HashSet<String>,
    user_mut: &HashSet<String>,
) {
    // Pre-scan for locals bound to an anonymous-class instance
    // (`var w = new Widget() { … };`). Anonymous-class methods always
    // lower with a `&mut self` receiver (matching the interface
    // trait-dispatch convention), so calling ANY method on such a local
    // takes `&mut`, which requires the binding to be `let mut`. We
    // collect these names so `collect_mutating_calls` can promote a
    // plain `obj.method()` call on them even when `method` isn't in the
    // mutating-method set.
    let mut anon_locals: HashSet<String> = HashSet::new();
    collect_anon_bound_locals(block, &mut anon_locals);
    // Promote any anon-bound local on which a method is invoked. We pass
    // the anon set in as extra `user_mut`-equivalent context: a method
    // call on an anon local mutates it regardless of the method name.
    collect_anon_method_calls(block, out, &anon_locals);
    collect_mutated_names_real(block, out, user_mut);
}

/// Walk `block` (recursively) and add to `out` any anon-bound local in
/// `anon_locals` on which a method is called (`w.render()`), since
/// anonymous-class methods lower with `&mut self`.
fn collect_anon_method_calls(
    block: &Block,
    out: &mut HashSet<String>,
    anon_locals: &HashSet<String>,
) {
    fn walk_expr(e: &Expr, out: &mut HashSet<String>, anon_locals: &HashSet<String>) {
        if let Expr::Call(c) = e {
            if let Expr::Field(f) = &*c.callee {
                if let Expr::Path(qn) = &*f.object {
                    if qn.segments.len() == 1
                        && anon_locals.contains(&qn.segments[0].text)
                    {
                        out.insert(qn.segments[0].text.clone());
                    }
                }
            }
        }
        // Recurse into the immediate sub-expressions that can carry a
        // call. Reuse the existing structural walk by re-running on
        // children via a small manual descent for the common shapes.
        match e {
            Expr::Call(c) => {
                walk_expr(&c.callee, out, anon_locals);
                for a in &c.args {
                    walk_expr(a, out, anon_locals);
                }
            }
            Expr::Field(f) => walk_expr(&f.object, out, anon_locals),
            Expr::Binary(b) => {
                walk_expr(&b.left, out, anon_locals);
                walk_expr(&b.right, out, anon_locals);
            }
            Expr::Unary(u) => walk_expr(&u.operand, out, anon_locals),
            Expr::InterpString(s) => {
                for seg in &s.segments {
                    if let juxc_ast::InterpSegment::Expr(inner) = seg {
                        walk_expr(inner, out, anon_locals);
                    }
                }
            }
            _ => {}
        }
    }
    fn walk_block(block: &Block, out: &mut HashSet<String>, anon_locals: &HashSet<String>) {
        for stmt in &block.statements {
            match stmt {
                Stmt::Expr(e) => walk_expr(e, out, anon_locals),
                Stmt::Assign(a) => walk_expr(&a.value, out, anon_locals),
                Stmt::VarDecl(v) => {
                    if let Some(init) = &v.init {
                        walk_expr(init, out, anon_locals);
                    }
                }
                Stmt::Return(Some(e)) => walk_expr(e, out, anon_locals),
                Stmt::If(if_stmt) => {
                    walk_expr(&if_stmt.condition, out, anon_locals);
                    walk_block(&if_stmt.then_block, out, anon_locals);
                    if let Some(eb) = if_stmt.else_branch.as_deref() {
                        match eb {
                            ElseBranch::Block(b) => walk_block(b, out, anon_locals),
                            ElseBranch::If(inner) => {
                                let synth = Block {
                                    statements: vec![Stmt::If(inner.clone())],
                                    span: Span::DUMMY,
                                };
                                walk_block(&synth, out, anon_locals);
                            }
                        }
                    }
                }
                Stmt::While(w) => {
                    walk_expr(&w.condition, out, anon_locals);
                    walk_block(&w.body, out, anon_locals);
                }
                Stmt::ForEach(f) => {
                    walk_expr(&f.iter, out, anon_locals);
                    walk_block(&f.body, out, anon_locals);
                }
                _ => {}
            }
        }
    }
    walk_block(block, out, anon_locals);
}

/// Collect every local-variable name in `block` (recursively) whose
/// initializer is an anonymous-class instantiation
/// (`new T() { … }`). Used by [`collect_mutated_names`] to promote
/// those bindings to `let mut` when a method is called on them, since
/// anonymous-class methods lower with `&mut self`.
fn collect_anon_bound_locals(block: &Block, out: &mut HashSet<String>) {
    for stmt in &block.statements {
        match stmt {
            Stmt::VarDecl(v) => {
                if let Some(Expr::NewObject(n)) = v.init.as_ref() {
                    if n.anonymous_body.is_some() {
                        out.insert(v.name.text.clone());
                    }
                }
            }
            Stmt::If(if_stmt) => {
                collect_anon_bound_locals(&if_stmt.then_block, out);
                if let Some(eb) = if_stmt.else_branch.as_deref() {
                    match eb {
                        ElseBranch::Block(b) => collect_anon_bound_locals(b, out),
                        ElseBranch::If(inner) => {
                            let synth = Block {
                                statements: vec![Stmt::If(inner.clone())],
                                span: Span::DUMMY,
                            };
                            collect_anon_bound_locals(&synth, out);
                        }
                    }
                }
            }
            Stmt::While(w) => collect_anon_bound_locals(&w.body, out),
            Stmt::ForEach(f) => collect_anon_bound_locals(&f.body, out),
            _ => {}
        }
    }
}

/// Original reassignment/mutating-call walker, split out from
/// [`collect_mutated_names`] so the public entry point can run the
/// anonymous-class pre-passes first.
fn collect_mutated_names_real(
    block: &Block,
    out: &mut HashSet<String>,
    user_mut: &HashSet<String>,
) {
    for stmt in &block.statements {
        match stmt {
            Stmt::Assign(a) => {
                // Walk down the lvalue to find the underlying name. For
                // `arr[i] = v` the base is `arr`; for `x = v` it's `x`.
                if let Some(name) = lvalue_base_name(&a.target) {
                    out.insert(name);
                }
                collect_mutating_calls(&a.value, out, user_mut);
            }
            Stmt::Expr(e) => collect_mutating_calls(e, out, user_mut),
            Stmt::VarDecl(v) => {
                if let Some(init) = &v.init {
                    collect_mutating_calls(init, out, user_mut);
                }
            }
            Stmt::Return(Some(e)) => collect_mutating_calls(e, out, user_mut),
            Stmt::If(if_stmt) => {
                collect_mutating_calls(&if_stmt.condition, out, user_mut);
                collect_mutated_names(&if_stmt.then_block, out, user_mut);
                collect_mutated_names_in_else(if_stmt.else_branch.as_deref(), out, user_mut);
            }
            Stmt::While(w) => {
                collect_mutating_calls(&w.condition, out, user_mut);
                collect_mutated_names(&w.body, out, user_mut);
            }
            Stmt::ForEach(f) => {
                collect_mutating_calls(&f.iter, out, user_mut);
                collect_mutated_names(&f.body, out, user_mut);
            }
            // Other statement kinds — Return(None), Break, Continue —
            // don't carry expressions that could mutate.
            _ => {}
        }
    }
}

/// Walk an expression looking for `obj.method(…)` calls where `method`
/// is one of the known Rust mutating methods on `Vec` and `obj` is a
/// single-segment Path. Each match adds that name to `out` so the
/// surrounding `let` binding for it gets promoted to `let mut`.
///
/// Without a real type table we can't tell whether `obj` is actually a
/// Vec — we just trust the method name. The cost is that calling a
/// same-named method on an unrelated type promotes the local to `mut`
/// unnecessarily, which only triggers Rust's `unused_mut` warning if
/// nothing else mutates the binding. Acceptable for Phase 1.
///
/// Sub-expressions are walked recursively so nested calls are caught.
pub(crate) fn collect_mutating_calls(e: &Expr, out: &mut HashSet<String>, user_mut: &HashSet<String>) {
    match e {
        Expr::Call(c) => {
            if let Expr::Field(f) = &*c.callee {
                // A method call counts as mutating the receiver when
                // either the hardcoded set covers it (`push`, `pop`,
                // …) or the per-program pre-pass flagged it as a user
                // method that takes `&mut self`.
                let mutates =
                    is_mutating_method(&f.field.text) || user_mut.contains(&f.field.text);
                if mutates {
                    if let Expr::Path(qn) = &*f.object {
                        if qn.segments.len() == 1 {
                            out.insert(qn.segments[0].text.clone());
                        }
                    }
                }
            }
            collect_mutating_calls(&c.callee, out, user_mut);
            for arg in &c.args {
                collect_mutating_calls(arg, out, user_mut);
            }
        }
        Expr::Binary(b) => {
            collect_mutating_calls(&b.left, out, user_mut);
            collect_mutating_calls(&b.right, out, user_mut);
        }
        Expr::Unary(u) => collect_mutating_calls(&u.operand, out, user_mut),
        Expr::Range(r) => {
            collect_mutating_calls(&r.start, out, user_mut);
            collect_mutating_calls(&r.end, out, user_mut);
        }
        Expr::Cast(c) => collect_mutating_calls(&c.value, out, user_mut),
        Expr::Index(idx) => {
            collect_mutating_calls(&idx.array, out, user_mut);
            collect_mutating_calls(&idx.index, out, user_mut);
        }
        Expr::Field(f) => collect_mutating_calls(&f.object, out, user_mut),
        Expr::NewArray(n) => collect_mutating_calls(&n.size, out, user_mut),
        Expr::NewArrayLit(n) => {
            for el in &n.elements {
                collect_mutating_calls(el, out, user_mut);
            }
        }
        Expr::SizeOf(s) => collect_mutating_calls(&s.operand, out, user_mut),
        Expr::InterpString(s) => {
            // Walk each `${expr}` interpolation segment; literal and
            // bare-ident segments don't contain mutating call shapes.
            for seg in &s.segments {
                if let juxc_ast::InterpSegment::Expr(inner) = seg {
                    collect_mutating_calls(inner, out, user_mut);
                }
            }
        }
        Expr::NewObject(n) => {
            for arg in &n.args {
                collect_mutating_calls(arg, out, user_mut);
            }
        }
        Expr::Switch(s) => {
            collect_mutating_calls(&s.scrutinee, out, user_mut);
            for arm in &s.arms {
                match &arm.body {
                    juxc_ast::SwitchBody::Expr(e) => {
                        collect_mutating_calls(e, out, user_mut);
                    }
                    juxc_ast::SwitchBody::Block(b) => {
                        collect_mutated_names(b, out, user_mut);
                    }
                }
            }
        }
        // Leaves with no further sub-expressions:
        // - Literal, Path: source-level atoms.
        // - This: implicit-receiver token; no body to walk.
        Expr::Literal(_) | Expr::Path(_) | Expr::This(_) => {}
        // Walk the lambda body so a mutating call inside (e.g.
        // `xs -> xs.push(1)`) still marks the captured receiver.
        Expr::Lambda(l) => match &l.body {
            juxc_ast::LambdaBody::Expr(e) => collect_mutating_calls(e, out, user_mut),
            juxc_ast::LambdaBody::Block(b) => collect_mutated_names(b, out, user_mut),
        },
        // Elvis evaluates both sides — both can contain a mutating
        // call (e.g. `xs.pop() ?: empty()`); walk recursively.
        Expr::Elvis(e) => {
            collect_mutating_calls(&e.value, out, user_mut);
            collect_mutating_calls(&e.fallback, out, user_mut);
        }
        // Method-ref is a closure construction; no calls happen
        // until the user invokes the returned value.
        Expr::MethodRef(_) => {}
        // Ternary evaluates condition + one branch; walk all
        // three for nested mutating calls.
        Expr::Ternary(t) => {
            collect_mutating_calls(&t.condition, out, user_mut);
            collect_mutating_calls(&t.then_branch, out, user_mut);
            collect_mutating_calls(&t.else_branch, out, user_mut);
        }
        // `await expr` — the operand drives evaluation, so any
        // mutating calls inside it count just like in any other
        // value position.
        Expr::Await(inner, _) => {
            collect_mutating_calls(inner, out, user_mut);
        }
    }
}

/// Recursively scan a Block for any direct `await` expression. Used
/// by `emit_try` to pick the sync vs async catch_unwind lowering.
///
/// Walks into nested statements (if/while/for/switch/try arms) but
/// does NOT descend into closure / lambda bodies — those introduce
/// a new function boundary and their `.await` belongs to whatever
/// async context wraps them, not the surrounding block.
pub(crate) fn block_contains_await(block: &Block) -> bool {
    block.statements.iter().any(stmt_contains_await)
}

fn stmt_contains_await(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Expr(e) | Stmt::Throw(e, _) => expr_contains_await(e),
        Stmt::Return(Some(e)) => expr_contains_await(e),
        Stmt::Return(None) => false,
        Stmt::VarDecl(v) => v.init.as_ref().is_some_and(expr_contains_await),
        Stmt::Assign(a) => expr_contains_await(&a.value) || expr_contains_await(&a.target),
        Stmt::If(i) => if_contains_await(i),
        Stmt::While(w) => expr_contains_await(&w.condition) || block_contains_await(&w.body),
        Stmt::ForEach(f) => expr_contains_await(&f.iter) || block_contains_await(&f.body),
        Stmt::Try(t) => {
            block_contains_await(&t.body)
                || t.catches.iter().any(|c| block_contains_await(&c.body))
                || t.finally.as_ref().is_some_and(block_contains_await)
        }
        Stmt::SuperCall(args, _) => args.iter().any(expr_contains_await),
        Stmt::Break(_) | Stmt::Continue(_) => false,
    }
}

fn if_contains_await(i: &juxc_ast::IfStmt) -> bool {
    expr_contains_await(&i.condition)
        || block_contains_await(&i.then_block)
        || match i.else_branch.as_deref() {
            Some(ElseBranch::Block(b)) => block_contains_await(b),
            Some(ElseBranch::If(inner)) => if_contains_await(inner),
            None => false,
        }
}

fn expr_contains_await(e: &Expr) -> bool {
    match e {
        Expr::Await(_, _) => true,
        Expr::Call(c) => {
            expr_contains_await(&c.callee) || c.args.iter().any(expr_contains_await)
        }
        Expr::Binary(b) => expr_contains_await(&b.left) || expr_contains_await(&b.right),
        Expr::Unary(u) => expr_contains_await(&u.operand),
        Expr::Cast(c) => expr_contains_await(&c.value),
        Expr::Range(r) => expr_contains_await(&r.start) || expr_contains_await(&r.end),
        Expr::Field(f) => expr_contains_await(&f.object),
        Expr::Index(i) => expr_contains_await(&i.array) || expr_contains_await(&i.index),
        Expr::NewArray(n) => expr_contains_await(&n.size),
        Expr::NewArrayLit(n) => n.elements.iter().any(expr_contains_await),
        Expr::NewObject(n) => n.args.iter().any(expr_contains_await),
        Expr::Elvis(e) => expr_contains_await(&e.value) || expr_contains_await(&e.fallback),
        Expr::Ternary(t) => {
            expr_contains_await(&t.condition)
                || expr_contains_await(&t.then_branch)
                || expr_contains_await(&t.else_branch)
        }
        Expr::InterpString(s) => s.segments.iter().any(|seg| match seg {
            juxc_ast::InterpSegment::Expr(e) => expr_contains_await(e),
            _ => false,
        }),
        Expr::SizeOf(s) => expr_contains_await(&s.operand),
        Expr::Switch(s) => {
            expr_contains_await(&s.scrutinee)
                || s.arms.iter().any(|a| match &a.body {
                    juxc_ast::SwitchBody::Expr(e) => expr_contains_await(e),
                    juxc_ast::SwitchBody::Block(b) => block_contains_await(b),
                })
        }
        // Closures / lambdas open a new fn boundary — the `.await`
        // inside belongs to the closure's own async context, not the
        // surrounding block.
        Expr::Lambda(_) | Expr::MethodRef(_) => false,
        Expr::Literal(_) | Expr::Path(_) | Expr::This(_) => false,
    }
}

/// Known mutating methods on `Vec` (and similar Rust containers).
/// Hardcoded for Phase 1; a real type table would derive this from
/// the receiver's type signature.
pub(crate) fn is_mutating_method(name: &str) -> bool {
    matches!(
        name,
        "push" | "pop" | "clear" | "insert" | "remove" | "extend" | "truncate"
        | "swap" | "reverse" | "sort"
        // Jux-spec method names: List<T> and Map<K, V> both
        // mutate through `add`/`put` and various others. Listing
        // them here lets `let` → `let mut` promotion kick in for
        // `var xs = new List<int>(); xs.add(1);` without forcing
        // the user to write a manual `mut` annotation.
        | "add" | "put" | "set"
    )
}

/// Walk down a (possibly index-chained) lvalue expression to find its
/// underlying variable name. Returns `None` for shapes the parser
/// shouldn't produce as an lvalue.
///
/// - `x`       → `Some("x")`
/// - `arr[i]`  → `Some("arr")`
/// - `arr[i][j]` → `Some("arr")`
pub(crate) fn lvalue_base_name(e: &Expr) -> Option<String> {
    match e {
        Expr::Path(qn) if qn.segments.len() == 1 => Some(qn.segments[0].text.clone()),
        Expr::Index(idx) => lvalue_base_name(&idx.array),
        // Field chains walk through to find the lvalue root. For
        // `u.nickname = ...` the root is `u`, which must be
        // `let mut` so Rust can take `&mut self` on the field
        // assignment. For `this.field = ...` the root is `This`,
        // which isn't a local binding — `body_writes_to_this`
        // tracks that case for receiver `&mut self` promotion.
        Expr::Field(f) => lvalue_base_name(&f.object),
        Expr::This(_) => None,
        _ => None,
    }
}

/// Returns `true` if any statement in `block` (transitively) assigns to
/// a field whose lvalue root is `Expr::This` — i.e., something like
/// `this.x = …`, `this.x[i] = …`, or compound forms. Used by
/// `emit_method` to decide between `&self` and `&mut self` receivers.
///
/// We only need a yes/no answer; the method body's per-local mutation
/// analysis (`mutated_in_fn`) is unaffected.
pub(crate) fn body_writes_to_this(block: &Block) -> bool {
    for stmt in &block.statements {
        match stmt {
            Stmt::Assign(a) => {
                if lvalue_root_is_this(&a.target) {
                    return true;
                }
            }
            Stmt::If(if_stmt) => {
                if body_writes_to_this(&if_stmt.then_block) {
                    return true;
                }
                if body_writes_to_this_in_else(if_stmt.else_branch.as_deref()) {
                    return true;
                }
            }
            Stmt::While(w) => {
                if body_writes_to_this(&w.body) {
                    return true;
                }
            }
            Stmt::ForEach(f) => {
                if body_writes_to_this(&f.body) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// True when the lvalue's deepest non-Field/Index expression is `Expr::This`.
pub(crate) fn lvalue_root_is_this(e: &Expr) -> bool {
    match e {
        Expr::This(_) => true,
        Expr::Field(f) => lvalue_root_is_this(&f.object),
        Expr::Index(i) => lvalue_root_is_this(&i.array),
        _ => false,
    }
}

/// `body_writes_to_this` helper for else-branch chains.
pub(crate) fn body_writes_to_this_in_else(branch: Option<&ElseBranch>) -> bool {
    let mut cursor = branch;
    while let Some(b) = cursor {
        match b {
            ElseBranch::If(inner) => {
                if body_writes_to_this(&inner.then_block) {
                    return true;
                }
                cursor = inner.else_branch.as_deref();
            }
            ElseBranch::Block(block) => {
                return body_writes_to_this(block);
            }
        }
    }
    false
}

/// Try to flatten a dotted-path expression (`a.b.c.d`) into a sequence
/// of identifier segments. After the parser refactor that switched
/// `parse_primary`'s identifier arm to single-segment, source paths
/// like `std.io.Stream` arrive as `Field(Field(Path("std"), "io"), "Stream")`.
/// This helper recovers the flat segment list when every node in the
/// chain is a simple ident — used by `sizeof` to apply the §5.9.3
/// rule-4 "multi-segment path → type form" branch.
///
/// Returns `None` if any node along the chain is something other than
/// a simple `Path`/`Field` (e.g. a call, index, or literal anywhere).
pub(crate) fn try_flatten_dotted_path(e: &Expr) -> Option<Vec<String>> {
    match e {
        Expr::Path(qn) => Some(qn.segments.iter().map(|s| s.text.clone()).collect()),
        Expr::Field(f) => {
            let mut base = try_flatten_dotted_path(&f.object)?;
            base.push(f.field.text.clone());
            Some(base)
        }
        _ => None,
    }
}

/// Helper for [`collect_mutated_names`]: walks an `else` chain
/// (`else if … else …`), descending into each `if`/`block` arm.
pub(crate) fn collect_mutated_names_in_else(
    branch: Option<&ElseBranch>,
    out: &mut HashSet<String>,
    user_mut: &HashSet<String>,
) {
    let mut cursor = branch;
    while let Some(b) = cursor {
        match b {
            ElseBranch::If(inner) => {
                collect_mutated_names(&inner.then_block, out, user_mut);
                cursor = inner.else_branch.as_deref();
            }
            ElseBranch::Block(block) => {
                collect_mutated_names(block, out, user_mut);
                cursor = None;
            }
        }
    }
}

/// True if `e` is a `Literal::String(_)`. Used by [`RustEmitter::emit_binary`]
/// to decide when `+` should lower as string concatenation.
pub(crate) fn is_string_literal(e: &Expr) -> bool {
    matches!(e, Expr::Literal(Literal::String(_)))
}

/// Inspect a constructor body to see whether every statement is a
/// `this.field = expr;` assignment. Returns the list of
/// `(field-name, init-expr)` pairs in source order when so, else
/// `None`. The emitter uses this for the **simple-ctor fast path** —
/// a direct `Self { field: expr, … }` literal that avoids `Default`
/// initialization (and so works for generic-typed fields).
/// Output of [`extract_simple_ctor_inits`] when the constructor body
/// matches the "simple" shape — pure `this.field = expr;` lines,
/// optionally preceded by a single `super(args);` delegation.
pub(crate) struct SimpleCtorInits {
    /// `Some(args)` when the body started with `super(args);` —
    /// extracted so the backend can lift it into the child struct's
    /// literal as `__parent: Parent::new(args)`. `None` when no super
    /// call appears (either no parent, or the user omitted the
    /// explicit call).
    pub(crate) super_args: Option<Vec<Expr>>,
    /// `(field-name, init-expr)` pairs in source order — the
    /// `this.field = expr;` assignments. Same semantics as before.
    pub(crate) inits: Vec<(String, Expr)>,
    /// Side-effect statements that don't touch `this.field` —
    /// most commonly static-field compound assignments like
    /// `MyStatic = MyStatic + 1`. Emitted before the struct
    /// literal in their original source order so a counter bump
    /// still happens at construction time even when the simple
    /// path is taken. Order is preserved relative to each other
    /// but field inits all happen "logically together" in the
    /// final `Self { ... }` literal.
    pub(crate) side_effects: Vec<juxc_ast::Stmt>,
}

pub(crate) fn extract_simple_ctor_inits(ctor: &juxc_ast::ConstructorDecl) -> Option<SimpleCtorInits> {
    let mut super_args: Option<Vec<Expr>> = None;
    let mut inits = Vec::with_capacity(ctor.body.statements.len());
    let mut side_effects: Vec<juxc_ast::Stmt> = Vec::new();
    for (i, stmt) in ctor.body.statements.iter().enumerate() {
        match stmt {
            // `super(args);` is allowed as the first statement and
            // gets lifted into the struct literal's `__parent` slot.
            Stmt::SuperCall(args, _) => {
                if i != 0 {
                    // Java semantics: super must be first. Disqualify
                    // the simple path so the user gets the fallback
                    // (which will emit it inline at whatever position
                    // — Rust will still error, but the error will
                    // include the call site).
                    return None;
                }
                super_args = Some(args.clone());
            }
            Stmt::Assign(a) => {
                // `this.field = expr;` → field init.
                if let Expr::Field(f) = &a.target {
                    if !matches!(&*f.object, Expr::This(_)) {
                        return None;
                    }
                    inits.push((f.field.text.clone(), a.value.clone()));
                    continue;
                }
                // `staticField = expr;` (bare-name Path target) →
                // side effect. The simple path can run these
                // before the struct literal without needing
                // Default initialization for any generic-typed
                // field. Anything more complex (array indexing,
                // arbitrary lvalues) still falls through to the
                // slow path.
                if matches!(&a.target, Expr::Path(_)) {
                    side_effects.push(stmt.clone());
                    continue;
                }
                return None;
            }
            // Any other statement disqualifies the fast path.
            _ => return None,
        }
    }
    Some(SimpleCtorInits { super_args, inits, side_effects })
}

/// Whether a pattern's source form carried explicit parens. Lets the
/// backend distinguish unit-variant patterns (`Color.Red`) from
/// empty-paren tuple-variant patterns (`Color.Red()`) when emitting.
/// In Rust both forms compile, but only the no-paren form is canonical
/// for unit variants.
pub(crate) fn pattern_has_parens(p: &juxc_ast::Pattern) -> bool {
    matches!(p, juxc_ast::Pattern::EnumVariant { has_parens: true, .. })
}

/// Pre-pass over the compilation unit collecting the names of user-
/// defined methods whose bodies write to `this.field` — i.e. methods
/// the backend will emit with `&mut self`. The mutation analyzer in
/// every function consults this set so calling such a method on a
/// receiver promotes the receiver's binding to `let mut`.
///
/// This is a workaround for the absent type table: a same-named method
/// on a different class will also get flagged, which only costs an
/// unnecessary `mut`. Once tycheck carries receiver types we can do
/// the precise per-class lookup instead.
pub(crate) fn collect_user_mut_methods(unit: &CompilationUnit) -> HashSet<String> {
    // Seed: methods that directly write to `this.field`, plus every
    // method declared on an interface (interface trait methods all
    // emit as `&mut self`, so any call to one on a `this.field`
    // receiver propagates the `&mut self` requirement up the call
    // chain).
    let mut out = HashSet::new();
    for item in &unit.items {
        match item {
            TopLevelDecl::Class(class) => {
                for method in &class.methods {
                    if let Some(body) = &method.body {
                        if body_writes_to_this(body) {
                            out.insert(method.name.text.clone());
                        }
                    }
                }
            }
            TopLevelDecl::Interface(iface) => {
                for method in &iface.methods {
                    out.insert(method.name.text.clone());
                }
            }
            _ => {}
        }
    }
    // Fixed-point closure: a method that calls a `&mut self`-style
    // method (one already in `out`) on a `this`-rooted receiver
    // must itself be `&mut self`. Iterate until no new additions.
    loop {
        let mut added = false;
        for item in &unit.items {
            if let TopLevelDecl::Class(class) = item {
                for method in &class.methods {
                    if out.contains(method.name.text.as_str()) {
                        continue;
                    }
                    let Some(body) = &method.body else { continue };
                    if body_calls_mut_method_on_this(body, &out) {
                        out.insert(method.name.text.clone());
                        added = true;
                    }
                }
                for ctor in &class.constructors {
                    // Constructors don't have a "name in user_mut_methods"
                    // (they're keyed by class), but their bodies are
                    // emitted via the same `&mut self` path —
                    // already handled by ctor emission.
                    let _ = ctor;
                }
            }
        }
        if !added {
            break;
        }
    }
    out
}

/// True iff `block` contains a method call whose receiver expression
/// is rooted at `this` (e.g. `this.method(...)`, `this.field.method(...)`)
/// AND the called method's name is in `mut_methods`. Used by the
/// cascade-aware mutation pass — if such a call exists in a method
/// body, that method also needs `&mut self`.
pub(crate) fn body_calls_mut_method_on_this(
    block: &Block,
    mut_methods: &HashSet<String>,
) -> bool {
    for stmt in &block.statements {
        if stmt_calls_mut_method_on_this(stmt, mut_methods) {
            return true;
        }
    }
    false
}

fn stmt_calls_mut_method_on_this(stmt: &Stmt, mut_methods: &HashSet<String>) -> bool {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) => expr_calls_mut_method_on_this(e, mut_methods),
        Stmt::Return(None) => false,
        Stmt::VarDecl(v) => v
            .init
            .as_ref()
            .map(|e| expr_calls_mut_method_on_this(e, mut_methods))
            .unwrap_or(false),
        Stmt::Assign(a) => {
            expr_calls_mut_method_on_this(&a.target, mut_methods)
                || expr_calls_mut_method_on_this(&a.value, mut_methods)
        }
        Stmt::If(if_stmt) => {
            if let Expr::Binary(_) | Expr::Unary(_) | Expr::Path(_) = &if_stmt.condition {
                // condition rarely matters; fall through
            }
            if expr_calls_mut_method_on_this(&if_stmt.condition, mut_methods) {
                return true;
            }
            if body_calls_mut_method_on_this(&if_stmt.then_block, mut_methods) {
                return true;
            }
            // Walk else chain.
            let mut cursor = if_stmt.else_branch.as_deref();
            while let Some(b) = cursor {
                match b {
                    ElseBranch::If(inner) => {
                        if body_calls_mut_method_on_this(&inner.then_block, mut_methods) {
                            return true;
                        }
                        cursor = inner.else_branch.as_deref();
                    }
                    ElseBranch::Block(block) => {
                        return body_calls_mut_method_on_this(block, mut_methods);
                    }
                }
            }
            false
        }
        Stmt::While(w) => {
            expr_calls_mut_method_on_this(&w.condition, mut_methods)
                || body_calls_mut_method_on_this(&w.body, mut_methods)
        }
        Stmt::ForEach(f) => {
            expr_calls_mut_method_on_this(&f.iter, mut_methods)
                || body_calls_mut_method_on_this(&f.body, mut_methods)
        }
        Stmt::SuperCall(args, _) => args
            .iter()
            .any(|a| expr_calls_mut_method_on_this(a, mut_methods)),
        Stmt::Throw(e, _) => expr_calls_mut_method_on_this(e, mut_methods),
        Stmt::Try(t) => {
            if body_calls_mut_method_on_this(&t.body, mut_methods) {
                return true;
            }
            for c in &t.catches {
                if body_calls_mut_method_on_this(&c.body, mut_methods) {
                    return true;
                }
            }
            if let Some(fin) = &t.finally {
                if body_calls_mut_method_on_this(fin, mut_methods) {
                    return true;
                }
            }
            false
        }
        Stmt::Break(_) | Stmt::Continue(_) => false,
    }
}

fn expr_calls_mut_method_on_this(expr: &Expr, mut_methods: &HashSet<String>) -> bool {
    match expr {
        Expr::Call(c) => {
            if let Expr::Field(f) = &*c.callee {
                if receiver_root_is_this(&f.object) && mut_methods.contains(f.field.text.as_str()) {
                    return true;
                }
            }
            // Recurse into args / callee for chained / nested calls.
            if expr_calls_mut_method_on_this(&c.callee, mut_methods) {
                return true;
            }
            c.args
                .iter()
                .any(|a| expr_calls_mut_method_on_this(a, mut_methods))
        }
        Expr::Field(f) => expr_calls_mut_method_on_this(&f.object, mut_methods),
        Expr::Binary(b) => {
            expr_calls_mut_method_on_this(&b.left, mut_methods)
                || expr_calls_mut_method_on_this(&b.right, mut_methods)
        }
        Expr::Unary(u) => expr_calls_mut_method_on_this(&u.operand, mut_methods),
        Expr::Cast(c) => expr_calls_mut_method_on_this(&c.value, mut_methods),
        Expr::Index(i) => {
            expr_calls_mut_method_on_this(&i.array, mut_methods)
                || expr_calls_mut_method_on_this(&i.index, mut_methods)
        }
        Expr::Elvis(e) => {
            expr_calls_mut_method_on_this(&e.value, mut_methods)
                || expr_calls_mut_method_on_this(&e.fallback, mut_methods)
        }
        Expr::Ternary(t) => {
            expr_calls_mut_method_on_this(&t.condition, mut_methods)
                || expr_calls_mut_method_on_this(&t.then_branch, mut_methods)
                || expr_calls_mut_method_on_this(&t.else_branch, mut_methods)
        }
        Expr::InterpString(s) => s.segments.iter().any(|seg| match seg {
            juxc_ast::InterpSegment::Expr(e) => expr_calls_mut_method_on_this(e, mut_methods),
            _ => false,
        }),
        Expr::NewObject(n) => n
            .args
            .iter()
            .any(|a| expr_calls_mut_method_on_this(a, mut_methods)),
        // `await expr` — walk into the operand.
        Expr::Await(inner, _) => expr_calls_mut_method_on_this(inner, mut_methods),
        _ => false,
    }
}

fn receiver_root_is_this(expr: &Expr) -> bool {
    match expr {
        Expr::This(_) => true,
        Expr::Field(f) => receiver_root_is_this(&f.object),
        Expr::Call(c) => receiver_root_is_this(&c.callee),
        Expr::Index(i) => receiver_root_is_this(&i.array),
        _ => false,
    }
}

// ============================================================================
// Auto-derive eligibility (§O.3 — auto-derivation for value types)
// ============================================================================
//
// Records and enums get every applicable operator for free from their
// structure. The Rust mapping uses `derive(...)` for the common cases
// (`PartialEq`, `Eq`, `Hash`, `Clone`, `Copy`). Eligibility depends on
// the field/payload types — Rust won't derive `Eq` on a struct with a
// `f32` field, for instance. These helpers walk a `TypeRef` and answer:
// can this type participate in `Eq` / `Hash` / `Copy`?
//
// Conservative whenever shape is ambiguous (user-defined classes,
// unresolved generics) — being optimistic risks `derive` failures at
// rustc time, and a missing derive is recoverable while a spurious one
// breaks the build.

/// True iff `name` is a Jux primitive whose Rust mapping is `Copy`
/// **and** `Eq` (i.e., every primitive except the IEEE-754 floats and
/// — by intent — the platform-sized `int`/`uint` which are still `Eq`,
/// just listed explicitly so the set stays auditable).
fn is_copy_eq_primitive(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "byte"
            | "ubyte"
            | "short"
            | "ushort"
            | "int"
            | "uint"
            | "long"
            | "ulong"
            | "char"
            | "i8"
            | "u8"
            | "i16"
            | "u16"
            | "i32"
            | "u32"
            | "i64"
            | "u64",
    )
}

/// True iff `name` is a Jux float primitive — `Copy` but **not** `Eq`
/// or `Hash` in Rust.
fn is_float_primitive(name: &str) -> bool {
    matches!(name, "float" | "double" | "f32" | "f64")
}

/// True if a field of type `ty` is **Copy**-compatible. Records can
/// derive `Copy` iff every field qualifies.
///
/// Rules (conservative — anything we can't statically prove disqualifies):
/// - Arrays: never. Dynamic arrays lower to `Vec<T>` (not `Copy`); we
///   skip fixed arrays too since their Copy-ness depends on the element
///   AND on `T: Copy` propagation through user types.
/// - Nullable types (`T?`): never — they lower to `Option<T>` which is
///   `Copy` only if the inner is. Be conservative.
/// - Single-segment primitive name: `Copy` iff numeric/bool/char/float.
///   `String` is **not** `Copy`.
/// - Anything else (user types, multi-segment paths, generic params,
///   unresolved): not `Copy`.
pub(crate) fn field_supports_copy(ty: &juxc_ast::TypeRef) -> bool {
    if ty.array_shape.is_some() || ty.nullable {
        return false;
    }
    if !ty.generic_args.is_empty() || ty.name.segments.len() != 1 {
        return false;
    }
    let name = ty.name.segments[0].text.as_str();
    is_copy_eq_primitive(name) || is_float_primitive(name)
}

/// True if a field of type `ty` is **Eq**-compatible. Records can
/// derive `Eq` iff every field qualifies. `Hash` follows the same
/// eligibility rule — every Eq-eligible type we recognize is also
/// `Hash` in std.
///
/// Rules:
/// - Floats disqualify (`f32`/`f64` are `PartialEq` only).
/// - Other primitives qualify.
/// - `String` qualifies (Rust's `String` is `Eq` + `Hash`).
/// - Arrays: qualify iff the element does — `Vec<T>` and `[T; N]` are
///   both `Eq + Hash` when `T` is.
/// - Nullable types qualify when the inner does (`Option<T>` is `Eq`
///   when `T` is).
/// - User types, generic args, multi-segment paths: conservatively
///   **disqualify**. A future turn can grow a real symbol-table walk
///   to recognize Eq-bearing user types.
pub(crate) fn field_supports_eq(ty: &juxc_ast::TypeRef) -> bool {
    if let Some(_shape) = &ty.array_shape {
        // Construct a non-array view of the same element and recurse.
        let element = juxc_ast::TypeRef {
            name: ty.name.clone(),
            generic_args: ty.generic_args.clone(),
            nullable: ty.nullable,
            array_shape: None,
            fn_shape: ty.fn_shape.clone(),
            span: ty.span,
        };
        return field_supports_eq(&element);
    }
    if ty.nullable {
        let inner = juxc_ast::TypeRef {
            name: ty.name.clone(),
            generic_args: ty.generic_args.clone(),
            nullable: false,
            array_shape: None,
            fn_shape: ty.fn_shape.clone(),
            span: ty.span,
        };
        return field_supports_eq(&inner);
    }
    if ty.name.segments.len() != 1 || !ty.generic_args.is_empty() {
        return false;
    }
    let name = ty.name.segments[0].text.as_str();
    if name == "String" {
        return true;
    }
    is_copy_eq_primitive(name)
}

/// True if a field of type `ty` is `Hash`-compatible. Identical to the
/// `Eq` predicate for the types we recognize — std's `Hash` impls
/// cover the same set.
pub(crate) fn field_supports_hash(ty: &juxc_ast::TypeRef) -> bool {
    field_supports_eq(ty)
}

/// True if a field of type `ty` is `Default`-compatible (every Jux
/// primitive and `String` implements `Default`, arrays of those do
/// too via `Vec::default()` / array-of-Default initialization, and
/// nullable wraps default to `None`). Records derive `Default` when
/// every component qualifies so a class that stores a record-typed
/// field without an explicit initializer can still flow through
/// the struct-init shim's `Default::default()` fallback.
pub(crate) fn field_supports_default(ty: &juxc_ast::TypeRef) -> bool {
    if ty.nullable {
        // `Option<T>::default()` is `None` regardless of T.
        return true;
    }
    if ty.array_shape.is_some() {
        // Dynamic arrays lower to `Vec<T>` which is always `Default`;
        // fixed-size `[T; N]` needs T: Default + Copy — the existing
        // const-context paths take care of constant N initializers,
        // so we conservatively require the element to qualify.
        let element = juxc_ast::TypeRef {
            name: ty.name.clone(),
            generic_args: ty.generic_args.clone(),
            nullable: false,
            array_shape: None,
            fn_shape: ty.fn_shape.clone(),
            span: ty.span,
        };
        return field_supports_default(&element);
    }
    if ty.name.segments.len() != 1 || !ty.generic_args.is_empty() {
        return false;
    }
    let name = ty.name.segments[0].text.as_str();
    if name == "String" {
        return true;
    }
    is_copy_eq_primitive(name) || is_float_primitive(name)
}

/// True if a field of type `ty` is `Display`-compatible — used to
/// decide whether to emit the auto-derived `operator string` impl
/// (§O.3.1) for the enclosing record/enum.
///
/// Rules (conservative — same shape as the other predicates):
/// - Primitives and `String`: yes (every Rust primitive plus `String`
///   implements `Display`).
/// - Arrays: no (`Vec<T>` and `[T; N]` don't implement `Display`).
/// - Nullable types: no (`Option<T>` doesn't implement `Display`).
/// - Generic params, user types, multi-segment paths: conservatively
///   **disqualify**. A future turn with a real symbol-table walk could
///   recognize Display-bearing user types and add a `T: Display` bound
///   to the generated `impl`; today we skip the impl entirely so we
///   never emit Rust that fails to compile.
pub(crate) fn field_supports_display(ty: &juxc_ast::TypeRef) -> bool {
    if ty.array_shape.is_some() || ty.nullable {
        return false;
    }
    if ty.name.segments.len() != 1 || !ty.generic_args.is_empty() {
        return false;
    }
    let name = ty.name.segments[0].text.as_str();
    name == "String" || is_copy_eq_primitive(name) || is_float_primitive(name)
}

/// True if `ty` is exactly the Jux primitive `String` — single-segment
/// path named "String", no generic args, no array shape, not nullable.
pub(crate) fn is_jux_string_type(ty: &juxc_ast::TypeRef) -> bool {
    ty.array_shape.is_none()
        && !ty.nullable
        && ty.generic_args.is_empty()
        && ty.name.segments.len() == 1
        && ty.name.segments[0].text == "String"
}

/// Alias for [`is_jux_string_type`] exported under the name some
/// Phase-H call sites prefer for readability. Kept around as a
/// stable handle even though the post-Fix-1 backend no longer
/// performs the `&str → String` coercion this was originally
/// introduced for.
#[allow(dead_code)]
pub(crate) fn is_jux_string_type_ref(ty: &juxc_ast::TypeRef) -> bool {
    is_jux_string_type(ty)
}

impl crate::RustEmitter {
    /// True iff `expr`'s emitted Rust value is already
    /// `Option<T>`-shaped — meaning no additional `Some(...)` wrap
    /// is needed when feeding it into a `T?` slot. Recognized
    /// shapes:
    ///
    /// - `null` literal → already `None`.
    /// - A `Path` (single-segment) to a binding we've tagged
    ///   nullable via [`Self::nullable_locals`].
    /// - A `Call` to a known function / method whose declared
    ///   return type is `T?`.
    /// - A `Field` access on a known class/record whose field's
    ///   declared type is `T?`.
    /// - An `Elvis` expression — `?:` / `??` produces a non-null
    ///   `T`, never an `Option`, so it must STILL be wrapped. So
    ///   Elvis returns `false` here.
    /// - A safe-call `obj?.field` / `obj?.method()` produces an
    ///   `Option<T>` by construction; recognized as nullable.
    ///
    /// Conservative on the no-info side: anything we can't classify
    /// returns `false`, meaning the caller wraps. Over-wrapping a
    /// value that's actually nullable would produce
    /// `Some(Some(...))` — wrong type. Under-wrapping (the case
    /// here) produces `Some(plain_value)`, which is correct as
    /// long as the value really IS plain. The trade-off favors
    /// safety: returning `false` defaults to wrap, which is the
    /// safer direction.
    pub(crate) fn expression_is_already_nullable(&self, expr: &juxc_ast::Expr) -> bool {
        // **Path queries** (single-segment ident) consult
        // `nullable_locals` as the source of truth so that
        // smart-cast removal (`emit_if`'s `if (x != null) { … }`
        // pops the binding from the set for the body) takes
        // effect. The tycheck-recorded `expr_types[span]` would
        // still say the binding is nullable at the AST level —
        // we override that for the inside of the smart-cast
        // block. Other expression shapes fall through to the
        // `expr_types` consult below.
        if let juxc_ast::Expr::Path(qn) = expr {
            if qn.segments.len() == 1 {
                return self.nullable_locals.contains(&qn.segments[0].text);
            }
        }
        // For non-Path shapes, ask tycheck directly. With the
        // `Ty::Nullable` refactor, every expression visited by
        // `check::Checker` carries its full type — including the
        // nullable wrap — in `expr_types[span]`. Reading this
        // answer is more precise than the syntactic fallback
        // paths below.
        if let Some(juxc_tycheck::Ty::Nullable(_)) =
            self.expr_types.get(&crate::exprs::expr_span_of(expr))
        {
            return true;
        }
        match expr {
            juxc_ast::Expr::Literal(juxc_ast::Literal::Null) => true,
            juxc_ast::Expr::Path(qn) => {
                qn.segments.len() == 1
                    && self.nullable_locals.contains(&qn.segments[0].text)
            }
            juxc_ast::Expr::Call(c) => {
                // Single-segment callee: top-level fn whose return
                // type tells us the result's shape.
                if let juxc_ast::Expr::Path(qn) = &*c.callee {
                    if qn.segments.len() == 1 {
                        if let Some(f) = self.symbols.functions.get(&qn.segments[0].text) {
                            return matches!(
                                &f.return_type,
                                juxc_ast::ReturnType::Type(t) if t.nullable
                            );
                        }
                    }
                }
                // Method call: receiver-typed lookup is a tycheck
                // job. Without the dynamic dispatch we treat the
                // result as non-nullable; conservative.
                false
            }
            juxc_ast::Expr::Field(f) => {
                // `?.` produces `Option<T>` regardless of what
                // `field`'s declared type is — safe-nav semantics.
                if f.safe {
                    return true;
                }
                // Plain `obj.field`: look up the receiver's class
                // via tycheck's `expr_types`, then ask the class
                // signature whether the field is `T?`. The lookup
                // mirrors `assign_target_is_nullable` but as a
                // read-side query — Phase 1 only checks the
                // immediate class, not the inheritance chain.
                let Some(juxc_tycheck::Ty::User { name, .. }) =
                    self.expr_types.get(&crate::exprs::expr_span_of(&f.object))
                else {
                    return false;
                };
                if let Some(class) = self.symbols.classes.get(name) {
                    if let Some(field) = class.fields.get(&f.field.text) {
                        return field.ty.nullable;
                    }
                }
                if let Some(record) = self.symbols.records.get(name) {
                    if let Some(c) =
                        record.components.iter().find(|c| c.name == f.field.text)
                    {
                        return c.ty.nullable;
                    }
                }
                false
            }
            // Elvis returns the non-null inner — wrap when feeding
            // into a `T?` slot.
            juxc_ast::Expr::Elvis(_) => false,
            _ => false,
        }
    }

    /// Return the declared `nullable` flag of the *positional*
    /// parameter at `arg_idx` of `callee`, when we can figure out
    /// which function/method the call resolves to. `None` means
    /// "unknown" — caller should NOT wrap, since we can't tell
    /// whether the slot is `T?` or `T`.
    ///
    /// Recognized callees:
    /// - Single-segment `Path` → top-level function in
    ///   `symbols.functions`.
    /// - `Field` over a `Path` that resolves to a class, and the
    ///   field name matches a known method → static or instance
    ///   method. The class lookup uses
    ///   [`Self::path_resolves_to_class_in_emit`]; same code path
    ///   the static-method call routing uses.
    ///
    /// Instance methods on non-Path receivers (e.g. `foo().bar(x)`)
    /// would need tycheck's per-expression type to identify the
    /// receiver's class; left as a future refinement.
    pub(crate) fn callee_param_is_nullable(
        &self,
        callee: &juxc_ast::Expr,
        arg_idx: usize,
    ) -> bool {
        // Top-level fn: `f(...)`.
        if let juxc_ast::Expr::Path(qn) = callee {
            if qn.segments.len() == 1 {
                if let Some(f) = self.symbols.functions.get(&qn.segments[0].text) {
                    return f
                        .params
                        .get(arg_idx)
                        .map(|p| p.ty.nullable)
                        .unwrap_or(false);
                }
            }
        }
        // Static or instance method: `Receiver.method(...)`.
        if let juxc_ast::Expr::Field(f) = callee {
            if let juxc_ast::Expr::Path(qn) = &*f.object {
                if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                    if let Some(class) = self.symbols.classes.get(&class_fqn) {
                        if let Some(m) = class.methods.get(f.field.text.as_str()) {
                            return m
                                .params
                                .get(arg_idx)
                                .map(|p| p.ty.nullable)
                                .unwrap_or(false);
                        }
                    }
                }
            }
        }
        false
    }

    /// Look up the i-th formal parameter's declared type ref for
    /// the given callee expression. Mirrors
    /// [`Self::callee_param_is_nullable`] but returns the whole
    /// `TypeRef` so the caller can compare it against the arg's
    /// actual type for upcast detection. `None` when the callee
    /// can't be resolved or doesn't have a parameter at that
    /// position.
    pub(crate) fn callee_param_type(
        &self,
        callee: &juxc_ast::Expr,
        arg_idx: usize,
    ) -> Option<juxc_ast::TypeRef> {
        if let juxc_ast::Expr::Path(qn) = callee {
            if qn.segments.len() == 1 {
                if let Some(f) = self.symbols.functions.get(&qn.segments[0].text) {
                    return f.params.get(arg_idx).map(|p| p.ty.clone());
                }
            }
        }
        if let juxc_ast::Expr::Field(f) = callee {
            // Static method call: `ClassName.method(args)` — the
            // receiver path resolves to a class.
            if let juxc_ast::Expr::Path(qn) = &*f.object {
                if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                    if let Some(class) = self.symbols.classes.get(&class_fqn) {
                        if let Some(m) = class.methods.get(f.field.text.as_str()) {
                            return m.params.get(arg_idx).map(|p| p.ty.clone());
                        }
                    }
                }
            }
            // Instance method call: `recv.method(args)` where `recv`
            // is a value of some user class (`t.step(new Red(30))`).
            // Resolve the receiver's inferred type to its class, then
            // walk the `extends` chain for the method — so the param
            // type drives a sealed-upcast `.into()` wrap on the arg.
            // This is the path that fixes passing a permitted subclass
            // value (`Red`) into a sealed-parent param (`Light`).
            if let Some(juxc_tycheck::Ty::User { name, .. }) =
                self.expr_types.get(&crate::exprs::expr_span_of(&f.object))
            {
                let bare = name.rsplit('.').next().unwrap_or(name.as_str());
                let mut cursor: Option<String> = Some(bare.to_string());
                let mut depth = 0usize;
                while let Some(cname) = cursor {
                    if depth > 64 {
                        break;
                    }
                    let Some(class) = self.lookup_class_by_bare_or_fqn(&cname) else {
                        break;
                    };
                    if let Some(m) = class.methods.get(f.field.text.as_str()) {
                        return m.params.get(arg_idx).map(|p| p.ty.clone());
                    }
                    cursor = class
                        .extends
                        .as_ref()
                        .and_then(|t| t.name.segments.last().map(|s| s.text.clone()));
                    depth += 1;
                }
            }
        }
        None
    }

    /// True when `expr` would flow into a slot declared as
    /// `target_ty` and that slot is a sealed parent whose
    /// permits list names the expression's inferred class. Used
    /// by [`Self::arg_needs_sealed_upcast`] and the return /
    /// variable-init upcast paths. Factored out so the matching
    /// logic stays in one place.
    pub(crate) fn expr_needs_sealed_upcast_to(
        &self,
        target_ty: &juxc_ast::TypeRef,
        expr: &juxc_ast::Expr,
    ) -> bool {
        let Some(target_bare) = target_ty.name.segments.last().map(|s| s.text.as_str())
        else {
            return false;
        };
        let Some(parent_class) = self.lookup_class_by_bare_or_fqn(target_bare) else {
            return false;
        };
        let arg_span = crate::exprs::expr_span_of(expr);
        let Some(arg_ty) = self.expr_types.get(&arg_span) else {
            return false;
        };
        let arg_bare = match arg_ty {
            juxc_tycheck::Ty::User { name, .. } => name.split('.').next_back().unwrap_or(name),
            _ => return false,
        };
        if arg_bare == target_bare {
            return false;
        }
        // Sealed parent: arg must be in the explicit `permits`
        // list. The `From<Sub> for Sealed` impl wraps the
        // subclass into the matching enum variant.
        if parent_class.is_sealed {
            return parent_class.permits.iter().any(|p| p.as_str() == arg_bare);
        }
        // Non-sealed open parent: the auto-emitted
        // `From<Sub> for Parent` impl (see `emit_class_decl`)
        // extracts the parent slice via `.__parent`. Phase-1
        // caveat — this drops subclass identity at the boundary,
        // so an overridden method on the subclass doesn't fire
        // after the upcast. Use a sealed hierarchy for full
        // virtual dispatch.
        if let Some(arg_class) = self.lookup_class_by_bare_or_fqn(arg_bare) {
            if let Some(extends_fqn) = arg_class.extends_fqn.as_deref() {
                let parent_seg = extends_fqn
                    .rsplit('.')
                    .next()
                    .unwrap_or(extends_fqn);
                return parent_seg == target_bare;
            }
        }
        false
    }

    /// True when the enclosing function's return type is a sealed
    /// parent of the expression's inferred type — the return value
    /// needs `.into()` so the auto-`From<Sub> for Sealed` impl
    /// wraps the subclass into the matching enum variant before
    /// the value crosses the function boundary.
    pub(crate) fn return_needs_sealed_upcast(&self, expr: &juxc_ast::Expr) -> bool {
        let target_ty = match &self.current_return_type {
            Some(juxc_ast::ReturnType::Type(t)) => t.clone(),
            Some(juxc_ast::ReturnType::AsyncType(t)) => t.clone(),
            _ => return false,
        };
        self.expr_needs_sealed_upcast_to(&target_ty, expr)
    }

    /// True when the arg at `arg_idx` would be flowing into a slot
    /// of a SEALED parent class whose declared type differs from
    /// the arg's inferred type — i.e. a Java-style upcast site
    /// where we need to wrap the value via `.into()` so the emitted
    /// `From<Sub> for Sealed` impl lifts the subclass into the
    /// matching enum variant.
    ///
    /// Returns false (no wrap) when:
    ///   - The callee can't be resolved (defensive).
    ///   - The param's declared type isn't a user-defined class.
    ///   - The arg's inferred type already matches the param.
    ///   - The arg's class isn't a permitted subclass of the
    ///     param's sealed parent.
    pub(crate) fn arg_needs_sealed_upcast(
        &self,
        callee: &juxc_ast::Expr,
        arg_idx: usize,
        arg: &juxc_ast::Expr,
    ) -> bool {
        let Some(param_ty) = self.callee_param_type(callee, arg_idx) else {
            return false;
        };
        // Delegate to the general "param-typed slot wants `.into()`"
        // predicate, which now handles both sealed (variant wrap)
        // and non-sealed (`__parent` extraction) upcasts.
        self.expr_needs_sealed_upcast_to(&param_ty, arg)
    }

    /// Wrap `arg` in `Some(arg)` if the target slot wants a
    /// nullable value and the expression isn't already
    /// `Option<T>`-shaped. Emits the wrapping parentheses; the
    /// caller calls `emit_expr` between the `Some(` and the `)`
    /// or — to keep call sites readable — relies on
    /// [`Self::emit_arg_with_nullable_wrap`] which does the whole
    /// arg emission in one call.
    pub(crate) fn emit_arg_with_nullable_wrap(
        &mut self,
        arg: &juxc_ast::Expr,
        target_is_nullable: bool,
    ) {
        let already_nullable = self.expression_is_already_nullable(arg);
        let wrap = target_is_nullable && !already_nullable;
        if wrap {
            self.w.push_str("Some(");
        }
        self.emit_expr(arg);
        if wrap {
            self.w.push(')');
            return;
        }
        // Auto-clone when forwarding a *nullable local* into a
        // call's nullable arg slot. Rust's `Option<T>` isn't
        // `Copy`, so passing the bare path would move the local
        // — preventing further use by the surrounding function.
        // The clone restores Java-shape "the call borrows my
        // handle" ergonomics. Only fires for single-segment path
        // arguments referring to a binding in `nullable_locals`
        // — call-result paths (`f()`) and `?.` chains are fresh
        // values where cloning is wasteful.
        if target_is_nullable && already_nullable {
            if let juxc_ast::Expr::Path(qn) = arg {
                if qn.segments.len() == 1
                    && self.nullable_locals.contains(&qn.segments[0].text)
                {
                    self.w.push_str(".clone()");
                }
            }
        }
    }

    /// Emit `arg` as the body of a `println!` / `format!` value
    /// slot. When `arg` has nullable shape (Option<T>), wrap in
    /// the prelude's `JuxOpt(&value)` adapter so Display works —
    /// `Some(v)` prints as `v`, `None` as `"null"`. Non-nullable
    /// args pass straight through.
    pub(crate) fn emit_format_arg(&mut self, arg: &juxc_ast::Expr) {
        if self.expression_is_already_nullable(arg) {
            self.w.push_str("JuxOpt(&");
            self.emit_expr(arg);
            self.w.push(')');
        } else {
            self.emit_expr(arg);
        }
    }
}

/// True if `name`'s first character is an uppercase letter. Used by
/// [`RustEmitter::emit_sizeof_path`] to classify a bare identifier as
/// type-form (PascalCase) vs. value-form (camelCase) per §5.9.3.
///
/// Names starting with `_` or a digit don't apply — they aren't valid
/// identifier starts per §A.1.3 anyway, so this can only see real
/// letter-led identifiers.
pub(crate) fn starts_with_uppercase(name: &str) -> bool {
    name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
}

// ============================================================================
// Wildcard generic-arg lifting — backend Phase 1 strategy for PECS
// ============================================================================

/// State threaded through [`lift_wildcards_in_type_ref`] across the
/// params of a single function/method. Each wildcard encountered is
/// replaced by a fresh synthetic type-param name (`__W0`, `__W1`, …)
/// and its bound is recorded so the caller can extend the
/// declaration-site generic-params list.
///
/// Phase-1 lowering rule: a wildcard in a parameter-position type
/// reads as `<__Wn: Bound + Clone>` on the enclosing function. Mimics
/// what Java itself does after type erasure — Java's `void
/// f(List<? extends Animal> xs)` and Rust's
/// `fn f<__W: AnimalKind + Clone>(xs: List<__W>)` are isomorphic.
///
/// Wildcards in storage positions (locals, fields, return types) are
/// not lifted here — tycheck flags those with a placeholder
/// diagnostic until a proper `Box<dyn>`-erasure strategy lands.
pub(crate) struct WildcardLifter {
    /// Synthetic TypeParams produced during the rewrite, in
    /// declaration order. Caller concatenates them after the
    /// function's own generic params.
    pub new_params: Vec<TypeParam>,
    /// Counter for the next `__Wn` name. Bumped on each fresh
    /// wildcard regardless of bound shape.
    next: usize,
}

impl WildcardLifter {
    pub(crate) fn new() -> Self {
        Self {
            new_params: Vec::new(),
            next: 0,
        }
    }

    /// Recursively walk `ty`, replacing each `GenericArg::Wildcard`
    /// with a freshly-named `GenericArg::Type` referencing a
    /// synthetic param. Returns the rewritten `TypeRef` (cloning
    /// only the spine — concrete subterms are kept by reference via
    /// `clone()`).
    pub(crate) fn rewrite_type_ref(&mut self, ty: &TypeRef) -> TypeRef {
        let generic_args = ty
            .generic_args
            .iter()
            .map(|arg| match arg {
                GenericArg::Type(inner) => GenericArg::Type(self.rewrite_type_ref(inner)),
                GenericArg::Wildcard(w) => GenericArg::Type(self.synthesize(&w.bound)),
            })
            .collect();
        TypeRef {
            name: ty.name.clone(),
            generic_args,
            nullable: ty.nullable,
            array_shape: ty.array_shape.clone(),
            fn_shape: ty.fn_shape.clone(),
            span: ty.span,
        }
    }

    /// Mint a fresh `__Wn` TypeParam with the wildcard's bound and
    /// return a TypeRef pointing at it. `? super B` collapses to the
    /// same shape as `? extends B` here — Rust generics can't
    /// express "supertype of B" directly, and Phase 1 treats the
    /// bound as a marker constraint either way. (Tycheck still
    /// enforces the variance distinction via PECS in `compatible`.)
    fn synthesize(&mut self, bound: &Option<WildcardBound>) -> TypeRef {
        let name = format!("__W{}", self.next);
        self.next += 1;
        let bounds: Vec<TypeRef> = match bound {
            None => Vec::new(),
            Some(WildcardBound::Extends(b)) => vec![b.clone()],
            Some(WildcardBound::Super(b)) => vec![b.clone()],
        };
        let ident = Ident {
            text: name.clone(),
            span: Span::DUMMY,
        };
        self.new_params.push(TypeParam {
            name: ident.clone(),
            bounds,
            span: Span::DUMMY,
        });
        TypeRef {
            name: QualifiedName {
                segments: vec![ident],
                span: Span::DUMMY,
            },
            generic_args: Vec::new(),
            nullable: false,
            array_shape: None,
            fn_shape: None,
            span: Span::DUMMY,
        }
    }
}

/// True iff `ty` contains a wildcard anywhere in its generic-arg
/// tree. Cheap guard so call sites can skip the rewrite allocation
/// for the overwhelmingly-common no-wildcards case.
pub(crate) fn type_ref_has_wildcard(ty: &TypeRef) -> bool {
    ty.generic_args.iter().any(|arg| match arg {
        GenericArg::Type(inner) => type_ref_has_wildcard(inner),
        GenericArg::Wildcard(_) => true,
    })
}
