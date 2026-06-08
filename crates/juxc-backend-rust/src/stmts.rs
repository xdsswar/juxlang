//! Statement-level lowering — blocks, var decls, control flow, assignment.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{
    AssignStmt, Block, ElseBranch, Expr, ForEachStmt, IfStmt, Literal, Stmt, VarDecl, WhileStmt,
};
use juxc_source::Span;
use juxc_tycheck::Ty;

use crate::exprs::expr_span_of;
use crate::RustEmitter;

/// True when `e` is the AST `null` literal — used to decide
/// whether a value flowing into a nullable slot needs the
/// `Some(...)` wrap or is already `None`.
fn is_null_literal(e: &Expr) -> bool {
    matches!(e, Expr::Literal(Literal::Null))
}

/// True when the loop body somewhere consumes `name` — uses it in
/// a position that needs an OWNED `T` rather than just a `&T`.
/// Drives the for-each lowering: when this returns `false`, we
/// can iterate by reference and skip the per-iteration clone.
///
/// Considered "moves":
/// - `var y = x;` (the init is exactly `Path(x)`)
/// - `f(x)`, `obj.method(x)`, `new T(x)` — fn / method / ctor arg
/// - `return x;`
/// - `obj.field = x;` — assign rhs
/// - `super(x);` — super-constructor arg
/// - `x as T` — cast operand (cast consumes the value)
/// - Inside an array literal: `[x, ...]`
///
/// NOT considered moves:
/// - `x.method()`, `x.field` — read through borrow
/// - `x == y`, `x != y` — comparisons borrow
/// - `format!`/`println!` args — borrow via `Display`
/// - `if (x)` / `while (x)` — bool conditions borrow
///
/// The walker is conservative: any uncertainty returns `true` so
/// the loop falls back to the clone form, which always compiles.
fn body_moves_path(block: &Block, name: &str) -> bool {
    for stmt in &block.statements {
        if stmt_moves_path(stmt, name) {
            return true;
        }
    }
    false
}

fn stmt_moves_path(stmt: &Stmt, name: &str) -> bool {
    match stmt {
        Stmt::Expr(e) => expr_moves_path_at_top(e, name),
        Stmt::VarDecl(v) => v.init.as_ref().map_or(false, |e| is_path_named(e, name) || expr_moves_path_at_top(e, name)),
        Stmt::Return(opt) => opt.as_ref().map_or(false, |e| is_path_named(e, name) || expr_moves_path_at_top(e, name)),
        Stmt::Assign(a) => {
            is_path_named(&a.value, name)
                || expr_moves_path_at_top(&a.value, name)
                || expr_moves_path_at_top(&a.target, name)
        }
        Stmt::If(s) => {
            expr_moves_path_at_top(&s.condition, name)
                || body_moves_path(&s.then_block, name)
                || else_branch_moves_path(s.else_branch.as_deref(), name)
        }
        Stmt::While(s) => {
            expr_moves_path_at_top(&s.condition, name)
                || body_moves_path(&s.body, name)
        }
        Stmt::ForEach(s) => {
            // A nested for-each that consumes the outer var
            // (`for y in xs` where xs shadows our name) is a move.
            // The shadowing case — same-named inner loop var —
            // can't appear in well-formed Jux source because the
            // resolver would see two scopes; if it does we
            // conservatively report a move.
            is_path_named(&s.iter, name)
                || expr_moves_path_at_top(&s.iter, name)
                || body_moves_path(&s.body, name)
        }
        Stmt::SuperCall(args, _) => {
            args.iter().any(|a| is_path_named(a, name) || expr_moves_path_at_top(a, name))
        }
        Stmt::Throw(e, _) => is_path_named(e, name) || expr_moves_path_at_top(e, name),
        Stmt::Try(t) => {
            if body_moves_path(&t.body, name) {
                return true;
            }
            for c in &t.catches {
                if body_moves_path(&c.body, name) {
                    return true;
                }
            }
            if let Some(fin) = &t.finally {
                if body_moves_path(fin, name) {
                    return true;
                }
            }
            false
        }
        Stmt::Break(_) | Stmt::Continue(_) => false,
    }
}

fn else_branch_moves_path(branch: Option<&ElseBranch>, name: &str) -> bool {
    let mut cursor = branch;
    while let Some(b) = cursor {
        match b {
            ElseBranch::If(inner) => {
                if expr_moves_path_at_top(&inner.condition, name)
                    || body_moves_path(&inner.then_block, name)
                {
                    return true;
                }
                cursor = inner.else_branch.as_deref();
            }
            ElseBranch::Block(block) => {
                return body_moves_path(block, name);
            }
        }
    }
    false
}

/// True iff `e` is exactly `Path(name)` — a bare reference to the
/// loop variable. Used at consume sites (var-decl init, return
/// value, assign rhs, call args) to detect "the whole expression
/// IS the loop var".
fn is_path_named(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Path(qn) => qn.segments.len() == 1 && qn.segments[0].text == name,
        _ => false,
    }
}

