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
                    self.emit_expr(e);
                    // Pre Fix 1 we appended `.to_string()` to bare
                    // string literals here to bridge `&str` →
                    // `String`. Fix 1 lifted every Jux string source
                    // to owned `String` directly inside `emit_literal`,
                    // so the coercion is now baked in. Keeping the
                    // call would emit `.to_string().to_string()`.
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
        }
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

        // Decide between borrow-and-pattern-deref vs clone shape from
        // the element type recorded by tycheck. Missing entries fall
        // back to the clone form — it's the universally-correct
        // shape; the borrow form only wins when we *know* the
        // element is Copy.
        let element_is_copy = match self.expr_types.get(&expr_span_of(&f.iter)) {
            Some(Ty::Array { element, .. }) => matches!(element.as_ref(), Ty::Primitive(_)),
            _ => false,
        };

        self.w.push_str("for ");
        if element_is_copy {
            self.w.push('&');
        }
        self.w.push_str(&f.var_name.text);
        self.w.push_str(" in ");
        if element_is_copy {
            self.w.push('&');
            self.emit_expr(&f.iter);
        } else {
            self.emit_expr(&f.iter);
            self.w.push_str(".iter().cloned()");
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
        self.w.push_str("let ");
        if self.mutated_in_fn.contains(&var.name.text) {
            self.w.push_str("mut ");
        }
        self.w.push_str(&var.name.text);
        // Java-style typed local (`int x = 5;`) carries an explicit
        // type annotation; emit it as `let x: T = init;`. The `var`
        // form leaves `ty == None` and we let Rust infer.
        if let Some(ty) = &var.ty {
            self.w.push_str(": ");
            self.emit_type_as_rust(ty);
        }
        if let Some(init) = &var.init {
            self.w.push_str(" = ");
            self.emit_expr(init);
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
        // LHS: emit with the lvalue flag set so `emit_field` skips its
        // String-read `.clone()` insertion.
        self.emitting_lvalue = true;
        self.emit_expr(&a.target);
        self.emitting_lvalue = false;
        self.w.push_str(" = ");
        self.emit_expr(&a.value);
        self.w.push_str(";\n");
    }

    /// Lower `if (cond) { … } else if (…) { … } else { … }` to its
    /// directly-corresponding Rust form. Rust uses no parentheses around
    /// `if` conditions, so we drop them.
    pub(crate) fn emit_if(&mut self, if_stmt: &IfStmt) {
        self.w.push_str("if ");
        self.emit_expr(&if_stmt.condition);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.emit_block_contents(&if_stmt.then_block);
        self.w.indent_dec();
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
    }
}
