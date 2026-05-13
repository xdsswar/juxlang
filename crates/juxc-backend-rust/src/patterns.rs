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
            juxc_ast::Pattern::Range { start, end, inclusive, .. } => {
                // Rust supports `start..=end` and `start..end` in
                // match patterns as long as both endpoints are
                // literals. Emit the captured literals through the
                // regular literal-pattern path so the produced
                // Rust matches `0..=10` exactly.
                self.emit_literal(start);
                self.w.push_str(if *inclusive { "..=" } else { ".." });
                self.emit_literal(end);
            }
            juxc_ast::Pattern::TypeBind { type_name, binder, .. } => {
                // `case Type ident ->` — shorthand for
                // `case Type(var ident) ->`. We rewrite into the
                // same shape the sealed-subclass EnumVariant path
                // handles below, but bind the WHOLE variant value
                // rather than destructuring fields. Output shape:
                // `Sealed::Type(ident)` so the user can call
                // `ident.method(...)` directly on the subclass.
                let parent_fqn = self
                    .lookup_class_by_bare_or_fqn(&type_name.text)
                    .and_then(|sub| sub.extends_fqn.clone())
                    .unwrap_or_default();
                let parent_bare = parent_fqn
                    .rsplit('.')
                    .next()
                    .unwrap_or(parent_fqn.as_str())
                    .to_string();
                if parent_bare.is_empty() {
                    // No sealed parent — emit as a bare bind.
                    // (Rust will fail to type-check; the diagnostic
                    // points at the source.)
                    self.w.push_str(&binder.text);
                } else {
                    self.w.push_str(&parent_bare);
                    self.w.push_str("::");
                    self.w.push_str(&type_name.text);
                    self.w.push('(');
                    self.w.push_str(&binder.text);
                    self.w.push(')');
                }
            }
            juxc_ast::Pattern::EnumVariant { path, args, .. } => {
                // Three shapes to handle:
                //
                // 1. **Enum variant** (`Color.Red`, `Token.Number(_)`)
                //    — rewrite `.` to `::`. The match scrutinee's
                //    type is an enum.
                //
                // 2. **Sealed-class subclass pattern** (`Red(var s)`
                //    inside a `switch (light) { … }` where `Light`
                //    is `sealed permits Red, Yellow, Green`). The
                //    pattern path is the bare subclass name; the
                //    lowered match is on a Rust enum
                //    `Light::Red(Red { seconds: s, .. })`. We need
                //    to (a) prepend the sealed parent's name, and
                //    (b) translate positional pattern args to the
                //    subclass struct's named-field pattern.
                //
                // 3. **Single bare name** (`Red` without parens,
                //    inside an enum-variant context) — kept as-is;
                //    falls into shape 1.
                //
                // Detection: single-segment path AND the bare name
                // resolves to a class whose parent is sealed.
                let is_single_subclass = path.segments.len() == 1
                    && self
                        .lookup_class_by_bare_or_fqn(&path.segments[0].text)
                        .and_then(|sub| sub.extends_fqn.clone())
                        .and_then(|fqn| {
                            // Look up the parent class; check sealed.
                            // Use the last FQN segment as the bare
                            // name for our lookup helper.
                            let bare = fqn
                                .rsplit('.')
                                .next()
                                .unwrap_or(&fqn)
                                .to_string();
                            self.lookup_class_by_bare_or_fqn(&bare)
                                .map(|p| p.is_sealed)
                        })
                        .unwrap_or(false);
                if is_single_subclass {
                    // Sealed-subclass shape — emit
                    // `Sealed::Sub(Sub { f0: arg0, f1: arg1, .. })`.
                    let sub_name = &path.segments[0].text;
                    let sub_class = self
                        .lookup_class_by_bare_or_fqn(sub_name)
                        .cloned();
                    let parent_fqn = sub_class
                        .as_ref()
                        .and_then(|c| c.extends_fqn.clone())
                        .unwrap_or_default();
                    let parent_bare = parent_fqn
                        .rsplit('.')
                        .next()
                        .unwrap_or(parent_fqn.as_str())
                        .to_string();
                    // Field-name lookup: positional pattern arg i
                    // maps to subclass field i in declaration order
                    // (static fields filtered out — they aren't
                    // instance state, can't appear in a struct
                    // pattern).
                    let field_names: Vec<String> = self
                        .class_asts
                        .get(sub_name.as_str())
                        .map(|ast| {
                            ast.fields
                                .iter()
                                .filter(|f| !f.is_static)
                                .map(|f| f.name.text.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    self.w.push_str(&parent_bare);
                    self.w.push_str("::");
                    self.w.push_str(sub_name);
                    self.w.push('(');
                    if args.is_empty() {
                        // Unit-style subclass pattern — empty
                        // struct. Use `..` to match any contents
                        // even if Rust later requires it; for an
                        // empty struct this just lowers to `Sub`.
                        self.w.push_str(sub_name);
                        self.w.push_str(" { .. }");
                    } else {
                        self.w.push_str(sub_name);
                        self.w.push_str(" { ");
                        for (i, sub) in args.iter().enumerate() {
                            if i > 0 {
                                self.w.push_str(", ");
                            }
                            // Field-name : pattern. We need the
                            // sub-pattern's text rendered — recurse
                            // through `emit_pattern`. The field name
                            // comes from the i-th non-static field;
                            // out-of-range positions get a synthetic
                            // `__pos{i}` so the error message points
                            // at the right shape.
                            let fname = field_names
                                .get(i)
                                .cloned()
                                .unwrap_or_else(|| format!("__pos{i}"));
                            self.w.push_str(&fname);
                            self.w.push_str(": ");
                            self.emit_pattern(sub);
                        }
                        // `..` rest pattern in case the subclass
                        // has more fields than the pattern listed
                        // (Java would let the user destructure
                        // only the prefix they care about; Rust's
                        // struct patterns require exhaustiveness
                        // unless `..` is present).
                        self.w.push_str(", .. }");
                    }
                    self.w.push(')');
                    return;
                }
                // Shape 1 / 3: rewrite `.` to `::` verbatim.
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