/// Recursive walker for "does this expression contain a consume
/// site for `name`?" Distinct from `is_path_named` — this walks
/// into sub-expressions looking for fn-call args, ctor args, etc.
/// that consume the loop var. Returns false for borrow-shaped
/// uses (`.method()`, `==`, format args) so the caller can
/// safely emit `for x in &xs`.
fn expr_moves_path_at_top(e: &Expr, name: &str) -> bool {
    match e {
        // Function / method call: each arg is a consume site
        // (passes by value). Method receivers (`x.method()`)
        // borrow via auto-deref, so we walk the callee for nested
        // consume shapes but don't treat the receiver itself as
        // moved.
        //
        // **Exception** — the builtin `print(...)` lowers to
        // `println!(...)` which borrows its args via `Display`,
        // so a bare path arg here doesn't move. The recognition
        // mirrors `emit_call`'s `print` dispatch: single-segment
        // path named `print`.
        Expr::Call(c) => {
            let is_print_builtin = matches!(
                &*c.callee,
                Expr::Path(qn) if qn.segments.len() == 1 && qn.segments[0].text == "print",
            );
            if is_print_builtin {
                // Args of `print` borrow; only walk for nested
                // consume shapes inside complex sub-expressions.
                return c.args.iter().any(|a| expr_moves_path_at_top(a, name));
            }
            for arg in &c.args {
                if is_path_named(arg, name) || expr_moves_path_at_top(arg, name) {
                    return true;
                }
            }
            // Walk callee for nested calls (`f(g(x))`).
            expr_moves_path_at_top(&c.callee, name)
        }
        Expr::NewObject(n) => n
            .args
            .iter()
            .any(|a| is_path_named(a, name) || expr_moves_path_at_top(a, name)),
        Expr::NewArray(n) => expr_moves_path_at_top(&n.size, name),
        Expr::NewArrayLit(n) => n
            .elements
            .iter()
            .any(|el| is_path_named(el, name) || expr_moves_path_at_top(el, name)),
        Expr::Cast(c) => is_path_named(&c.value, name) || expr_moves_path_at_top(&c.value, name),
        Expr::Binary(b) => {
            // String concat (`+` with a string operand) emits as
            // `format!` which borrows — no move. Other binaries
            // are arithmetic/comparison which also borrow for
            // `String`/user types via the trait method. So walk
            // for nested calls but don't treat top-level
            // operands as moves.
            expr_moves_path_at_top(&b.left, name)
                || expr_moves_path_at_top(&b.right, name)
        }
        Expr::Unary(u) => expr_moves_path_at_top(&u.operand, name),
        Expr::Range(r) => {
            expr_moves_path_at_top(&r.start, name) || expr_moves_path_at_top(&r.end, name)
        }
        Expr::Index(i) => {
            expr_moves_path_at_top(&i.array, name) || expr_moves_path_at_top(&i.index, name)
        }
        Expr::Field(f) => expr_moves_path_at_top(&f.object, name),
        Expr::InterpString(s) => s.segments.iter().any(|seg| match seg {
            // Bare-ident interp is a borrow (Display); no move.
            juxc_ast::InterpSegment::Literal(_) | juxc_ast::InterpSegment::Bare(_) => false,
            juxc_ast::InterpSegment::Expr(inner) => expr_moves_path_at_top(inner, name),
        }),
        Expr::Switch(s) => {
            if expr_moves_path_at_top(&s.scrutinee, name) {
                return true;
            }
            for arm in &s.arms {
                let arm_moves = match &arm.body {
                    juxc_ast::SwitchBody::Expr(e) => {
                        is_path_named(e, name) || expr_moves_path_at_top(e, name)
                    }
                    juxc_ast::SwitchBody::Block(b) => body_moves_path(b, name),
                };
                if arm_moves {
                    return true;
                }
            }
            false
        }
        Expr::Elvis(e) => {
            // Both sides of elvis are value-consuming via
            // `.unwrap_or(...)`, so a bare `Path(name)` on
            // either side is a move.
            is_path_named(&e.value, name)
                || expr_moves_path_at_top(&e.value, name)
                || is_path_named(&e.fallback, name)
                || expr_moves_path_at_top(&e.fallback, name)
        }
        Expr::Lambda(l) => match &l.body {
            // A lambda captures by value (the emitter wraps in
            // `move`), so any read of the loop var inside the
            // body is a move-capture.
            juxc_ast::LambdaBody::Expr(e) => is_path_named(e, name) || expr_moves_path_at_top(e, name),
            juxc_ast::LambdaBody::Block(b) => body_moves_path(b, name),
        },
        Expr::SizeOf(s) => expr_moves_path_at_top(&s.operand, name),
        Expr::Literal(_) | Expr::Path(_) | Expr::This(_) => false,
        // Method reference is a static expression — no sub-paths
        // referring to the loop variable.
        Expr::MethodRef(_) => false,
        // Ternary: both branches are value-consuming positions
        // (the surrounding context picks one). A bare `Path(name)`
        // on either side is a move.
        Expr::Ternary(t) => {
            expr_moves_path_at_top(&t.condition, name)
                || is_path_named(&t.then_branch, name)
                || expr_moves_path_at_top(&t.then_branch, name)
                || is_path_named(&t.else_branch, name)
                || expr_moves_path_at_top(&t.else_branch, name)
        }
        // `await expr` — the operand is the position that gets
        // evaluated, so any move semantics flow through it.
        Expr::Await(inner, _) => {
            is_path_named(inner, name) || expr_moves_path_at_top(inner, name)
        }
    }
}

impl RustEmitter {
    /// True iff the enclosing function's declared return type is
    /// `T?` (nullable) AND `expr` isn't itself a `null` literal —
    /// meaning the value flowing through `return …` is a `T`
    /// that needs the `Some(...)` lift to match the `Option<T>`
    /// declared return type.
    pub(crate) fn return_wants_some_wrap(&self, expr: &Expr) -> bool {
        let returns_nullable = match &self.current_return_type {
            Some(juxc_ast::ReturnType::Type(t)) => t.nullable,
            Some(juxc_ast::ReturnType::AsyncType(t)) => t.nullable,
            _ => false,
        };
        returns_nullable && !is_null_literal(expr)
    }
}

/// Match the Kotlin-style null-smart-cast head: `name != null`
/// where `name` is a bare single-segment path. Returns
/// `Some(name)` when the shape matches, else `None`. Used by
/// `emit_if` to lower the canonical null-guard
/// (`if (x != null) { … }`) to Rust's `if let Some(x) = x`
/// pattern — inside the block, `x` is the unwrapped inner type.
///
/// Composite lvalues (`obj.field != null`, `arr[i] != null`) are
/// intentionally NOT matched here. Lowering them to `if let
/// Some(name) = obj.field` introduces a fresh binding `name`
/// without giving the user a way to write it — they'd have to
/// stash the result into a local anyway. A future smart-cast
/// pass can extend this.
fn match_simple_not_null_check(cond: &Expr) -> Option<&str> {
    let Expr::Binary(b) = cond else { return None };
    if !matches!(b.op, juxc_ast::BinaryOp::NotEq) {
        return None;
    }
    // The non-null side must be a bare identifier (single-segment
    // path). The null side must be the `null` literal.
    let (target, other) = match (&*b.left, &*b.right) {
        (Expr::Literal(Literal::Null), other) => (other, &*b.left),
        (other, Expr::Literal(Literal::Null)) => (other, &*b.right),
        _ => return None,
    };
    let _ = other;
    if let Expr::Path(qn) = target {
        if qn.segments.len() == 1 {
            return Some(qn.segments[0].text.as_str());
        }
    }
    None
}

