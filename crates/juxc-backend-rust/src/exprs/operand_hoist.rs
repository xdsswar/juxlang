//! Operand pre-hoist: no cell guard is alive while Jux code runs
//! (JUX-CLASS-REPRESENTATION §CR.4.1; ERRATA E119, gap 29).
//!
//! A field read of a shared object is `a.0.borrow().n`, and the guard that
//! `borrow()` returns is a temporary: Rust keeps it alive to the end of the
//! enclosing statement. In an expression with several operands that is too
//! long whenever a LATER operand runs Jux code:
//!
//! ```text
//! print(a.n + a.inc());           // println!("{}", a.0.borrow().n + a.inc())
//! if (this.state == this.step())  // self.0.borrow().state == self.step()
//! $"${a.n} ${a.inc()}"            // format!("{} {}", a.0.borrow().n, a.inc())
//! ```
//!
//! `inc` takes `a.0.borrow_mut()` while the read's guard is still held, and
//! the program stops with "object of type C was already in use". Calls
//! already bind their arguments (`call_needs_borrow_hoist`) and receivers
//! (`emit_call_with_hoisted_receiver`); this does the same for the operands
//! of an operator chain and the holes of an interpolated string.
//!
//! The rule: walk the operands in evaluation order. When one that leaves a
//! guard behind comes before one that may run Jux code, every operand ahead
//! of the last such call that reads a cell or runs code is bound first, in
//! order, and the expression reads the temps:
//!
//! ```text
//! ({ let __jux_op0 = a.0.borrow().n; __jux_op0 + a.inc() })
//! ```
//!
//! The order is Java's (left to right), so the value each operand sees is
//! unchanged; only when each guard is dropped moves. Operands that neither
//! read a cell nor run code (literals, locals) stay where they are. Whether
//! the call really reaches the object is not asked, the same choice §CR.4.1
//! makes for stores: being wrong costs a redundant binding, not a crash.

use juxc_ast::{BinaryOp, Expr};
use juxc_source::Span;

use crate::exprs::expr_span_of;
use crate::RustEmitter;

impl RustEmitter {
    /// The name an already-hoisted operand is written as, if `expr` is one.
    /// Matched by span AND by kind of node: a lowering that synthesizes a
    /// node out of an operand's span (`tap(v)` read as `this.tap(v)`) must
    /// not pick up the operand's temp.
    pub(crate) fn operand_substitute(&self, expr: &Expr) -> Option<&str> {
        if self.operand_subst.is_empty() {
            return None;
        }
        let span = expr_span_of(expr);
        let kind = std::mem::discriminant(expr);
        self.operand_subst
            .iter()
            .rev()
            .find(|(s, k, _)| *s == span && *k == kind)
            .map(|(_, _, n)| n.as_str())
    }

    /// The operands of `expr` to bind before it, in evaluation order, or
    /// `None` when no guard is alive where Jux code runs. See the module docs.
    pub(crate) fn prehoist_plan(&self, expr: &Expr) -> Option<Vec<Expr>> {
        if self.emitting_const_context || self.emitting_lvalue {
            return None;
        }
        // A bare `$name` hole is not an expression node; the interpolation
        // lowering synthesizes a path over the name's span, and so does this.
        let mut bare: Vec<Expr> = Vec::new();
        if let Expr::InterpString(s) = expr {
            for seg in &s.segments {
                if let juxc_ast::InterpSegment::Bare(ident) = seg {
                    bare.push(Expr::Path(juxc_ast::QualifiedName { segments: vec![ident.clone()], span: ident.span }));
                }
            }
        }
        let mut operands: Vec<&Expr> = Vec::new();
        match expr {
            Expr::Binary(b) if !matches!(b.op, BinaryOp::And | BinaryOp::Or) => flatten_operands(expr, &mut operands),
            Expr::InterpString(s) => {
                let mut next_bare = bare.iter();
                for seg in &s.segments {
                    match seg {
                        juxc_ast::InterpSegment::Expr(e) => operands.push(e),
                        juxc_ast::InterpSegment::Bare(_) => operands.extend(next_bare.next()),
                        juxc_ast::InterpSegment::Literal(_) => {}
                    }
                }
            }
            Expr::NewObject(n) if n.anonymous_body.is_none() => operands.extend(n.args.iter()),
            Expr::NewArrayLit(a) => operands.extend(a.elements.iter()),
            _ => return None,
        }
        let last_code = operands.iter().rposition(|e| self.operand_may_run_jux_code(e))?;
        if !operands[..last_code].iter().any(|e| self.operand_leaves_guard(e)) {
            return None;
        }
        let bound: Vec<Expr> = operands[..last_code]
            .iter()
            .filter(|e| self.operand_leaves_guard(e) || self.operand_may_run_jux_code(e))
            .map(|e| (*e).clone())
            .collect();
        // A node with no span of its own (a literal) cannot be told apart
        // from any other; such a node never needs binding, but refuse rather
        // than guess.
        if bound.iter().any(|e| expr_span_of(e).is_empty()) {
            return None;
        }
        Some(bound)
    }

