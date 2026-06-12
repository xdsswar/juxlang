//! Call-expression emission — generic function/method calls plus the
//! built-in `print(...)` special case. Both paths share the enum-
//! variant String-payload coercion that injects `.to_string()` on
//! positional args matching a `String` slot.

use juxc_ast::{BinaryExpr, CallExpr, Expr, Literal};

use crate::analysis::is_string_literal;
use crate::exprs::ArgRef;
use crate::RustEmitter;

/// Mirror of `binary::collect_string_concat_operands` for the
/// `print(...)`-collapse hot path. Kept here to avoid exposing the
/// binary-module helper across modules.
fn flatten_concat<'a>(b: &'a BinaryExpr, out: &mut Vec<&'a Expr>) {
    push_concat_operand(&b.left, out);
    push_concat_operand(&b.right, out);
}

fn push_concat_operand<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::Binary(inner) = e {
        if inner.op == juxc_ast::BinaryOp::Add
            && (is_string_literal(&inner.left) || is_string_literal(&inner.right))
        {
            flatten_concat(inner, out);
            return;
        }
    }
    out.push(e);
}

/// True iff the print path should treat `e` as `String`-typed.
/// Mirrors `binary.rs::operand_is_string_typed` — kept module-
/// local rather than sharing the helper to avoid cross-module
/// privacy churn. Both paths use the same `expr_types` lookup so
/// the trigger fires consistently.
impl super::super::RustEmitter {
    fn operand_is_string_typed_for_print(&self, e: &Expr) -> bool {
        let recorded = self.expr_types.get(&crate::exprs::expr_span_of(e));
        // Mirror `binary::operand_is_string_typed`'s smart-cast
        // unwrap: when `e` is a path that the smart-cast pass
        // has removed from `nullable_locals`, peel a recorded
        // `Ty::Nullable` so the inner `String` matches the
        // type-driven concat trigger.
        let effective = if let (Expr::Path(qn), Some(juxc_tycheck::Ty::Nullable(inner))) =
            (e, recorded)
        {
            if qn.segments.len() == 1
                && !self.nullable_locals.contains(&qn.segments[0].text)
            {
                Some(inner.as_ref())
            } else {
                recorded
            }
        } else {
            recorded
        };
        matches!(effective, Some(juxc_tycheck::Ty::String))
    }
}

/// Print-path mirror of `binary::fold_concat_into_format`. Folds
/// `Literal::String` operands directly into the `println!` template
/// (re-escaped + brace-doubled); non-literal operands become runtime
/// args with a single `{}` placeholder each.
fn fold_concat_for_print<'a>(operands: &[&'a Expr]) -> (String, Vec<&'a Expr>) {
    let mut template = String::new();
    let mut runtime: Vec<&'a Expr> = Vec::new();
    for op in operands {
        if let Expr::Literal(Literal::String(s)) = op {
            for ch in s.chars() {
                match ch {
                    '{' => template.push_str("{{"),
                    '}' => template.push_str("}}"),
                    '"' => template.push_str("\\\""),
                    '\\' => template.push_str("\\\\"),
                    '\n' => template.push_str("\\n"),
                    '\r' => template.push_str("\\r"),
                    '\t' => template.push_str("\\t"),
                    c => template.push(c),
                }
            }
        } else {
            template.push_str("{}");
            runtime.push(op);
        }
    }
    (template, runtime)
}

