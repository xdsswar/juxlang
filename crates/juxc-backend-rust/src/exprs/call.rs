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
                            self.emit_arg_with_nullable_wrap(arg, nullable);
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
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = false;
        for (i, arg) in call.args.iter().enumerate() {
            if i > 0 { self.w.push_str(", "); }
            let nullable = self.callee_param_is_nullable(&call.callee, i);
            self.emit_arg_with_nullable_wrap(arg, nullable);
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
}
