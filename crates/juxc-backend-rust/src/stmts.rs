//! Statement-level lowering — blocks, var decls, control flow, assignment.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original methods.

use juxc_ast::{
    AssignStmt, Block, ElseBranch, Expr, ForEachStmt, IfStmt, Literal, Stmt, VarDecl, WhileStmt,
};
use juxc_tycheck::Ty;

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
    pub(crate) fn emit_for_each(&mut self, f: &ForEachStmt) {
        self.w.push_str("for ");
        // For non-range iterables (arrays, Vecs) we iterate by
        // **borrowed reference** so the source value isn't moved —
        // matching Java's for-each semantics where the array stays
        // usable after the loop. The `&x` pattern destructures the
        // borrowed item, leaving `x` as a value-typed binding for the
        // body. This works for any `T: Copy`, which covers every
        // Phase-1 element type (primitive ints/floats, `bool`, `char`,
        // `&str`).
        //
        // Ranges (`0..10`) keep their existing naked form — they're
        // cheap-to-move self-iterators and don't need a borrow.
        let is_range = matches!(&f.iter, Expr::Range(_));
        if !is_range {
            self.w.push('&');
        }
        self.w.push_str(&f.var_name.text);
        self.w.push_str(" in ");
        if !is_range {
            self.w.push('&');
        }
        self.emit_expr(&f.iter);
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
    /// **String-field coercion.** If the LHS is a field access whose
    /// declared type is the Jux primitive `String` (resolved via the
    /// receiver's class/record signature in tycheck's [`SymbolTable`]),
    /// append `.to_string()` to the RHS so a `&str` value (the
    /// natural shape of `String` parameters and string literals)
    /// becomes an owned `String` ready for the field. Calling
    /// `.to_string()` on an already-`String` value clones — slightly
    /// wasteful but always correct.
    pub(crate) fn emit_assign(&mut self, a: &AssignStmt) {
        // LHS: emit with the lvalue flag set so `emit_field` skips its
        // String-read `.clone()` insertion.
        self.emitting_lvalue = true;
        self.emit_expr(&a.target);
        self.emitting_lvalue = false;
        self.w.push_str(" = ");
        self.emit_expr(&a.value);
        // Position-aware String coercion on assign-to-String-field.
        //
        // Phase H: consult tycheck's per-expression `Ty` map for the
        // assignment target. If the target's recorded type is
        // `Ty::String`, the field expects an owned `String` and the
        // RHS (typically a `&str` parameter or a literal) needs
        // `.to_string()`. Anything else — primitives, user types,
        // generics, arrays — emits straight.
        //
        // Fallback: when the target isn't in `expr_types`, drop back
        // to the symbol-table lookup so we don't regress on the rare
        // case where tycheck didn't visit the expression. A miss in
        // both gives `false`, matching the conservative path the old
        // heuristic would have taken on an unrecognized field name.
        if let Expr::Field(f) = &a.target {
            if self.assign_target_is_string(f) {
                self.w.push_str(".to_string()");
            }
        }
        self.w.push_str(";\n");
    }

    /// Decide whether the RHS of `target = value;` needs an automatic
    /// `.to_string()` coercion — true exactly when `target` is a Jux
    /// `String`-typed field. See [`Self::emit_assign`] for the rule
    /// and the fallback semantics.
    ///
    /// Resolves via [`Self::lookup_field_type`] — receiver-driven —
    /// rather than `expr_types.get(&f.span)`, for the same reason
    /// [`Self::field_read_needs_clone`] does: spans inside
    /// interpolated-string segments are substring-local and can collide
    /// across the unit. A missing field entry returns false (no
    /// coercion); the same conservative fallback the heuristic used
    /// to take when the field name wasn't in the pre-pass set.
    pub(crate) fn assign_target_is_string(&self, f: &juxc_ast::FieldExpr) -> bool {
        matches!(self.lookup_field_type(f), Some(Ty::String))
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
