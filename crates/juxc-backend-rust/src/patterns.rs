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
                    self.emit_expr(e);
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
                // For two-segment paths whose first segment is a known
                // enum (`Color.Red` → `Color::Red`), emit with `::`.
                // Otherwise emit the path verbatim with `.` rewritten
                // to `::` (Rust path syntax).
                let segs: Vec<&str> =
                    path.segments.iter().map(|s| s.text.as_str()).collect();
                let is_enum_path =
                    segs.len() == 2 && self.symbols.enums.contains_key(segs[0]);
                let joiner = if is_enum_path { "::" } else { "::" };
                // Even when not a known enum, Rust expects `::` between
                // path segments in match patterns — `Foo::Bar(…)`. The
                // joiner above is the same in both branches; the
                // discrimination matters only for diagnostic clarity.
                let _ = joiner;
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
