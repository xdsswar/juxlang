//! Interpolated-string lowering — `$"…"` and `${expr}` segments.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::Expr;

use crate::{ArgRef, RustEmitter};

impl RustEmitter {
    /// Lower an interpolated string literal per §3.4 to a Rust
    /// `format!("…", arg, arg, …)` call — or, when there are no
    /// `${…}` segments at all, to `"…".to_string()` (cheaper, no
    /// `format!` setup, no `useless_format` clippy lint).
    ///
    /// **Format-string assembly.** We walk the segment list:
    /// - `Literal(text)` — write the bytes into the Rust format string,
    ///   doubling any `{` / `}` so `format!`'s own parser keeps its
    ///   hands off them. The lexer already filtered out unterminated
    ///   forms; escape sequences (`\n`, `\t`, …) pass through verbatim
    ///   into the Rust string literal, which interprets them the same
    ///   way Jux's spec asks for.
    /// - `Bare(ident)` / `Expr(expr)` — write `{}` into the format
    ///   string and collect the value into the args list.
    ///
    /// **No-interp form** (Fix 5). When every segment is a literal —
    /// e.g. `$"stop"`, `$""`, `$"hello world"` — we emit
    /// `"…".to_string()` instead of `format!("…")`. Output is a
    /// `String` value in both cases, so callers that store the
    /// result (`var msg = $"stop"`) or pattern-merge it across
    /// `switch` arms see identical types.
    pub(crate) fn emit_interp_string(&mut self, s: &juxc_ast::InterpStringExpr) {
        let has_interp = s.segments.iter().any(|seg| {
            matches!(
                seg,
                juxc_ast::InterpSegment::Bare(_) | juxc_ast::InterpSegment::Expr(_),
            )
        });
        if !has_interp {
            // Fast path: concatenate every literal chunk into a single Rust
            // string literal, then call `.to_string()` on it. No `{` / `}`
            // doubling, because with no placeholders there is no format parser
            // to fool -- but the chunk still has to be ESCAPED for the literal
            // it is being written into. A triple-quoted string may hold a bare
            // `"` (that is what the form is for), and pushing it verbatim
            // ended the Rust literal early: an SVG document in a `$"""…"""`
            // came out as a page of rustc syntax errors.
            self.w.push('"');
            for seg in &s.segments {
                if let juxc_ast::InterpSegment::Literal(text) = seg {
                    for ch in text.chars() {
                        self.push_escaped_for_rust(ch, /*format_string=*/ false);
                    }
                }
            }
            self.w.push('"');
            // Same two exemptions a plain string literal gets: a `const` can
            // not run `.to_string()` (its type is `&'static str`), and a
            // format argument borrows through `Display` and needs no alloc.
            if !self.emitting_const_context && !self.emitting_format_arg {
                self.w.push_str(".to_string()");
            }
            return;
        }
        self.w.push_str("format!(\"");
        let mut args: Vec<&Expr> = Vec::new();
        // We can't easily hold args as owned because borrows on `s`
        // outlive the loop body; collect references for a deferred emit.
        let mut bare_args: Vec<&juxc_ast::Ident> = Vec::new();
        // Track segment order for the second pass — each segment's
        // contribution to the args list is recorded as either a Bare
        // ident reference or a recurse-into Expr.
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
                    args.push(expr);
                    arg_order.push(ArgRef::Expr(args.len() - 1));
                }
            }
        }
        self.w.push('"');
        // `format!` borrows its args via `Display`, so any string
        // literal nested in `${…}` (or in the bare-ident path's
        // fallback) can stay a `&str` — no heap alloc for an arg
        // that's about to be borrowed anyway.
        let prev = self.emitting_format_arg;
        self.emitting_format_arg = true;
        for arg_ref in &arg_order {
            self.w.push_str(", ");
            match arg_ref {
                ArgRef::Bare(i) => {
                    // Wrap nullable bare-ident interps in `JuxOpt`
                    // so `${maybe_name}` prints "value" or "null"
                    // instead of failing to compile.
                    let ident = bare_args[*i].clone();
                    let qn = juxc_ast::QualifiedName {
                        segments: vec![ident],
                        span: bare_args[*i].span,
                    };
                    let synth = juxc_ast::Expr::Path(qn);
                    self.emit_format_arg(&synth);
                }
                ArgRef::Expr(i) => self.emit_format_arg(args[*i]),
            }
        }
        self.emitting_format_arg = prev;
        self.w.push(')');
    }

    /// Write a literal-text chunk from an interp segment into the
    /// surrounding Rust format string. Doubles `{` and `}` so they
    /// reach the format-string parser as escaped braces. Backslash
    /// escapes (`\n`, `\t`, `\"`, etc.) pass through verbatim — Rust's
    /// string-literal parser interprets them the same way Jux does.
    pub(crate) fn emit_interp_literal_chunk(&mut self, text: &str) {
        for ch in text.chars() {
            match ch {
                '{' => self.w.push_str("{{"),
                '}' => self.w.push_str("}}"),
                // The segment text holds DECODED content (cooked
                // strings) or VERBATIM bytes (raw strings) — both
                // re-escape for the Rust string literal we are
                // writing into. Without this a raw `\d` or an
                // embedded newline would break the emitted source.
                '\\' => self.w.push_str("\\\\"),
                '"' => self.w.push_str("\\\""),
                '\n' => self.w.push_str("\\n"),
                '\r' => self.w.push_str("\\r"),
                '\t' => self.w.push_str("\\t"),
                _ => self.w.push(ch),
            }
        }
    }
}
