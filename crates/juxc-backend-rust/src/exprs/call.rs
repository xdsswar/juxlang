//! Call-expression emission — generic function/method calls plus the
//! built-in `print(...)` special case. Both paths share the enum-
//! variant String-payload coercion that injects `.to_string()` on
//! positional args matching a `String` slot.

use juxc_ast::{CallExpr, Expr, Literal};

use crate::exprs::ArgRef;
use crate::RustEmitter;

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
        for (i, arg) in call.args.iter().enumerate() {
            if i > 0 { self.w.push_str(", "); }
            self.emit_expr(arg);
        }
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
                for arg_ref in &arg_order {
                    self.w.push_str(", ");
                    match arg_ref {
                        ArgRef::Bare(i) => self.w.push_str(&bare_args[*i].text),
                        ArgRef::Expr(i) => self.emit_expr(expr_args[*i]),
                    }
                }
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
        for arg in &call.args {
            self.w.push_str(", ");
            self.emit_expr(arg);
        }
        self.w.push(')');
    }
}