    /// Emit `expr` (an operator chain or an interpolated string) with its
    /// guard-leaving operands bound ahead of the Jux code that follows them.
    /// `false` when nothing needs binding, or when the expression's lowering
    /// did not read every bound operand back (the caller then emits it as
    /// usual, and the driver's borrow self-check is the net).
    pub(crate) fn emit_with_prehoisted_operands(&mut self, expr: &Expr) -> bool {
        // The expression itself, being emitted inside its own hoist block.
        if self.operand_hoist_skip == Some(expr_span_of(expr)) {
            self.operand_hoist_skip = None;
            return false;
        }
        let Some(bound) = self.prehoist_plan(expr) else { return false };
        // Emitted aside: if the lowering below does not read every temp
        // back, none of this is used.
        let saved = std::mem::replace(&mut self.w, crate::writer::Writer::new());
        let armed = self.signed_slot_target.take();
        let prev_fmt = std::mem::take(&mut self.emitting_format_arg);
        let prev_cmp = std::mem::take(&mut self.emitting_comparison_operand);
        let prev_callee = std::mem::take(&mut self.emitting_call_callee);
        self.w.push_str("({ ");
        let mut names: Vec<(Span, std::mem::Discriminant<Expr>, String)> = Vec::new();
        for e in &bound {
            let name = format!("__jux_op{}", self.operand_hoist_seq);
            self.operand_hoist_seq += 1;
            self.w.push_str("let ");
            self.w.push_str(&name);
            self.w.push_str(" = ");
            self.emit_expr(e);
            // Bound by value: a place read out of a cell is copied (a
            // String, a record) or shared (an object handle).
            if self.wrapper_value_needs_clone(e) || self.value_place_needs_clone(e) {
                self.w.push_str(".clone()");
            }
            self.w.push_str("; ");
            names.push((expr_span_of(e), std::mem::discriminant(e), name));
        }
        self.emitting_format_arg = prev_fmt;
        self.emitting_comparison_operand = prev_cmp;
        self.emitting_call_callee = prev_callee;
        self.signed_slot_target = armed;
        let depth = self.operand_subst.len();
        self.operand_subst.extend(names.iter().cloned());
        let body_mark = self.w.mark();
        self.operand_hoist_skip = Some(expr_span_of(expr));
        self.emit_expr(expr);
        self.operand_hoist_skip = None;
        self.operand_subst.truncate(depth);
        let body = self.w.text_from(body_mark).to_string();
        let every_read = names.iter().all(|(_, _, n)| contains_ident(&body, n));
        self.w.push_str(" })");
        let text = std::mem::replace(&mut self.w, saved).into_string();
        if !every_read {
            return false;
        }
        self.w.push_str(&text);
        true
    }