impl RustEmitter {
    /// True when `call` targets a foreign (`.jux.d`) function or static method
    /// whose `throws E` clause maps a Rust `Result<T, E>` return (§G.5.4) — the
    /// `is_foreign_result` flag on its signature. Such a call must have its
    /// `Result` unwrapped at the use site (see the `Expr::Call` arm of
    /// `emit_expr`). Covers bare free-function calls and `ClassName.method(...)`
    /// static calls; instance-method foreign-result calls are a later refinement.
    pub(crate) fn call_is_foreign_result(&self, call: &CallExpr) -> bool {
        match &*call.callee {
            // Free function `f(args)` — exact key, else last-segment match for an
            // imported foreign fn keyed by its full `rust.<crate>.<fn>` path.
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let bare = qn.segments[0].text.as_str();
                if let Some((_, sig)) = self.symbols.lookup_function(bare) {
                    return sig.is_foreign_result;
                }
                self.symbols
                    .functions
                    .iter()
                    .find(|(k, _)| k.rsplit('.').next() == Some(bare))
                    .map(|(_, s)| s.is_foreign_result)
                    .unwrap_or(false)
            }
            // Method call `recv.method(args)` — two shapes:
            //  - Static `ClassName.method(...)`: the receiver is a type name.
            //  - Instance `value.method(...)`: resolve the receiver's inferred
            //    type to its class, then look up the method.
            Expr::Field(f) => {
                let method = f.field.text.as_str();
                // Static: receiver is a bare/qualified class name.
                if let Expr::Path(qn) = &*f.object {
                    if let Some(last) = qn.segments.last() {
                        if let Some(fqn) = self.symbols.find_fqn_by_bare(&last.text) {
                            if let Some(cls) = self.symbols.classes.get(&fqn) {
                                if let Some(m) = cls.methods.get(method) {
                                    return m.is_foreign_result;
                                }
                            }
                        }
                    }
                }
                // Instance: resolve the receiver's type → class → method.
                if let Some(juxc_tycheck::Ty::User { name, .. }) =
                    self.receiver_ty_for_call(&f.object)
                {
                    if let Some(cls) = self.symbols.classes.get(&name) {
                        if let Some(m) = cls.methods.get(method) {
                            return m.is_foreign_result;
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Best-effort inferred type of a method-call receiver — the same
    /// `local_types` → `expr_types` → string-literal resolution
    /// [`Self::try_emit_stdlib_method`] uses, factored out for the
    /// foreign-result check. Returns `None` when the receiver wasn't typed.
    fn receiver_ty_for_call(&self, recv: &Expr) -> Option<juxc_tycheck::Ty> {
        if let Expr::Path(qn) = recv {
            if qn.segments.len() == 1 {
                let bare = qn.segments[0].text.as_str();
                if let Some(ty) = self
                    .local_types
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(bare).cloned())
                {
                    return Some(ty);
                }
            }
        }
        self.expr_types
            .get(&crate::exprs::expr_span_of(recv))
            .cloned()
    }

    /// Emit a call expression. Special-cases the built-in `print` to
    /// `println!(…)`. Every other callee is emitted verbatim (the
    /// resolver guarantees the name exists).
    pub(crate) fn emit_call(&mut self, call: &CallExpr) {
        // Method-overload pick (§T.3 Phase-1): tycheck recorded which
        // group member this call resolved to; member K > 0 emits
        // under `name__ovK`. Armed here, consumed by the single path
        // that writes the member name. Cleared first so a stale value
        // from an aborted emission can't leak in.
        self.pending_method_suffix = None;
        if let Some(k) = self.symbols.method_selections.get(&call.span) {
            if *k > 0 {
                self.pending_method_suffix = Some(format!("__ov{k}"));
            }
        }
        // `super.method(args)` (§6.9.4) — a STATIC call to the nearest
        // concrete ancestor's version of `method`, bypassing virtual dispatch
        // for this one call. We emit `<self>.__jux_super_<method>(args)`, a
        // per-class shim carrying the ancestor's body specialized to this
        // class (emitted by `emit_super_shims`). Other (virtual) calls inside
        // that body still dispatch to the subclass, matching Java.
        if let Expr::Field(f) = &*call.callee {
            if matches!(f.object.as_ref(), Expr::Super(_)) {
                let alias = self.this_alias.as_deref().unwrap_or("self").to_string();
                self.w.push_str(&alias);
                self.w.push_str(".__jux_super_");
                self.w.push_str(&f.field.text);
                self.w.push('(');
                for (i, arg) in call.args.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.w.push(')');
                return;
            }
        }
        // `weakField.get()` (§6.5) — promote a weak reference to a strong one.
        // The field stores a `Weak<RefCell<Target_Inner>>`, so `.get()` lowers
        // to `<recv>.0.borrow(){.__parent…}.<field>.upgrade().map(Target)`,
        // yielding `Option<Target>` = Jux `Target?` (null when the target has
        // already been dropped). Intercepted before the generic call paths so
        // the bare weak-field read of the receiver is never emitted on its own.
        if let Expr::Field(getf) = &*call.callee {
            if getf.field.text == "get" && call.args.is_empty() && !getf.safe {
                if let Expr::Field(wf) = getf.object.as_ref() {
                    if let Some(target) =
                        self.wrapper_weak_field_target(&wf.object, &wf.field.text)
                    {
                        let depth = self
                            .wrapper_field_parent_depth(&wf.object, &wf.field.text)
                            .unwrap_or(0);
                        self.emit_expr(&wf.object);
                        self.w.push_str(".0.borrow()");
                        for _ in 0..depth {
                            self.w.push_str(".__parent");
                        }
                        self.w.push('.');
                        self.w.push_str(&wf.field.text);
                        self.w.push_str(".upgrade().map(");
                        self.w.push_str(&target);
                        self.w.push(')');
                        return;
                    }
                }
            }
        }
        // `operator()` dispatch (§O.2.4): the callee is a VALUE whose
        // type declares the call overload — `adder(5)` routes to
        // `adder.__op_call(5)`. Checked before the named-callee paths
        // so a callable local never shadows into a function lookup.
        if self.expr_declares_operator(&call.callee, juxc_ast::OperatorKind::Call) {
            self.emit_expr_with_parent_prec(&call.callee, u8::MAX, false);
            self.w.push_str(".__op_call(");
            let prev = self.emitting_format_arg;
            self.emitting_format_arg = false;
            for (i, arg) in call.args.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.emit_expr(arg);
                if self.wrapper_value_needs_clone(arg) {
                    self.w.push_str(".clone()");
                }
            }
            self.emitting_format_arg = prev;
            self.w.push(')');
            return;
        }
        // Recognize a single-segment path `print` for the built-in.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "print" {
                return self.emit_print_call(call);
            }
        }
        // `assert(cond)` / `assert(cond, msg)` (§S.7.2) → Rust's
        // `debug_assert!`: checked in debug builds, elided in release
        // — exactly the jux-full profile defaults. The message slot
        // goes through the format machinery so interpolated strings
        // and String values both work; the macro evaluates it lazily
        // (only on failure).
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "assert" {
                self.w.push_str("debug_assert!(");
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                if let Some(cond) = call.args.first() {
                    self.emit_expr(cond);
                }
                if let Some(msg) = call.args.get(1) {
                    self.w.push_str(", \"{}\", ");
                    self.emitting_format_arg = true;
                    self.emit_expr(msg);
                }
                self.emitting_format_arg = prev;
                self.w.push(')');
                return;
            }
        }
        // `withTimeout(ms, f)` — §18.1.9: race the work against a
        // timer task; the loser is dropped (cancelling the work on
        // timeout) and a TimeoutException unwinds into the normal
        // catch machinery. Produces a Future — `await` it.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "withTimeout" {
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                self.w.push_str("async {\n");
                self.w.indent_inc();
                self.w.emit_indent();
                self.w.push_str(
                    "let __jux_timer = crate::__jux_spawn(async move { std::thread::sleep(std::time::Duration::from_millis((",
                );
                if let Some(ms) = call.args.first() {
                    self.emit_expr(ms);
                } else {
                    self.w.push('0');
                }
                self.w.push_str(") as u64)) });\n");
                self.w.emit_indent();
                self.w.push_str(
                    "match futures::future::select(std::pin::pin!(async move { ",
                );
                match call.args.get(1) {
                    Some(Expr::Lambda(l)) if l.params.is_empty() => match &l.body {
                        juxc_ast::LambdaBody::Expr(e) => self.emit_expr(e),
                        juxc_ast::LambdaBody::Block(b) => {
                            let (stmts, tail) = match b.statements.split_last() {
                                Some((juxc_ast::Stmt::Expr(t), rest)) => (rest, Some(t)),
                                _ => (&b.statements[..], None),
                            };
                            for stmt in stmts {
                                self.emit_stmt(stmt);
                            }
                            if let Some(tail) = tail {
                                self.emit_expr(tail);
                            }
                        }
                    },
                    Some(other) => {
                        self.emit_expr(other);
                        self.w.push_str(".await");
                    }
                    None => {}
                }
                self.w.push_str(" }), __jux_timer).await {\n");
                self.w.indent_inc();
                self.w.line("futures::future::Either::Left((__jux_v, _)) => __jux_v,");
                self.w.line(
                    "futures::future::Either::Right(_) => std::panic::panic_any(crate::jux::std::exceptions::TimeoutException::new(\"operation timed out\".to_string())),",
                );
                self.w.indent_dec();
                self.w.line("}");
                self.w.indent_dec();
                self.w.emit_indent();
                self.w.push('}');
                self.emitting_format_arg = prev;
                return;
            }
        }
        // `Task.all / Task.race / Task.delay` (§18.1.4) — statics on
        // the task runtime. `all` joins same-typed tasks into a
        // Task<List<T>>; `race` resolves with the first to settle;
        // `delay(ms)` is a timer task (a pool thread sleeps — fine
        // for Phase 1's pool sizes).
        if let Expr::Field(f) = &*call.callee {
            if let Expr::Path(qn) = &*f.object {
                if qn.segments.len() == 1 && qn.segments[0].text == "Task" {
                    let prev = self.emitting_format_arg;
                    self.emitting_format_arg = false;
                    match f.field.text.as_str() {
                        "all" => {
                            self.w.push_str(
                                "crate::__jux_spawn(async move { futures::future::join_all(vec![",
                            );
                            for (i, arg) in call.args.iter().enumerate() {
                                if i > 0 {
                                    self.w.push_str(", ");
                                }
                                self.emit_expr(arg);
                            }
                            self.w.push_str("]).await })");
                            self.emitting_format_arg = prev;
                            return;
                        }
                        "race" => {
                            self.w.push_str(
                                "crate::__jux_spawn(async move { futures::future::select_all(vec![",
                            );
                            for (i, arg) in call.args.iter().enumerate() {
                                if i > 0 {
                                    self.w.push_str(", ");
                                }
                                self.emit_expr(arg);
                            }
                            self.w.push_str("]).await.0 })");
                            self.emitting_format_arg = prev;
                            return;
                        }
                        "delay" => {
                            self.w.push_str(
                                "crate::__jux_spawn(async move { std::thread::sleep(std::time::Duration::from_millis((",
                            );
                            if let Some(ms) = call.args.first() {
                                self.emit_expr(ms);
                            } else {
                                self.w.push('0');
                            }
                            self.w.push_str(") as u64)) })");
                            self.emitting_format_arg = prev;
                            return;
                        }
                        _ => {
                            self.emitting_format_arg = prev;
                        }
                    }
                }
            }
        }
        // `spawn(f)` — JUX-ASYNC v2 §18.1.3: schedule the zero-arg
        // lambda's body on the task pool, returning a JuxTask<T>
        // immediately. The body inlines into an `async move` block
        // (no closure indirection), so an async lambda's awaits work
        // and a sync body just computes its value.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "spawn" {
                // Clone-rebind shared captures: a lambda capture
                // moves into the task, but the caller usually keeps
                // using the value (channels especially). Re-binding
                // `let x = x.clone();` in a wrapper block hands the
                // task its own handle. Only known non-primitive
                // locals rebind (primitives are Copy; body-local
                // names aren't in scope here).
                let mut rebinds: Vec<String> = Vec::new();
                if let Some(Expr::Lambda(l)) = call.args.first() {
                    let mut names: Vec<String> = Vec::new();
                    crate::exprs::collect_bare_names_in_lambda(l, &mut |n| {
                        if !names.iter().any(|x| x == n) {
                            names.push(n.to_string());
                        }
                    });
                    for name in names {
                        let known = self
                            .local_types
                            .iter()
                            .rev()
                            .find_map(|s| s.get(&name).cloned());
                        if let Some(ty) = known {
                            if !matches!(ty, juxc_tycheck::Ty::Primitive(_)) {
                                rebinds.push(name);
                            }
                        }
                    }
                }
                if rebinds.is_empty() {
                    self.w.push_str("crate::__jux_spawn(async move { ");
                } else {
                    self.w.push_str("crate::__jux_spawn({ ");
                    for name in &rebinds {
                        self.w.push_str("let ");
                        self.w.push_str(name);
                        self.w.push_str(" = ");
                        self.w.push_str(name);
                        self.w.push_str(".clone(); ");
                    }
                    self.w.push_str("async move { ");
                }
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                match call.args.first() {
                    Some(Expr::Lambda(l)) if l.params.is_empty() => match &l.body {
                        juxc_ast::LambdaBody::Expr(e) => self.emit_expr(e),
                        juxc_ast::LambdaBody::Block(b) => {
                            // Trailing-expression block: the last
                            // expression statement is the task's
                            // value (emitted without a semicolon).
                            let (stmts, tail) = match b.statements.split_last() {
                                Some((juxc_ast::Stmt::Expr(t), rest)) => (rest, Some(t)),
                                _ => (&b.statements[..], None),
                            };
                            self.w.push('\n');
                            self.w.indent_inc();
                            for stmt in stmts {
                                self.w.emit_indent();
                                self.emit_stmt(stmt);
                            }
                            if let Some(tail) = tail {
                                self.w.emit_indent();
                                self.emit_expr(tail);
                                self.w.push('\n');
                            }
                            self.w.indent_dec();
                            self.w.emit_indent();
                        }
                    },
                    Some(other) => {
                        // Non-lambda argument: a future-valued
                        // expression — await it inside the task.
                        self.emit_expr(other);
                        self.w.push_str(".await");
                    }
                    None => {}
                }
                self.emitting_format_arg = prev;
                if rebinds.is_empty() {
                    self.w.push_str(" })");
                } else {
                    // close: async block, wrapper block, call paren.
                    self.w.push_str(" } })");
                }
                return;
            }
        }
        // `parallel(a, b, c, ...)` — async-runtime builtin per
        // JUX-ASYNC-ADDENDUM-v2. Wraps `futures::join!(...)` in an
        // `async { ... }` block, so the call evaluates to a **Future**
        // yielding the tuple `(R_a, R_b, R_c, …)`. Uniform shape:
        //
        //   - In async context: `await parallel(a, b)` resolves to
        //     the tuple after both futures complete.
        //   - From sync code:   `block_on(parallel(a, b))` drives
        //     the Future to completion via the executor.
        //
        // The `move` on the async block captures the argument
        // expressions by value (matches Rust's default for async
        // blocks and keeps lifetimes happy when the Future is
        // shuttled across `block_on`).
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "parallel" {
                self.w.push_str("async move { futures::join!(");
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                for (i, arg) in call.args.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.emitting_format_arg = prev;
                self.w.push_str(") }");
                return;
            }
        }
        // `block_on(future)` — async-runtime builtin: drive a Future
        // to completion synchronously, returning its resolved value.
        // Lowers to `futures::executor::block_on(future)`. The user
        // is responsible for ensuring the argument really is a
        // Future (i.e. the result of an `async` call or
        // `parallel(...)`); calling `block_on` on a non-Future
        // surfaces as a rustc type-mismatch at the emit site.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "block_on" {
                self.w.push_str("futures::executor::block_on(");
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                if let Some(arg) = call.args.first() {
                    self.emit_expr(arg);
                }
                self.emitting_format_arg = prev;
                self.w.push(')');
                return;
            }
        }
        // `yield_now()` — cooperative suspension point. Lowers to a
        // call into the emitted runtime helper (`__jux_yield_now()`,
        // defined in the prelude when async is detected). The
        // helper returns a Future; the caller is expected to
        // `await` it (`await yield_now()`), which is how the spec
        // shape reads.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "yield_now" {
                self.w.push_str("crate::__jux_yield_now()");
                return;
            }
        }
        // `Clock.nowMs()` — stdlib wall-clock reading. Routes
        // through the same `__jux_now_ms()` helper as the bare
        // `now_ms()` builtin; the class-qualified form is the
        // Java-shaped entry point per JUX-CORE-LIB-ADDENDUM.
        if let Expr::Field(f) = &*call.callee {
            if let Expr::Path(qn) = &*f.object {
                if qn.segments.len() == 1
                    && qn.segments[0].text == "Clock"
                    && f.field.text == "nowMs"
                {
                    self.w.push_str("crate::__jux_now_ms()");
                    return;
                }
            }
        }
        // `now_ms()` — monotonic-ish clock reading. Lowers to the
        // emitted `__jux_now_ms()` helper (defined in the prelude
        // whenever async support is active, since timing is
        // commonly needed alongside async work). Returns the
        // milliseconds since the UNIX epoch as `i64` — `long` at
        // the Jux level.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "now_ms" {
                self.w.push_str("__jux_now_ms()");
                return;
            }
        }
        // `Worker.spawn(lambda)` — true multi-thread parallelism
        // per JUX-ASYNC-ADDENDUM §18.2. Runs the closure on the
        // OS thread pool, returns a `Task<T>` that can be `await`-ed
        // for the closure's value.
        //
        // Special-case the closure emit: the regular `emit_lambda`
        // wraps every Jux closure in `Rc<dyn Fn>` (so it can be
        // stored / passed around freely), but `Rc` isn't `Send`,
        // so a wrapped closure can't be shipped to a worker
        // thread. Here we strip the wrapper and emit a bare
        // `move || body` closure directly — `Worker::spawn` takes
        // an `FnOnce + Send + 'static`, which a `move ||` closure
        // capturing Send/'static values satisfies natively.
        if let Expr::Field(f) = &*call.callee {
            if let Expr::Path(qn) = &*f.object {
                if qn.segments.len() == 1
                    && qn.segments[0].text == "Worker"
                    && f.field.text == "spawn"
                {
                    // Crate-rooted: the Worker shim lives at the crate
                    // root, while THIS call site may sit inside a
                    // package's module nest.
                    //
                    // Clone-rebind shared captures (same rule as the
                    // event-loop `spawn`): `move ||` would steal the
                    // caller's handle, but Arc-backed values (atomics,
                    // channels) are meant to be SHARED with the worker
                    // — rebinding `let x = x.clone();` in a wrapper
                    // block hands the closure its own handle.
                    let mut rebinds: Vec<String> = Vec::new();
                    if let Some(Expr::Lambda(l)) = call.args.first() {
                        let mut names: Vec<String> = Vec::new();
                        crate::exprs::collect_bare_names_in_lambda(l, &mut |n| {
                            if !names.iter().any(|x| x == n) {
                                names.push(n.to_string());
                            }
                        });
                        for name in names {
                            let known = self
                                .local_types
                                .iter()
                                .rev()
                                .find_map(|s| s.get(&name).cloned());
                            if let Some(ty) = known {
                                if !matches!(ty, juxc_tycheck::Ty::Primitive(_)) {
                                    rebinds.push(name);
                                }
                            }
                        }
                    }
                    self.w.push_str("crate::Worker::spawn(");
                    if !rebinds.is_empty() {
                        self.w.push_str("{ ");
                        for name in &rebinds {
                            self.w.push_str("let ");
                            self.w.push_str(name);
                            self.w.push_str(" = ");
                            self.w.push_str(name);
                            self.w.push_str(".clone(); ");
                        }
                    }
                    let prev = self.emitting_format_arg;
                    self.emitting_format_arg = false;
                    if let Some(arg) = call.args.first() {
                        match arg {
                            Expr::Lambda(l) => self.emit_bare_move_lambda(l),
                            // Anything else (method ref, named fn,
                            // path) goes through as-is — the user
                            // gets a clear rustc error if the
                            // value doesn't satisfy Worker.spawn's
                            // `FnOnce + Send + 'static` bound.
                            _ => self.emit_expr(arg),
                        }
                    }
                    if !rebinds.is_empty() {
                        self.w.push_str(" }");
                    }
                    self.emitting_format_arg = prev;
                    self.w.push(')');
                    return;
                }
            }
        }
        // `File.readText(path)` / `File.writeText(path, body)`
        // / `File.exists(path)` — stdlib I/O entry points per
        // JUX-CORE-LIB-ADDENDUM. Lowers to `std::fs::*` calls;
        // Phase-1 panic-on-error (no Result<T, IOException>
        // wiring yet).
        if let Expr::Field(f) = &*call.callee {
            if let Expr::Path(qn) = &*f.object {
                if qn.segments.len() == 1 && qn.segments[0].text == "File" {
                    let method = f.field.text.as_str();
                    match method {
                        "readText" => {
                            // Borrow the path (AsRef<Path>) so a String
                            // path variable survives for later calls.
                            self.w.push_str("std::fs::read_to_string(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).unwrap()");
                            return;
                        }
                        "writeText" => {
                            // `std::fs::write(&path, &content)` — borrow
                            // BOTH (they satisfy AsRef) so the caller can
                            // keep using its Strings after the write,
                            // instead of moving them out (rustc E0382).
                            self.w.push_str("std::fs::write(&(");
                            if let Some(path) = call.args.first() {
                                self.emit_expr(path);
                            }
                            self.w.push(')');
                            if let Some(content) = call.args.get(1) {
                                self.w.push_str(", &(");
                                self.emit_expr(content);
                                self.w.push(')');
                            }
                            self.w.push_str(").unwrap()");
                            return;
                        }
                        "exists" => {
                            self.w.push_str("std::path::Path::new(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).exists()");
                            return;
                        }
                        "appendText" => {
                            // OpenOptions append+create, then write_all —
                            // wrapped in a block so the handle drops (and
                            // flushes) immediately.
                            self.w.push_str("{ use std::io::Write as _; let mut __jux_f = std::fs::OpenOptions::new().create(true).append(true).open(&(");
                            if let Some(path) = call.args.first() {
                                self.emit_expr(path);
                            }
                            self.w.push_str(")).unwrap(); __jux_f.write_all((");
                            if let Some(content) = call.args.get(1) {
                                self.emit_expr(content);
                            }
                            self.w.push_str(").as_bytes()).unwrap(); }");
                            return;
                        }
                        "readLines" => {
                            self.w.push_str("std::fs::read_to_string(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).unwrap().lines().map(|l| l.to_string()).collect::<Vec<_>>()");
                            return;
                        }
                        "delete" => {
                            self.w.push_str("std::fs::remove_file(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).unwrap()");
                            return;
                        }
                        "listDir" => {
                            self.w.push_str("std::fs::read_dir(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect::<Vec<_>>()");
                            return;
                        }
                        _ => {}
                    }
                }
                // `Path.join/parent/fileName/extension/isDir/isFile` —
                // static path-string helpers (jux.std.io.Path). Paths
                // are plain Strings in Phase-1; the query forms produce
                // `Option<String>` (Jux `String?`).
                if qn.segments.len() == 1 && qn.segments[0].text == "Path" {
                    let method = f.field.text.as_str();
                    match method {
                        "join" => {
                            self.w.push_str("{ let mut __jux_p = std::path::PathBuf::from(&(");
                            if let Some(base) = call.args.first() {
                                self.emit_expr(base);
                            }
                            self.w.push_str(")); __jux_p.push(&(");
                            if let Some(child) = call.args.get(1) {
                                self.emit_expr(child);
                            }
                            self.w.push_str(")); __jux_p.to_string_lossy().into_owned() }");
                            return;
                        }
                        "parent" | "fileName" | "extension" => {
                            let accessor = match method {
                                "parent" => ".parent().map(|x| x.to_string_lossy().into_owned())",
                                "fileName" => ".file_name().map(|x| x.to_string_lossy().into_owned())",
                                _ => ".extension().map(|x| x.to_string_lossy().into_owned())",
                            };
                            self.w.push_str("std::path::Path::new(&(");
                            self.emit_call_args(call);
                            self.w.push_str("))");
                            self.w.push_str(accessor);
                            return;
                        }
                        "isDir" => {
                            self.w.push_str("std::path::Path::new(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).is_dir()");
                            return;
                        }
                        "isFile" => {
                            self.w.push_str("std::path::Path::new(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).is_file()");
                            return;
                        }
                        _ => {}
                    }
                }
                // `Console.readLine()` — stdin line read with the Jux
                // nullable protocol: `None` at EOF, trailing `\r\n` /
                // `\n` stripped on success.
                if qn.segments.len() == 1
                    && qn.segments[0].text == "Console"
                    && f.field.text == "readLine"
                {
                    self.w.push_str("{ let mut __jux_line = String::new(); match std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut __jux_line) { Ok(0) | Err(_) => None, Ok(_) => { while __jux_line.ends_with('\\n') || __jux_line.ends_with('\\r') { __jux_line.pop(); } Some(__jux_line) } } }");
                    return;
                }
                // `Instant.now()` — monotonic time-point capture
                // (jux.std.time). The elapsed readings are instance
                // methods, dispatched in `try_emit_stdlib_method`.
                if qn.segments.len() == 1
                    && qn.segments[0].text == "Instant"
                    && f.field.text == "now"
                {
                    self.w.push_str("std::time::Instant::now()");
                    return;
                }
            }
        }
        // Stdlib method dispatch — rewrites Jux's Java-spec
        // method names (`xs.add(v)`, `s.toUpperCase()`,
        // `m.contains(k)`, …) into the matching Rust shape
        // (`xs.push(v)`, `s.to_uppercase()`,
        // `m.contains_key(&k)`, …). Receiver-type drives the
        // routing — arrays / String / HashMap / HashSet each
        // get a bespoke emit function.
        if self.try_emit_stdlib_method(call) {
            return;
        }
        // Bare-name method-call rewrite inside a class/interface body.
        // `foo(args)` inside `class C` or `interface I` should resolve
        // to `self.foo(args)` when `foo` is a non-static method on
        // the enclosing type (Java's implicit-`this` rule). The
        // resolver pre-declares parameter and local names so a
        // bare-name reference there shadows the method lookup; we
        // only get here when no shadowing happened.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 {
                let name = &qn.segments[0].text;
                let mut on_self = false;
                // Static-method emit: when the bare call resolves
                // to a static on the enclosing class, emit
                // `EnclosingClass::method(args)` so we don't fall
                // through to the generic free-function path
                // (which would emit a bare `method(args)` that
                // Rust can't find).
                let mut as_static_on: Option<String> = None;
                if let Some(iface_name) = &self.enclosing_interface {
                    if let Some((_, iface)) = self.lookup_interface_by_bare_or_fqn(iface_name) {
                        if let Some(m) = iface.methods.get(name.as_str()) {
                            if !m.is_static {
                                on_self = true;
                            }
                        }
                    }
                }
                if !on_self {
                    // Walk the enclosing class's `extends` chain so a
                    // bare call to an inherited method (`name()` in
                    // `Dog.bark()` finding `Animal::name`) resolves
                    // through `self.method()` and Rust's `Deref` does
                    // the rest. Static methods don't inherit Java-
                    // style — we record the FQN so the emitter can
                    // produce `Class::method(args)` instead.
                    let mut cursor: Option<String> = self.enclosing_class.clone();
                    while let Some(class_name) = cursor {
                        let Some(class) = self.lookup_class_by_bare_or_fqn(&class_name) else {
                            break;
                        };
                        if let Some(m) = class.methods.get(name.as_str()) {
                            if m.is_static {
                                as_static_on = Some(class_name.clone());
                            } else {
                                on_self = true;
                            }
                            break;
                        }
                        cursor = class
                            .extends
                            .as_ref()
                            .and_then(|t| t.name.segments.first())
                            .map(|s| s.text.clone());
                    }
                }
                if let Some(class_name) = as_static_on {
                    self.w.push_str(&class_name);
                    self.w.push_str("::");
                    self.w.push_str(name);
                    if let Some(sfx) = self.pending_method_suffix.take() {
                        self.w.push_str(&sfx);
                    }
                    self.w.push('(');
                    let prev = self.emitting_format_arg;
                    self.emitting_format_arg = false;
                    for (i, arg) in call.args.iter().enumerate() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.emitting_format_arg = prev;
                    self.w.push(')');
                    return;
                }
                if on_self {
                    let alias = self.this_alias.as_deref().unwrap_or("self");
                    self.w.push_str(alias);
                    self.w.push('.');
                    self.w.push_str(name);
                    if let Some(sfx) = self.pending_method_suffix.take() {
                        self.w.push_str(&sfx);
                    }
                    self.w.push('(');
                    let prev = self.emitting_format_arg;
                    self.emitting_format_arg = false;
                    for (i, arg) in call.args.iter().enumerate() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.emitting_format_arg = prev;
                    self.w.push(')');
                    return;
                }
            }
        }
        // Safe-navigation method call (`obj?.method(args)`): the
        // callee parses as a `Field` with `safe: true`. Lower to
        // `obj.as_ref().map(|__t| __t.method(args))` so the result
        // is `Option<ReturnType>` and the receiver isn't moved.
        // Cleared inside the closure: args are still consumed
        // values, so the format-arg flag (if set) doesn't leak.
        if let Expr::Field(f) = &*call.callee {
            if f.safe {
                self.emit_safe_method_call(f, call);
                return;
            }
        }
        // Static interface-method call: `Interface.staticMethod(args)`
        // → `<Interface>::staticMethod(args)`. Interface methods
        // declared `static` lower to Rust trait associated functions
        // and are called the same way as class statics. We check
        // this BEFORE the class-static path because interfaces are
        // a separate namespace; `path_resolves_to_class_in_emit`
        // doesn't see them.
        if let Expr::Field(f) = &*call.callee {
            if let Expr::Path(qn) = &*f.object {
                if qn.segments.len() == 1 {
                    let iface_name = &qn.segments[0].text;
                    if let Some(iface) = self.symbols.interfaces.get(iface_name) {
                        if iface
                            .methods
                            .get(f.field.text.as_str())
                            .map(|m| m.is_static)
                            .unwrap_or(false)
                        {
                            // `Iface_method` free function — see
                            // `emit_interface_decl` for the
                            // companion definition site.
                            self.w.push_str(iface_name);
                            self.w.push('_');
                            self.w.push_str(&f.field.text);
                            self.w.push('(');
                            let prev = self.emitting_format_arg;
                            self.emitting_format_arg = false;
                            for (i, arg) in call.args.iter().enumerate() {
                                if i > 0 {
                                    self.w.push_str(", ");
                                }
                                self.emit_expr(arg);
                            }
                            self.emitting_format_arg = prev;
                            self.w.push(')');
                            return;
                        }
                    }
                }
            }
        }
        // Static method call: `ClassName.staticMethod(args)` (or
        // `pkg.Cls.method(args)`) → `Path::method(args)`. Recognize
        // the receiver as a class name and switch the dot to `::`.
        if let Expr::Field(f) = &*call.callee {
            if let Expr::Path(qn) = &*f.object {
                if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                    let is_static_method = self
                        .symbols
                        .classes
                        .get(&class_fqn)
                        .and_then(|c| c.methods.get(f.field.text.as_str()))
                        .map(|m| m.is_static)
                        .unwrap_or(false);
                    if is_static_method {
                        // §G.9.2: a static call on a foreign stub class
                        // (`Url.parse(...)`) lowers through its REAL Rust path
                        // (`url::Url::parse(...)`) from the `@rust` annotation,
                        // not the flat `crate::rust::url::Url` spelling.
                        let external_real = self
                            .symbols
                            .classes
                            .get(&class_fqn)
                            .filter(|c| c.is_external)
                            .and_then(|c| c.rust_path.clone());
                        if let Some(real) = external_real {
                            self.w.push_str(&real);
                        } else {
                            self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                        }
                        self.w.push_str("::");
                        self.w.push_str(&f.field.text);
                        if let Some(sfx) = self.pending_method_suffix.take() {
                            self.w.push_str(&sfx);
                        }
                        self.w.push('(');
                        // Args of a regular call consume their values
                        // — clear the format-arg flag so any nested
                        // string literal still self-coerces into
                        // owned `String` (the param's declared type).
                        // Per-arg nullable-wrap: when the static
                        // method's matching positional parameter is
                        // `T?`, a non-nullable value is lifted into
                        // `Some(value)`.
                        let prev = self.emitting_format_arg;
                        self.emitting_format_arg = false;
                        for (i, arg) in call.args.iter().enumerate() {
                            if i > 0 {
                                self.w.push_str(", ");
                            }
                            // Interface-typed param slot: wrap a class value in
                            // `Rc<dyn Trait>` / clone a dyn handle.
                            if let Some(pty) = self.callee_param_type(&call.callee, i) {
                                if !matches!(
                                    self.iface_coercion_to(&pty, arg),
                                    crate::analysis::IfaceCoercion::None,
                                ) {
                                    self.emit_expr_coerced_to_iface(&pty, arg);
                                    continue;
                                }
                            }
                            let nullable = self.callee_param_is_nullable(&call.callee, i);
                            let upcast = self.arg_needs_sealed_upcast(&call.callee, i, arg);
                            // Foreign by-ref param (`&str`, …): re-attach the
                            // call-site borrow (§G.9.2). Resolved directly off the
                            // already-known static method, since the class-name
                            // receiver never appears in `expr_types`.
                            let is_ref = self
                                .symbols
                                .classes
                                .get(&class_fqn)
                                .and_then(|c| c.methods.get(f.field.text.as_str()))
                                .and_then(|m| m.params.get(i))
                                .map(|p| p.is_ref)
                                .unwrap_or(false);
                            if is_ref {
                                self.w.push('&');
                            }
                            self.emit_arg_with_nullable_wrap(arg, nullable);
                            if upcast {
                                self.w.push_str(".into()");
                            } else if !nullable && self.wrapper_value_needs_clone(arg) {
                                // Wrapper-class share-on-pass (§CR.4.1) —
                                // same shared-handle rule as the generic
                                // call path, for `Class.staticMethod(arg)`.
                                self.w.push_str(".clone()");
                            }
                        }
                        self.emitting_format_arg = prev;
                        self.w.push(')');
                        return;
                    }
                }
            }
        }
        // **Borrow-hoist pre-pass.** `a.addTwice(a.bump())` on a plain
        // (non-wrapper) class would emit two overlapping `&mut a`
        // borrows — rustc E0499 (two-phase borrows only cover SHARED
        // argument borrows). Java semantics evaluate the argument
        // first, so when any argument contains a call to a mutating
        // method on the SAME receiver place, hoist every argument into
        // a temp inside a block expression:
        //
        //   { let __jux_arg0 = a.bump(); a.addTwice(__jux_arg0) }
        //
        // Hoisting ALL args (not just the offending one) preserves the
        // left-to-right evaluation order. Wrapper-class receivers don't
        // need this (their methods take `&self`, interior-mutable), but
        // applying the hoist there too would be harmless — the trigger
        // simply fires on the textual shape.
        if self.call_needs_borrow_hoist(call) {
            self.emit_call_with_hoisted_args(call);
            return;
        }
        // **Re-entrancy borrow-hoist.** If the receiver is read through a
        // wrapper `.0.borrow()` guard, hoist it into a temp so the guard drops
        // before the call — otherwise a re-entrant method (one that, directly
        // or through a callee, mutates the same object) panics `already
        // borrowed` (§CR.4.1).
        if let Some(cf) = self.callee_receiver_reads_through_borrow(&call.callee) {
            self.emit_call_with_hoisted_receiver(call, cf);
            return;
        }
        // **Function-typed field call** — `obj.task()` where `task` is
        // declared as a `() -> T` field (stored as `Rc<dyn Fn(…)>`).
        // Methods live on the wrapper newtype, so `emit_call_callee=true`
        // suppresses `.0.borrow()` to avoid the guard. But function-typed
        // fields live INSIDE `C_Inner`, so the borrow IS required. Detect
        // and handle this before the generic path sets the flag.
        if let Expr::Field(f) = &*call.callee {
            let class_bare = if matches!(*f.object, Expr::This(_)) {
                self.enclosing_class.clone()
            } else {
                self.receiver_class_bare(&f.object)
            };
            // Use lookup_class_by_bare_or_fqn (bare-name aware) instead of
            // symbols.lookup_field (FQN-only) so probes.TaskRunner resolves
            // from the bare "TaskRunner" key stored in enclosing_class.
            let is_fn_field = class_bare.as_deref().and_then(|bare| {
                let class = self.lookup_class_by_bare_or_fqn(bare)?;
                class.fields.get(f.field.text.as_str())
            }).map(|fsig| fsig.ty.fn_shape.is_some()).unwrap_or(false);
            if is_fn_field {
                // Emit as `(field_read)(args)` — parens prevent Rust from
                // interpreting this as a method call on the struct/wrapper.
                // For plain structs: `(self.task)(args)`
                // For wrapper classes: `(self.0.borrow().task.clone())(args)`
                // Both are valid because Rc<dyn Fn(...)> implements Fn via Deref.
                self.w.push('(');
                self.emit_expr(&call.callee);  // emitting_call_callee=false → borrow fires
                self.w.push(')');
                self.w.push('(');
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                for (i, arg) in call.args.iter().enumerate() {
                    if i > 0 { self.w.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.emitting_format_arg = prev;
                self.w.push(')');
                return;
            }
        }
        // Generic call: emit `callee(args, …)` literally. Post Fix 1
        // every Jux `String` value is already an owned Rust `String`,
        // so the previous per-arg enum-variant payload coercion is
        // unnecessary — the string-literal site self-coerces inside
        // `emit_literal` and identifier references are typed `String`
        // directly.
        // Mark the callee so the outermost `Field` (the method name)
        // skips the wrapper `.0.borrow()` rewrite — a method lives on
        // the newtype, not in `C_Inner`, even when a same-named field
        // exists up the chain (`legs` field + `legs()` method).
        let prev_callee = self.emitting_call_callee;
        self.emitting_call_callee = true;
        // Clear the borrow-context flags while emitting the callee. The
        // *receiver* of a method call (`recv.method()`) is a fresh
        // evaluation, never a Display/comparison slot itself — only the
        // call's RESULT flows into the surrounding format-arg /
        // comparison position. Leaving these set would wrongly suppress
        // the statement-scoped clone on a wrapper-borrowed field receiver
        // (`$"${this.item.greet()}"` → the `.item` read through
        // `.0.borrow()` must clone out before `.greet()` takes `&mut`).
        let prev_fmt = std::mem::take(&mut self.emitting_format_arg);
        let prev_cmp = std::mem::take(&mut self.emitting_comparison_operand);
        self.emit_expr(&call.callee);
        self.emitting_format_arg = prev_fmt;
        self.emitting_comparison_operand = prev_cmp;
        self.emitting_call_callee = prev_callee;
        // Explicit call-site type arguments (`id<int>(5)`) lower to a
        // Rust turbofish `id::<i32>(5)`. Required for correctness: Rust
        // would otherwise infer the type-param from the argument
        // literals/values, silently ignoring the user's annotation
        // (`identity<long>(5)` must bind `T = i64`, not the `i32` the
        // literal would default to). Each arg is lowered as a
        // generic-arg slot (owned `String`, `Rc<dyn …>` for poly/iface
        // types) so it matches how the same `T` is monomorphized when
        // the call relies on inference.
        if !call.explicit_generic_args.is_empty() {
            self.w.push_str("::<");
            for (i, ty) in call.explicit_generic_args.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                if crate::analysis::is_jux_string_type(ty) {
                    self.w.push_str("String");
                } else {
                    self.emit_value_type_as_rust(ty);
                }
            }
            self.w.push('>');
        }
        self.w.push('(');
        // Same flag discipline as above: a regular call's args
        // consume String values, so any inner string literal needs
        // the Fix-1 self-coerce — clear the format-arg context here.
        // Per-arg nullable-wrap when the callee's declared
        // parameter type is `T?` and the value isn't already
        // `Option<T>`-shaped.
        // Per-arg sealed-upcast wrap when the param is a sealed
        // parent and the arg is one of its permitted subclasses:
        // emit `arg.into()` so the auto-`From<Sub> for Sealed`
        // impl from `emit_sealed_enum` lifts the subclass into
        // the matching variant.
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = false;
        for (i, arg) in call.args.iter().enumerate() {
            if i > 0 { self.w.push_str(", "); }
            // §G.9.2: a borrowed parameter (`&T`) of an external method gets the
            // call-site `&` back — `m.containsKey("a")` → `m.contains_key(&"a"…)`.
            if self.callee_param_is_ref(&call.callee, i) {
                self.w.push('&');
            }
            self.emit_call_arg_value(call, i, arg);
        }
        self.emitting_format_arg = prev;
        self.w.push(')');

        // Phase-1 workaround: Rust's `Vec::pop` returns `Option<T>` but
        // Jux doesn't yet have an `Option` type, so Jux user code uses
        // `var top = stack.pop();` expecting a `T`-typed value. We
        // bridge that by appending `.unwrap()` here — pop on an empty
        // Vec then panics, which mirrors Java's `NoSuchElementException`
        // shape. Remove this special case once `Option<T>` lands and
        // pop can return `T?` directly.
        if let Expr::Field(f) = &*call.callee {
            if f.field.text == "pop" && call.args.is_empty() {
                self.w.push_str(".unwrap()");
            }
        }
    }

    /// Emit ONE call argument through the full coercion ladder —
    /// interface/poly-base `Rc<dyn>` wrap, sealed upcast `.into()`,
    /// nullable `Some(…)` wrap, and the wrapper share-on-pass
    /// `.clone()` (§CR.4.1). Shared between the regular inline arg
    /// loop and the borrow-hoisted form (`let __jux_argN = …;`), so
    /// both produce identical values. The by-ref `&` prefix is NOT
    /// emitted here — it stays at the call slot (a hoisted temp is
    /// borrowed at the call, `x.m(&__jux_arg0)`).
    fn emit_call_arg_value(&mut self, call: &CallExpr, i: usize, arg: &Expr) {
        // `out <place>` argument (§M.4): pass `&mut <place>` — no value
        // coercion / share-clone. `emit_expr` handles the `Expr::Out` shape.
        if matches!(arg, Expr::Out(..)) {
            self.emit_expr(arg);
            return;
        }
        // Interface-typed param slot: wrap a class value in `Rc<dyn
        // Trait>` / clone a dyn handle, before the sealed/nullable
        // paths (which never apply to an interface value slot).
        if let Some(pty) = self.callee_param_type(&call.callee, i) {
            if !matches!(
                self.iface_coercion_to(&pty, arg),
                crate::analysis::IfaceCoercion::None,
            ) {
                self.emit_expr_coerced_to_iface(&pty, arg);
                return;
            }
        }
        let nullable = self.callee_param_is_nullable(&call.callee, i);
        let upcast = self.arg_needs_sealed_upcast(&call.callee, i, arg);
        if upcast {
            self.emit_arg_with_nullable_wrap(arg, nullable);
            self.w.push_str(".into()");
        } else {
            self.emit_arg_with_nullable_wrap(arg, nullable);
            // **Wrapper-class share-on-pass (§CR.4.1).** A wrapped
            // place passed as an argument hands the callee a SHARED
            // handle — append the cheap `Rc` refcount-bump clone so
            // the caller's binding stays live and both point at the
            // same `RefCell` (mutation through the param is observed
            // by the caller). Skipped under nullable/upcast wraps,
            // which never carry a bare wrapped place.
            if !nullable && self.wrapper_value_needs_clone(arg) {
                self.w.push_str(".clone()");
            }
        }
    }

    /// True when `call` needs the **borrow-hoist** form — the callee is
    /// a method on a simple place (`x.m(…)` / `this.m(…)`) and some
    /// argument contains a call to a *mutating* method on that same
    /// place. Emitted inline, receiver and argument would hold two
    /// overlapping `&mut` borrows (rustc E0499/E0502 — two-phase
    /// borrows only cover shared argument borrows). Wrapper-class
    /// receivers are exempt: their methods take `&self` and mutate
    /// through the interior `RefCell`, so no conflict exists.
    fn call_needs_borrow_hoist(&self, call: &CallExpr) -> bool {
        let Expr::Field(f) = call.callee.as_ref() else { return false };
        let root: &str = match f.object.as_ref() {
            Expr::Path(qn) if qn.segments.len() == 1 => &qn.segments[0].text,
            Expr::This(_) => "this",
            _ => return false,
        };
        // A class-named receiver is a static call (no instance borrow);
        // a wrapper-class instance dispatches through `&self`.
        if root != "this" {
            if self.lookup_class_by_bare_or_fqn(root).is_some() {
                return false;
            }
            let recv_class = self
                .local_types
                .iter()
                .rev()
                .find_map(|s| s.get(root))
                .and_then(|ty| match ty {
                    juxc_tycheck::Ty::User { name, .. } => {
                        Some(name.rsplit('.').next().unwrap_or(name).to_string())
                    }
                    _ => None,
                });
            if let Some(c) = recv_class {
                if self.wrapper_classes.contains(&c) {
                    return false;
                }
            }
        } else if let Some(enclosing) = &self.enclosing_class {
            // Inside a wrapper class's own method, `this.m(this.bump())`
            // dispatches through `&self` too.
            if self.wrapper_classes.contains(enclosing) {
                return false;
            }
        }
        call.args
            .iter()
            .any(|a| self.contains_mut_call_on(a, root))
    }

    /// When the callee is `recv.method(...)` and `recv` is itself read through
    /// a wrapper `.0.borrow()` guard (a wrapper-class instance field), return
    /// the callee `Field`. Such a call holds the receiver's `borrow()` alive
    /// across `method(...)`; if `method` re-enters and mutates the same object
    /// (`a.bump()` → `b.ping(a)` → `a.bump()`), the re-entrant `borrow_mut()`
    /// panics `already borrowed`. The fix (see `emit_call_with_hoisted_receiver`)
    /// hoists the receiver into a temp so the guard drops before the call —
    /// upholding §CR.4.1's statement-scoped borrow discipline under re-entrancy.
    fn callee_receiver_reads_through_borrow<'c>(
        &self,
        callee: &'c Expr,
    ) -> Option<&'c juxc_ast::FieldExpr> {
        let Expr::Field(cf) = callee else { return None };
        // Look through a `!!` non-null assertion on the receiver: `this.inner!!.m()`
        // parses as `Field(NotNullAssert(Field(inner)), m)`. The `!!` doesn't change
        // that `.inner` is read through the wrapper's `.0.borrow()` guard, so the
        // same statement-scoped re-entrancy hazard applies and the receiver must
        // still be hoisted into a temp before `m(...)` runs.
        let recv = match cf.object.as_ref() {
            Expr::NotNullAssert(inner, _) => inner.as_ref(),
            other => other,
        };
        let Expr::Field(rf) = recv else { return None };
        if self.receiver_is_wrapper_class(&rf.object)
            && self
                .wrapper_field_parent_depth(&rf.object, &rf.field.text)
                .is_some()
        {
            Some(cf)
        } else {
            None
        }
    }

    /// Recursive walk: does `e` contain a call to a mutating method
    /// (per `user_mut_methods`) whose receiver is the bare place
    /// `root` (`x.bump()` for root `x`, `this.bump()` for `this`)?
    fn contains_mut_call_on(&self, e: &Expr, root: &str) -> bool {
        match e {
            Expr::Call(c) => {
                if let Expr::Field(f) = c.callee.as_ref() {
                    let on_root = match f.object.as_ref() {
                        Expr::Path(qn) => {
                            qn.segments.len() == 1 && qn.segments[0].text == root
                        }
                        Expr::This(_) => root == "this",
                        _ => false,
                    };
                    if on_root && self.user_mut_methods.contains(&f.field.text) {
                        return true;
                    }
                }
                self.contains_mut_call_on(&c.callee, root)
                    || c.args.iter().any(|a| self.contains_mut_call_on(a, root))
            }
            Expr::Binary(b) => {
                self.contains_mut_call_on(&b.left, root)
                    || self.contains_mut_call_on(&b.right, root)
            }
            Expr::Unary(u) => self.contains_mut_call_on(&u.operand, root),
            Expr::Field(f) => self.contains_mut_call_on(&f.object, root),
            Expr::Index(ix) => {
                self.contains_mut_call_on(&ix.array, root)
                    || self.contains_mut_call_on(&ix.index, root)
            }
            Expr::Cast(c) => self.contains_mut_call_on(&c.value, root),
            _ => false,
        }
    }

    /// Emit `x.m(args…)` in the **borrow-hoisted** block form — every
    /// argument lands in a `let __jux_argN` temp (evaluated left to
    /// right, full coercion ladder via `emit_call_arg_value`), then the
    /// call reads only the temps:
    ///
    ///   { let __jux_arg0 = a.bump(); a.addTwice(__jux_arg0) }
    ///
    /// The argument's `&mut` borrow ends at its `;`, so the receiver
    /// borrow that follows is the only live one. Mirrors the regular
    /// path's callee flag discipline, turbofish, by-ref `&`, and the
    /// `pop()`-unwrap special.
    fn emit_call_with_hoisted_args(&mut self, call: &CallExpr) {
        self.w.push_str("{ ");
        let prev_args_fmt = self.emitting_format_arg;
        self.emitting_format_arg = false;
        for (i, arg) in call.args.iter().enumerate() {
            self.w.push_str("let __jux_arg");
            self.w.push_str(&i.to_string());
            self.w.push_str(" = ");
            self.emit_call_arg_value(call, i, arg);
            self.w.push_str("; ");
        }
        self.emitting_format_arg = prev_args_fmt;
        let prev_callee = self.emitting_call_callee;
        self.emitting_call_callee = true;
        let prev_fmt = std::mem::take(&mut self.emitting_format_arg);
        let prev_cmp = std::mem::take(&mut self.emitting_comparison_operand);
        self.emit_expr(&call.callee);
        self.emitting_format_arg = prev_fmt;
        self.emitting_comparison_operand = prev_cmp;
        self.emitting_call_callee = prev_callee;
        if !call.explicit_generic_args.is_empty() {
            self.w.push_str("::<");
            for (i, ty) in call.explicit_generic_args.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                if crate::analysis::is_jux_string_type(ty) {
                    self.w.push_str("String");
                } else {
                    self.emit_value_type_as_rust(ty);
                }
            }
            self.w.push('>');
        }
        self.w.push('(');
        for i in 0..call.args.len() {
            if i > 0 {
                self.w.push_str(", ");
            }
            if self.callee_param_is_ref(&call.callee, i) {
                self.w.push('&');
            }
            self.w.push_str("__jux_arg");
            self.w.push_str(&i.to_string());
        }
        self.w.push(')');
        if let Expr::Field(f) = &*call.callee {
            if f.field.text == "pop" && call.args.is_empty() {
                self.w.push_str(".unwrap()");
            }
        }
        self.w.push_str(" }");
    }

    /// Emit `recv.m(args…)` with the RECEIVER hoisted out of its `.0.borrow()`:
    ///
    ///   { let __jux_recv = <recv>; __jux_recv.m(args) }
    ///
    /// `recv` (a wrapper-class instance field) clones out of the borrow when
    /// bound, so the guard drops at the `;` — releasing it BEFORE `m(...)` runs.
    /// Without this, a re-entrant `m` that mutates the same object panics with
    /// `already borrowed` (§CR.4.1). Args stay inline: each is a temporary whose
    /// own borrow ends before `m` is entered, and `__jux_recv` is already owned.
    fn emit_call_with_hoisted_receiver(
        &mut self,
        call: &CallExpr,
        callee: &juxc_ast::FieldExpr,
    ) {
        self.w.push_str("{ let __jux_recv = ");
        // Value position → the wrapper-field read appends `.clone()`, producing
        // an owned handle and dropping the `borrow()` temporary at the `;`.
        let prev_fmt = std::mem::take(&mut self.emitting_format_arg);
        let prev_cmp = std::mem::take(&mut self.emitting_comparison_operand);
        self.emit_expr(&callee.object);
        self.emitting_format_arg = prev_fmt;
        self.emitting_comparison_operand = prev_cmp;
        self.w.push_str("; __jux_recv.");
        self.w.push_str(&callee.field.text);
        if let Some(sfx) = self.pending_method_suffix.take() {
            self.w.push_str(&sfx);
        }
        if !call.explicit_generic_args.is_empty() {
            self.w.push_str("::<");
            for (i, ty) in call.explicit_generic_args.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                if crate::analysis::is_jux_string_type(ty) {
                    self.w.push_str("String");
                } else {
                    self.emit_value_type_as_rust(ty);
                }
            }
            self.w.push('>');
        }
        self.w.push('(');
        let prev = std::mem::take(&mut self.emitting_format_arg);
        for (i, arg) in call.args.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            if self.callee_param_is_ref(&call.callee, i) {
                self.w.push('&');
            }
            self.emit_call_arg_value(call, i, arg);
        }
        self.emitting_format_arg = prev;
        self.w.push(')');
        if callee.field.text == "pop" && call.args.is_empty() {
            self.w.push_str(".unwrap()");
        }
        self.w.push_str(" }");
    }

    /// Lower `obj?.method(args)` to
    /// `obj.as_ref().map(|__t| __t.method(args))`. Closure body
    /// emits with `emitting_format_arg=false` so any string-literal
    /// arg still self-coerces — same discipline as a regular
    /// `emit_call`'s args. The result type is `Option<ReturnType>`.
    pub(crate) fn emit_safe_method_call(
        &mut self,
        callee: &juxc_ast::FieldExpr,
        call: &CallExpr,
    ) {
        let needs_parens = !matches!(
            *callee.object,
            Expr::Path(_)
                | Expr::This(_)
                | Expr::Field(_)
                | Expr::Call(_)
                | Expr::Index(_)
                | Expr::Literal(_)
                | Expr::InterpString(_)
                | Expr::NewObject(_)
                | Expr::NewArray(_)
                | Expr::NewArrayLit(_)
        );
        if needs_parens {
            self.w.push('(');
        }
        self.emit_expr(&callee.object);
        if needs_parens {
            self.w.push(')');
        }
        // `.and_then` flattens when the called method itself returns `T?`
        // (`a?.getC()` where `getC(): C?` yields `Option<C>`, not
        // `Option<Option<C>>` — otherwise a further `?.` chains off the wrong
        // type). `.map` for a non-nullable return. Stdlib methods stay `.map`
        // (their nullable lowering is handled inside the closure).
        if self.safe_method_returns_nullable(callee) {
            self.w.push_str(".as_ref().and_then(|__t| ");
        } else {
            self.w.push_str(".as_ref().map(|__t| ");
        }
        // **Route through the stdlib-method dispatch with `__t` as receiver**
        // (gap N7): a String/collection method on a nullable receiver
        // (`s?.length()`, `xs?.size()`) must map to its Rust equivalent
        // (`length` → `.chars().count() as isize`), not emit the raw Jux name.
        // `__t` is `&Underlying` from `as_ref()`; type it as the receiver's
        // underlying (non-nullable) type and synthesize a plain `__t.method(args)`
        // call for `try_emit_stdlib_method` to lower.
        let underlying = self
            .expr_types
            .get(&crate::exprs::expr_span_of(&callee.object))
            .map(|t| {
                let mut u = t;
                while let juxc_tycheck::Ty::Nullable(inner) = u {
                    u = inner;
                }
                u.clone()
            });
        let mut handled = false;
        if let Some(uty) = underlying {
            let synth = CallExpr {
                callee: Box::new(Expr::Field(juxc_ast::FieldExpr {
                    object: Box::new(Expr::Path(juxc_ast::QualifiedName {
                        segments: vec![juxc_ast::Ident {
                            text: "__t".to_string(),
                            span: callee.span,
                        }],
                        span: callee.span,
                    })),
                    field: callee.field.clone(),
                    safe: false,
                    span: callee.span,
                })),
                explicit_generic_args: Vec::new(),
                args: call.args.clone(),
                arg_names: vec![None; call.args.len()],
                span: call.span,
            };
            // Expose `__t`'s type for the duration of the synthetic dispatch
            // (the bare-receiver type lookup reads `local_types`).
            let mut scope = std::collections::HashMap::new();
            scope.insert("__t".to_string(), uty);
            self.local_types.push(scope);
            handled = self.try_emit_stdlib_method(&synth);
            self.local_types.pop();
        }
        if !handled {
            // Plain user-method (or unknown receiver): emit `__t.method(args)`.
            self.w.push_str("__t.");
            self.w.push_str(&callee.field.text);
            self.w.push('(');
            let prev = self.emitting_format_arg;
            self.emitting_format_arg = false;
            for (i, arg) in call.args.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.emit_expr(arg);
            }
            self.emitting_format_arg = prev;
            self.w.push(')');
        }
        self.w.push(')');
    }

    /// True iff the user method named by a `?.`-call returns a nullable `T?`,
    /// so [`Self::emit_safe_method_call`] flattens with `.and_then` instead of
    /// `.map`. Resolves the receiver's underlying (non-nullable) class from
    /// `expr_types` and walks the `extends` chain. Unknown / stdlib methods
    /// return false — their `.map` form is correct (any nullable stdlib
    /// lowering is produced inside the closure, already `Option`-shaped).
    fn safe_method_returns_nullable(&self, callee: &juxc_ast::FieldExpr) -> bool {
        // Resolve the receiver's class structurally (robust to unrecorded
        // intermediate safe-nav spans, e.g. `a?.b()?.c()`).
        let recvc = match self.safe_nav_member_class_bare(&callee.object) {
            Some(c) => c,
            None => return false,
        };
        let method = callee.field.text.as_str();
        let mut cursor = self.lookup_class_by_bare_or_fqn(&recvc);
        while let Some(sig) = cursor {
            if let Some(m) = sig.methods.get(method) {
                return matches!(
                    &m.return_type,
                    juxc_ast::ReturnType::Type(t) | juxc_ast::ReturnType::AsyncType(t)
                        if t.nullable
                );
            }
            cursor = sig
                .extends_fqn
                .as_deref()
                .and_then(|p| self.symbols.classes.get(p));
        }
        false
    }

    /// Lower a call to the built-in `print(…)` into the most natural Rust
    /// `println!` shape we can.
    ///
    /// Rules:
    /// - `print("literal")` → `println!("literal")`. We bake the string
    ///   directly into the format-string slot, doubling any `{` / `}` so
    ///   `println!`'s parser keeps its hands off them.
    /// - `print(expr)` (single non-literal arg) → `println!("{}", expr)`.
    /// - `print(a, b, …)` (multi-arg) → `println!("{} {} …", a, b, …)`
    ///   with one `{}` per argument. This is a placeholder shape until
    ///   `std.io.print` is properly specced.
    pub(crate) fn emit_print_call(&mut self, call: &CallExpr) {
        // Hot path: one string-literal argument. Inline it as the format.
        if call.args.len() == 1 {
            if let Expr::Literal(Literal::String(s)) = &call.args[0] {
                self.w.push_str("println!(");
                self.emit_rust_format_string_literal(s);
                self.w.push(')');
                return;
            }
            // Hot path: a string-concat chain (`"a" + b + "c"`) as
            // the sole argument. The naive lowering would be
            // `println!("{}", format!("{}{}{}", "a", b, "c"))` — a
            // wasted heap alloc for the intermediate `String`.
            // Inline the concat's operands directly as `println!`
            // args so the macro formats straight into the writer,
            // AND fold any literal operands into the template so
            // we end up with one `println!("hello, {}!", name)`
            // instead of `println!("{}{}{}", "hello, ", name, "!")`.
            if let Expr::Binary(b) = &call.args[0] {
                // Mirror the binary emitter's string-concat trigger:
                // literal-shape OR `Ty::String`-typed either side.
                // Either condition routes through the inline-print
                // path and the intermediate `format!` evaporates.
                let lhs_string = is_string_literal(&b.left)
                    || self.operand_is_string_typed_for_print(&b.left);
                let rhs_string = is_string_literal(&b.right)
                    || self.operand_is_string_typed_for_print(&b.right);
                if b.op == juxc_ast::BinaryOp::Add && (lhs_string || rhs_string) {
                    let mut operands: Vec<&Expr> = Vec::new();
                    flatten_concat(b, &mut operands);
                    let (template, runtime) =
                        fold_concat_for_print(&operands);
                    self.w.push_str("println!(\"");
                    self.w.push_str(&template);
                    self.w.push('"');
                    let prev = self.emitting_format_arg;
                    self.emitting_format_arg = true;
                    for op in &runtime {
                        self.w.push_str(", ");
                        self.emit_format_arg(op);
                    }
                    self.emitting_format_arg = prev;
                    self.w.push(')');
                    return;
                }
            }
            // Hot path: one interpolated-string argument. Inline its
            // segments directly into the println! call instead of
            // emitting `println!("{}", format!("…", args))`. Same
            // shape format!() would produce, one less call frame.
            if let Expr::InterpString(s) = &call.args[0] {
                self.w.push_str("println!(\"");
                let mut bare_args: Vec<&juxc_ast::Ident> = Vec::new();
                let mut expr_args: Vec<&Expr> = Vec::new();
                let mut arg_order: Vec<ArgRef> = Vec::new();
                for seg in &s.segments {
                    match seg {
                        juxc_ast::InterpSegment::Literal(text) => {
                            self.emit_interp_literal_chunk(text);
                        }
                        juxc_ast::InterpSegment::Bare(ident) => {
                            self.w.push_str("{}");
                            bare_args.push(ident);
                            arg_order.push(ArgRef::Bare(bare_args.len() - 1));
                        }
                        juxc_ast::InterpSegment::Expr(expr) => {
                            self.w.push_str("{}");
                            expr_args.push(expr);
                            arg_order.push(ArgRef::Expr(expr_args.len() - 1));
                        }
                    }
                }
                self.w.push('"');
                // `println!` borrows its args, so nested string
                // literals stay `&str` (saves an alloc per literal).
                // Nullable args are wrapped in `JuxOpt(&v)` so
                // `Display` works — `Some(v)` prints `v`, `None`
                // prints `"null"`.
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = true;
                for arg_ref in &arg_order {
                    self.w.push_str(", ");
                    match arg_ref {
                        ArgRef::Bare(i) => {
                            // Bare-ident interp `$name` — synthesize
                            // a Path expression so `emit_format_arg`
                            // can run its nullable-shape check.
                            let qn = juxc_ast::QualifiedName {
                                segments: vec![bare_args[*i].clone()],
                                span: bare_args[*i].span,
                            };
                            let synth = Expr::Path(qn);
                            self.emit_format_arg(&synth);
                        }
                        ArgRef::Expr(i) => self.emit_format_arg(expr_args[*i]),
                    }
                }
                self.emitting_format_arg = prev;
                self.w.push(')');
                return;
            }
        }
        // General path: one `{}` placeholder per arg, then the args.
        self.w.push_str("println!(\"");
        for i in 0..call.args.len() {
            if i > 0 {
                self.w.push(' ');
            }
            self.w.push_str("{}");
        }
        self.w.push('"');
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = true;
        for arg in &call.args {
            self.w.push_str(", ");
            self.emit_format_arg(arg);
        }
        self.emitting_format_arg = prev;
        self.w.push(')');
    }

    /// Stdlib method dispatch — rewrites Jux's spec-level method
    /// names (`add`, `isEmpty`, `toUpperCase`, …) on arrays and
    /// `String` receivers into the matching Rust shape.
    ///
    /// Returns `true` when this path handled the call (so the
    /// surrounding `emit_call` should return immediately). Returns
    /// `false` for any call shape this method doesn't recognize —
    /// receiver type unknown, method name unknown, receiver isn't
    /// a method call's Field-callee, etc. — and lets the regular
    /// emit path proceed.
    ///
    /// The receiver's type comes from `expr_types`, the tycheck
    /// inference map. The dispatch is best-effort: if the
    /// expression hasn't been typed (e.g. inside a lambda body
    /// where inference doesn't run), the helper falls through and
    /// the user gets either the regular emit (which may compile
    /// if the method name happens to be a Vec/String method) or a
    /// clear rustc error pointing at the offending site.
    pub(crate) fn try_emit_stdlib_method(&mut self, call: &CallExpr) -> bool {
        // Must be a `receiver.method(args)` shape.
        let Expr::Field(f) = &*call.callee else {
            return false;
        };
        let method = f.field.text.as_str();
        // Receiver-type lookup. Three paths:
        //   1. `local_types` map for Path receivers — keyed by
        //      name, immune to span collisions inside interp
        //      strings.
        //   2. `expr_types` map (the normal route — typed by the
        //      inference pass for paths, calls, fields).
        //   3. Literal short-circuit — literal expressions have
        //      `Span::DUMMY`, so they never appear in the map. We
        //      special-case string and array literals here.
        let recv_span = crate::exprs::expr_span_of(&f.object);
        let recv_ty_from_locals: Option<juxc_tycheck::Ty> =
            if let Expr::Path(qn) = &*f.object {
                if qn.segments.len() == 1 {
                    let bare = qn.segments[0].text.as_str();
                    self.local_types
                        .iter()
                        .rev()
                        .find_map(|scope| scope.get(bare).cloned())
                } else {
                    None
                }
            } else {
                None
            };
        let recv_ty = recv_ty_from_locals
            .or_else(|| self.expr_types.get(&recv_span).cloned())
            .or_else(|| match &*f.object {
                Expr::Literal(juxc_ast::Literal::String(_)) => {
                    Some(juxc_tycheck::Ty::String)
                }
                // Numeric / char receivers built purely from literals
                // (`255.toHex()`, `(0.0 / 0.0).isNaN()`) — literals
                // carry `Span::DUMMY`, and a binary over two literals
                // JOINS those into another DUMMY span, so neither has
                // an `expr_types` entry. Type them structurally so
                // §K.11 intrinsics still dispatch.
                e => literal_numeric_ty(e).map(juxc_tycheck::Ty::Primitive),
            });
        let Some(recv_ty) = recv_ty else {
            return false;
        };
        let is_array = matches!(&recv_ty, juxc_tycheck::Ty::Array { .. });
        let is_string =
            matches!(&recv_ty, juxc_tycheck::Ty::String);
        let is_map = matches!(
            &recv_ty,
            juxc_tycheck::Ty::User { name, .. }
                if name.rsplit('.').next().unwrap_or(name) == "HashMap"
        );
        let is_set = matches!(
            &recv_ty,
            juxc_tycheck::Ty::User { name, .. }
                if name.rsplit('.').next().unwrap_or(name) == "HashSet"
        );
        let is_deque = matches!(
            &recv_ty,
            juxc_tycheck::Ty::User { name, .. }
                if name.rsplit('.').next().unwrap_or(name) == "Deque"
        );
        // `Instant` elapsed readings (jux.std.time) — the receiver is
        // a Copy `std::time::Instant` value.
        if matches!(
            &recv_ty,
            juxc_tycheck::Ty::User { name, .. }
                if name.rsplit('.').next().unwrap_or(name) == "Instant"
        ) {
            let suffix = match method {
                "elapsedMs" => ".elapsed().as_millis() as i64",
                "elapsedNanos" => ".elapsed().as_nanos() as i64",
                _ => return false,
            };
            self.emit_expr(&f.object);
            self.w.push_str(suffix);
            return true;
        }
        // `AtomicInt` / `AtomicLong` (§S.6.2) — Arc<Atomic*> handles.
        // The no-order overloads default to SeqCst; explicit orders
        // pass the Jux `MemoryOrder` through the emitted
        // `__jux_order` adapter. `fetch*` return the PREVIOUS value.
        if matches!(
            &recv_ty,
            juxc_tycheck::Ty::User { name, .. }
                if matches!(
                    name.rsplit('.').next().unwrap_or(name),
                    "AtomicInt" | "AtomicLong"
                )
        ) {
            let rust = match method {
                "load" => "load",
                "store" => "store",
                "fetchAdd" => "fetch_add",
                "fetchSub" => "fetch_sub",
                "fetchAnd" => "fetch_and",
                "fetchOr" => "fetch_or",
                "fetchXor" => "fetch_xor",
                _ => return false,
            };
            // The ordering is the LAST argument when the overload
            // carries one: load(order) has 1 arg, store/fetch*(v,
            // order) have 2.
            let order_arg = match (method, call.args.len()) {
                ("load", 1) => call.args.first(),
                (_, 2) => call.args.get(1),
                _ => None,
            };
            let prev = self.emitting_format_arg;
            self.emitting_format_arg = false;
            self.emit_expr(&f.object);
            self.w.push('.');
            self.w.push_str(rust);
            self.w.push('(');
            // The value operand (store / fetch* first arg).
            if method != "load" {
                if let Some(value) = call.args.first() {
                    self.emit_expr(value);
                    self.w.push_str(", ");
                }
            }
            match order_arg {
                Some(order) => {
                    self.w.push_str("crate::__jux_order(");
                    self.emit_expr(order);
                    self.w.push(')');
                }
                None => self.w.push_str("std::sync::atomic::Ordering::SeqCst"),
            }
            self.w.push(')');
            self.emitting_format_arg = prev;
            return true;
        }
        // Numeric / char intrinsics (§K.11) — Primitive-typed
        // receivers get their own dispatch table.
        if let juxc_tycheck::Ty::Primitive(prim) = &recv_ty {
            return self.emit_numeric_stdlib_method(call, method, *prim);
        }
        if !is_array && !is_string && !is_map && !is_set && !is_deque {
            return false;
        }
        // **Gap N1: mutating collection method on a wrapped-class field.**
        // `this.items.add(v)` where `items` is a collection field of a
        // shared-reference class reads the field through `borrow_mut()` and
        // hoists args ahead of that borrow — see `emit_mut_collection_method`.
        // (String has no mutating-in-place methods on this path, so it's
        // excluded by `collection_method_mutates`.)
        if self.collection_method_mutates(&recv_ty, method)
            && self.callee_receiver_reads_through_borrow(&call.callee).is_some()
        {
            return self.emit_mut_collection_method(call, method, &recv_ty);
        }
        if is_array {
            return self.emit_array_stdlib_method(call, method);
        }
        if is_string {
            return self.emit_string_stdlib_method(call, method);
        }
        if is_map {
            return self.emit_map_stdlib_method(call, method);
        }
        if is_set {
            return self.emit_set_stdlib_method(call, method);
        }
        if is_deque {
            return self.emit_deque_stdlib_method(call, method);
        }
        false
    }

    /// Emit the Rust equivalent of a Jux `Deque<T>` method call —
    /// lowered onto `std::collections::VecDeque<T>`. The remove/peek
    /// forms return `T?` in Jux, which is exactly the `Option<T>` the
    /// Rust methods produce (peeks clone the element out).
    fn emit_deque_stdlib_method(&mut self, call: &CallExpr, method: &str) -> bool {
        let Expr::Field(f) = &*call.callee else {
            return false;
        };
        let receiver = &*f.object;
        match method {
            "addFirst" => {
                self.emit_expr(receiver);
                self.w.push_str(".push_front(");
                self.emit_call_args(call);
                self.w.push(')');
                true
            }
            "addLast" => {
                self.emit_expr(receiver);
                self.w.push_str(".push_back(");
                self.emit_call_args(call);
                self.w.push(')');
                true
            }
            "removeFirst" => {
                self.emit_expr(receiver);
                self.w.push_str(".pop_front()");
                true
            }
            "removeLast" => {
                self.emit_expr(receiver);
                self.w.push_str(".pop_back()");
                true
            }
            "peekFirst" => {
                self.emit_expr(receiver);
                self.w.push_str(".front().cloned()");
                true
            }
            "peekLast" => {
                self.emit_expr(receiver);
                self.w.push_str(".back().cloned()");
                true
            }
            "contains" => {
                self.emit_expr(receiver);
                self.w.push_str(".contains(&(");
                self.emit_call_args(call);
                self.w.push_str("))");
                true
            }
            "size" => {
                self.emit_expr(receiver);
                self.w.push_str(".len() as isize");
                true
            }
            "isEmpty" => {
                self.emit_expr(receiver);
                self.w.push_str(".is_empty()");
                true
            }
            "clear" => {
                self.emit_expr(receiver);
                self.w.push_str(".clear()");
                true
            }
            _ => false,
        }
    }

    /// Emit the Rust equivalent of a Jux `HashMap<K, V>` method
    /// call. Returns `true` when the method was handled.
    fn emit_map_stdlib_method(&mut self, call: &CallExpr, method: &str) -> bool {
        let Expr::Field(f) = &*call.callee else {
            return false;
        };
        let receiver = &*f.object;
        match method {
            "put" => {
                self.emit_expr(receiver);
                self.w.push_str(".insert(");
                self.emit_call_args(call);
                self.w.push(')');
                true
            }
            "get" => {
                self.emit_expr(receiver);
                self.w.push_str(".get(&(");
                self.emit_call_args(call);
                self.w.push_str(")).cloned().unwrap()");
                true
            }
            "contains" => {
                self.emit_expr(receiver);
                self.w.push_str(".contains_key(&(");
                self.emit_call_args(call);
                self.w.push_str("))");
                true
            }
            "remove" => {
                self.emit_expr(receiver);
                self.w.push_str(".remove(&(");
                self.emit_call_args(call);
                self.w.push_str(")).unwrap()");
                true
            }
            "size" => {
                self.emit_expr(receiver);
                self.w.push_str(".len() as isize");
                true
            }
            "isEmpty" => {
                self.emit_expr(receiver);
                self.w.push_str(".is_empty()");
                true
            }
            "clear" => {
                self.emit_expr(receiver);
                self.w.push_str(".clear()");
                true
            }
            "keys" => {
                self.emit_expr(receiver);
                self.w
                    .push_str(".keys().cloned().collect::<Vec<_>>()");
                true
            }
            "values" => {
                self.emit_expr(receiver);
                self.w
                    .push_str(".values().cloned().collect::<Vec<_>>()");
                true
            }
            _ => false,
        }
    }

    /// Emit the Rust equivalent of a Jux `HashSet<T>` method call.
    fn emit_set_stdlib_method(&mut self, call: &CallExpr, method: &str) -> bool {
        let Expr::Field(f) = &*call.callee else {
            return false;
        };
        let receiver = &*f.object;
        match method {
            "add" => {
                self.emit_expr(receiver);
                self.w.push_str(".insert(");
                self.emit_call_args(call);
                self.w.push(')');
                true
            }
            "contains" => {
                self.emit_expr(receiver);
                self.w.push_str(".contains(&(");
                self.emit_call_args(call);
                self.w.push_str("))");
                true
            }
            "remove" => {
                self.emit_expr(receiver);
                self.w.push_str(".remove(&(");
                self.emit_call_args(call);
                self.w.push_str("))");
                true
            }
            "size" => {
                self.emit_expr(receiver);
                self.w.push_str(".len() as isize");
                true
            }
            "isEmpty" => {
                self.emit_expr(receiver);
                self.w.push_str(".is_empty()");
                true
            }
            "clear" => {
                self.emit_expr(receiver);
                self.w.push_str(".clear()");
                true
            }
            _ => false,
        }
    }

    /// Emit a stdlib-collection method **receiver** that the method will
    /// mutate (`add`/`push`, `set`, `remove`, `put`, `clear`, …). When the
    /// receiver is a field of a shared-reference (wrapped) class, the field
    /// must be read through the **mutable** interior borrow
    /// (`self.0.borrow_mut().items`) — the default read path takes
    /// `self.0.borrow().items`, an immutable `Ref`, so `.push()`/`.insert()`
    /// fail to compile (E0596). Setting both `emitting_out_place` (selects
    /// `borrow_mut()` in `emit_field`) and `emitting_lvalue` (suppresses the
    /// auto-`.clone()` that would otherwise mutate a throwaway copy) gives the
    /// exact `self.0.borrow_mut().items` shape. A non-wrapper receiver (a plain
    /// local `Vec`) is a `Path`, never reaches `emit_field`, so the flags are
    /// harmless there.
    fn emit_mut_collection_receiver(&mut self, receiver: &Expr) {
        let prev_out = self.emitting_out_place;
        let prev_lv = self.emitting_lvalue;
        self.emitting_out_place = true;
        self.emitting_lvalue = true;
        self.emit_expr(receiver);
        self.emitting_out_place = prev_out;
        self.emitting_lvalue = prev_lv;
    }

    /// True when `method` **mutates** its stdlib-collection receiver — the
    /// methods that need `&mut` on the underlying `Vec`/`HashMap`/`HashSet`/
    /// `VecDeque`. Read-only methods (`size`, `get`, `contains`, `keys`, …)
    /// answer `false`. Drives the gap-N1 borrow_mut routing.
    fn collection_method_mutates(&self, recv_ty: &juxc_tycheck::Ty, method: &str) -> bool {
        match recv_ty {
            juxc_tycheck::Ty::Array { .. } => matches!(
                method,
                "add" | "set" | "remove" | "insert" | "clear" | "reverse" | "sort"
            ),
            juxc_tycheck::Ty::User { name, .. } => {
                match name.rsplit('.').next().unwrap_or(name) {
                    "HashMap" => matches!(method, "put" | "remove" | "clear"),
                    "HashSet" => matches!(method, "add" | "remove" | "clear"),
                    "Deque" => matches!(
                        method,
                        "addFirst" | "addLast" | "removeFirst" | "removeLast" | "clear"
                    ),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// Emit a **mutating** stdlib-collection method whose receiver is a field
    /// of a shared-reference (wrapped) class — `this.items.add(v)` (gap N1).
    /// Two defects are fixed together:
    ///   - **A (mutability):** the field is read through `borrow_mut()` (via
    ///     the receiver-mut flags) so the mutation lands in the real cell, not
    ///     a temporary `Ref` (would be rustc E0596).
    ///   - **B (re-entrancy):** every argument is hoisted into a temp BEFORE
    ///     the receiver borrow is taken, so an argument that re-enters the same
    ///     object (`this.items.add(this.next())`) runs its own short-lived
    ///     borrow first instead of colliding with the open collection borrow
    ///     (would be a runtime `already borrowed` panic).
    /// The temps carry the full element coercion ladder; the delegated per-kind
    /// emitter then reads bare temps (`collection_args_prehoisted`).
    fn emit_mut_collection_method(
        &mut self,
        call: &CallExpr,
        method: &str,
        recv_ty: &juxc_tycheck::Ty,
    ) -> bool {
        // Delegate to the per-kind emitter with the receiver-mut flags set.
        let dispatch = |this: &mut Self, c: &CallExpr| -> bool {
            let prev_out = this.emitting_out_place;
            let prev_lv = this.emitting_lvalue;
            let prev_hoist = this.collection_args_prehoisted;
            this.emitting_out_place = true;
            this.emitting_lvalue = true;
            this.collection_args_prehoisted = true;
            let handled = match recv_ty {
                juxc_tycheck::Ty::Array { .. } => this.emit_array_stdlib_method(c, method),
                juxc_tycheck::Ty::User { name, .. } => {
                    match name.rsplit('.').next().unwrap_or(name) {
                        "HashMap" => this.emit_map_stdlib_method(c, method),
                        "HashSet" => this.emit_set_stdlib_method(c, method),
                        "Deque" => this.emit_deque_stdlib_method(c, method),
                        _ => false,
                    }
                }
                _ => false,
            };
            this.emitting_out_place = prev_out;
            this.emitting_lvalue = prev_lv;
            this.collection_args_prehoisted = prev_hoist;
            handled
        };
        // No args → no re-entrancy / coercion to hoist; emit in place.
        if call.args.is_empty() {
            return dispatch(self, call);
        }
        self.w.push_str("{ ");
        let prev_fmt = std::mem::take(&mut self.emitting_format_arg);
        for (i, arg) in call.args.iter().enumerate() {
            self.w.push_str("let __jux_carg");
            self.w.push_str(&i.to_string());
            self.w.push_str(" = ");
            self.emit_collection_arg(call, i, arg);
            self.w.push_str("; ");
        }
        self.emitting_format_arg = prev_fmt;
        // Synthetic call: same callee/receiver, args replaced by the temps.
        let temp_args: Vec<Expr> = (0..call.args.len())
            .map(|i| {
                Expr::Path(juxc_ast::QualifiedName {
                    segments: vec![juxc_ast::Ident {
                        text: format!("__jux_carg{i}"),
                        span: call.span,
                    }],
                    span: call.span,
                })
            })
            .collect();
        let temp_call = CallExpr {
            callee: call.callee.clone(),
            explicit_generic_args: call.explicit_generic_args.clone(),
            args: temp_args,
            arg_names: vec![None; call.args.len()],
            span: call.span,
        };
        let handled = dispatch(self, &temp_call);
        self.w.push_str(" }");
        handled
    }

    /// Emit the Rust equivalent of a Jux `List<T>` / array method
    /// call. Returns `true` when the method was handled.
    fn emit_array_stdlib_method(&mut self, call: &CallExpr, method: &str) -> bool {
        let Expr::Field(f) = &*call.callee else {
            return false;
        };
        // Helpers — emit the receiver with proper grouping, and
        // the comma-separated arg list with format-arg flag
        // cleared so nested string literals self-coerce.
        let receiver = &*f.object;
        match method {
            // `xs.add(v)` → `xs.push(v)` — Java/spec name vs Rust.
            "add" => {
                self.emit_expr(receiver);
                self.w.push_str(".push(");
                self.emit_call_args(call);
                self.w.push(')');
                true
            }
            // `xs.size()` → `xs.len() as isize` — same as `.length`
            // field shape but used as a method.
            "size" => {
                self.emit_expr(receiver);
                self.w.push_str(".len() as isize");
                true
            }
            // `xs.isEmpty()` → `xs.is_empty()` — pure rename.
            "isEmpty" => {
                self.emit_expr(receiver);
                self.w.push_str(".is_empty()");
                true
            }
            // `xs.contains(v)` → `xs.contains(&v)` — Rust needs &T.
            "contains" => {
                self.emit_expr(receiver);
                self.w.push_str(".contains(&(");
                self.emit_call_args(call);
                self.w.push_str("))");
                true
            }
            // `xs.indexOf(v)` → linear scan returning -1 on miss.
            // Matches Java's API contract.
            "indexOf" => {
                self.w.push_str("(");
                self.emit_expr(receiver);
                self.w.push_str(".iter().position(|__e| *__e == ");
                self.emit_call_args(call);
                self.w.push_str(").map(|__i| __i as isize).unwrap_or(-1))");
                true
            }
            // `xs.get(i)` → `xs[i as usize].clone()` — clone so the
            // value-shape consistent with index-access elsewhere.
            "get" => {
                self.emit_expr(receiver);
                self.w.push_str("[(");
                self.emit_call_args(call);
                self.w.push_str(") as usize].clone()");
                true
            }
            // `xs.set(i, v)` → block expression that mutates in
            // place, returning the old value (consistent with
            // Java's List.set contract).
            "set" => {
                self.w.push_str("{ let __i = (");
                // Args are (index, value). Emit index first then value.
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                if let Some(idx) = call.args.first() {
                    self.emit_expr(idx);
                }
                self.w.push_str(") as usize; let __old = ");
                self.emit_expr(receiver);
                self.w.push_str("[__i].clone(); ");
                self.emit_expr(receiver);
                self.w.push_str("[__i] = ");
                if let Some(val) = call.args.get(1) {
                    self.emit_expr(val);
                }
                self.emitting_format_arg = prev;
                self.w.push_str("; __old }");
                true
            }
            // `xs.first()` / `xs.last()` — indexed access with clone.
            "first" => {
                self.emit_expr(receiver);
                self.w.push_str("[0].clone()");
                true
            }
            "last" => {
                self.w.push('(');
                self.emit_expr(receiver);
                self.w.push_str(".last().cloned().unwrap())");
                true
            }
            // `xs.clear()` / `xs.reverse()` / `xs.sort()` — direct
            // Rust equivalents.
            "clear" | "reverse" => {
                self.emit_expr(receiver);
                self.w.push('.');
                self.w.push_str(method);
                self.w.push_str("()");
                true
            }
            "sort" => {
                self.emit_expr(receiver);
                self.w.push_str(".sort()");
                true
            }
            // `xs.remove(i)` / `xs.insert(i, v)` with isize→usize cast.
            "remove" => {
                self.emit_expr(receiver);
                self.w.push_str(".remove((");
                self.emit_call_args(call);
                self.w.push_str(") as usize)");
                true
            }
            "insert" => {
                self.emit_expr(receiver);
                self.w.push_str(".insert((");
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                if let Some(idx) = call.args.first() {
                    self.emit_expr(idx);
                }
                self.w.push_str(") as usize, ");
                if let Some(val) = call.args.get(1) {
                    self.emit_expr(val);
                }
                self.emitting_format_arg = prev;
                self.w.push(')');
                true
            }
            // `xs.join(sep)` — only well-defined for `Vec<String>`;
            // Rust's `Vec<String>::join(&str)` returns String.
            "join" => {
                self.emit_expr(receiver);
                self.w.push_str(".join(&(");
                self.emit_call_args(call);
                self.w.push_str("))");
                true
            }
            // forEach: iterator chain calling the closure on each
            // borrowed element. Closure capture rules let it borrow
            // surrounding state.
            "forEach" => {
                self.emit_expr(receiver);
                self.w.push_str(".iter().for_each(|__e| (");
                self.emit_call_args(call);
                self.w.push_str(")(__e.clone()))");
                true
            }
            // map / filter: collect into a fresh Vec so the result
            // stays Jux-array-shaped.
            "map" => {
                self.emit_expr(receiver);
                self.w
                    .push_str(".iter().cloned().map(|__e| (");
                self.emit_call_args(call);
                self.w.push_str(")(__e)).collect::<Vec<_>>()");
                true
            }
            "filter" => {
                self.emit_expr(receiver);
                self.w
                    .push_str(".iter().cloned().filter(|__e| (");
                self.emit_call_args(call);
                self.w.push_str(")(__e.clone())).collect::<Vec<_>>()");
                true
            }
            _ => false,
        }
    }

    /// Numeric / char intrinsics (§K.11) on primitive receivers.
    /// Numeric receivers cast to their exact Rust type first — that
    /// resolves Rust's ambiguous-`{integer}` inference, pins the
    /// method set, AND keeps width semantics honest (a `byte`
    /// wrapping-add wraps at 8 bits, not pointer width). Chars
    /// dispatch on `char` directly. Checked forms produce the Jux
    /// `Result<T, E>` enum.
    fn emit_numeric_stdlib_method(
        &mut self,
        call: &CallExpr,
        method: &str,
        prim: juxc_tycheck::Primitive,
    ) -> bool {
        use juxc_tycheck::Primitive as P;
        let Expr::Field(f) = &*call.callee else { return false };
        let receiver = &f.object;
        let is_float = matches!(prim, P::Float | P::Double | P::F32 | P::F64);
        let is_char = matches!(prim, P::Char);
        if matches!(prim, P::Bool) {
            return false;
        }
        // Exact Rust spelling of the receiver's primitive — the cast
        // target that keeps overflow/wrap behavior width-faithful.
        let rust_ty: &str = match prim {
            P::Int => "isize",
            P::Uint => "usize",
            P::Byte | P::I8 => "i8",
            P::Ubyte | P::U8 => "u8",
            P::Short | P::I16 => "i16",
            P::Ushort | P::U16 => "u16",
            P::Long | P::I64 => "i64",
            P::Ulong | P::U64 => "u64",
            P::I32 => "i32",
            P::U32 => "u32",
            P::Float | P::F32 => "f32",
            P::Double | P::F64 => "f64",
            P::Char | P::Bool => "",
        };
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = false;
        let emit_recv = |this: &mut Self| {
            this.w.push_str("((");
            this.emit_expr(receiver);
            if is_char {
                this.w.push(')');
            } else {
                this.w.push_str(") as ");
                this.w.push_str(rust_ty);
            }
            this.w.push(')');
        };
        let simple: Option<&str> = if is_char {
            match method {
                "isDigit" => Some(".is_ascii_digit()"),
                "isAlphabetic" => Some(".is_alphabetic()"),
                "isWhitespace" => Some(".is_whitespace()"),
                "isUppercase" => Some(".is_uppercase()"),
                "isLowercase" => Some(".is_lowercase()"),
                "toUppercase" => Some(".to_ascii_uppercase()"),
                "toLowercase" => Some(".to_ascii_lowercase()"),
                // `codePoint()` — the Unicode scalar value as `uint`.
                "codePoint" => Some(" as usize"),
                _ => None,
            }
        } else if is_float {
            match method {
                "sqrt" => Some(".sqrt()"),
                "floor" => Some(".floor()"),
                "ceil" => Some(".ceil()"),
                // Spec: round-half-to-even (banker's rounding).
                "round" => Some(".round_ties_even()"),
                "abs" => Some(".abs()"),
                "isNaN" => Some(".is_nan()"),
                "isInfinite" => Some(".is_infinite()"),
                "isFinite" => Some(".is_finite()"),
                // IEEE bit pattern, widened to `uint` (§K.11).
                "bits" => Some(".to_bits() as usize"),
                _ => None,
            }
        } else {
            // `abs` only exists on SIGNED integers in Rust; unsigned
            // receivers fall through to the generic passthrough (and
            // rustc's method set) rather than emitting a bad call.
            let signed = !rust_ty.starts_with('u');
            match method {
                "abs" if signed => Some(".abs()"),
                "saturatingAbs" if signed => Some(".saturating_abs()"),
                "countOnes" => Some(".count_ones() as isize"),
                "leadingZeros" => Some(".leading_zeros() as isize"),
                "trailingZeros" => Some(".trailing_zeros() as isize"),
                _ => None,
            }
        };
        if let Some(suffix) = simple {
            emit_recv(self);
            self.w.push_str(suffix);
            self.emitting_format_arg = prev;
            return true;
        }
        // One-argument float forms (§K.11).
        if is_float {
            match method {
                // Exact bit equality, NaN payloads included.
                "bitsEqual" => {
                    emit_recv(self);
                    self.w.push_str(".to_bits() == ((");
                    self.emit_call_args(call);
                    self.w.push_str(") as ");
                    self.w.push_str(rust_ty);
                    self.w.push_str(").to_bits()");
                    self.emitting_format_arg = prev;
                    return true;
                }
                // IEEE 754 total order (backs `<=>` on floats):
                // -Inf < … < -0.0 < +0.0 < … < +Inf < NaN.
                "totalOrder" => {
                    emit_recv(self);
                    self.w.push_str(".total_cmp(&((");
                    self.emit_call_args(call);
                    self.w.push_str(") as ");
                    self.w.push_str(rust_ty);
                    self.w.push_str(")) as isize");
                    self.emitting_format_arg = prev;
                    return true;
                }
                // Fixed-decimal formatting: `3.14159.toFixed(2)` → "3.14".
                "toFixed" => {
                    self.w.push_str("format!(\"{:.1$}\", ");
                    emit_recv(self);
                    self.w.push_str(", (");
                    self.emit_call_args(call);
                    self.w.push_str(") as usize)");
                    self.emitting_format_arg = prev;
                    return true;
                }
                _ => {}
            }
        }
        // One-argument integer forms.
        if !is_float && !is_char {
            let one_arg: Option<&str> = match method {
                "saturatingAdd" => Some("saturating_add"),
                "saturatingSub" => Some("saturating_sub"),
                "saturatingMul" => Some("saturating_mul"),
                "wrappingAdd" => Some("wrapping_add"),
                "wrappingSub" => Some("wrapping_sub"),
                "wrappingMul" => Some("wrapping_mul"),
                _ => None,
            };
            if let Some(rust) = one_arg {
                emit_recv(self);
                self.w.push('.');
                self.w.push_str(rust);
                self.w.push_str("((");
                self.emit_call_args(call);
                self.w.push_str(") as ");
                self.w.push_str(rust_ty);
                self.w.push(')');
                self.emitting_format_arg = prev;
                return true;
            }
            let rotate: Option<&str> = match method {
                "rotateLeft" => Some("rotate_left"),
                "rotateRight" => Some("rotate_right"),
                _ => None,
            };
            if let Some(rust) = rotate {
                emit_recv(self);
                self.w.push('.');
                self.w.push_str(rust);
                self.w.push_str("((");
                self.emit_call_args(call);
                self.w.push_str(") as u32)");
                self.emitting_format_arg = prev;
                return true;
            }
            // Checked arithmetic → the Jux Result enum (§K.11).
            let checked: Option<&str> = match method {
                "checkedAdd" => Some("checked_add"),
                "checkedSub" => Some("checked_sub"),
                "checkedMul" => Some("checked_mul"),
                "checkedDiv" => Some("checked_div"),
                _ => None,
            };
            if let Some(rust) = checked {
                self.w.push_str("(match ");
                emit_recv(self);
                self.w.push('.');
                self.w.push_str(rust);
                self.w.push_str("((");
                self.emit_call_args(call);
                self.w.push_str(") as ");
                self.w.push_str(rust_ty);
                self.w.push_str(") { Some(__jux_v) => crate::jux::std::result::Result::Ok(__jux_v), None => crate::jux::std::result::Result::Err(crate::jux::std::exceptions::ArithmeticException::new(\"");
                self.w.push_str(method);
                self.w.push_str(" overflowed\".to_string())) })");
                self.emitting_format_arg = prev;
                return true;
            }
            // Width conversions to `int` (§K.11). `toInt` is checked
            // (Result); `saturatingToInt` clamps. Comparing through
            // `i128` covers every source width and signedness.
            if method == "toInt" {
                self.w.push_str("(match isize::try_from(");
                emit_recv(self);
                self.w.push_str(") { Ok(__jux_v) => crate::jux::std::result::Result::Ok(__jux_v), Err(_) => crate::jux::std::result::Result::Err(crate::jux::std::exceptions::ArithmeticException::new(\"toInt out of range\".to_string())) })");
                self.emitting_format_arg = prev;
                return true;
            }
            if method == "saturatingToInt" {
                self.w.push_str("({ let __jux_v = ");
                emit_recv(self);
                self.w.push_str(" as i128; if __jux_v > isize::MAX as i128 { isize::MAX } else if __jux_v < isize::MIN as i128 { isize::MIN } else { __jux_v as isize } })");
                self.emitting_format_arg = prev;
                return true;
            }
            // Radix formatting.
            let radix: Option<&str> = match method {
                "toHex" => Some("{:x}"),
                "toBinary" => Some("{:b}"),
                "toOctal" => Some("{:o}"),
                _ => None,
            };
            if let Some(fmt) = radix {
                self.w.push_str("format!(\"");
                self.w.push_str(fmt);
                self.w.push_str("\", ");
                emit_recv(self);
                self.w.push(')');
                self.emitting_format_arg = prev;
                return true;
            }
        }
        self.emitting_format_arg = prev;
        false
    }

    fn emit_string_stdlib_method(&mut self, call: &CallExpr, method: &str) -> bool {
        let Expr::Field(f) = &*call.callee else {
            return false;
        };
        let receiver = &*f.object;
        match method {
            // `s.length()` → `s.chars().count() as isize` — Java's
            // length counts code-units, but Phase-1 lowers to
            // char-count for usability. A `len_bytes()` variant
            // can land later when raw-byte counts matter.
            "length" => {
                self.emit_expr(receiver);
                self.w.push_str(".chars().count() as isize");
                true
            }
            "isEmpty" => {
                self.emit_expr(receiver);
                self.w.push_str(".is_empty()");
                true
            }
            // §K.7: explicit length forms. `byteLength` is the
            // UTF-8 byte count (Rust `len`); `charLength` counts
            // scalar values (O(N) per the spec note).
            "byteLength" => {
                self.emit_expr(receiver);
                self.w.push_str(".len() as isize");
                true
            }
            "charLength" => {
                self.emit_expr(receiver);
                self.w.push_str(".chars().count() as isize");
                true
            }
            "repeat" => {
                self.emit_expr(receiver);
                self.w.push_str(".repeat((");
                self.emit_call_args(call);
                self.w.push_str(") as usize)");
                true
            }
            // Pure renames: snake_case Rust spelling.
            "toUpperCase" => {
                self.emit_expr(receiver);
                self.w.push_str(".to_uppercase()");
                true
            }
            "toLowerCase" => {
                self.emit_expr(receiver);
                self.w.push_str(".to_lowercase()");
                true
            }
            "trim" => {
                self.emit_expr(receiver);
                self.w.push_str(".trim().to_string()");
                true
            }
            "startsWith" => {
                self.emit_expr(receiver);
                self.w.push_str(".starts_with(");
                self.emit_call_args(call);
                self.w.push_str(".as_str())");
                true
            }
            "endsWith" => {
                self.emit_expr(receiver);
                self.w.push_str(".ends_with(");
                self.emit_call_args(call);
                self.w.push_str(".as_str())");
                true
            }
            "contains" => {
                self.emit_expr(receiver);
                self.w.push_str(".contains(");
                self.emit_call_args(call);
                self.w.push_str(".as_str())");
                true
            }
            "replace" => {
                self.emit_expr(receiver);
                self.w.push_str(".replace(");
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                if let Some(needle) = call.args.first() {
                    self.emit_expr(needle);
                }
                self.w.push_str(".as_str(), ");
                if let Some(rep) = call.args.get(1) {
                    self.emit_expr(rep);
                }
                self.w.push_str(".as_str())");
                self.emitting_format_arg = prev;
                true
            }
            "indexOf" => {
                self.w.push('(');
                self.emit_expr(receiver);
                self.w.push_str(".find(");
                self.emit_call_args(call);
                self.w.push_str(".as_str()).map(|__i| __i as isize).unwrap_or(-1))");
                true
            }
            "split" => {
                self.emit_expr(receiver);
                self.w.push_str(".split(");
                self.emit_call_args(call);
                self.w
                    .push_str(".as_str()).map(::std::string::String::from).collect::<Vec<_>>()");
                true
            }
            "substring" => {
                // `s.substring(start, end)` — char-indexed slice.
                self.w.push('(');
                self.emit_expr(receiver);
                self.w.push_str(".chars().skip((");
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                if let Some(start) = call.args.first() {
                    self.emit_expr(start);
                }
                self.w.push_str(") as usize).take(((");
                if let Some(end) = call.args.get(1) {
                    self.emit_expr(end);
                }
                self.w.push_str(") - (");
                if let Some(start) = call.args.first() {
                    self.emit_expr(start);
                }
                self.emitting_format_arg = prev;
                self.w
                    .push_str(")) as usize).collect::<String>())");
                true
            }
            "charAt" => {
                self.emit_expr(receiver);
                self.w.push_str(".chars().nth((");
                self.emit_call_args(call);
                self.w.push_str(") as usize).unwrap()");
                true
            }
            _ => false,
        }
    }

    /// Emit a call's args as a comma-separated list, with the
    /// format-arg flag cleared so nested string literals
    /// self-coerce. Used by the stdlib-method rewriter to splat
    /// the original args into the rewritten Rust shape.
    fn emit_call_args(&mut self, call: &CallExpr) {
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = false;
        for (i, arg) in call.args.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.emit_collection_arg(call, i, arg);
        }
        self.emitting_format_arg = prev;
    }

    /// Emit ONE builtin-container argument with its element coercion
    /// ladder: nullable `Some(…)` wrap and wrapper share-`.clone()`. Shared
    /// by `emit_call_args` and the arg-hoisting path so both produce the
    /// same stored value. When `collection_args_prehoisted` is set the
    /// argument is already a coerced temp, so the ladder is skipped (the
    /// bare temp is emitted) — see that flag's doc.
    fn emit_collection_arg(&mut self, call: &CallExpr, i: usize, arg: &Expr) {
        if self.collection_args_prehoisted {
            self.emit_expr(arg);
            return;
        }
        // **Nullable element slot** — storing into a container whose
        // element type-arg is `T?` (`ArrayList<int?>` → `Vec<Option
        // <isize>>`): a non-null value lifts into `Some(...)`; a
        // `null` literal / already-`Option` value passes through.
        let wrap_some = self
            .builtin_arg_elem_nullable(call, i)
            && !self.expression_is_already_nullable(arg);
        if wrap_some {
            self.w.push_str("Some(");
        }
        self.emit_expr(arg);
        // **Wrapper-class share-on-pass (§CR.4.1)** for the builtin
        // collection dispatches (`xs.add(obj)` → `xs.push(obj)`):
        // storing a wrapped place must SHARE the handle (`Rc`
        // refcount bump), not move it — `l1.add(c); l2.add(c);`
        // would otherwise be a rustc E0382 on the second use, and
        // a mutation through the container element must stay
        // visible through the original binding.
        if self.wrapper_value_needs_clone(arg) {
            self.w.push_str(".clone()");
        }
        if wrap_some {
            self.w.push(')');
        }
    }

    /// True when argument `i` of a **builtin container call** lands in
    /// an element slot whose generic type-arg is nullable — `xs.add(v)`
    /// on an `ArrayList<int?>`, `m.put(k, v)` on a `HashMap<String,
    /// int?>`, etc. Maps the arg index to the receiver's generic-arg
    /// position per method: list `add`/`set@1`/`insert@1`, set `add`,
    /// map `put@1` (values; keys stay non-null). Non-container shapes
    /// answer `false`.
    fn builtin_arg_elem_nullable(&self, call: &CallExpr, arg_idx: usize) -> bool {
        let Expr::Field(f) = call.callee.as_ref() else { return false };
        let method = f.field.text.as_str();
        // Which generic-arg slot does this argument store into?
        let generic_idx = match (method, arg_idx) {
            ("add", 0) => 0,                  // list/set value
            ("set", 1) | ("insert", 1) => 0,  // list value (idx, value)
            ("put", 1) => 1,                  // map value (key, value)
            _ => return false,
        };
        // Receiver type: span-keyed `expr_types` first, then the
        // name-keyed `local_types` fallback (span collisions and
        // unrecorded `Path` leaves miss the first map — same fallback
        // the field/receiver resolvers use).
        let recv_ty = self
            .expr_types
            .get(&crate::exprs::expr_span_of(&f.object))
            .cloned()
            .or_else(|| {
                if let Expr::Path(qn) = f.object.as_ref() {
                    if qn.segments.len() == 1 {
                        return self
                            .local_types
                            .iter()
                            .rev()
                            .find_map(|s| s.get(&qn.segments[0].text))
                            .cloned();
                    }
                }
                None
            });
        match recv_ty {
            // `ArrayList<T>` lowers to `Ty::Array { element }` (dynamic
            // kind), not `Ty::User` — the element IS generic-arg 0.
            Some(juxc_tycheck::Ty::Array { element, .. }) => {
                generic_idx == 0 && matches!(*element, juxc_tycheck::Ty::Nullable(_))
            }
            Some(juxc_tycheck::Ty::User { generic_args, .. }) => matches!(
                generic_args.get(generic_idx),
                Some(juxc_tycheck::Ty::Nullable(_)),
            ),
            _ => false,
        }
    }
}

/// Structural typing for receivers built PURELY from numeric/char
/// literals. Literals have `Span::DUMMY`, and a binary expression over
/// two literals joins those into another DUMMY span, so none of them
/// ever land in `expr_types`. Mixed int/float arithmetic widens to
/// `double`, matching the inference pass. Returns `None` as soon as a
/// non-literal leaf appears (those have real spans and use the map).
pub(crate) fn literal_numeric_ty(e: &Expr) -> Option<juxc_tycheck::Primitive> {
    use juxc_tycheck::Primitive as P;
    match e {
        Expr::Literal(juxc_ast::Literal::Int(_)) => Some(P::Int),
        Expr::Literal(juxc_ast::Literal::Float(_)) => Some(P::Double),
        Expr::Literal(juxc_ast::Literal::Char(_)) => Some(P::Char),
        Expr::Unary(u) => literal_numeric_ty(&u.operand),
        Expr::Binary(b) => {
            let l = literal_numeric_ty(&b.left)?;
            let r = literal_numeric_ty(&b.right)?;
            if matches!(l, P::Char) || matches!(r, P::Char) {
                return None;
            }
            Some(if matches!(l, P::Double) || matches!(r, P::Double) {
                P::Double
            } else {
                l
            })
        }
        _ => None,
    }
}
