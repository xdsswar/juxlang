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
    /// Emit a call expression. Special-cases the built-in `print` to
    /// `println!(…)`. Every other callee is emitted verbatim (the
    /// resolver guarantees the name exists).
    pub(crate) fn emit_call(&mut self, call: &CallExpr) {
        // Recognize a single-segment path `print` for the built-in.
        if let Expr::Path(qn) = &*call.callee {
            if qn.segments.len() == 1 && qn.segments[0].text == "print" {
                return self.emit_print_call(call);
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
                self.w.push_str("__jux_yield_now()");
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
                    self.w.push_str("__jux_now_ms()");
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
                    self.w.push_str("Worker::spawn(");
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
                            self.w.push_str("std::fs::read_to_string(");
                            self.emit_call_args(call);
                            self.w.push_str(").unwrap()");
                            return;
                        }
                        "writeText" => {
                            self.w.push_str("std::fs::write(");
                            self.emit_call_args(call);
                            self.w.push_str(").unwrap()");
                            return;
                        }
                        "exists" => {
                            self.w.push_str("std::path::Path::new(&(");
                            self.emit_call_args(call);
                            self.w.push_str(")).exists()");
                            return;
                        }
                        _ => {}
                    }
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
                        self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                        self.w.push_str("::");
                        self.w.push_str(&f.field.text);
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
                            let nullable = self.callee_param_is_nullable(&call.callee, i);
                            let upcast = self.arg_needs_sealed_upcast(&call.callee, i, arg);
                            self.emit_arg_with_nullable_wrap(arg, nullable);
                            if upcast {
                                self.w.push_str(".into()");
                            }
                        }
                        self.emitting_format_arg = prev;
                        self.w.push(')');
                        return;
                    }
                }
            }
        }
        // Generic call: emit `callee(args, …)` literally. Post Fix 1
        // every Jux `String` value is already an owned Rust `String`,
        // so the previous per-arg enum-variant payload coercion is
        // unnecessary — the string-literal site self-coerces inside
        // `emit_literal` and identifier references are typed `String`
        // directly.
        self.emit_expr(&call.callee);
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
            let nullable = self.callee_param_is_nullable(&call.callee, i);
            let upcast = self.arg_needs_sealed_upcast(&call.callee, i, arg);
            if upcast {
                self.emit_arg_with_nullable_wrap(arg, nullable);
                self.w.push_str(".into()");
            } else {
                self.emit_arg_with_nullable_wrap(arg, nullable);
            }
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
        self.w.push_str(".as_ref().map(|__t| __t.");
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
        self.w.push_str("))");
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
                _ => None,
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
        if !is_array && !is_string && !is_map && !is_set {
            return false;
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
        false
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

    /// Emit the Rust equivalent of a Jux `String` method call.
    /// Returns `true` when the method was handled.
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
            self.emit_expr(arg);
        }
        self.emitting_format_arg = prev;
    }
}