    /// True when evaluating `e` can leave a cell guard alive to the end of
    /// the statement: a field of a shared object read through its cell
    /// (`a.n`, `this.n`, a bare field `n` in the class's own method), a `ref`
    /// local, or an element read through an array or collection handle.
    pub(crate) fn operand_leaves_guard(&self, e: &Expr) -> bool {
        if self.operand_substitute(e).is_some() || matches!(e, Expr::Lambda(_)) {
            return false;
        }
        if self.expr_reads_wrapper_field(e) || !self.handle_roots_read_in(e).is_empty() {
            return true;
        }
        self.reads_own_field_by_bare_name(e)
    }

    /// True when `e` reads a field of the object whose method is being
    /// emitted by its bare name (`n` for `this.n`), through `self`'s cell.
    pub(crate) fn reads_own_field_by_bare_name(&self, e: &Expr) -> bool {
        let mut found = false;
        juxc_ast::visit::for_each_expr_in(e, &mut |sub| {
            if !found {
                if let Expr::Path(qn) = sub {
                    found = qn.segments.len() == 1 && self.bare_name_reads_own_cell(&qn.segments[0].text);
                }
            }
        });
        found
    }

    /// A bare name that is an instance field of the shared class whose method
    /// is being emitted, read through `self.0.borrow()`.
    fn bare_name_reads_own_cell(&self, name: &str) -> bool {
        self.emitting_wrapper_class
            && self.enclosing_class.as_deref().is_some_and(|c| self.is_wrapper_class(c))
            && !self.local_types.iter().any(|scope| scope.contains_key(name))
            && !self.current_fn_params.iter().any(|p| p == name)
            && (self.bare_name_is_instance_member(name)
                || self.wrapper_field_parent_depth(&Expr::This(Span::new(0, 0)), name).is_some())
    }

    /// True when evaluating `e` may run Jux code, which may borrow any shared
    /// object again: a call of a Jux function, method or stored function, a
    /// constructor, a user operator. A method of a string, a collection or
    /// a number runs no Jux code. Lambdas are not run where they are written.
    pub(crate) fn operand_may_run_jux_code(&self, e: &Expr) -> bool {
        if self.operand_substitute(e).is_some() {
            return false;
        }
        let mut found = false;
        self.walk_outside_lambdas(e, &mut |this, sub| {
            if found {
                return;
            }
            found = match sub {
                Expr::Call(c) => this.call_may_run_jux_code(c),
                Expr::NewObject(n) => this.class_name_is_jux_class(&n.class_name),
                Expr::Binary(b) => {
                    this.symbols.free_operator_calls.contains_key(&b.span) || this.user_operator_for_binary(b)
                }
                Expr::Index(i) => this.is_jux_object_ty(this.receiver_ty_of(&i.array)),
                _ => false,
            };
        });
        found
    }

    /// Visit `e` and its subexpressions, skipping lambda bodies and anything
    /// already hoisted.
    fn walk_outside_lambdas(&self, e: &Expr, f: &mut dyn FnMut(&Self, &Expr)) {
        if matches!(e, Expr::Lambda(_)) || self.operand_substitute(e).is_some() {
            return;
        }
        f(self, e);
        let mut children: Vec<&Expr> = Vec::new();
        direct_children(e, &mut children);
        for c in children {
            self.walk_outside_lambdas(c, f);
        }
    }

    fn call_may_run_jux_code(&self, c: &juxc_ast::CallExpr) -> bool {
        match &*c.callee {
            Expr::Field(f) => {
                // `Class.m()`: a static method, Jux code unless foreign.
                if let Expr::Path(qn) = &*f.object {
                    let last = qn.segments.last().map(|s| s.text.as_str()).unwrap_or("");
                    if !self.local_types.iter().any(|s| s.contains_key(last)) {
                        if let Some(c) = self.lookup_class_by_bare_or_fqn(last) {
                            return !c.is_external;
                        }
                    }
                }
                match self.receiver_ty_of(&f.object) {
                    Some(ty) => self.is_jux_object_ty(Some(ty)),
                    // Not known: assume the worst.
                    None => true,
                }
            }
            Expr::Path(qn) => {
                let name = qn.segments.last().map(|s| s.text.as_str()).unwrap_or("");
                !matches!(name, "print" | "println" | "eprint" | "eprintln")
            }
            _ => true,
        }
    }