impl RustEmitter {
    /// Emit the body of a block — statements one per line, each indented.
    /// The enclosing `{ … }` is emitted by the caller so we can match
    /// either a function body or a nested block.
    ///
    /// **Indent contract.** Callers must `indent_inc()` *before* invoking
    /// this method (and `indent_dec()` after) so the writer's current
    /// depth matches the body depth — this method then emits a leading
    /// `emit_indent()` per statement and delegates to [`Self::emit_stmt`]
    /// for the statement text itself.
    pub(crate) fn emit_block_contents(&mut self, block: &Block) {
        for stmt in &block.statements {
            // Per-statement source-map marker (only when `source` is
            // attached on the emitter — see `lower_with_source`).
            // Goes ahead of the leading indent so rustc errors on the
            // emitted Rust can scan up to find the nearest `.jux`
            // line/col.
            self.emit_source_marker(stmt_span(stmt));
            self.w.emit_indent();
            self.emit_stmt(stmt);
        }
    }

    /// Emit a single statement. The writer's current indent level is
    /// the statement's depth — the caller is responsible for emitting
    /// the leading indent before the statement text starts (via
    /// [`Writer::emit_indent`]), and for bumping the writer's level
    /// when nested blocks need to land one deeper.
    pub(crate) fn emit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(e) => {
                self.emit_expr(e);
                self.w.push_str(";\n");
            }
            Stmt::Return(value) => {
                self.w.push_str("return");
                if let Some(e) = value {
                    self.w.push(' ');
                    // Nullable-return coercion: when the enclosing
                    // fn returns `T?` (lowered as `Option<T>`) and
                    // the value being returned isn't already a
                    // `null` literal, wrap it in `Some(...)` so the
                    // type-check passes. A `return null;` already
                    // lowers to `return None;` via `emit_literal`.
                    let wrap_some = self.return_wants_some_wrap(e);
                    // Sealed-upcast coercion: `return new Err(...)`
                    // inside a `Result`-returning function wraps
                    // through `.into()` so the auto-`From<Err> for
                    // Result` impl produces `Result::Err(err)`.
                    let wrap_upcast = self.return_needs_sealed_upcast(e);
                    if wrap_some {
                        self.w.push_str("Some(");
                    }
                    self.emit_expr(e);
                    // **Wrapper-class share-on-return (§CR.4.1).** A
                    // `return <wrapped place>;` (a `Path`/`this` local or
                    // an `xs[i]` index read of a wrapped class) must hand
                    // the caller a SHARED handle, not move out of the
                    // place — append the cheap `Rc` refcount-bump clone.
                    // Skipped under `Some(...)`/upcast wraps, which only
                    // fire for nullable / sealed shapes (never a bare
                    // wrapped place) — the helper would return false there
                    // anyway, but gating keeps the emit unambiguous.
                    if !wrap_some && !wrap_upcast && self.wrapper_value_needs_clone(e) {
                        self.w.push_str(".clone()");
                    }
                    if wrap_upcast {
                        self.w.push_str(".into()");
                    }
                    if wrap_some {
                        self.w.push(')');
                    }
                }
                self.w.push_str(";\n");
            }
            Stmt::VarDecl(var) => self.emit_var_decl(var),
            Stmt::If(if_stmt) => self.emit_if(if_stmt),
            Stmt::While(w) => self.emit_while(w),
            Stmt::ForEach(f) => self.emit_for_each(f),
            Stmt::Assign(a) => self.emit_assign(a),
            Stmt::Break(_) => self.w.push_str("break;\n"),
            Stmt::Continue(_) => self.w.push_str("continue;\n"),
            Stmt::SuperCall(_, _) => {
                // `super(args);` is lifted out of the body by
                // `emit_constructor` into the child struct's literal
                // (`__parent: Parent::new(args)`). Any super call that
                // reaches this point is dead — extract it before
                // calling `emit_stmt`. The arm exists for exhaustive
                // matching; emitting nothing keeps generated Rust
                // valid even if a future refactor leaves one behind.
            }
            Stmt::Throw(e, _) => {
                // Typed payload: the thrown value goes through
                // `std::panic::panic_any` with its concrete type
                // preserved. The catch_unwind in any enclosing
                // `try` block downcasts the payload to the catch
                // clause's declared type (`Box<dyn Any +
                // Send>.downcast::<T>()`), so `catch (T ex)` binds
                // `ex` as the actual `T` instance — fields and
                // methods on it work as written.
                //
                // For panic-aborted binaries the rendered panic
                // header still needs a printable representation;
                // every user class derives `Debug` so the
                // default-hook output reads like the value's
                // `{:?}` form. We don't synthesize an extra
                // String payload here — the typed object IS the
                // payload, and the catch-site recovers it
                // verbatim.
                self.w.push_str("std::panic::panic_any(");
                self.emit_expr(e);
                self.w.push_str(");\n");
            }
            Stmt::Try(t) => self.emit_try(t),
        }
    }

    /// Lower a Jux `try / catch / finally` statement to Rust using
    /// `std::panic::catch_unwind` as the unwinding mechanism. The
    /// shape per spec §X.3.2:
    ///
    /// ```text
    /// try B0 catch (T1 e1) B1 ... finally Bf
    /// ```
    ///
    /// becomes:
    ///
    /// ```text
    /// {
    ///     let __jux_try_result = std::panic::catch_unwind(
    ///         std::panic::AssertUnwindSafe(|| { B0 })
    ///     );
    ///     match __jux_try_result {
    ///         Ok(_) => {}
    ///         Err(__payload) => {
    ///             let e1: String = /* downcast __payload to String */;
    ///             B1
    ///         }
    ///     }
    ///     Bf
    /// }
    /// ```
    ///
    /// **Phase-1 caveat.** The caught name is bound as `String`
    /// regardless of the declared catch type — the full
    /// typed-exception story lands when the Result-mode pass
    /// arrives. Single catch only in this shape; multi-catch and
    /// per-type filtering chain as `else if`/`match` arms.
    pub(crate) fn emit_try(&mut self, t: &juxc_ast::TryStmt) {
        // Two lowering shapes, chosen by whether the try body
        // contains an `await`:
        //
        //   - **Sync**: `std::panic::catch_unwind(AssertUnwindSafe(||
        //     { body }))`. The closure captures locals by
        //     reference; `body` mutations on outer vars
        //     propagate.
        //   - **Async**: `AssertUnwindSafe(async move { body })
        //     .catch_unwind().await` (from `futures::FutureExt`).
        //     The async block captures locals by move, so try
        //     bodies that need to mutate outer state in an async
        //     context must thread the value out via the result
        //     instead.
        //
        // Both paths produce `Result<(), Box<dyn Any + Send>>`,
        // so the catch / finally machinery downstream is shared.
        let is_async = crate::analysis::block_contains_await(&t.body);
        // Wrap the whole thing in a block so locals introduced by
        // the lowering don't leak.
        self.w.push_str("{\n");
        self.w.indent_inc();
        self.w.emit_indent();
        self.w.push_str("let __jux_try_result: std::thread::Result<()> = ");
        if is_async {
            // `futures::FutureExt::catch_unwind(...)` is fully
            // qualified so we don't need a `use` statement at the
            // emit site. `AssertUnwindSafe<Fut>` impls `Future +
            // UnwindSafe`, satisfying the extension trait's bound.
            self.w.push_str(
                "futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async move {\n",
            );
            self.w.indent_inc();
            self.emit_block_contents(&t.body);
            self.w.indent_dec();
            self.w.emit_indent();
            self.w.push_str("})).await;\n");
        } else {
            self.w
                .push_str("std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {\n");
            self.w.indent_inc();
            self.emit_block_contents(&t.body);
            self.w.indent_dec();
            self.w.emit_indent();
            self.w.push_str("}));\n");
        }
        // Match on the result and run the appropriate catch.
        self.w.emit_indent();
        self.w.push_str("match __jux_try_result {\n");
        self.w.indent_inc();
        self.w.line("Ok(_) => {}");
        self.w.emit_indent();
        self.w.push_str("Err(__jux_payload) => {\n");
        self.w.indent_inc();
        // Typed-payload dispatch: try each catch clause in source
        // order. Each clause attempts `downcast::<T>()`; on success
        // it binds the catch name to the recovered typed value and
        // breaks out of the labelled block. On failure, the payload
        // threads through to the next clause. If no clause matches
        // we resume the panic so an outer handler / runtime hook
        // can deal with it (mirrors Java's "uncaught propagates").
        //
        // A labelled block (`'__jux_catch: { ... break '__jux_catch;
        // ... }`) is the cleanest way to express "stop dispatch
        // after the first match" without nesting matches arbitrarily
        // deep.
        if t.catches.is_empty() {
            // No catch clauses (try/finally form). Drop the payload
            // silently — `finally` still runs below.
            self.w.line("let _ = __jux_payload;");
        } else {
            // Fully qualify `::std::boxed::Box` so a user class
            // named `Box` doesn't shadow it. `std::panic::catch_unwind`
            // hands back the typed-erased payload as `Box<dyn Any +
            // Send>`; we keep the same Box type to feed back into
            // `resume_unwind` if no catch matches.
            self.w.line(
                "let mut __jux_payload_slot: Option<::std::boxed::Box<dyn ::std::any::Any + ::std::marker::Send>> = Some(__jux_payload);",
            );
            self.w.line("'__jux_catch: {");
            self.w.indent_inc();
            for clause in &t.catches {
                // Pull the payload back out, try the downcast, and
                // either run the body (consuming the value) or thread
                // the unrecovered payload back to the slot.
                self.w
                    .line("if let Some(__jux_p) = __jux_payload_slot.take() {");
                self.w.indent_inc();
                self.w.emit_indent();
                self.w.push_str("match __jux_p.downcast::<");
                self.emit_type_as_rust(&clause.ty);
                self.w.push_str(">() {\n");
                self.w.indent_inc();
                self.w.emit_indent();
                self.w.push_str("Ok(__jux_boxed) => {\n");
                self.w.indent_inc();
                self.w.emit_indent();
                self.w.push_str("let ");
                self.w.push_str(&clause.name.text);
                self.w.push_str(" = *__jux_boxed;\n");
                self.emit_block_contents(&clause.body);
                self.w.line("break '__jux_catch;");
                self.w.indent_dec();
                self.w.line("}");
                self.w
                    .line("Err(__jux_rest) => { __jux_payload_slot = Some(__jux_rest); }");
                self.w.indent_dec();
                self.w.line("}");
                self.w.indent_dec();
                self.w.line("}");
            }
            // No clause matched — resume the panic so it
            // propagates to whoever was waiting on this future /
            // call.
            self.w.line(
                "if let Some(__jux_unhandled) = __jux_payload_slot.take() { std::panic::resume_unwind(__jux_unhandled); }",
            );
            self.w.indent_dec();
            self.w.line("}");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        // Finally: emit its body verbatim after the match. Runs
        // in both success and failure paths.
        if let Some(fin) = &t.finally {
            self.emit_block_contents(fin);
        }
        self.w.indent_dec();
        self.w.emit_indent();
        self.w.push_str("}\n");
    }

    /// Lower `for (var name : iter) { body }` to Rust's `for name in iter { body }`.
    ///
    /// **Type annotations:** Rust's `for` pattern doesn't accept a type
    /// annotation in the same shape as a `let`. For now we drop the
    /// `var_type` (if any) and let Rust infer from the iterator's
    /// `Item` type. If users need an explicit type, they can write
    /// `for x in iter { let x: int = x; … }` — a future enhancement.
    ///
    /// **Two shapes, chosen by element type:**
    ///
    /// 1. **Copy elements** (`int`, `bool`, `char`, `float`, …) →
    ///    `for &x in &iter { … }`. Pattern-derefs the borrowed item
    ///    so `x` is a value-typed binding without an allocation.
    ///    Zero overhead, exactly what hand-written Rust would say.
    /// 2. **Non-Copy elements** (`String`, user classes, records,
    ///    enums with payloads) → `for x in iter.iter().cloned() { … }`.
    ///    Clones each item so the body sees an owned `T`, matching
    ///    Jux's "Java-shaped" expectation that the loop variable
    ///    behaves like a value. Every user type derives `Clone`, so
    ///    the bound holds.
    ///
    /// In both cases the source array stays usable after the loop —
    /// we borrow it, not move it.
    ///
    /// **Ranges** (`0..10`) keep their naked form. They're cheap-to-
    /// move self-iterators with `Item = isize`; no borrow needed.
    pub(crate) fn emit_for_each(&mut self, f: &ForEachStmt) {
        if matches!(&f.iter, Expr::Range(_)) {
            self.w.push_str("for ");
            self.w.push_str(&f.var_name.text);
            self.w.push_str(" in ");
            self.emit_expr(&f.iter);
            self.w.push_str(" {\n");
            self.w.indent_inc();
            self.emit_block_contents(&f.body);
            self.w.indent_dec();
            self.w.emit_indent();
            self.w.push_str("}\n");
            return;
        }

        // Three lowering shapes:
        //
        // - **Copy element type** (`int`, `bool`, `char`, `f64`, …):
        //   `for &x in &xs { … }`. Pattern-derefs the borrowed
        //   item; zero overhead.
        // - **Non-Copy element type, body never moves x**:
        //   `for x in &xs { … }`. The loop variable binds as
        //   `&T`; auto-deref covers method calls, `==`, format
        //   args, etc. Saves the `.iter().cloned()` heap clone
        //   per iteration.
        // - **Non-Copy element type, body moves x**: fall back to
        //   `for x in xs.iter().cloned() { … }` so `x` is owned
        //   and the move sites compile.
        //
        // "Moves x" = the loop variable appears as the immediate
        // value in a position that consumes ownership: a fn-call
        // arg, a `new T` arg, a var-decl init, an assignment rhs,
        // a return value, or a super-call arg. Reads through `.`
        // / `[]` / comparisons / format don't move it.
        let element_is_copy = match self.expr_types.get(&expr_span_of(&f.iter)) {
            Some(Ty::Array { element, .. }) => matches!(element.as_ref(), Ty::Primitive(_)),
            _ => false,
        };
        let body_moves_var =
            !element_is_copy && body_moves_path(&f.body, &f.var_name.text);

        self.w.push_str("for ");
        if element_is_copy {
            self.w.push('&');
        }
        self.w.push_str(&f.var_name.text);
        self.w.push_str(" in ");
        if element_is_copy {
            self.w.push('&');
            self.emit_expr(&f.iter);
        } else if body_moves_var {
            self.emit_expr(&f.iter);
            self.w.push_str(".iter().cloned()");
        } else {
            // Borrow-iter: yields `&T`, so `x.method()` /
            // `format!("{}", x)` / `x == y` all work through
            // auto-deref / `Display` / `PartialEq` blanket impls.
            self.w.push('&');
            self.emit_expr(&f.iter);
        }
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.emit_block_contents(&f.body);
        self.w.indent_dec();
        self.w.emit_indent();
        self.w.push_str("}\n");
    }

    /// Lower `var name = init ;` to `let name = init ;` (or `let mut`
    /// when this binding is reassigned anywhere in the function body).
    ///
    /// The mutability decision comes from [`Self::mutated_in_fn`], which
    /// is populated by [`collect_mutated_names`] in [`Self::emit_fn_decl`]
    /// before this method is called. The effect: bindings that never get
    /// reassigned emit as plain `let`, which silences Rust's
    /// `unused_mut` lint and reads better.
    ///
    /// We emit Rust without a type annotation and let the Rust compiler
    /// infer it. Once tycheck carries a real type for each `VarDecl`, we
    /// can emit explicit annotations here.
    pub(crate) fn emit_var_decl(&mut self, var: &VarDecl) {
        // Record the local's declared type in the backend's
        // `local_types` map so `@Intrinsic` dispatch can resolve
        // the receiver class when `expr_types` lookups are
        // unreliable (interp-string synthetic-source collisions).
        // Falls back to `Ty::Unknown` for the `var x = …` form
        // where no type was written.
        if let Some(ty_ref) = &var.ty {
            let ty = juxc_tycheck::ty_from_ref_in_env(
                ty_ref,
                &self.symbols,
            );
            if let Some(scope) = self.local_types.last_mut() {
                scope.insert(var.name.text.clone(), ty);
            }
        } else if let Some(init) = &var.init {
            // `var x = init;` carries no written type — recover one from
            // the initializer's inferred type so name-keyed receiver
            // resolution (`local_types`) still works for inferred locals.
            // This is what makes the wrapper-class `.0.borrow()` rewrite
            // fire for `var i = new Inner(...); print($"${i.field}")`,
            // where the interpolated `i`'s span collides in `expr_types`
            // but its NAME reliably maps to the right class here. Only a
            // `Ty::User` is worth recording (it's the only kind the
            // wrapper / stdlib-dispatch receiver lookups consult).
            if let Some(ty @ juxc_tycheck::Ty::User { .. }) =
                self.expr_types.get(&expr_span_of(init)).cloned()
            {
                if let Some(scope) = self.local_types.last_mut() {
                    scope.insert(var.name.text.clone(), ty);
                }
            }
        }
        self.w.push_str("let ");
        if self.mutated_in_fn.contains(&var.name.text) {
            self.w.push_str("mut ");
        }
        self.w.push_str(&var.name.text);
        // Java-style typed local (`int x = 5;`) carries an explicit
        // type annotation; emit it as `let x: T = init;`. The `var`
        // form leaves `ty == None` and we let Rust infer.
        let declared_nullable = var.ty.as_ref().map_or(false, |t| t.nullable);
        // Inferred nullability for `var` (no explicit type):
        // when the init expression is itself `Option<T>`-shaped
        // (a nullable-returning call, a `?.`-chain, a known
        // nullable local, etc.), the resulting binding also has
        // nullable shape. Seed `nullable_locals` so downstream
        // sites can recognize reads of this binding.
        let init_is_nullable = var
            .init
            .as_ref()
            .map_or(false, |e| self.expression_is_already_nullable(e));
        if declared_nullable || init_is_nullable {
            self.nullable_locals.insert(var.name.text.clone());
        }
        if let Some(ty) = &var.ty {
            self.w.push_str(": ");
            self.emit_type_as_rust(ty);
        }
        if let Some(init) = &var.init {
            self.w.push_str(" = ");
            // When the declared type is nullable (`T?` → `Option<T>`)
            // and the init isn't a `null` literal, wrap in `Some(...)`
            // so the assignment type-checks. A `null` init already
            // lowers to `None` via `emit_literal`, so no wrap there.
            let wrap_some = declared_nullable && !is_null_literal(init);
            if wrap_some {
                self.w.push_str("Some(");
            }
            self.emit_expr(init);
            // **Wrapper-class share-on-assignment (§CR.4.1).** When the
            // init re-reads an existing wrapper-class binding
            // (`var y = x;`, `var y = obj.child;`, `var y = this;`),
            // the two bindings must SHARE the same instance — Java
            // reference semantics. A bare move would invalidate the
            // source. Append `.clone()` (a cheap `Rc` refcount bump)
            // so both handles stay live and point at the same
            // `RefCell`. Fresh values (`new C(...)`, a call result)
            // are already owned handles and don't need the clone.
            //
            // A `Field` read of a wrapper-class field already gets its
            // `.clone()` from `emit_field`'s class-field auto-clone, so
            // the shared helper covers only the bare-`Path` / `this` and
            // index-read (`var r = xs[0]`) places the field path doesn't.
            if !wrap_some && self.wrapper_value_needs_clone(init) {
                self.w.push_str(".clone()");
            }
            if wrap_some {
                self.w.push(')');
            }
        }
        self.w.push_str(";\n");
    }

    /// `while (cond) { body }` Jux → `while cond { body }` Rust.
    ///
    /// **Cosmetic special case:** when the Jux source uses the literal
    /// constant `true` as the condition (the canonical "loop forever"
    /// idiom), we emit Rust's dedicated `loop { … }` keyword instead of
    /// `while true { … }`. Both produce identical machine code, but `loop`
    /// is what a Rust developer would write and what clippy would
    /// recommend. The shape change matters for readability of the emitted
    /// source, not for semantics.
    ///
    /// We only special-case the **literal** `true` token — `while (1 == 1)`
    /// stays as a `while` even though it's also always true. Recognizing
    /// always-true expressions would need const evaluation, which is a
    /// later phase.
    pub(crate) fn emit_while(&mut self, w: &WhileStmt) {
        if matches!(w.condition, Expr::Literal(Literal::Bool(true))) {
            self.w.push_str("loop {\n");
        } else {
            self.w.push_str("while ");
            self.emit_expr(&w.condition);
            self.w.push_str(" {\n");
        }
        self.w.indent_inc();
        self.emit_block_contents(&w.body);
        self.w.indent_dec();
        self.w.emit_indent();
        self.w.push_str("}\n");
    }

    /// `target = value ;` Jux → `target = value;` Rust.
    ///
    /// The target is whatever the parser validated as an lvalue —
    /// today: simple name (single-segment `Path`), array index
    /// (`Index`), or field access (`Field`, including `this.field`).
    ///
    /// Post Fix 1 the RHS of a String-typed assignment is always an
    /// owned `String` value (literal self-coerces inside
    /// `emit_literal`; identifiers refer to `String`-typed bindings).
    /// No `.to_string()` injection is needed here anymore.
    pub(crate) fn emit_assign(&mut self, a: &AssignStmt) {
        // String `+=` special-case: Rust's `String + String` and
        // `String += String` aren't implemented (only the
        // `&str`-RHS variants exist), and emitting the regular
        // `s += rhs` path would force the literal-coerce on the
        // RHS to produce a `String` that Rust then rejects. The
        // idiomatic form is `s.push_str(&rhs)` — works for both
        // `String` and `&str` RHS via `AsRef<str>` semantics.
        if matches!(a.op, Some(juxc_ast::BinaryOp::Add))
            && matches!(
                self.expr_types.get(&expr_span_of(&a.target)),
                Some(Ty::String),
            )
        {
            self.emitting_lvalue = true;
            self.emit_expr(&a.target);
            self.emitting_lvalue = false;
            self.w.push_str(".push_str(&");
            // Borrow context so a literal RHS stays `&str` (no
            // wasted `.to_string()`).
            let prev = self.emitting_format_arg;
            self.emitting_format_arg = true;
            self.emit_expr(&a.value);
            self.emitting_format_arg = prev;
            self.w.push_str(");\n");
            return;
        }
        // Mutable-static target: evaluate the RHS first into a local
        // (releasing any locks the RHS itself takes), then acquire
        // the LHS lock once for the write. Without this scoping the
        // statement `Class.x = …` deadlocks whenever the RHS reads
        // the same mutable static (`x = x + 1`, `Class.x = Class.x`,
        // …) because the LHS's `MutexGuard` is a statement-scoped
        // temporary that's still live while the RHS runs. Compound
        // forms (`x += rhs`) get the same wrap so `x += x` doesn't
        // hit the same trap. See §CR.5.7 in
        // `JUX-CLASS-REPRESENTATION-ADDENDUM.md` for the lowering
        // contract.
        let target_is_mutable_static = self.target_is_mutable_static(&a.target);
        if target_is_mutable_static {
            self.w.push_str("{ let __jux_v = ");
            self.emit_expr(&a.value);
            self.w.push_str("; ");
            self.emitting_lvalue = true;
            self.emit_expr(&a.target);
            self.emitting_lvalue = false;
            if let Some(op) = a.op {
                self.w.push(' ');
                self.w.push_str(op.as_rust_str());
                self.w.push_str("= __jux_v");
            } else {
                self.w.push_str(" = __jux_v");
            }
            self.w.push_str("; }\n");
            return;
        }
        // Wrapper-class field write (§CR.4.1): `obj.f = v` where `obj`
        // is a wrapper-shape class. Field state lives inside
        // `Rc<RefCell<C_Inner>>`, so we evaluate the RHS into a
        // statement-scoped temp first (releasing any `borrow()` the
        // RHS itself takes — `obj.f = obj.g` must not deadlock the
        // RefCell), then take a one-statement `borrow_mut()` for the
        // write:
        //
        //   { let __jux_v = <rhs>; obj.0.borrow_mut().f = __jux_v; }
        //
        // Compound forms (`obj.f += rhs`) use the same wrap so
        // `obj.f += obj.f` releases the read borrow before the
        // write borrow is taken. This mirrors the mutable-static
        // scoped-temp shape above (§CR.5.7).
        if let Expr::Field(tf) = &a.target {
            if !tf.safe && self.receiver_is_wrapper_class(&tf.object) {
                // Walk the `__parent` chain to the slot that actually
                // declares the field — inherited-field writes
                // (`child.parentField = v`) land deeper in the inner
                // (`child.0.borrow_mut().__parent.field = v`). A `None`
                // depth (no such instance field) shouldn't reach here
                // for a wrapper receiver, but fall back to depth 0 so
                // the emitted Rust still type-checks against the
                // receiver's own class.
                let depth = self
                    .wrapper_field_parent_depth(&tf.object, &tf.field.text)
                    .unwrap_or(0);
                self.w.push_str("{ let __jux_v = ");
                let assign_nullable =
                    a.op.is_none() && self.assign_target_is_nullable(&a.target);
                self.emit_arg_with_nullable_wrap(&a.value, assign_nullable);
                // Wrapper-class share-on-store: a wrapped place stored
                // into a field hands the field a SHARED handle (§CR.4.1).
                if !assign_nullable && self.wrapper_value_needs_clone(&a.value) {
                    self.w.push_str(".clone()");
                }
                self.w.push_str("; ");
                // LHS place expression with a MUTABLE borrow.
                self.emit_expr(&tf.object);
                self.w.push_str(".0.borrow_mut()");
                for _ in 0..depth {
                    self.w.push_str(".__parent");
                }
                self.w.push('.');
                self.w.push_str(&tf.field.text);
                if let Some(op) = a.op {
                    self.w.push(' ');
                    self.w.push_str(op.as_rust_str());
                    self.w.push_str("= __jux_v");
                } else {
                    self.w.push_str(" = __jux_v");
                }
                self.w.push_str("; }\n");
                return;
            }
        }
        // LHS: emit with the lvalue flag set so `emit_field` skips its
        // String-read `.clone()` insertion.
        self.emitting_lvalue = true;
        self.emit_expr(&a.target);
        self.emitting_lvalue = false;
        // Compound assignment lowers to Rust's matching `op=`:
        // `x += y`, `arr[i] *= n`, etc. Rust evaluates the place
        // expression exactly once even for side-effecting shapes
        // like `arr[next()] += 1`, so we don't have to introduce
        // any temp. The op spelling is the regular Rust binary
        // operator with `=` appended.
        let is_compound = a.op.is_some();
        if let Some(op) = a.op {
            self.w.push(' ');
            self.w.push_str(op.as_rust_str());
            self.w.push_str("= ");
        } else {
            self.w.push_str(" = ");
        }
        // Nullable-field assign coercion: when the LHS is a field
        // whose declared type is `T?` and the RHS isn't already
        // nullable-shaped, wrap RHS in `Some(...)`. Skipped for
        // compound forms (`obj.x += y`) because `Option<T> +=` has
        // no sensible meaning and rustc will surface the misuse.
        let assign_nullable = !is_compound && self.assign_target_is_nullable(&a.target);
        self.emit_arg_with_nullable_wrap(&a.value, assign_nullable);
        // Wrapper-class share-on-assign (§CR.4.1): when the RHS is a
        // wrapped place (`Path`/`this` local or `xs[i]` index read), the
        // assignment must SHARE the same instance — append the cheap `Rc`
        // refcount-bump clone instead of moving out of the place. Skipped
        // for compound forms (`x += y` has no wrapped-place meaning) and
        // when the value was lifted into `Some(...)` (a nullable field
        // never takes a bare wrapped place; the helper returns false too).
        if !is_compound && !assign_nullable && self.wrapper_value_needs_clone(&a.value) {
            self.w.push_str(".clone()");
        }
        self.w.push_str(";\n");
    }

    /// True iff the assignment target resolves to a non-`final`
    /// `static` class field — i.e. one of the
    /// `LazyLock<Mutex<T>>`-lowered slots. Recognized in two shapes:
    ///
    /// - **Qualified** `Class.field` — `Expr::Field` whose object is
    ///   a `Path` resolving to a class FQN, and the named field is
    ///   `is_static && !is_final`.
    /// - **Bare-name** `field` inside `class Class { … }` — single-
    ///   segment `Expr::Path` that matches a static field on
    ///   `self.enclosing_class`.
    ///
    /// Both shapes share the same Mutex-deadlock concern, so they
    /// share the temp-binding wrap in [`Self::emit_assign`].
    pub(crate) fn target_is_mutable_static(&self, target: &Expr) -> bool {
        match target {
            Expr::Field(f) => {
                let Expr::Path(qn) = f.object.as_ref() else { return false };
                let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) else { return false };
                let Some(class) = self.symbols.classes.get(&class_fqn) else { return false };
                let Some(field) = class.fields.get(f.field.text.as_str()) else { return false };
                field.is_static && !field.is_final
            }
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let Some(class_name) = &self.enclosing_class else { return false };
                let Some(class) = self.lookup_class_by_bare_or_fqn(class_name) else {
                    return false;
                };
                let Some(field) = class.fields.get(qn.segments[0].text.as_str()) else {
                    return false;
                };
                field.is_static && !field.is_final
            }
            _ => false,
        }
    }

    /// True iff the assignment target is a class/record field whose
    /// declared type carries the `nullable` flag. Walks the field
    /// expression to find the receiver's class via `expr_types`,
    /// then looks up the field on that class. Conservative — a
    /// miss (no class info, no such field) returns false so the
    /// caller won't add a wrap and rustc will surface any real
    /// mismatch.
    pub(crate) fn assign_target_is_nullable(&self, target: &Expr) -> bool {
        let Expr::Field(f) = target else { return false };
        let Some(juxc_tycheck::Ty::User { name, .. }) =
            self.expr_types.get(&expr_span_of(&f.object))
        else {
            return false;
        };
        // Walk the class's own fields; ancestor fields would need
        // an inheritance walk like `lookup_field_type` does. For
        // Phase 1 the assign-coercion fires only on direct fields
        // — Java/Kotlin user code that assigns to an inherited
        // nullable field is rare enough that we'll wait for an
        // example to motivate the deeper walk.
        if let Some(class) = self.symbols.classes.get(name) {
            if let Some(field) = class.fields.get(&f.field.text) {
                return field.ty.nullable;
            }
        }
        if let Some(record) = self.symbols.records.get(name) {
            if let Some(c) = record.components.iter().find(|c| c.name == f.field.text) {
                return c.ty.nullable;
            }
        }
        false
    }

    /// Lower `if (cond) { … } else if (…) { … } else { … }` to its
    /// directly-corresponding Rust form. Rust uses no parentheses around
    /// `if` conditions, so we drop them.
    ///
    /// **Null smart-cast** (Kotlin-style): when the condition is
    /// `name != null` for a bare identifier `name`, lower the head
    /// to `if let Some(name) = name` so the binding inside the
    /// `then` block sees the unwrapped inner type. Pairs with the
    /// `is_some()`/`is_none()` peephole in `emit_binary` — the
    /// peephole stays the right shape for boolean-context uses
    /// (`var ok = x != null;`), while this branch handles the
    /// narrower `if` form.
    pub(crate) fn emit_if(&mut self, if_stmt: &IfStmt) {
        // Smart-cast bookkeeping: when the condition is `name !=
        // null`, `name` inside the `then` block is the unwrapped
        // inner `T` (no longer `Option<T>`). Remove it from
        // `nullable_locals` for the duration of the body so
        // format-arg JuxOpt wrapping and elvis null-checks treat
        // it correctly. Restore on the way out so the rest of the
        // function still sees the original nullable shape.
        let cast_name: Option<String> =
            match_simple_not_null_check(&if_stmt.condition).map(|s| s.to_string());
        let was_nullable = cast_name
            .as_ref()
            .map_or(false, |n| self.nullable_locals.contains(n));

        if let Some(name) = &cast_name {
            self.w.push_str("if let Some(");
            self.w.push_str(name);
            self.w.push_str(") = ");
            self.w.push_str(name);
            self.w.push_str(" {\n");
        } else {
            self.w.push_str("if ");
            self.emit_expr(&if_stmt.condition);
            self.w.push_str(" {\n");
        }
        // Apply the smart-cast: inside the `then` block the
        // binding is no longer nullable.
        if let Some(name) = &cast_name {
            if was_nullable {
                self.nullable_locals.remove(name);
            }
        }
        self.w.indent_inc();
        self.emit_block_contents(&if_stmt.then_block);
        self.w.indent_dec();
        // Restore: outside the block, the binding regains its
        // declared nullable type for subsequent uses (the next
        // else-arm, code after the if).
        if let Some(name) = &cast_name {
            if was_nullable {
                self.nullable_locals.insert(name.clone());
            }
        }
        self.w.emit_indent();
        self.w.push('}');

        // Walk an arbitrarily-long else-if chain without recursing into
        // `emit_stmt`: each nested IfStmt becomes another `} else if …`
        // segment on the same source line.
        let mut else_branch = if_stmt.else_branch.as_deref();
        while let Some(branch) = else_branch {
            match branch {
                ElseBranch::If(inner) => {
                    self.w.push_str(" else if ");
                    self.emit_expr(&inner.condition);
                    self.w.push_str(" {\n");
                    self.w.indent_inc();
                    self.emit_block_contents(&inner.then_block);
                    self.w.indent_dec();
                    self.w.emit_indent();
                    self.w.push('}');
                    else_branch = inner.else_branch.as_deref();
                }
                ElseBranch::Block(block) => {
                    self.w.push_str(" else {\n");
                    self.w.indent_inc();
                    self.emit_block_contents(block);
                    self.w.indent_dec();
                    self.w.emit_indent();
                    self.w.push('}');
                    else_branch = None;
                }
            }
        }
        self.w.push('\n');
    }
}

/// Reach into a [`Stmt`] for its source span. Used by source-map
/// marker emission. Several `Stmt` variants store their span on the
/// inner payload (`IfStmt.span`, `VarDecl.span`, …); two (`Break`,
/// `Continue`) carry a bare `Span`; `SuperCall` puts the span second
/// in the tuple. For `Stmt::Expr` and `Stmt::Return(Some)` we forward
/// to [`expr_span_of`] on the inner expression. `Stmt::Return(None)`
/// has no expression span — falls back to `Span::DUMMY` so the
/// marker emission skips it cleanly.
pub(crate) fn stmt_span(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::Expr(e) => expr_span_of(e),
        Stmt::Return(Some(e)) => expr_span_of(e),
        Stmt::Return(None) => Span::DUMMY,
        Stmt::VarDecl(v) => v.span,
        Stmt::If(i) => i.span,
        Stmt::While(w) => w.span,
        Stmt::ForEach(f) => f.span,
        Stmt::Assign(a) => a.span,
        Stmt::Break(s) => *s,
        Stmt::Continue(s) => *s,
        Stmt::SuperCall(_, s) => *s,
        Stmt::Throw(_, s) => *s,
        Stmt::Try(t) => t.span,
    }
}
