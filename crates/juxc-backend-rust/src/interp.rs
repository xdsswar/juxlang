//! Interpolated-string lowering — `$"…"` and `${expr}` segments.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::Expr;

use crate::{ArgRef, RustEmitter};

impl RustEmitter {
    /// Lower an interpolated string literal per §3.4 to a Rust
    /// `format!("…", arg, arg, …)` call.
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
    /// **Empty form.** `$""` lowers to `format!("")` — a no-op-shaped
    /// empty string. Cheap; no special path needed.
    pub(crate) fn emit_interp_string(&mut self, s: &juxc_ast::InterpStringExpr) {
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
        for arg_ref in &arg_order {
            self.w.push_str(", ");
            match arg_ref {
                ArgRef::Bare(i) => self.w.push_str(&bare_args[*i].text),
                ArgRef::Expr(i) => self.emit_expr(args[*i]),
            }
        }
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
                _ => self.w.push(ch),
            }
        }
    }
}