    /// A Jux class or interface (a handle whose methods are Jux code), or a
    /// function value. A string, collection, array, number or foreign type is
    /// not.
    fn is_jux_object_ty(&self, ty: Option<juxc_tycheck::Ty>) -> bool {
        let Some(ty) = ty.map(crate::exprs::field::strip_nullable) else { return false };
        match ty {
            juxc_tycheck::Ty::User { ref name, .. } => {
                let bare = name.rsplit('.').next().unwrap_or(name);
                self.lookup_class_by_bare_or_fqn(bare).is_some_and(|c| !c.is_external)
                    || self.lookup_interface_by_bare_or_fqn(bare).is_some_and(|(_, i)| !i.is_external)
            }
            other => matches!(other, juxc_tycheck::Ty::Fn { .. }),
        }
    }

    fn class_name_is_jux_class(&self, name: &juxc_ast::QualifiedName) -> bool {
        let Some(last) = name.segments.last() else { return false };
        self.lookup_class_by_bare_or_fqn(&last.text).is_some_and(|c| !c.is_external)
    }

    /// A binary operator resolved to a user `operator` method.
    fn user_operator_for_binary(&self, b: &juxc_ast::BinaryExpr) -> bool {
        let Some(kind) = crate::exprs::binary::binary_operator_kind(b.op) else { return false };
        self.expr_recorded_ty(&b.left).is_some_and(|t| self.ty_declares_operator(&t, kind))
    }
}

/// The operands of a chain of non-short-circuit binary operators, in
/// evaluation order. `&&` / `||` stay whole: their right side runs only
/// sometimes, and Rust already drops each side's guards on its own.
fn flatten_operands<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
    match e {
        Expr::Binary(b) if !matches!(b.op, BinaryOp::And | BinaryOp::Or) => {
            flatten_operands(&b.left, out);
            flatten_operands(&b.right, out);
        }
        other => out.push(other),
    }
}

/// The immediate subexpressions of `e`.
fn direct_children<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
    match e {
        Expr::Out(inner, _)
        | Expr::TypeOf(inner, _)
        | Expr::Await(inner, _)
        | Expr::ErrorProp(inner, _)
        | Expr::NotNullAssert(inner, _)
        | Expr::Throw(inner, _) => out.push(inner),
        Expr::Call(c) => {
            out.push(&c.callee);
            out.extend(c.args.iter());
        }
        Expr::Binary(b) => {
            out.push(&b.left);
            out.push(&b.right);
        }
        Expr::Unary(u) => out.push(&u.operand),
        Expr::Cast(c) => out.push(&c.value),
        Expr::Index(i) => {
            out.push(&i.array);
            out.push(&i.index);
        }
        Expr::Field(f) => out.push(&f.object),
        Expr::InterpString(s) => {
            for seg in &s.segments {
                if let juxc_ast::InterpSegment::Expr(x) = seg {
                    out.push(x);
                }
            }
        }
        Expr::TypeTest(t) => out.push(&t.value),
        Expr::NewObject(n) => out.extend(n.args.iter()),
        Expr::NewArrayLit(a) => out.extend(a.elements.iter()),
        Expr::Elvis(el) => {
            out.push(&el.value);
            out.push(&el.fallback);
        }
        Expr::Ternary(t) => {
            out.push(&t.condition);
            out.push(&t.then_branch);
            out.push(&t.else_branch);
        }
        Expr::TupleLit(elems, _) => out.extend(elems.iter()),
        _ => {}
    }
}

/// True when `text` mentions `name` as a whole identifier.
fn contains_ident(text: &str, name: &str) -> bool {
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(i) = text[from..].find(name) {
        let start = from + i;
        let end = start + name.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
