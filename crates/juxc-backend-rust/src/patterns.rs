//! Switch-expression and pattern lowering.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::Literal;

use crate::analysis::pattern_has_parens;
use crate::RustEmitter;

impl RustEmitter {
    /// Lower a `switch` expression to a Rust `match`. The same node
    /// covers both expression-form (`var y = switch(…) {…}`) and
    /// statement-form (`switch(…) {…}`) — Rust's `match` is always an
    /// expression, so the lowering is identical.
    ///
    /// Each arm becomes `pattern => body,`. Block bodies emit as
    /// `pattern => { stmts… },`. Expression-bodies emit naked.
    pub(crate) fn emit_switch(&mut self, s: &juxc_ast::SwitchExpr) {
        // When the surrounding context requires `Option<T>` (the
        // `emitting_nullable_target` flag is set, currently fired
        // by `emit_tail_stmt` for a `T?`-returning fn), push the
        // `Some(...)` wrap into each arm body so mixed
        // `T` / `null` arms unify cleanly. We clear the flag
        // BEFORE walking each arm body so a nested switch inside
        // an arm doesn't re-wrap; the arm-body context resets.
        let wrap_each_arm = self.emitting_nullable_target;
        let prev_nullable_target = self.emitting_nullable_target;
        self.emitting_nullable_target = false;
        self.w.push_str("match ");
        self.emit_expr(&s.scrutinee);
        self.w.push_str(" {\n");
        for arm in &s.arms {
            // Modest indent — switch is usually nested at depth >= 1
            // inside a function body. Two spaces of leading indent
            // keeps the match contents readable while the surrounding
            // emit_block indenter handles outer alignment. The raw
            // 4-space / 8-space prefixes are intentionally **not**
            // bound to the writer's current `indent_level` — they
            // represent the match arms' depth relative to the `match`
            // keyword itself, which was emitted naked above.
            self.w.push_str("    ");
            self.emit_pattern(&arm.pattern);
            self.w.push_str(" => ");
            match &arm.body {
                juxc_ast::SwitchBody::Expr(e) => {
                    // Per-arm nullable wrap: skip when the value
                    // is already `null` (a `Literal::Null` lowers
                    // to `None`) or already nullable-shaped (the
                    // generic-arg helper recognizes paths to
                    // nullable locals and `?.`-chain results).
                    let wrap = wrap_each_arm
                        && !matches!(&**e, juxc_ast::Expr::Literal(juxc_ast::Literal::Null))
                        && !self.expression_is_already_nullable(e);
                    if wrap {
                        self.w.push_str("Some(");
                    }
                    self.emit_expr(e);
                    if wrap {
                        self.w.push(')');
                    }
                }
                juxc_ast::SwitchBody::Block(b) => {
                    self.w.push_str("{\n");
                    // Statements inside a block-bodied arm sit at the
                    // arm-depth + 1 (two levels of 4-space prefix from
                    // the surrounding `match`). We emit the indent
                    // explicitly and delegate to `emit_stmt` for the
                    // text itself — `emit_stmt` no longer takes an
                    // indent parameter, so any nested `if` / `while`
                    // inside the arm relies on the writer's current
                    // `indent_level` for further nesting. We don't
                    // adjust that here: the surrounding emitter set
                    // it to the function-body depth, which gives
                    // sensible (if not arithmetically perfect) nested
                    // indents in the rare deeply-nested case.
                    for stmt in &b.statements {
                        self.w.push_str("        ");
                        self.emit_stmt(stmt);
                    }
                    self.w.push_str("    ");
                    self.w.push('}');
                }
            }
            self.w.push_str(",\n");
        }
        self.w.push('}');
        self.emitting_nullable_target = prev_nullable_target;
    }

    /// Emit a single pattern in Rust source. Recursive for variant
    /// patterns with nested sub-patterns. The `Color.Variant` form
    /// rewrites through the `::`-path syntax used by Rust enums; for
    /// dotted paths with non-enum first-segment we fall back on raw
    /// `.`-joining, but that path doesn't arise from real Jux source
    /// today.
    pub(crate) fn emit_pattern(&mut self, pattern: &juxc_ast::Pattern) {
        match pattern {
            juxc_ast::Pattern::Wildcard(_) => self.w.push('_'),
            juxc_ast::Pattern::Literal(lit, _) => {
                // Pattern context: Rust match patterns require bare
                // literals (not `String` values), so we suppress the
                // `.to_string()` wrap that `emit_literal` applies for
                // value-position uses. Pure `&str` literal goes into
                // the pattern slot; the scrutinee side is unaffected.
                if let Literal::String(s) = lit {
                    self.emit_rust_string_literal(s);
                } else {
                    self.emit_literal(lit);
                }
            }
            juxc_ast::Pattern::Bind(name) => self.w.push_str(&name.text),
            juxc_ast::Pattern::EnumVariant { path, args, .. } => {
                // Rust match patterns join path segments with `::`,
                // regardless of whether the first segment is a
                // known enum (`Color.Red` → `Color::Red`) or any
                // other dotted shape. Emit verbatim with `.`
                // rewritten to `::`.
                let segs: Vec<&str> =
                    path.segments.iter().map(|s| s.text.as_str()).collect();
                self.w.push_str(&segs.join("::"));
                if !args.is_empty() || pattern_has_parens(pattern) {
                    self.w.push('(');
                    for (i, sub) in args.iter().enumerate() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        self.emit_pattern(sub);
                    }
                    self.w.push(')');
                }
            }
        }
    }
}
