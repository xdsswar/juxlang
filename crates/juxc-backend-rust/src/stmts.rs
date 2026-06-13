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

/// True when `e` is the AST `null` literal — used to decide
/// whether a value flowing into a nullable slot needs the
/// `Some(...)` wrap or is already `None`.
pub(crate) fn is_null_literal(e: &Expr) -> bool {
    matches!(e, Expr::Literal(Literal::Null))
}

/// True when the loop body somewhere consumes `name` — uses it in
/// a position that needs an OWNED `T` rather than just a `&T`.
/// Drives the for-each lowering: when this returns `false`, we
/// can iterate by reference and skip the per-iteration clone.
///
/// Considered "moves":
/// - `var y = x;` (the init is exactly `Path(x)`)
/// - `f(x)`, `obj.method(x)`, `new T(x)` — fn / method / ctor arg
/// - `return x;`
/// - `obj.field = x;` — assign rhs
/// - `super(x);` — super-constructor arg
/// - `x as T` — cast operand (cast consumes the value)
/// - Inside an array literal: `[x, ...]`
///
/// NOT considered moves:
/// - `x.method()`, `x.field` — read through borrow
/// - `x == y`, `x != y` — comparisons borrow
/// - `format!`/`println!` args — borrow via `Display`
/// - `if (x)` / `while (x)` — bool conditions borrow
///
/// The walker is conservative: any uncertainty returns `true` so
/// the loop falls back to the clone form, which always compiles.
fn body_moves_path(block: &Block, name: &str) -> bool {
    for stmt in &block.statements {
        if stmt_moves_path(stmt, name) {
            return true;
        }
    }
    false
}

fn stmt_moves_path(stmt: &Stmt, name: &str) -> bool {
    match stmt {
        Stmt::Labeled { stmt, .. } => stmt_moves_path(stmt, name),
        Stmt::Expr(e) => expr_moves_path_at_top(e, name),
        Stmt::VarDecl(v) => v.init.as_ref().map_or(false, |e| is_path_named(e, name) || expr_moves_path_at_top(e, name)),
        Stmt::Return(opt) => opt.as_ref().map_or(false, |e| is_path_named(e, name) || expr_moves_path_at_top(e, name)),
        Stmt::Assign(a) => {
            is_path_named(&a.value, name)
                || expr_moves_path_at_top(&a.value, name)
                || expr_moves_path_at_top(&a.target, name)
        }
        Stmt::If(s) => {
            expr_moves_path_at_top(&s.condition, name)
                || body_moves_path(&s.then_block, name)
                || else_branch_moves_path(s.else_branch.as_deref(), name)
        }
        Stmt::While(s) => {
            expr_moves_path_at_top(&s.condition, name)
                || body_moves_path(&s.body, name)
        }
        Stmt::DoWhile(s) => {
            body_moves_path(&s.body, name)
                || expr_moves_path_at_top(&s.condition, name)
        }
        Stmt::ForEach(s) => {
            // A nested for-each that consumes the outer var
            // (`for y in xs` where xs shadows our name) is a move.
            // The shadowing case — same-named inner loop var —
            // can't appear in well-formed Jux source because the
            // resolver would see two scopes; if it does we
            // conservatively report a move.
            is_path_named(&s.iter, name)
                || expr_moves_path_at_top(&s.iter, name)
                || body_moves_path(&s.body, name)
        }
        Stmt::ForC(f) => body_moves_path(&f.body, name),
        Stmt::SuperCall(args, _) => {
            args.iter().any(|a| is_path_named(a, name) || expr_moves_path_at_top(a, name))
        }
        Stmt::Throw(e, _) => is_path_named(e, name) || expr_moves_path_at_top(e, name),
        Stmt::Try(t) => {
            if body_moves_path(&t.body, name) {
                return true;
            }
            for c in &t.catches {
                if body_moves_path(&c.body, name) {
                    return true;
                }
            }
            if let Some(fin) = &t.finally {
                if body_moves_path(fin, name) {
                    return true;
                }
            }
            false
        }
        Stmt::Unsafe(b) => body_moves_path(b, name),
        Stmt::Break(..) | Stmt::Continue(..) => false,
    }
}

fn else_branch_moves_path(branch: Option<&ElseBranch>, name: &str) -> bool {
    let mut cursor = branch;
    while let Some(b) = cursor {
        match b {
            ElseBranch::If(inner) => {
                if expr_moves_path_at_top(&inner.condition, name)
                    || body_moves_path(&inner.then_block, name)
                {
                    return true;
                }
                cursor = inner.else_branch.as_deref();
            }
            ElseBranch::Block(block) => {
                return body_moves_path(block, name);
            }
        }
    }
    false
}

/// True iff `e` is exactly `Path(name)` — a bare reference to the
/// loop variable. Used at consume sites (var-decl init, return
/// value, assign rhs, call args) to detect "the whole expression
/// IS the loop var".
fn is_path_named(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Path(qn) => qn.segments.len() == 1 && qn.segments[0].text == name,
        _ => false,
    }
}

/// Recursive walker for "does this expression contain a consume
/// site for `name`?" Distinct from `is_path_named` — this walks
/// into sub-expressions looking for fn-call args, ctor args, etc.
/// that consume the loop var. Returns false for borrow-shaped
/// uses (`.method()`, `==`, format args) so the caller can
/// safely emit `for x in &xs`.
fn expr_moves_path_at_top(e: &Expr, name: &str) -> bool {
    match e {
        // `out <place>` borrows the place (`&mut`), it does not move it.
        Expr::Out(_, _) => false,
        // `typeof` never evaluates its operand — nothing moves.
        Expr::TypeOf(..) => false,
        // Tuple literal: each element is a by-value consume site,
        // same as a call argument.
        Expr::TupleLit(elems, _) => elems
            .iter()
            .any(|el| is_path_named(el, name) || expr_moves_path_at_top(el, name)),
        // Try-expression: the closure captures by reference (the
        // catch_unwind body), so treat reads conservatively as moves
        // only when the body's own statements move them.
        Expr::ErrorProp(inner, _) => expr_moves_path_at_top(inner, name),
        Expr::TryExpr(t) => {
            body_moves_path(&t.body, name)
                || t.catches.iter().any(|c| body_moves_path(&c.body, name))
        }
        // Function / method call: each arg is a consume site
        // (passes by value). Method receivers (`x.method()`)
        // borrow via auto-deref, so we walk the callee for nested
        // consume shapes but don't treat the receiver itself as
        // moved.
        //
        // **Exception** — the builtin `print(...)` lowers to
        // `println!(...)` which borrows its args via `Display`,
        // so a bare path arg here doesn't move. The recognition
        // mirrors `emit_call`'s `print` dispatch: single-segment
        // path named `print`.
        Expr::Call(c) => {
            let is_print_builtin = matches!(
                &*c.callee,
                Expr::Path(qn) if qn.segments.len() == 1 && qn.segments[0].text == "print",
            );
            if is_print_builtin {
                // Args of `print` borrow; only walk for nested
                // consume shapes inside complex sub-expressions.
                return c.args.iter().any(|a| expr_moves_path_at_top(a, name));
            }
            for arg in &c.args {
                if is_path_named(arg, name) || expr_moves_path_at_top(arg, name) {
                    return true;
                }
            }
            // Walk callee for nested calls (`f(g(x))`).
            expr_moves_path_at_top(&c.callee, name)
        }
        Expr::NewObject(n) => n
            .args
            .iter()
            .any(|a| is_path_named(a, name) || expr_moves_path_at_top(a, name)),
        Expr::NewArray(n) => expr_moves_path_at_top(&n.size, name),
        Expr::NewArrayLit(n) => n
            .elements
            .iter()
            .any(|el| is_path_named(el, name) || expr_moves_path_at_top(el, name)),
        Expr::Cast(c) => is_path_named(&c.value, name) || expr_moves_path_at_top(&c.value, name),
        Expr::NotNullAssert(inner, _) => {
            is_path_named(inner, name) || expr_moves_path_at_top(inner, name)
        }
        Expr::TypeTest(t) => expr_moves_path_at_top(&t.value, name),
        Expr::Binary(b) => {
            // String concat (`+` with a string operand) emits as
            // `format!` which borrows — no move. Other binaries
            // are arithmetic/comparison which also borrow for
            // `String`/user types via the trait method. So walk
            // for nested calls but don't treat top-level
            // operands as moves.
            expr_moves_path_at_top(&b.left, name)
                || expr_moves_path_at_top(&b.right, name)
        }
        Expr::Unary(u) => expr_moves_path_at_top(&u.operand, name),
        Expr::Range(r) => {
            expr_moves_path_at_top(&r.start, name) || expr_moves_path_at_top(&r.end, name)
        }
        Expr::Index(i) => {
            expr_moves_path_at_top(&i.array, name) || expr_moves_path_at_top(&i.index, name)
        }
        Expr::Field(f) => expr_moves_path_at_top(&f.object, name),
        Expr::InterpString(s) => s.segments.iter().any(|seg| match seg {
            // Bare-ident interp is a borrow (Display); no move.
            juxc_ast::InterpSegment::Literal(_) | juxc_ast::InterpSegment::Bare(_) => false,
            juxc_ast::InterpSegment::Expr(inner) => expr_moves_path_at_top(inner, name),
        }),
        Expr::Switch(s) => {
            if expr_moves_path_at_top(&s.scrutinee, name) {
                return true;
            }
            for arm in &s.arms {
                if let Some(g) = &arm.guard {
                    if expr_moves_path_at_top(g, name) {
                        return true;
                    }
                }
                let arm_moves = match &arm.body {
                    juxc_ast::SwitchBody::Expr(e) => {
                        is_path_named(e, name) || expr_moves_path_at_top(e, name)
                    }
                    juxc_ast::SwitchBody::Block(b) => body_moves_path(b, name),
                };
                if arm_moves {
                    return true;
                }
            }
            false
        }
        Expr::Elvis(e) => {
            // Both sides of elvis are value-consuming via
            // `.unwrap_or(...)`, so a bare `Path(name)` on
            // either side is a move.
            is_path_named(&e.value, name)
                || expr_moves_path_at_top(&e.value, name)
                || is_path_named(&e.fallback, name)
                || expr_moves_path_at_top(&e.fallback, name)
        }
        Expr::Lambda(l) => match &l.body {
            // A lambda captures by value (the emitter wraps in
            // `move`), so any read of the loop var inside the
            // body is a move-capture.
            juxc_ast::LambdaBody::Expr(e) => is_path_named(e, name) || expr_moves_path_at_top(e, name),
            juxc_ast::LambdaBody::Block(b) => body_moves_path(b, name),
        },
        Expr::SizeOf(s) => expr_moves_path_at_top(&s.operand, name),
        Expr::Literal(_) | Expr::Path(_) | Expr::This(_) | Expr::Super(_) => false,
        // Method reference is a static expression — no sub-paths
        // referring to the loop variable.
        Expr::MethodRef(_) => false,
        // Ternary: both branches are value-consuming positions
        // (the surrounding context picks one). A bare `Path(name)`
        // on either side is a move.
        Expr::Ternary(t) => {
            expr_moves_path_at_top(&t.condition, name)
                || is_path_named(&t.then_branch, name)
                || expr_moves_path_at_top(&t.then_branch, name)
                || is_path_named(&t.else_branch, name)
                || expr_moves_path_at_top(&t.else_branch, name)
        }
        // `await expr` — the operand is the position that gets
        // evaluated, so any move semantics flow through it.
        Expr::Await(inner, _) => {
            is_path_named(inner, name) || expr_moves_path_at_top(inner, name)
        }
    }
}

impl RustEmitter {
    /// True iff the enclosing function's declared return type is
    /// `T?` (nullable) AND `expr` isn't itself a `null` literal —
    /// meaning the value flowing through `return …` is a `T`
    /// that needs the `Some(...)` lift to match the `Option<T>`
    /// declared return type.
    pub(crate) fn return_wants_some_wrap(&self, expr: &Expr) -> bool {
        let returns_nullable = match &self.current_return_type {
            Some(juxc_ast::ReturnType::Type(t)) => t.nullable,
            Some(juxc_ast::ReturnType::AsyncType(t)) => t.nullable,
            _ => false,
        };
        // An expression that is *already* `Option`-shaped (`return
        // this.nullableField;`, `return maybeX();`, `return nullableLocal;`)
        // flows back unchanged — wrapping it would yield `Some(Some(...))`.
        returns_nullable
            && !is_null_literal(expr)
            && !self.expression_is_already_nullable(expr)
    }
}

/// Match the Kotlin-style null-smart-cast head: `name != null`
/// where `name` is a bare single-segment path. Returns
/// `Some(name)` when the shape matches, else `None`. Used by
/// `emit_if` to lower the canonical null-guard
/// (`if (x != null) { … }`) to Rust's `if let Some(x) = x`
/// pattern — inside the block, `x` is the unwrapped inner type.
///
/// Composite lvalues (`obj.field != null`, `arr[i] != null`) are
/// intentionally NOT matched here. Lowering them to `if let
/// Some(name) = obj.field` introduces a fresh binding `name`
/// without giving the user a way to write it — they'd have to
/// stash the result into a local anyway. A future smart-cast
/// pass can extend this.
fn match_simple_not_null_check(cond: &Expr) -> Option<&str> {
    let Expr::Binary(b) = cond else { return None };
    if !matches!(b.op, juxc_ast::BinaryOp::NotEq) {
        return None;
    }
    // The non-null side must be a bare identifier (single-segment
    // path). The null side must be the `null` literal.
    let (target, other) = match (&*b.left, &*b.right) {
        (Expr::Literal(Literal::Null), other) => (other, &*b.left),
        (other, Expr::Literal(Literal::Null)) => (other, &*b.right),
        _ => return None,
    };
    let _ = other;
    if let Expr::Path(qn) = target {
        if qn.segments.len() == 1 {
            return Some(qn.segments[0].text.as_str());
        }
    }
    None
}

/// One active loop-control threading channel (O2). A `try` statement
/// whose body (or catch arms) contains a `break`/`continue` targeting a
/// loop OUTSIDE the try can't emit it directly — the body lowers inside
/// a `catch_unwind` closure (E0267) and the catch arms inside the
/// `'__jux_catch` labeled dispatch block (E0695). Instead the escape
/// records a code in a `u8` flag local, exits its enclosing construct,
/// and the try machinery re-issues the real `break`/`continue` AFTER
/// `finally` ran (matching Java's "finally before loop control" order).
pub(crate) struct TryLoopCtl {
    /// The flag local's name — `__jux_loopctl_<stack-depth>`, unique
    /// among the channels active at any given emission point.
    pub(crate) flag: String,
    /// Channel storage shape. Sync tries use a plain `u8` local (the
    /// `catch_unwind` closure captures it by reference). ASYNC tries
    /// capture by `move`, so the flag is an `Arc<AtomicU8>` instead —
    /// a `_body`-suffixed clone moves into the `async move` block
    /// while the original stays out for the catch arms and the
    /// post-`finally` dispatch. `Arc`, not `Rc<Cell>`: spawned futures
    /// must stay `Send` (O9).
    pub(crate) is_async: bool,
    /// `loop_emit_depth` recorded when the try began. A
    /// `break`/`continue` intercepts only when emitted at this SAME
    /// depth — one emitted inside a loop nested within the try binds
    /// that inner loop and passes through untouched.
    pub(crate) base_depth: usize,
    /// Whether the try's closure threads a return value (`Option<T>`
    /// carrier) — picks `return None;` vs `return;` for the
    /// body-region escape.
    pub(crate) closure_has_ret: bool,
    /// Where emission currently sits relative to this try's machinery —
    /// decides the escape's replacement text (or none).
    pub(crate) region: LoopCtlRegion,
    /// Distinct escapes seen so far, in first-encounter order. The
    /// flag code for an escape is its index + 1 (0 = "no escape").
    /// Each entry is `(is_break, optional label)`.
    pub(crate) escapes: Vec<(bool, Option<String>)>,
}

/// Emission region for a [`TryLoopCtl`] channel.
#[derive(PartialEq)]
pub(crate) enum LoopCtlRegion {
    /// Inside the try body's `catch_unwind` closure — escape via
    /// `{ flag = N; return …; }`.
    Body,
    /// Inside a catch arm (the `'__jux_catch` dispatch block) —
    /// escape via `{ flag = N; break '__jux_catch; }`.
    Catch,
    /// Machinery / `finally` — no interception by this channel
    /// (an enclosing channel may still apply).
    Off,
}

/// True when a `try` statement needs an O2 loop-control channel: its
/// body or a catch arm contains a `break`/`continue` that targets a
/// loop OUTSIDE the try. The walker descends statement forms that emit
/// inline (if/else, labeled, unsafe, switch arms, and nested `try`
/// including its finally — all of those land inside the outer closure)
/// but STOPS at loops: an escape inside a nested loop binds that loop,
/// which lives inside the closure too. The try's OWN finally is
/// excluded — it emits outside the closure, where plain loop control
/// is already legal.
pub(crate) fn try_needs_loopctl(t: &juxc_ast::TryStmt) -> bool {
    block_has_loop_escape(&t.body)
        || t.catches.iter().any(|c| block_has_loop_escape(&c.body))
}

fn block_has_loop_escape(b: &juxc_ast::Block) -> bool {
    b.statements.iter().any(stmt_has_loop_escape)
}

fn stmt_has_loop_escape(s: &Stmt) -> bool {
    match s {
        Stmt::Break(..) | Stmt::Continue(..) => true,
        Stmt::If(i) => {
            if block_has_loop_escape(&i.then_block) {
                return true;
            }
            let mut cursor = i.else_branch.as_deref();
            while let Some(branch) = cursor {
                match branch {
                    juxc_ast::ElseBranch::If(inner) => {
                        if block_has_loop_escape(&inner.then_block) {
                            return true;
                        }
                        cursor = inner.else_branch.as_deref();
                    }
                    juxc_ast::ElseBranch::Block(b) => return block_has_loop_escape(b),
                }
            }
            false
        }
        // Loops swallow their own escapes — don't descend.
        Stmt::While(_) | Stmt::DoWhile(_) | Stmt::ForEach(_) | Stmt::ForC(_) => false,
        Stmt::Labeled { stmt, .. } => stmt_has_loop_escape(stmt),
        // A nested try emits ENTIRELY inside the outer closure — its
        // body, catch arms, and finally all need the outer channel
        // when they escape (the nested machinery threads through its
        // own channel first, then its dispatch re-escapes outward).
        Stmt::Try(t) => {
            block_has_loop_escape(&t.body)
                || t.catches.iter().any(|c| block_has_loop_escape(&c.body))
                || t.finally
                    .as_ref()
                    .map(|f| block_has_loop_escape(f))
                    .unwrap_or(false)
        }
        Stmt::Unsafe(b) => block_has_loop_escape(b),
        Stmt::Expr(juxc_ast::Expr::Switch(sw)) => sw.arms.iter().any(|arm| match &arm.body {
            juxc_ast::SwitchBody::Block(b) => block_has_loop_escape(b),
            juxc_ast::SwitchBody::Expr(_) => false,
        }),
        _ => false,
    }
}

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
                // **Try-body return threading.** Inside a `try` block's
                // `catch_unwind` closure a `return` can't exit the
                // enclosing fn — it threads the value out as
                // `Some(value)` (`return;` in a void fn → `Some(())`),
                // and the try lowering's post-`finally` step performs
                // the real return. See `emit_try`.
                let in_try = self.in_try_closure;
                // **Catch-arm return parking** (§X.3.2): a `return`
                // in a catch body must run the try's `finally` first.
                // The value parks in `__jux_ret` and the dispatch
                // block breaks; the post-`finally` tail performs the
                // real return. (Inside a nested try's closure the
                // closure-threading takes precedence.)
                let park = self.in_catch_arm;
                if park {
                    self.w.push_str("{ __jux_ret = ");
                } else {
                    self.w.push_str("return");
                }
                let channel_wrap = in_try || park;
                if value.is_none() && channel_wrap {
                    if park {
                        self.w.push_str("Some(()); break '__jux_catch; }\n");
                    } else {
                        self.w.push_str(" Some(());\n");
                    }
                    return;
                }
                if let Some(e) = value {
                    if !park {
                        self.w.push(' ');
                    }
                    if channel_wrap {
                        self.w.push_str("Some(");
                    }
                    // Nullable-return coercion: when the enclosing
                    // fn returns `T?` (lowered as `Option<T>`) and
                    // the value being returned isn't already a
                    // `null` literal, wrap it in `Some(...)` so the
                    // type-check passes. A `return null;` already
                    // lowers to `return None;` via `emit_literal`.
                    let wrap_some = self.return_wants_some_wrap(e);
                    // Sealed-upcast coercion: `return new Err(...)`
                    // inside a `Result`-returning function wraps
                    // through `.into()` so the auto-`From<Err> for
                    // Result` impl produces `Result::Err(err)`.
                    let wrap_upcast = self.return_needs_sealed_upcast(e);
                    // Interface return slot: a class value is wrapped in
                    // `Rc<dyn Trait>`, an interface value is `Rc`-cloned — so
                    // a `Shape`-returning factory hands back the same
                    // trait-object representation locals / params use.
                    let ret_iface_ty = match &self.current_return_type {
                        Some(juxc_ast::ReturnType::Type(t))
                        | Some(juxc_ast::ReturnType::AsyncType(t))
                            if !matches!(
                                self.iface_coercion_to(t, e),
                                crate::analysis::IfaceCoercion::None,
                            ) =>
                        {
                            Some(t.clone())
                        }
                        _ => None,
                    };
                    // A nullable dyn return (`Animal? f() { return new Dog(); }`)
                    // is `Some`-wrapped INSIDE the coercion helper — don't add a
                    // second `Some(...)` here.
                    let do_some = wrap_some && ret_iface_ty.is_none();
                    if do_some {
                        self.w.push_str("Some(");
                    }
                    if let Some(ret_ty) = ret_iface_ty {
                        self.emit_expr_coerced_to_iface(&ret_ty, e);
                    } else {
                        self.emit_expr(e);
                        // **Wrapper-class share-on-return (§CR.4.1).** A
                        // `return <wrapped place>;` (a `Path`/`this` local or
                        // an `xs[i]` index read of a wrapped class) must hand
                        // the caller a SHARED handle, not move out of the
                        // place — append the cheap `Rc` refcount-bump clone.
                        // Skipped under `Some(...)`/upcast wraps, which only
                        // fire for nullable / sealed shapes (never a bare
                        // wrapped place) — the helper would return false there
                        // anyway, but gating keeps the emit unambiguous.
                        if !wrap_some && !wrap_upcast && self.wrapper_value_needs_clone(e) {
                            self.w.push_str(".clone()");
                        }
                        if wrap_upcast {
                            self.w.push_str(".into()");
                        }
                    }
                    if do_some {
                        self.w.push(')');
                    }
                    if channel_wrap {
                        self.w.push(')');
                    }
                }
                if park {
                    self.w.push_str("; break '__jux_catch; }\n");
                } else {
                    self.w.push_str(";\n");
                }
            }
            Stmt::VarDecl(var) => self.emit_var_decl(var),
            Stmt::If(if_stmt) => self.emit_if(if_stmt),
            Stmt::While(w) => self.emit_while(w),
            Stmt::DoWhile(d) => self.emit_do_while(d),
            // Labeled loop: Rust spells it `'label: while …`. The label
            // parks in `pending_loop_label`; the inner loop's emitter
            // attaches it directly to its loop keyword (for `for_c`
            // that's the INNER `loop`, past the init-scope block).
            Stmt::Labeled { label, stmt } => {
                self.pending_loop_label = Some(label.text.clone());
                self.emit_stmt(stmt);
            }
            Stmt::ForEach(f) => self.emit_for_each(f),
            Stmt::ForC(f) => self.emit_for_c(f),
            Stmt::Assign(a) => self.emit_assign(a),
            // Loop-control statements — the optional label targets an
            // enclosing `Stmt::Labeled` loop (`break outer;` →
            // `break 'outer;`). Inside a `try` lowering the escape may
            // thread through a `__jux_loopctl` channel instead — see
            // `emit_loop_escape`.
            Stmt::Break(label, _) => {
                self.emit_loop_escape(true, label.as_ref().map(|l| l.text.as_str()));
                self.w.push('\n');
            }
            Stmt::Continue(label, _) => {
                self.emit_loop_escape(false, label.as_ref().map(|l| l.text.as_str()));
                self.w.push('\n');
            }
            Stmt::SuperCall(_, _) => {
                // `super(args);` is lifted out of the body by
                // `emit_constructor` into the child struct's literal
                // (`__parent: Parent::new(args)`). Any super call that
                // reaches this point is dead — extract it before
                // calling `emit_stmt`. The arm exists for exhaustive
                // matching; emitting nothing keeps generated Rust
                // valid even if a future refactor leaves one behind.
            }
            Stmt::Throw(e, _) => {
                // Typed payload: the thrown value goes through
                // `std::panic::panic_any` with its concrete type
                // preserved. The catch_unwind in any enclosing
                // `try` block downcasts the payload to the catch
                // clause's declared type (`Box<dyn Any +
                // Send>.downcast::<T>()`), so `catch (T ex)` binds
                // `ex` as the actual `T` instance — fields and
                // methods on it work as written.
                //
                // For panic-aborted binaries the rendered panic
                // header still needs a printable representation;
                // every user class derives `Debug` so the
                // default-hook output reads like the value's
                // `{:?}` form. We don't synthesize an extra
                // String payload here — the typed object IS the
                // payload, and the catch-site recovers it
                // verbatim.
                if self.in_catch_arm {
                    // **Catch-arm throw parking** (§X.3.2): the new
                    // exception runs `finally` first, then propagates
                    // — same channel an unmatched payload uses.
                    self.w.push_str(
                        "{ __jux_unhandled = Some(::std::boxed::Box::new(",
                    );
                    self.emit_expr(e);
                    self.w.push_str(
                        ") as ::std::boxed::Box<dyn ::std::any::Any + ::std::marker::Send>); break '__jux_catch; }\n",
                    );
                    return;
                }
                self.w.push_str("std::panic::panic_any(");
                self.emit_expr(e);
                self.w.push_str(");\n");
            }
            Stmt::Try(t) => self.emit_try(t),
            Stmt::Unsafe(block) => {
                // `unsafe { … }` lowers verbatim to a Rust `unsafe { … }`
                // block — the body's statements (which may call `unsafe`
                // foreign fns or use raw-pointer ops) emit unchanged inside.
                self.w.push_str("unsafe {\n");
                self.w.indent_inc();
                self.emit_block_contents(block);
                self.w.indent_dec();
                self.w.emit_indent();
                self.w.push_str("}\n");
            }
        }
    }

    /// Lower a **try-expression** (§X.3.3) — the value-producing
    /// form. The try block runs inside `catch_unwind` with its
    /// trailing expression as the closure's value; on unwind, the
    /// catch dispatch runs inside a value-labelled block where each
    /// matching arm `break`s with ITS trailing expression; an
    /// unmatched payload resumes the unwind (re-throw).
    pub(crate) fn emit_try_expr(&mut self, t: &juxc_ast::TryStmt) {
        // Split a block into (leading stmts, trailing value expr).
        // Tycheck guarantees the trailing-expression shape; fall back
        // to unit-yield on malformed recovery trees.
        fn split_tail(b: &juxc_ast::Block) -> (&[juxc_ast::Stmt], Option<&Expr>) {
            match b.statements.split_last() {
                Some((juxc_ast::Stmt::Expr(tail), rest)) => (rest, Some(tail)),
                _ => (&b.statements[..], None),
            }
        }
        let (body_stmts, body_tail) = split_tail(&t.body);
        self.w.push_str(
            "match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {\n",
        );
        self.w.indent_inc();
        let prev_catch_arm = self.in_catch_arm;
        self.in_catch_arm = false;
        for stmt in body_stmts {
            self.emit_source_marker(stmt_span(stmt));
            self.w.emit_indent();
            self.emit_stmt(stmt);
        }
        if let Some(tail) = body_tail {
            self.w.emit_indent();
            self.emit_expr(tail);
            self.w.push('\n');
        }
        self.in_catch_arm = prev_catch_arm;
        self.w.indent_dec();
        self.w.emit_indent();
        self.w.push_str("})) {\n");
        self.w.indent_inc();
        self.w.line("Ok(__jux_v) => __jux_v,");
        self.w.line("Err(__jux_payload) => '__jux_catch_v: {");
        self.w.indent_inc();
        self.w.line(
            "let mut __jux_payload_slot: Option<::std::boxed::Box<dyn ::std::any::Any + ::std::marker::Send>> = Some(__jux_payload);",
        );
        for clause in &t.catches {
            let mut clause_tys = vec![&clause.ty];
            clause_tys.extend(clause.alt_tys.iter());
            let binder_fqn = self.catch_binder_fqn(&clause_tys);
            let mut muts = std::collections::HashSet::new();
            crate::analysis::collect_mutated_names(
                &clause.body,
                &mut muts,
                &self.user_mut_methods,
            );
            let binder_mut = muts.contains(&clause.name.text);
            for ty in clause_tys {
                let arm_fqn = self.resolve_catch_ty_fqn(ty);
                let depth = match (&arm_fqn, &binder_fqn) {
                    (Some(a), Some(b)) => self.extends_chain_distance(a, b).unwrap_or(0),
                    _ => 0,
                };
                self.w
                    .line("if let Some(__jux_p) = __jux_payload_slot.take() {");
                self.w.indent_inc();
                self.w.emit_indent();
                self.w.push_str("match __jux_p.downcast::<");
                self.emit_type_as_rust(ty);
                self.w.push_str(">() {\n");
                self.emit_try_expr_arm(clause, depth, binder_mut);
                for sub_fqn in self.catch_subclass_fqns(ty) {
                    let sub_depth = match &binder_fqn {
                        Some(b) => self.extends_chain_distance(&sub_fqn, b).unwrap_or(0),
                        None => 0,
                    };
                    self.w
                        .line("if let Some(__jux_p) = __jux_payload_slot.take() {");
                    self.w.indent_inc();
                    self.w.emit_indent();
                    self.w.push_str("match __jux_p.downcast::<");
                    self.emit_fqn_path_in_rust(&sub_fqn, sub_fqn.contains('.'));
                    self.w.push_str(">() {\n");
                    self.emit_try_expr_arm(clause, sub_depth, binder_mut);
                }
            }
        }
        // No clause matched — re-throw (§X.3.3). `resume_unwind`
        // diverges, so the labelled block's type stays the arms'.
        self.w.line(
            "std::panic::resume_unwind(__jux_payload_slot.take().expect(\"unmatched try-expression payload\"))",
        );
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.emit_indent();
        self.w.push('}');
    }

    /// One value-yielding downcast arm of a try-expression's catch
    /// dispatch: bind (slicing to the binder's static type), run the
    /// clause's leading statements, `break` the value block with the
    /// trailing expression.
    fn emit_try_expr_arm(
        &mut self,
        clause: &juxc_ast::CatchClause,
        slice_depth: usize,
        binder_mut: bool,
    ) {
        self.w.indent_inc();
        self.w.emit_indent();
        self.w.push_str("Ok(__jux_boxed) => {\n");
        self.w.indent_inc();
        self.w.emit_indent();
        self.w.push_str("let ");
        if binder_mut {
            self.w.push_str("mut ");
        }
        self.w.push_str(&clause.name.text);
        self.w.push_str(" = (*__jux_boxed)");
        for _ in 0..slice_depth {
            self.w.push_str(".__parent");
        }
        self.w.push_str(";\n");
        let (stmts, tail) = match clause.body.statements.split_last() {
            Some((juxc_ast::Stmt::Expr(tail), rest)) => (rest, Some(tail)),
            _ => (&clause.body.statements[..], None),
        };
        for stmt in stmts {
            self.emit_source_marker(stmt_span(stmt));
            self.w.emit_indent();
            self.emit_stmt(stmt);
        }
        self.w.emit_indent();
        self.w.push_str("break '__jux_catch_v ");
        if let Some(tail) = tail {
            self.emit_expr(tail);
        } else {
            self.w.push_str("()");
        }
        self.w.push_str(";\n");
        self.w.indent_dec();
        self.w.line("}");
        self.w
            .line("Err(__jux_rest) => { __jux_payload_slot = Some(__jux_rest); }");
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
    }

    /// One `downcast` match's arms for a catch clause: bind the
    /// recovered value, run the body, break out of the dispatch
    /// block; thread the payload onward on miss. Closes the match
    /// AND its enclosing `if let` (the caller opened both).
    fn emit_catch_arm_body(
        &mut self,
        binder: &str,
        body: &juxc_ast::Block,
        slice_depth: usize,
        binder_mut: bool,
    ) {
        self.w.indent_inc();
        self.w.emit_indent();
        self.w.push_str("Ok(__jux_boxed) => {\n");
        self.w.indent_inc();
        self.w.emit_indent();
        self.w.push_str("let ");
        if binder_mut {
            self.w.push_str("mut ");
        }
        self.w.push_str(binder);
        self.w.push_str(" = (*__jux_boxed)");
        // Upcast slice: a subclass payload binds the BASE slice the
        // body was type-checked against (`__parent` per inheritance
        // step). Phase-1 note: a rethrown binder carries the sliced
        // type, not the original concrete one.
        for _ in 0..slice_depth {
            self.w.push_str(".__parent");
        }
        self.w.push_str(";\n");
        let prev_arm = self.in_catch_arm;
        self.in_catch_arm = true;
        self.emit_block_contents(body);
        self.in_catch_arm = prev_arm;
        self.w.line("break '__jux_catch;");
        self.w.indent_dec();
        self.w.line("}");
        self.w
            .line("Err(__jux_rest) => { __jux_payload_slot = Some(__jux_rest); }");
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
    }

    /// Resolve a catch-clause TypeRef to its FQN key in the class
    /// table — multi-segment paths join verbatim; bare names try the
    /// exact key, then the unique-suffix scan.
    fn resolve_catch_ty_fqn(&self, ty: &juxc_ast::TypeRef) -> Option<String> {
        if ty.name.segments.len() > 1 {
            return Some(
                ty.name
                    .segments
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join("."),
            );
        }
        let bare = ty.name.segments.first()?.text.clone();
        if self.symbols.classes.contains_key(&bare) {
            return Some(bare);
        }
        self.symbols.find_fqn_by_bare(&bare)
    }

    /// Number of `extends` steps from `from` up to `to` (0 when they
    /// are the same class); `None` when `to` isn't an ancestor.
    fn extends_chain_distance(&self, from: &str, to: &str) -> Option<usize> {
        let mut cur = from.to_string();
        let mut depth = 0usize;
        loop {
            if cur == to {
                return Some(depth);
            }
            if depth > 64 {
                return None;
            }
            depth += 1;
            cur = self.symbols.classes.get(&cur)?.extends_fqn.clone()?;
        }
    }

    /// The catch binder's static type as an FQN: the declared type
    /// for a single-type clause, or the most specific common
    /// superclass of a multi-catch's alternatives (§X.3.6) — chain
    /// intersection, mirroring tycheck's computation.
    fn catch_binder_fqn(&self, tys: &[&juxc_ast::TypeRef]) -> Option<String> {
        let fqns: Vec<String> = tys
            .iter()
            .map(|t| self.resolve_catch_ty_fqn(t))
            .collect::<Option<Vec<_>>>()?;
        if fqns.len() == 1 {
            return Some(fqns[0].clone());
        }
        let chain = |start: &str| -> Vec<String> {
            let mut out = Vec::new();
            let mut cur = Some(start.to_string());
            let mut depth = 0usize;
            while let Some(n) = cur {
                if depth > 64 {
                    break;
                }
                depth += 1;
                cur = self.symbols.classes.get(&n).and_then(|c| c.extends_fqn.clone());
                out.push(n);
            }
            out
        };
        let first = chain(&fqns[0]);
        let rest: Vec<Vec<String>> = fqns[1..].iter().map(|f| chain(f)).collect();
        first
            .iter()
            .find(|cand| rest.iter().all(|ch| ch.contains(cand)))
            .cloned()
    }

    /// Every known transitive SUBCLASS of the catch type `ty`, by
    /// FQN, sorted for deterministic emission. Drives the §X.3.4
    /// subtype-matching arms — `Any::downcast` is exact-type, so the
    /// clause tries each concrete descendant explicitly.
    fn catch_subclass_fqns(&self, ty: &juxc_ast::TypeRef) -> Vec<String> {
        let base_fqn: String = if ty.name.segments.len() > 1 {
            ty.name
                .segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(".")
        } else {
            let bare = match ty.name.segments.first() {
                Some(s) => s.text.clone(),
                None => return Vec::new(),
            };
            if self.symbols.classes.contains_key(&bare) {
                bare
            } else {
                match self.symbols.find_fqn_by_bare(&bare) {
                    Some(fqn) => fqn,
                    None => return Vec::new(),
                }
            }
        };
        let mut out: Vec<String> = self
            .symbols
            .classes
            .keys()
            .filter(|fqn| **fqn != base_fqn)
            .filter(|fqn| {
                // Walk the extends chain up from the candidate.
                let mut cur = self
                    .symbols
                    .classes
                    .get(*fqn)
                    .and_then(|c| c.extends_fqn.clone());
                let mut depth = 0usize;
                while let Some(p) = cur {
                    if depth > 64 {
                        return false;
                    }
                    depth += 1;
                    if p == base_fqn {
                        return true;
                    }
                    cur = self.symbols.classes.get(&p).and_then(|c| c.extends_fqn.clone());
                }
                false
            })
            .cloned()
            .collect();
        out.sort();
        out
    }

    /// Emit a `break`/`continue` statement's text (NO trailing
    /// newline — callers add it so the dispatch emitter can inline the
    /// escape inside an `if … { … }` one-liner).
    ///
    /// **O2 interception.** When the innermost in-region
    /// [`TryLoopCtl`] channel was opened at the CURRENT loop depth,
    /// the escape targets a loop outside that try's machinery and
    /// can't be emitted directly. Instead:
    ///
    /// - **Body region** (inside the `catch_unwind` closure):
    ///   `{ __jux_loopctl_N = code; return …; }` — the early return
    ///   exits the closure; the carrier shape (`return None;` for a
    ///   return-threading closure, bare `return;` otherwise) keeps the
    ///   closure's type. ASYNC channels store through the
    ///   `_…_body` `Arc<AtomicU8>` clone instead (O9).
    /// - **Catch region** (inside the `'__jux_catch` dispatch block):
    ///   `{ __jux_loopctl_N = code; break '__jux_catch; }` (async:
    ///   `.store(code, Relaxed)` on the original handle).
    ///
    /// `emit_try`'s post-`finally` dispatch re-issues the real escape
    /// — through THIS same method, so a try nested inside another
    /// try's closure threads outward channel by channel.
    ///
    /// Channels whose region is `Off` are skipped (we're in their
    /// machinery or `finally`, which emit outside the closure) — an
    /// enclosing channel may still apply.
    pub(crate) fn emit_loop_escape(&mut self, is_break: bool, label: Option<&str>) {
        let depth = self.loop_emit_depth;
        let intercept = self
            .try_loopctl
            .iter_mut()
            .rev()
            .find(|ch| ch.region != LoopCtlRegion::Off)
            .filter(|ch| ch.base_depth == depth)
            .map(|ch| {
                let key = (is_break, label.map(|s| s.to_string()));
                let code = match ch.escapes.iter().position(|e| *e == key) {
                    Some(i) => i + 1,
                    None => {
                        ch.escapes.push(key);
                        ch.escapes.len()
                    }
                };
                (
                    ch.flag.clone(),
                    code,
                    ch.region == LoopCtlRegion::Body,
                    ch.closure_has_ret,
                    ch.is_async,
                )
            });
        if let Some((flag, code, in_body, has_ret, is_async)) = intercept {
            self.w.push_str("{ ");
            if is_async {
                // Atomic store (O9). Body region writes through the
                // `_`-prefixed clone moved into the `async move`
                // block; catch arms emit outside it and use the
                // original handle.
                if in_body {
                    self.w.push('_');
                }
                self.w.push_str(&flag);
                if in_body {
                    self.w.push_str("_body");
                }
                self.w.push_str(".store(");
                self.w.push_str(&code.to_string());
                self.w
                    .push_str(", ::std::sync::atomic::Ordering::Relaxed)");
            } else {
                self.w.push_str(&flag);
                self.w.push_str(" = ");
                self.w.push_str(&code.to_string());
            }
            if in_body {
                if has_ret {
                    self.w.push_str("; return None; }");
                } else {
                    self.w.push_str("; return; }");
                }
            } else {
                self.w.push_str("; break '__jux_catch; }");
            }
            return;
        }
        self.w.push_str(if is_break { "break" } else { "continue" });
        if let Some(l) = label {
            self.w.push_str(" '");
            self.w.push_str(l);
        }
        self.w.push(';');
    }

    /// Lower a Jux `try / catch / finally` statement to Rust using
    /// `std::panic::catch_unwind` as the unwinding mechanism. The
    /// shape per spec §X.3.2:
    ///
    /// ```text
    /// try B0 catch (T1 e1) B1 ... finally Bf
    /// ```
    ///
    /// becomes:
    ///
    /// ```text
    /// {
    ///     let __jux_try_result = std::panic::catch_unwind(
    ///         std::panic::AssertUnwindSafe(|| { B0 })
    ///     );
    ///     match __jux_try_result {
    ///         Ok(_) => {}
    ///         Err(__payload) => {
    ///             let e1: String = /* downcast __payload to String */;
    ///             B1
    ///         }
    ///     }
    ///     Bf
    /// }
    /// ```
    ///
    /// **Phase-1 caveat.** The caught name is bound as `String`
    /// regardless of the declared catch type — the full
    /// typed-exception story lands when the Result-mode pass
    /// arrives. Single catch only in this shape; multi-catch and
    /// per-type filtering chain as `else if`/`match` arms.
    pub(crate) fn emit_try(&mut self, t: &juxc_ast::TryStmt) {
        // Two lowering shapes, chosen by whether the try body
        // contains an `await`:
        //
        //   - **Sync**: `std::panic::catch_unwind(AssertUnwindSafe(||
        //     { body }))`. The closure captures locals by
        //     reference; `body` mutations on outer vars
        //     propagate.
        //   - **Async**: `AssertUnwindSafe(async move { body })
        //     .catch_unwind().await` (from `futures::FutureExt`).
        //     The async block captures locals by move, so try
        //     bodies that need to mutate outer state in an async
        //     context must thread the value out via the result
        //     instead.
        //
        // **Java control-flow semantics** (§X.3):
        //   - a `return` inside the try body computes its value, runs
        //     `finally`, then returns — the closure can't return from
        //     the enclosing fn, so the body's returns thread out as
        //     `Some(value)` (the `in_try_closure` flag rewrites them)
        //     and a post-`finally` `if let` performs the real return;
        //   - an UNMATCHED (or uncaught — no catch clauses) exception
        //     runs `finally` FIRST, then resumes unwinding — the
        //     payload parks in `__jux_unhandled` across the finally.
        let is_async = crate::analysis::block_contains_await(&t.body);
        // The return channel is needed when the BODY returns (threads
        // through the closure) OR any CATCH body returns (parks from
        // the dispatch arm — §X.3.2: finally runs before the return).
        let has_ret = block_contains_fn_return(&t.body)
            || t.catches.iter().any(|c| block_contains_fn_return(&c.body));
        // O2/O9: loop-control channel — needed when the body or a
        // catch arm `break`s/`continue`s a loop OUTSIDE this try (the
        // closure / dispatch block traps direct loop control). Sync
        // tries use a `u8` flag captured by reference; async tries use
        // an `Arc<AtomicU8>` whose clone moves into the `async move`
        // block (O9 — the by-`move` capture is why a plain local can't
        // thread out).
        let wants_loopctl = try_needs_loopctl(t);
        // Wrap the whole thing in a block so locals introduced by
        // the lowering don't leak.
        self.w.push_str("{\n");
        self.w.indent_inc();
        if wants_loopctl {
            // Flag local — 0 = no escape; other codes are assigned in
            // first-encounter order by `emit_loop_escape` and
            // dispatched after `finally` below. Named by channel
            // stack depth so nested try machineries never shadow.
            let flag = format!("__jux_loopctl_{}", self.try_loopctl.len());
            self.w.emit_indent();
            if is_async {
                // Atomic channel (O9): the `_body` clone is what the
                // `async move` block captures; the original stays
                // available to the catch arms and the post-`finally`
                // dispatch. Single-underscore prefix on the clone so
                // a try whose only escapes sit in catch arms doesn't
                // warn about the unused moved-in handle.
                self.w.push_str("let ");
                self.w.push_str(&flag);
                self.w
                    .push_str(" = ::std::sync::Arc::new(::std::sync::atomic::AtomicU8::new(0));\n");
                self.w.emit_indent();
                self.w.push_str("let _");
                self.w.push_str(&flag);
                self.w.push_str("_body = ");
                self.w.push_str(&flag);
                self.w.push_str(".clone();\n");
            } else {
                self.w.push_str("let mut ");
                self.w.push_str(&flag);
                self.w.push_str(": u8 = 0;\n");
            }
            self.try_loopctl.push(TryLoopCtl {
                flag,
                is_async,
                base_depth: self.loop_emit_depth,
                closure_has_ret: has_ret,
                region: LoopCtlRegion::Body,
                escapes: Vec::new(),
            });
        }
        if has_ret {
            // Return-value channel — `Option<RetT>`, `None` = the body
            // ran to completion without returning. Inside a LAMBDA
            // body (S9) the channel's type comes from inference: the
            // lambda's return type isn't in `current_return_type`
            // (that's the enclosing function's), and the threaded
            // `Some(v)` values pin it for rustc.
            self.w.emit_indent();
            if self.in_lambda_body {
                self.w.push_str("let mut __jux_ret = None;\n");
            } else {
                self.w.push_str("let mut __jux_ret: Option<");
                match self.current_return_type.clone() {
                    Some(juxc_ast::ReturnType::Type(rt))
                    | Some(juxc_ast::ReturnType::AsyncType(rt)) => {
                        self.emit_return_type_as_rust(&rt);
                    }
                    _ => self.w.push_str("()"),
                }
                self.w.push_str("> = None;\n");
            }
        }
        // Unhandled-exception channel — holds the payload across the
        // `finally` body so propagation happens AFTER it runs.
        self.w.line(
            "let mut __jux_unhandled: Option<::std::boxed::Box<dyn ::std::any::Any + ::std::marker::Send>> = None;",
        );
        // S10: the `async move` block would STEAL wrapper-class
        // handles referenced in the body (`c.inc()` moves `c`'s only
        // `Rc` clone in; any use after the try is E0382). Shadow each
        // such binding with a share-clone first — the block moves the
        // clone, the original stays live, both point at the same
        // `RefCell`. Same rule as lambda captures (`emit_lambda`).
        if is_async {
            let mut names: Vec<String> = Vec::new();
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            crate::exprs::collect_bare_names_block(&t.body, &mut |n| {
                if seen.insert(n.to_string()) {
                    names.push(n.to_string());
                }
            });
            names.retain(|n| {
                self.local_types
                    .iter()
                    .rev()
                    .find_map(|s| s.get(n))
                    .and_then(|ty| match ty {
                        juxc_tycheck::Ty::User { name, .. } => {
                            Some(name.rsplit('.').next().unwrap_or(name).to_string())
                        }
                        _ => None,
                    })
                    .map(|c| {
                        // Builtin runtime HANDLES (streams, channels,
                        // mutexes, atomics) share exactly like wrapper
                        // classes — without the rebind the async block
                        // steals the only handle.
                        self.wrapper_classes.contains(&c)
                            || matches!(
                                c.as_str(),
                                "Stream" | "Channel" | "AsyncMutex" | "AtomicInt" | "AtomicLong",
                            )
                    })
                    .unwrap_or(false)
            });
            for name in &names {
                self.w.emit_indent();
                self.w.push_str("let ");
                self.w.push_str(name);
                self.w.push_str(" = ");
                self.w.push_str(name);
                self.w.push_str(".clone();\n");
            }
        }
        self.w.emit_indent();
        if has_ret {
            if self.in_lambda_body {
                // S9: inferred — see the `__jux_ret` channel above.
                self.w.push_str("let __jux_try_result = ");
            } else {
                self.w.push_str("let __jux_try_result: std::thread::Result<Option<");
                match self.current_return_type.clone() {
                    Some(juxc_ast::ReturnType::Type(rt))
                    | Some(juxc_ast::ReturnType::AsyncType(rt)) => {
                        self.emit_return_type_as_rust(&rt);
                    }
                    _ => self.w.push_str("()"),
                }
                self.w.push_str(">> = ");
            }
        } else {
            self.w
                .push_str("let __jux_try_result: std::thread::Result<()> = ");
        }
        let prev_try_flag = self.in_try_closure;
        self.in_try_closure = has_ret;
        // A nested try inside a catch arm: its body's throws/returns
        // belong to ITS machinery, not the enclosing arm's parking.
        let prev_catch_arm = self.in_catch_arm;
        self.in_catch_arm = false;
        if is_async {
            // `futures::FutureExt::catch_unwind(...)` is fully
            // qualified so we don't need a `use` statement at the
            // emit site. `AssertUnwindSafe<Fut>` impls `Future +
            // UnwindSafe`, satisfying the extension trait's bound.
            self.w.push_str(
                "futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async move {\n",
            );
            self.w.indent_inc();
            self.emit_block_contents(&t.body);
            if has_ret {
                self.w.line("None");
            }
            self.w.indent_dec();
            self.w.emit_indent();
            self.w.push_str("})).await;\n");
        } else {
            self.w
                .push_str("std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {\n");
            self.w.indent_inc();
            self.emit_block_contents(&t.body);
            if has_ret {
                self.w.line("None");
            }
            self.w.indent_dec();
            self.w.emit_indent();
            self.w.push_str("}));\n");
        }
        self.in_try_closure = prev_try_flag;
        self.in_catch_arm = prev_catch_arm;
        // O2: emission is past the closure — catch arm bodies emit
        // inside the `'__jux_catch` dispatch block next, where an
        // escape threads via `break '__jux_catch` instead of an early
        // closure return.
        if wants_loopctl {
            if let Some(ch) = self.try_loopctl.last_mut() {
                ch.region = LoopCtlRegion::Catch;
            }
        }
        // Match on the result and run the appropriate catch.
        self.w.emit_indent();
        self.w.push_str("match __jux_try_result {\n");
        self.w.indent_inc();
        if has_ret {
            self.w.line("Ok(__jux_body_ret) => { __jux_ret = __jux_body_ret; }");
        } else {
            self.w.line("Ok(_) => {}");
        }
        self.w.emit_indent();
        self.w.push_str("Err(__jux_payload) => {\n");
        self.w.indent_inc();
        // Typed-payload dispatch: try each catch clause in source
        // order. Each clause attempts `downcast::<T>()`; on success
        // it binds the catch name to the recovered typed value and
        // breaks out of the labelled block. On failure, the payload
        // threads through to the next clause. If no clause matches
        // the payload parks in `__jux_unhandled` — `finally` runs,
        // THEN the panic resumes (mirrors Java's "finally before
        // propagation").
        //
        // A labelled block (`'__jux_catch: { ... break '__jux_catch;
        // ... }`) is the cleanest way to express "stop dispatch
        // after the first match" without nesting matches arbitrarily
        // deep.
        if t.catches.is_empty() {
            // No catch clauses (try/finally form). Park the payload —
            // `finally` runs below, then the unwind resumes.
            self.w.line("__jux_unhandled = Some(__jux_payload);");
        } else {
            // Fully qualify `::std::boxed::Box` so a user class
            // named `Box` doesn't shadow it. `std::panic::catch_unwind`
            // hands back the typed-erased payload as `Box<dyn Any +
            // Send>`; we keep the same Box type to feed back into
            // `resume_unwind` if no catch matches.
            self.w.line(
                "let mut __jux_payload_slot: Option<::std::boxed::Box<dyn ::std::any::Any + ::std::marker::Send>> = Some(__jux_payload);",
            );
            self.w.line("'__jux_catch: {");
            self.w.indent_inc();
            for clause in &t.catches {
                // Pull the payload back out, try the downcast, and
                // either run the body (consuming the value) or thread
                // the unrecovered payload back to the slot.
                //
                // `Any::downcast` matches the payload's EXACT type, so
                // a `catch (T e)` clause must also try every known
                // SUBCLASS of `T` (§X.3.4: a clause catches its type
                // and all subtypes) — one downcast arm per type, each
                // running the same body. The binder holds the
                // concrete value; inherited methods work via the
                // copy-down pass, and a rethrow keeps the original.
                //
                // A multi-catch (`catch (E1 | E2 e)`, §X.3.6) expands
                // each listed alternative the same way; alternatives
                // are pairwise unrelated (E0721) so at most one arm
                // can ever match.
                let mut clause_tys = vec![&clause.ty];
                clause_tys.extend(clause.alt_tys.iter());
                // The binder's STATIC type: the declared type for a
                // single-type clause; the most specific common
                // supertype of the alternatives for a multi-catch
                // (§X.3.6) — same computation tycheck used to type
                // the body. Every arm binds a value of exactly this
                // type by slicing the concrete payload's `__parent`
                // chain, so the (shared) body compiles uniformly.
                let binder_fqn = self.catch_binder_fqn(&clause_tys);
                // Bind mutably only when the body actually mutates
                // the binder (e.g. `e.addSuppressed(...)`).
                let mut muts = std::collections::HashSet::new();
                crate::analysis::collect_mutated_names(&clause.body, &mut muts, &self.user_mut_methods);
                let binder_mut = muts.contains(&clause.name.text);
                for ty in clause_tys {
                    let arm_fqn = self.resolve_catch_ty_fqn(ty);
                    let depth = match (&arm_fqn, &binder_fqn) {
                        (Some(a), Some(b)) => self.extends_chain_distance(a, b).unwrap_or(0),
                        _ => 0,
                    };
                    self.w
                        .line("if let Some(__jux_p) = __jux_payload_slot.take() {");
                    self.w.indent_inc();
                    self.w.emit_indent();
                    self.w.push_str("match __jux_p.downcast::<");
                    self.emit_type_as_rust(ty);
                    self.w.push_str(">() {\n");
                    self.emit_catch_arm_body(&clause.name.text, &clause.body, depth, binder_mut);
                    for sub_fqn in self.catch_subclass_fqns(ty) {
                        let sub_depth = match &binder_fqn {
                            Some(b) => {
                                self.extends_chain_distance(&sub_fqn, b).unwrap_or(0)
                            }
                            None => 0,
                        };
                        self.w
                            .line("if let Some(__jux_p) = __jux_payload_slot.take() {");
                        self.w.indent_inc();
                        self.w.emit_indent();
                        self.w.push_str("match __jux_p.downcast::<");
                        self.emit_fqn_path_in_rust(&sub_fqn, sub_fqn.contains('.'));
                        self.w.push_str(">() {\n");
                        self.emit_catch_arm_body(
                            &clause.name.text,
                            &clause.body,
                            sub_depth,
                            binder_mut,
                        );
                    }
                }
            }
            // No clause matched — park the payload; `finally` runs
            // first, then the unwind resumes below.
            self.w.line(
                "if let Some(__jux_p) = __jux_payload_slot.take() { __jux_unhandled = Some(__jux_p); }",
            );
            self.w.indent_dec();
            self.w.line("}");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        // O2: the dispatch block is closed — `finally` emits inline
        // (outside both the closure and the labeled block), where
        // plain loop control is already legal. Suspend this channel;
        // an ENCLOSING one may still intercept.
        if wants_loopctl {
            if let Some(ch) = self.try_loopctl.last_mut() {
                ch.region = LoopCtlRegion::Off;
            }
        }
        // Finally: emit its body verbatim after the match. Runs
        // in both success and failure paths — and BEFORE an
        // unmatched exception resumes or a try-body `return`
        // completes (Java ordering).
        if let Some(fin) = &t.finally {
            self.emit_block_contents(fin);
        }
        // Resume an unmatched/uncaught exception now that `finally`
        // ran.
        self.w.line(
            "if let Some(__jux_p) = __jux_unhandled { std::panic::resume_unwind(__jux_p); }",
        );
        // Complete a `return` the try body initiated. When THIS try
        // is itself nested inside another try's closure, the real
        // return threads outward as `Some(...)` again — the restored
        // `in_try_closure` flag picks the shape.
        if has_ret {
            self.w.emit_indent();
            if self.in_try_closure {
                self.w
                    .push_str("if let Some(__jux_ret_v) = __jux_ret { return Some(__jux_ret_v); }\n");
            } else {
                self.w
                    .push_str("if let Some(__jux_ret_v) = __jux_ret { return __jux_ret_v; }\n");
            }
        }
        // O2: re-issue the real `break`/`continue` the body or a
        // catch arm requested — AFTER `finally` ran (Java ordering).
        // The escape goes back through `emit_loop_escape`: when THIS
        // try sits inside another try's closure, the channel just
        // popped no longer applies and the enclosing one intercepts,
        // threading the escape outward one machinery layer at a time.
        if wants_loopctl {
            let ch = self.try_loopctl.pop().expect("loopctl channel pushed above");
            for (i, (is_break, label)) in ch.escapes.iter().enumerate() {
                self.w.emit_indent();
                self.w.push_str("if ");
                self.w.push_str(&ch.flag);
                if ch.is_async {
                    self.w
                        .push_str(".load(::std::sync::atomic::Ordering::Relaxed)");
                }
                self.w.push_str(" == ");
                self.w.push_str(&(i + 1).to_string());
                self.w.push_str(" { ");
                self.emit_loop_escape(*is_break, label.as_deref());
                self.w.push_str(" }\n");
            }
        }
        self.w.indent_dec();
        self.w.emit_indent();
        self.w.push_str("}\n");
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
        // §18.6.3 `for await (var x : stream)` — one-shot pull loop
        // over a `Stream<T>`:
        //
        //   {
        //       let __jux_stream = <iter>.clone();
        //       ['label:] while let Some(x) = __jux_stream.next().await { body }
        //   }
        //
        // The handle `.clone()` is an `Rc` bump — a bare local stays
        // usable after the loop, and a combinator-chain expression
        // lands in a fresh temp. `next()` borrows the inner `RefCell`
        // only INSIDE the call, so nothing is held across the body
        // (no H6-style snapshot needed — and none would make sense
        // for an async sequence). A pending loop label binds the
        // `while`, not the wrapper block, so `break 'label` works.
        if f.is_await {
            let label = self.pending_loop_label.take();
            self.w.push_str("{\n");
            self.w.indent_inc();
            self.w.emit_indent();
            self.w.push_str("let __jux_stream = ");
            self.emit_expr_with_parent_prec(&f.iter, u8::MAX, false);
            self.w.push_str(".clone();\n");
            self.w.emit_indent();
            if let Some(l) = label {
                self.w.push('\'');
                self.w.push_str(&l);
                self.w.push_str(": ");
            }
            self.w.push_str("while let Some(");
            self.w.push_str(&f.var_name.text);
            self.w.push_str(") = __jux_stream.next().await {\n");
            self.w.indent_inc();
            // Register the loop variable's element type (the stream's
            // generic arg) so wrapper-class elements deref correctly —
            // same rule as the collection for-each below.
            let elem_ty = match self
                .expr_types
                .get(&crate::exprs::expr_span_of(&f.iter))
            {
                Some(Ty::User { name, generic_args })
                    if name.rsplit('.').next() == Some("Stream") =>
                {
                    generic_args.first().cloned()
                }
                _ => None,
            };
            self.local_types.push(std::collections::HashMap::new());
            if let Some(ty @ Ty::User { .. }) = &elem_ty {
                if let Some(scope) = self.local_types.last_mut() {
                    scope.insert(f.var_name.text.clone(), ty.clone());
                }
            }
            self.loop_emit_depth += 1;
            self.emit_block_contents(&f.body);
            self.loop_emit_depth -= 1;
            self.local_types.pop();
            self.w.indent_dec();
            self.w.line("}");
            self.w.indent_dec();
            self.w.line("}");
            return;
        }
        self.emit_pending_loop_label();
        // Stepped range (§M.6): sign-aware while loop — positive
        // steps count up to the bound, negative steps count down,
        // zero throws a catchable ArithmeticException (ERRATA.md E1).
        if let Expr::Range(r) = &f.iter {
            if let Some(step) = &r.step {
                let var = f.var_name.text.clone();
                self.w.push_str("{\n");
                self.w.indent_inc();
                self.w.emit_indent();
                self.w.push_str("let __jux_step: isize = ");
                self.emit_expr(step);
                self.w.push_str(";\n");
                self.w.line(
                    "if __jux_step == 0 { std::panic::panic_any(crate::jux::std::exceptions::ArithmeticException::new(\"range step must be non-zero\".to_string())); }",
                );
                self.w.emit_indent();
                self.w.push_str("let __jux_end: isize = ");
                self.emit_expr(&r.end);
                self.w.push_str(";\n");
                self.w.emit_indent();
                self.w.push_str("let mut ");
                self.w.push_str(&var);
                self.w.push_str(": isize = ");
                self.emit_expr(&r.start);
                self.w.push_str(";\n");
                self.w.emit_indent();
                if r.inclusive {
                    self.w.push_str(&format!(
                        "while (__jux_step > 0 && {var} <= __jux_end) || (__jux_step < 0 && {var} >= __jux_end) {{\n",
                    ));
                } else {
                    self.w.push_str(&format!(
                        "while (__jux_step > 0 && {var} < __jux_end) || (__jux_step < 0 && {var} > __jux_end) {{\n",
                    ));
                }
                self.w.indent_inc();
                self.loop_emit_depth += 1;
                self.emit_block_contents(&f.body);
                self.loop_emit_depth -= 1;
                self.w.emit_indent();
                self.w.push_str(&var);
                self.w.push_str(" += __jux_step;\n");
                self.w.indent_dec();
                self.w.line("}");
                self.w.indent_dec();
                self.w.emit_indent();
                self.w.push_str("}\n");
                return;
            }
        }
        if matches!(&f.iter, Expr::Range(_)) {
            self.w.push_str("for ");
            self.w.push_str(&f.var_name.text);
            self.w.push_str(" in ");
            self.emit_expr(&f.iter);
            self.w.push_str(" {\n");
            self.w.indent_inc();
            self.loop_emit_depth += 1;
            self.emit_block_contents(&f.body);
            self.loop_emit_depth -= 1;
            self.w.indent_dec();
            self.w.emit_indent();
            self.w.push_str("}\n");
            return;
        }

        // **User iterable** (§O.6/§K.5): the receiver's class
        // declares `iterator()` — drive the protocol directly:
        //
        //   { let mut __jux_it = recv.iterator();
        //     while let Some(x) = __jux_it.next() { body } }
        //
        // `next()` returns Jux `T?` (= Option<T>), so `while let`
        // IS the exhaustion check.
        if let Some(Ty::User { name, .. }) = self.expr_types.get(&expr_span_of(&f.iter)) {
            let speaks_protocol = self.symbols.lookup_method(name, "iterator").is_some()
                || self
                    .symbols
                    .interfaces
                    .get(name.as_str())
                    .map(|i| i.methods.contains_key("iterator"))
                    .unwrap_or(false);
            if speaks_protocol {
                self.w.push_str("{
");
                self.w.indent_inc();
                self.w.emit_indent();
                self.w.push_str("let mut __jux_it = ");
                self.emit_expr_with_parent_prec(&f.iter, u8::MAX, false);
                self.w.push_str(".iterator();
");
                self.w.emit_indent();
                self.w.push_str("while let Some(");
                self.w.push_str(&f.var_name.text);
                self.w.push_str(") = __jux_it.next() {
");
                self.w.indent_inc();
                self.loop_emit_depth += 1;
                self.emit_block_contents(&f.body);
                self.loop_emit_depth -= 1;
                self.w.indent_dec();
                self.w.line("}");
                self.w.indent_dec();
                self.w.emit_indent();
                self.w.push_str("}
");
                return;
            }
        }
        // Three lowering shapes:
        //
        // - **Copy element type** (`int`, `bool`, `char`, `f64`, …):
        //   `for &x in &xs { … }`. Pattern-derefs the borrowed
        //   item; zero overhead.
        // - **Non-Copy element type, body never moves x**:
        //   `for x in &xs { … }`. The loop variable binds as
        //   `&T`; auto-deref covers method calls, `==`, format
        //   args, etc. Saves the `.iter().cloned()` heap clone
        //   per iteration.
        // - **Non-Copy element type, body moves x**: fall back to
        //   `for x in xs.iter().cloned() { … }` so `x` is owned
        //   and the move sites compile.
        //
        // "Moves x" = the loop variable appears as the immediate
        // value in a position that consumes ownership: a fn-call
        // arg, a `new T` arg, a var-decl init, an assignment rhs,
        // a return value, or a super-call arg. Reads through `.`
        // / `[]` / comparisons / format don't move it.
        let element_is_copy = match self.expr_types.get(&expr_span_of(&f.iter)) {
            Some(Ty::Array { element, .. }) => matches!(element.as_ref(), Ty::Primitive(_)),
            _ => false,
        };
        let body_moves_var =
            !element_is_copy && body_moves_path(&f.body, &f.var_name.text);

        // **Re-entrancy guard.** When the iterable is a collection field read
        // through a wrapper's `.0.borrow()` (`for (n : this.items) …`), iterating
        // it directly (`for n in &self.0.borrow().items`) holds that read-guard
        // across the whole body. Any in-body call that re-enters the same object
        // and takes `borrow_mut()` — even to a *different* field — then panics
        // `already borrowed`. Snapshot the collection into an owned local first
        // (cloning detaches the Rc element handles; element mutation still aliases
        // the live objects) so the field borrow drops before the body runs. This
        // also gives iterate-a-snapshot semantics if the body adds to the field.
        let snapshot = self.for_each_iter_reads_through_borrow(&f.iter);
        if snapshot {
            self.w.push_str("{\n");
            self.w.indent_inc();
            self.w.emit_indent();
            self.w.push_str("let __jux_fe_iter = ");
            self.emit_expr(&f.iter);
            self.w.push_str(".clone();\n");
            self.w.emit_indent();
        }

        self.w.push_str("for ");
        if element_is_copy {
            self.w.push('&');
        }
        self.w.push_str(&f.var_name.text);
        self.w.push_str(" in ");
        if element_is_copy {
            self.w.push('&');
        } else if !body_moves_var {
            // Borrow-iter: yields `&T`, so `x.method()` / `format!("{}", x)` /
            // `x == y` all work through auto-deref / `Display` / `PartialEq`.
            self.w.push('&');
        }
        if snapshot {
            self.w.push_str("__jux_fe_iter");
        } else {
            self.emit_expr(&f.iter);
        }
        if body_moves_var {
            self.w.push_str(".iter().cloned()");
        }
        self.w.push_str(" {\n");
        self.w.indent_inc();
        // Register the loop variable's element type in `local_types` for the
        // body, so a wrapper-class element (`for (var t : todos)` over a
        // `Vec<Todo>`) resolves `t.title` to the `t.0.borrow().title` deref
        // instead of a bare `t.title` (rustc "no field on &Todo"). Pushed as
        // its own scope so it doesn't leak past the loop.
        let elem_ty = self.for_each_element_ty(&f.iter);
        self.local_types.push(std::collections::HashMap::new());
        if let Some(ty @ Ty::User { .. }) = &elem_ty {
            if let Some(scope) = self.local_types.last_mut() {
                scope.insert(f.var_name.text.clone(), ty.clone());
            }
        }
        self.loop_emit_depth += 1;
        self.emit_block_contents(&f.body);
        self.loop_emit_depth -= 1;
        self.local_types.pop();
        self.w.indent_dec();
        self.w.emit_indent();
        self.w.push_str("}\n");
        if snapshot {
            self.w.indent_dec();
            self.w.emit_indent();
            self.w.push_str("}\n");
        }
    }

    /// True iff a for-each iterable is a collection field read through a wrapper
    /// class's `.0.borrow()` guard — `this.items` / `obj.items` where the owner
    /// uses the `Rc<RefCell>` representation. Such an iterable must be snapshotted
    /// into an owned local before the loop so the read-borrow drops before the
    /// body runs (see [`Self::emit_for_each`]).
    fn for_each_iter_reads_through_borrow(&self, iter: &Expr) -> bool {
        if let Expr::Field(fe) = iter {
            return self.receiver_is_wrapper_class(&fe.object)
                && self
                    .wrapper_field_parent_depth(&fe.object, &fe.field.text)
                    .is_some();
        }
        false
    }

    /// The element type of a for-each iterable: the element of an array, or the
    /// first generic argument of a `Vec<T>` / `HashSet<T>` / `List<T>` receiver.
    /// `None` when the iterable's type wasn't recorded or carries no element
    /// type. Drives the loop-variable [`Self::local_types`] registration above.
    fn for_each_element_ty(&self, iter: &Expr) -> Option<Ty> {
        // Prefer the iterable's recorded type.
        if let Some(elem) = self
            .expr_types
            .get(&expr_span_of(iter))
            .and_then(Self::element_of)
        {
            return Some(elem);
        }
        // A field-access iterable (`obj.field`) often has no `expr_types` entry;
        // resolve the field's declared type from the receiver's class instead.
        if let Expr::Field(f) = iter {
            if let Some(class) = self.receiver_class_ast(&f.object) {
                if let Some(field_ty) = class
                    .fields
                    .iter()
                    .find(|fd| fd.name.text == f.field.text)
                    .and_then(|fd| fd.ty.as_ref())
                {
                    let ty = juxc_tycheck::ty_from_ref_in_env(field_ty, &self.symbols);
                    return Self::element_of(&ty);
                }
            }
        }
        None
    }

    /// The element type of an iterable type: an array's element or the first
    /// generic argument of a `Vec<T>` / `HashSet<T>` / `List<T>`.
    fn element_of(ty: &Ty) -> Option<Ty> {
        match ty {
            Ty::Array { element, .. } => Some((**element).clone()),
            Ty::User { generic_args, .. } if !generic_args.is_empty() => {
                Some(generic_args[0].clone())
            }
            _ => None,
        }
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
        // `ref` local (§M.13): the slot is an `Rc<RefCell<T>>` shared
        // reference. Initializing from another `ref` binding shares
        // the handle; a plain value wraps into a fresh object. The
        // name is registered in `ref_locals` so reads clone out and
        // assignments store through.
        if var.is_ref {
            if let Some(ty_ref) = &var.ty {
                let ty = juxc_tycheck::ty_from_ref_in_env(ty_ref, &self.symbols);
                if let Some(scope) = self.local_types.last_mut() {
                    scope.insert(var.name.text.clone(), ty);
                }
            }
            self.w.push_str("let ");
            self.w.push_str(&var.name.text);
            self.w.push_str(" = ");
            let init_is_ref = matches!(
                var.init.as_ref(),
                Some(Expr::Path(qn))
                    if qn.segments.len() == 1
                        && self.ref_locals.contains(qn.segments[0].text.as_str())
            );
            if init_is_ref {
                if let Some(Expr::Path(qn)) = &var.init {
                    self.w.push_str(&qn.segments[0].text);
                    self.w.push_str(".clone()");
                }
            } else {
                self.w
                    .push_str("std::rc::Rc::new(std::cell::RefCell::new(");
                if let Some(init) = &var.init {
                    let prev = std::mem::take(&mut self.emitting_format_arg);
                    self.emit_expr(init);
                    self.emitting_format_arg = prev;
                }
                self.w.push_str("))");
            }
            self.w.push_str(";\n");
            self.ref_locals.insert(var.name.text.clone());
            return;
        }
        // Record the local's declared type in the backend's
        // `local_types` map so `@Intrinsic` dispatch can resolve
        // the receiver class when `expr_types` lookups are
        // unreliable (interp-string synthetic-source collisions).
        // Falls back to `Ty::Unknown` for the `var x = …` form
        // where no type was written.
        // Whether this local's type is an external (`rust.std` / crate) type —
        // used below to mark it `mut` conservatively (§G.9.2).
        let mut external_local = false;
        if let Some(ty_ref) = &var.ty {
            let ty = juxc_tycheck::ty_from_ref_in_env(
                ty_ref,
                &self.symbols,
            );
            external_local = self.is_external_user_ty(&ty);
            if let Some(scope) = self.local_types.last_mut() {
                scope.insert(var.name.text.clone(), ty);
            }
        } else if let Some(init) = &var.init {
            // `var x = init;` carries no written type — recover one from
            // the initializer's inferred type so name-keyed receiver
            // resolution (`local_types`) still works for inferred locals.
            // This is what makes the wrapper-class `.0.borrow()` rewrite
            // fire for `var i = new Inner(...); print($"${i.field}")`,
            // where the interpolated `i`'s span collides in `expr_types`
            // but its NAME reliably maps to the right class here. Only a
            // `Ty::User` is worth recording (it's the only kind the
            // wrapper / stdlib-dispatch receiver lookups consult).
            if let Some(ty @ juxc_tycheck::Ty::User { .. }) =
                self.expr_types.get(&expr_span_of(init)).cloned()
            {
                external_local = self.is_external_user_ty(&ty);
                if let Some(scope) = self.local_types.last_mut() {
                    scope.insert(var.name.text.clone(), ty);
                }
            }
        }
        self.w.push_str("let ");
        // §G.9.2: a local of an external (`rust.std` / crate) type is marked
        // `mut` conservatively. bindgen drops the `&mut self` receiver disposition
        // (§G.3.4), so the mutation analysis can't tell a `p.reserve(…)` (mutates)
        // from a `p.len()` (doesn't); marking external locals `mut` lets the
        // mutating calls compile. The crate prelude `#![allow(unused_mut)]`
        // absorbs the over-marking on read-only uses.
        if self.mutated_in_fn.contains(&var.name.text) || external_local {
            self.w.push_str("mut ");
        }
        self.w.push_str(&var.name.text);
        // Java-style typed local (`int x = 5;`) carries an explicit
        // type annotation; emit it as `let x: T = init;`. The `var`
        // form leaves `ty == None` and we let Rust infer.
        let declared_nullable = var.ty.as_ref().map_or(false, |t| t.nullable);
        // Inferred nullability for `var` (no explicit type):
        // when the init expression is itself `Option<T>`-shaped
        // (a nullable-returning call, a `?.`-chain, a known
        // nullable local, etc.), the resulting binding also has
        // nullable shape. Seed `nullable_locals` so downstream
        // sites can recognize reads of this binding.
        let init_is_nullable = var
            .init
            .as_ref()
            .map_or(false, |e| self.expression_is_already_nullable(e));
        if declared_nullable || init_is_nullable {
            self.nullable_locals.insert(var.name.text.clone());
        }
        if let Some(ty) = &var.ty {
            self.w.push_str(": ");
            // A local's declared type is a value slot — an interface-typed
            // local lowers to `Rc<dyn Trait>`.
            self.emit_value_type_as_rust(ty);
        }
        if let Some(init) = &var.init {
            self.w.push_str(" = ");
            // When the declared type is nullable (`T?` → `Option<T>`)
            // and the init isn't a `null` literal, wrap in `Some(...)`
            // so the assignment type-checks. A `null` init already
            // lowers to `None` via `emit_literal`, so no wrap there.
            // **Interface value slot.** When the local's declared type is an
            // interface (`Shape a = new Circle(...)`), the init must be
            // adapted into the `Rc<dyn Trait>` representation — a class value
            // is wrapped (`Rc::new(..) as Rc<dyn Trait>`), an interface value
            // is `Rc`-cloned. The coercion helper folds in the share-clone,
            // so we bypass the plain wrapper-clone path below for these.
            let iface_target = var.ty.as_ref().filter(|t| {
                !matches!(
                    self.iface_coercion_to(t, init),
                    crate::analysis::IfaceCoercion::None,
                )
            });
            // A nullable dyn local (`Animal? a = new Dog()`) is `Some`-wrapped
            // INSIDE the coercion helper — only add the bare-nullable `Some(...)`
            // when we're NOT routing through that helper. An init that is
            // *already* `Option`-shaped (`Animal? r = maybeAnimal()`) flows
            // through unwrapped — wrapping it would yield `Some(Some(...))`.
            let wrap_some = declared_nullable
                && !is_null_literal(init)
                && !self.expression_is_already_nullable(init)
                && iface_target.is_none();
            if wrap_some {
                self.w.push_str("Some(");
            }
            if let Some(decl_ty) = iface_target {
                self.emit_expr_coerced_to_iface(&decl_ty.clone(), init);
            } else {
                // §5.6: a `new T[N]` flowing into a DYNAMIC array slot
                // (`int[] a = …`) lowers to a heap `vec![…]`. The flag
                // is read by `emit_new_array`; a `var` / fixed-array
                // (`int[N]`) slot leaves it clear (stack array).
                let prev_dyn = self.dynamic_array_target;
                self.dynamic_array_target = var
                    .ty
                    .as_ref()
                    .map_or(false, |t| {
                        matches!(t.array_shape, Some(juxc_ast::ArrayShape::Dynamic))
                    });
                self.emit_expr(init);
                self.dynamic_array_target = prev_dyn;
                // **Wrapper-class share-on-assignment (§CR.4.1).** When the
                // init re-reads an existing wrapper-class binding
                // (`var y = x;`, `var y = obj.child;`, `var y = this;`),
                // the two bindings must SHARE the same instance — Java
                // reference semantics. A bare move would invalidate the
                // source. Append `.clone()` (a cheap `Rc` refcount bump)
                // so both handles stay live and point at the same
                // `RefCell`. Fresh values (`new C(...)`, a call result)
                // are already owned handles and don't need the clone.
                //
                // A `Field` read of a wrapper-class field already gets its
                // `.clone()` from `emit_field`'s class-field auto-clone, so
                // the shared helper covers only the bare-`Path` / `this` and
                // index-read (`var r = xs[0]`) places the field path doesn't.
                if !wrap_some && self.wrapper_value_needs_clone(init) {
                    self.w.push_str(".clone()");
                }
            }
            if wrap_some {
                self.w.push(')');
            }
        } else if let Some(ty) = &var.ty {
            // No initializer (`int n;`): seed a default. This makes the local
            // usable as an `out <place>` argument right away (Rust `&mut n`
            // needs an initialized binding) and removes a latent rustc
            // use-before-assign error for an otherwise-unassigned typed local.
            self.w.push_str(" = ");
            self.emit_default_value_for(ty);
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
    /// Lower a C-style `for (init; cond; update) body`. We can't map it to a
    /// Rust `while` directly because `continue` must still run the UPDATE — in
    /// Rust a `continue` skips to the condition. So we hoist the update to the
    /// TOP of the loop, guarded by a first-iteration flag:
    ///
    /// ```text
    /// { <init>
    ///   let mut __first = true;
    ///   loop {
    ///     if !__first { <update> }
    ///     __first = false;
    ///     if !(<cond>) { break; }
    ///     <body>
    ///   } }
    /// ```
    ///
    /// `continue` jumps to the loop top → runs the update → re-checks the
    /// condition → body (exactly C semantics); `break` exits the loop. The
    /// outer `{ }` scopes the init's loop variable.
    pub(crate) fn emit_for_c(&mut self, f: &juxc_ast::ForCStmt) {
        // The label (if any) belongs on the INNER `loop`, not the
        // init-scope block — a Rust block label can't `continue`.
        let label = self.pending_loop_label.take();
        self.w.push_str("{\n");
        self.w.indent_inc();
        // Init clause.
        if let Some(init) = f.init.as_deref() {
            self.w.emit_indent();
            self.emit_stmt(init);
        }
        self.w.line("let mut __jux_for_first = true;");
        if let Some(l) = &label {
            self.w.emit_indent();
            self.w.push('\'');
            self.w.push_str(l);
            self.w.push_str(": loop {\n");
        } else {
            self.w.line("loop {");
        }
        self.w.indent_inc();
        // Update (skipped on the first iteration).
        if let Some(upd) = f.update.as_deref() {
            self.w.line("if !__jux_for_first {");
            self.w.indent_inc();
            self.w.emit_indent();
            self.emit_stmt(upd);
            self.w.indent_dec();
            self.w.line("}");
        }
        self.w.line("__jux_for_first = false;");
        // Condition check (empty cond → always true → no break).
        if let Some(cond) = &f.cond {
            self.w.emit_indent();
            self.w.push_str("if !(");
            self.emit_expr(cond);
            self.w.push_str(") {\n");
            self.w.indent_inc();
            self.w.line("break;");
            self.w.indent_dec();
            self.w.line("}");
        }
        // Body.
        self.loop_emit_depth += 1;
        self.emit_block_contents(&f.body);
        self.loop_emit_depth -= 1;
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.emit_indent();
        self.w.push_str("}\n");
    }

    /// Emit a parked loop label (`'name: `) if the enclosing statement
    /// was `Stmt::Labeled` — see `pending_loop_label`.
    pub(crate) fn emit_pending_loop_label(&mut self) {
        if let Some(l) = self.pending_loop_label.take() {
            self.w.push('\'');
            self.w.push_str(&l);
            self.w.push_str(": ");
        }
    }

    pub(crate) fn emit_while(&mut self, w: &WhileStmt) {
        self.emit_pending_loop_label();
        if matches!(w.condition, Expr::Literal(Literal::Bool(true))) {
            self.w.push_str("loop {\n");
        } else {
            self.w.push_str("while ");
            self.emit_expr(&w.condition);
            self.w.push_str(" {\n");
        }
        self.w.indent_inc();
        self.loop_emit_depth += 1;
        self.emit_block_contents(&w.body);
        self.loop_emit_depth -= 1;
        self.w.indent_dec();
        self.w.emit_indent();
        self.w.push_str("}\n");
    }

    /// `do block while (cond);` → Rust has no do-while, so the body
    /// runs inside a `loop` with the exit test at the BOTTOM —
    /// preserving Java's run-at-least-once + check-after semantics:
    ///
    ///   loop { <body> if !(cond) { break; } }
    ///
    /// A literal-`true` condition drops the dead test entirely.
    pub(crate) fn emit_do_while(&mut self, d: &juxc_ast::DoWhileStmt) {
        self.emit_pending_loop_label();
        self.w.push_str("loop {\n");
        self.w.indent_inc();
        self.loop_emit_depth += 1;
        self.emit_block_contents(&d.body);
        self.loop_emit_depth -= 1;
        if !matches!(d.condition, Expr::Literal(Literal::Bool(true))) {
            self.w.emit_indent();
            self.w.push_str("if !(");
            self.emit_expr(&d.condition);
            self.w.push_str(") {\n");
            self.w.indent_inc();
            self.w.line("break;");
            self.w.indent_dec();
            self.w.line("}");
        }
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
    /// Emit an assignment's RHS value, appending `.clone()` when the
    /// value is a bare reference to an owned constructor parameter
    /// that a LATER statement still reads (`this.name = name;
    /// print(name);` — the move would poison the later read). The
    /// liveness set is maintained by `emit_ctor_body_stmts` and is
    /// empty outside constructor bodies, so this is a no-op
    /// everywhere else.
    fn emit_assign_rhs(&mut self, value: &Expr) {
        self.emit_expr(value);
        if let Expr::Path(qn) = value {
            if qn.segments.len() == 1
                && self.ctor_live_after.contains(&qn.segments[0].text)
                && !self.wrapper_value_needs_clone(value)
            {
                self.w.push_str(".clone()");
            }
        }
    }

    pub(crate) fn emit_assign(&mut self, a: &AssignStmt) {
        // Integer `/=` and `%=` desugar to `target = target / value` so
        // the compound forms get the same checked lowering as the binary
        // ops (`__jux_idiv`/`__jux_irem` — zero divisor throws a
        // catchable ArithmeticException, ERRATA.md E1). Every plain-`=`
        // target shape (local, field, static, wrapper) already exists,
        // so recursing with the rewritten statement reuses them all.
        // `operator[]=` targets are excluded: their compound branch
        // below must hoist the read to avoid E0499, so it special-cases
        // Div/Rem itself.
        if matches!(
            a.op,
            Some(juxc_ast::BinaryOp::Div) | Some(juxc_ast::BinaryOp::Rem),
        ) && self.operand_is_float(&a.target) == Some(false)
            && !matches!(&a.target, Expr::Index(ix)
                if self.expr_declares_operator(&ix.array, juxc_ast::OperatorKind::IndexSet))
        {
            let desugared = AssignStmt {
                target: a.target.clone(),
                op: None,
                value: Expr::Binary(juxc_ast::BinaryExpr {
                    op: a.op.unwrap(),
                    left: Box::new(a.target.clone()),
                    right: Box::new(a.value.clone()),
                    span: a.span,
                }),
                span: a.span,
            };
            self.emit_assign(&desugared);
            return;
        }
        // PROPERTY compound assigns (S4): `obj.Count += 1` desugars to
        // `obj.Count = obj.Count + 1` so the read routes through the
        // getter and the write through the setter — firing observers
        // and bindings like any other set. Without this the compound
        // form fell through to the raw wrapper-field write, which both
        // bypassed the setter AND named a nonexistent field (E0609).
        if a.op.is_some() && self.assign_target_is_property_with_setter(&a.target) {
            let desugared = AssignStmt {
                target: a.target.clone(),
                op: None,
                value: Expr::Binary(juxc_ast::BinaryExpr {
                    op: a.op.unwrap(),
                    left: Box::new(a.target.clone()),
                    right: Box::new(a.value.clone()),
                    span: a.span,
                }),
                span: a.span,
            };
            self.emit_assign(&desugared);
            return;
        }
        // AsyncMutex guard write (§18.3): `guard.value = v` (and the
        // compound forms) assign through the deref.
        if let Expr::Field(tf) = &a.target {
            if tf.field.text == "value" {
                let recv_ty = self
                    .expr_types
                    .get(&expr_span_of(&tf.object))
                    .cloned()
                    .or_else(|| {
                        if let Expr::Path(qn) = &*tf.object {
                            if qn.segments.len() == 1 {
                                return self
                                    .local_types
                                    .iter()
                                    .rev()
                                    .find_map(|s| s.get(&qn.segments[0].text).cloned());
                            }
                        }
                        None
                    });
                if let Some(Ty::User { name, .. }) = recv_ty {
                    if name == "__AsyncMutexGuard" {
                        self.w.push_str("*");
                        self.emit_expr(&tf.object);
                        self.w.push(' ');
                        if let Some(op) = a.op {
                            self.w.push_str(op.as_rust_str());
                        }
                        self.w.push_str("= ");
                        let prev = self.emitting_format_arg;
                        self.emitting_format_arg = false;
                        self.emit_expr(&a.value);
                        self.emitting_format_arg = prev;
                        self.w.push_str(";\n");
                        return;
                    }
                }
            }
        }
        // `operator[]=` dispatch (§O.2.4): `obj[i] = v` on a user type
        // declaring the overload becomes `obj.__op_index_set(i, v)`.
        //
        // Compound forms (`obj[i] += v`) used to inline the read into the
        // set call: `obj.__op_index_set(i, obj.__op_index(i) + v)`. That
        // triggers E0499 — Rust's borrow checker sees `&mut obj` (for the
        // outer `__op_index_set`) and `&obj` (for the inner `__op_index`)
        // alive at the same time.
        //
        // Fix: hoist the new value into a `let __jux_tmp` BEFORE calling
        // `__op_index_set`. The index `i` is cloned for the read step so it
        // remains available for the write step even when it's not `Copy`.
        // Plain `=` doesn't need the hoist (no read step, no aliasing).
        if let Expr::Index(ix) = &a.target {
            if self.expr_declares_operator(&ix.array, juxc_ast::OperatorKind::IndexSet) {
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                if let Some(op) = a.op {
                    // Compute the new value into a temporary so the mutable
                    // borrow for `__op_index_set` doesn't alias the shared
                    // borrow taken by `__op_index`. Integer `/=`/`%=` route
                    // the read-op-value through the checked division
                    // helpers (ERRATA.md E1) like every other assign shape.
                    let checked_div = matches!(
                        op,
                        juxc_ast::BinaryOp::Div | juxc_ast::BinaryOp::Rem,
                    ) && self.operand_is_float(&a.target) == Some(false);
                    self.w.push_str("let __jux_tmp = ");
                    if checked_div {
                        self.w.push_str(if matches!(op, juxc_ast::BinaryOp::Div) {
                            "crate::__jux_idiv("
                        } else {
                            "crate::__jux_irem("
                        });
                    }
                    self.emit_expr_with_parent_prec(&ix.array, u8::MAX, false);
                    self.w.push_str(".__op_index(");
                    self.emit_expr(&ix.index);
                    // Clone the index for the read so it's still available
                    // (and unmodified) for the write step below.
                    self.w.push_str(".clone()");
                    self.w.push(')');
                    if checked_div {
                        self.w.push_str(", ");
                    } else {
                        self.w.push(' ');
                        self.w.push_str(op.as_rust_str());
                        self.w.push(' ');
                    }
                    self.emit_expr(&a.value);
                    if self.wrapper_value_needs_clone(&a.value) {
                        self.w.push_str(".clone()");
                    }
                    if checked_div {
                        self.w.push(')');
                    }
                    self.w.push_str(";\n");
                    self.w.emit_indent();
                    self.emit_expr_with_parent_prec(&ix.array, u8::MAX, false);
                    self.w.push_str(".__op_index_set(");
                    self.emit_expr(&ix.index);
                    self.w.push_str(", __jux_tmp");
                } else if matches!(&a.value, Expr::Literal(_) | Expr::Path(_)) {
                    // Plain `=` with a trivial value — no read step,
                    // nothing in the value can touch the receiver.
                    self.emit_expr_with_parent_prec(&ix.array, u8::MAX, false);
                    self.w.push_str(".__op_index_set(");
                    self.emit_expr(&ix.index);
                    self.w.push_str(", ");
                    self.emit_expr(&a.value);
                    if self.wrapper_value_needs_clone(&a.value) {
                        self.w.push_str(".clone()");
                    }
                } else {
                    // Plain `=` with a compound value (S12): hoist it
                    // first. The value may read the SAME receiver
                    // (`g[1] = g[0] + g.total()`), and two-phase
                    // borrows don't cover a `&mut`-needing call inside
                    // the `__op_index_set` args — inline emission
                    // trips E0499.
                    self.w.push_str("let __jux_tmp = ");
                    self.emit_expr(&a.value);
                    if self.wrapper_value_needs_clone(&a.value) {
                        self.w.push_str(".clone()");
                    }
                    self.w.push_str(";\n");
                    self.w.emit_indent();
                    self.emit_expr_with_parent_prec(&ix.array, u8::MAX, false);
                    self.w.push_str(".__op_index_set(");
                    self.emit_expr(&ix.index);
                    self.w.push_str(", __jux_tmp");
                }
                self.emitting_format_arg = prev;
                self.w.push_str(");\n");
                return;
            }
        }
        // **Static-block first-use trigger (§S.4.1).** Writing a static field is
        // an observable use, so run the class's once-guarded `__static_init()`
        // before the write. (`__static_init` is re-entrancy-safe, so a write
        // from inside the static block itself is a harmless no-op.) Emitted as
        // a leading statement; the assignment follows after re-indenting.
        if let Expr::Field(tf) = &a.target {
            if let Expr::Path(qn) = &*tf.object {
                if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                    let needs = self
                        .symbols
                        .classes
                        .get(&class_fqn)
                        .map(|c| {
                            c.has_static_init
                                && c.fields
                                    .get(tf.field.text.as_str())
                                    .map(|f| f.is_static)
                                    .unwrap_or(false)
                        })
                        .unwrap_or(false);
                    if needs {
                        self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                        self.w.push_str("::__static_init();\n");
                        self.w.emit_indent();
                    }
                }
            }
        }
        // **Direct write to a `!Send` (thread_local) static slot** —
        // `Registry.global = new Counter()`. The thread_local form has
        // no place expression to assign into, so the write routes
        // through the slot's own `RefCell`:
        //   Class_field.with(|__s| { *__s.borrow_mut() = <rhs>; });
        // Compound ops keep their operator on the deref'd place.
        // (Chained writes `Registry.global.n = 5` don't come here —
        // the target root is read as an rvalue handle and the write
        // goes through the OBJECT's wrapper RefCell.)
        if let Expr::Field(tf) = &a.target {
            if let Expr::Path(qn) = &*tf.object {
                if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                    let tl_field = self
                        .symbols
                        .classes
                        .get(&class_fqn)
                        .and_then(|c| c.fields.get(tf.field.text.as_str()))
                        .filter(|fs| fs.is_static && !fs.is_final)
                        .map(|fs| fs.ty.clone())
                        .filter(|ty| self.static_type_needs_thread_local(ty));
                    if let Some(slot_ty) = tl_field {
                        self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                        self.w.push('_');
                        self.w.push_str(&tf.field.text);
                        self.w.push_str(".with(|__s| { *__s.borrow_mut() ");
                        if let Some(op) = a.op {
                            self.w.push_str(op.as_rust_str());
                        }
                        self.w.push_str("= ");
                        // S13: a nullable slot (`static Reg? global`) is an
                        // `Option<T>` cell — a non-null value needs the
                        // `Some(…)` lift, exactly like nullable field
                        // assigns elsewhere.
                        let wrap = slot_ty.nullable
                            && a.op.is_none()
                            && !is_null_literal(&a.value)
                            && !self.expression_is_already_nullable(&a.value);
                        if wrap {
                            self.w.push_str("Some(");
                        }
                        self.emit_assign_rhs(&a.value);
                        if self.wrapper_value_needs_clone(&a.value) {
                            self.w.push_str(".clone()");
                        }
                        if wrap {
                            self.w.push(')');
                        }
                        self.w.push_str("; });\n");
                        return;
                    }
                }
            }
        }
        // Same interception for the BARE-NAME form inside the class's
        // own body (`g = new Counter();` inside `class Reg` ≡
        // `Reg.g = …`): a thread_local static slot has no place
        // expression, so the generic lvalue path would emit the
        // `.with(…clone())` READ shape as a target (rustc E0070).
        if let Expr::Path(qn) = &a.target {
            if qn.segments.len() == 1 {
                if let Some(class_name) = self.enclosing_class.clone() {
                    let name = &qn.segments[0].text;
                    let shadowed = self.current_fn_params.contains(name)
                        || self.local_types.iter().any(|s| s.contains_key(name));
                    let tl = if shadowed {
                        None
                    } else {
                        self.lookup_class_by_bare_or_fqn(&class_name)
                            .and_then(|c| c.fields.get(name.as_str()))
                            .filter(|fs| fs.is_static && !fs.is_final)
                            .map(|fs| fs.ty.clone())
                            .filter(|ty| self.static_type_needs_thread_local(ty))
                    };
                    if let Some(slot_ty) = tl {
                        self.w.push_str(&class_name);
                        self.w.push('_');
                        self.w.push_str(name);
                        self.w.push_str(".with(|__s| { *__s.borrow_mut() ");
                        if let Some(op) = a.op {
                            self.w.push_str(op.as_rust_str());
                        }
                        self.w.push_str("= ");
                        // S13: nullable slot — see the qualified-name
                        // branch above.
                        let wrap = slot_ty.nullable
                            && a.op.is_none()
                            && !is_null_literal(&a.value)
                            && !self.expression_is_already_nullable(&a.value);
                        if wrap {
                            self.w.push_str("Some(");
                        }
                        self.emit_assign_rhs(&a.value);
                        if self.wrapper_value_needs_clone(&a.value) {
                            self.w.push_str(".clone()");
                        }
                        if wrap {
                            self.w.push(')');
                        }
                        self.w.push_str("; });\n");
                        return;
                    }
                }
            }
        }
        // **Property-setter routing (JUX-MISSING-DEFS §M.7).** When the
        // target is `obj.Prop` and `Prop` names a property with a
        // settable accessor (`set` / `init`), lower the write to a call
        // on the synthesized setter: `obj.__set_Prop(value)`. This runs
        // BEFORE the wrapper-field-write branch so custom setters (with
        // validation) actually fire instead of the write hitting the
        // backing field directly. Constructor bodies don't reach here
        // for auto-properties — the desugarer rewrote `this.AutoProp`
        // to the backing field `this.__prop_AutoProp` before emission.
        // Plain `=` only (compound `+=` on a property isn't in §M.7).
        if a.op.is_none() {
            if let Expr::Field(tf) = &a.target {
                if !tf.safe {
                    if let Some(prop) =
                        self.property_on_receiver(&tf.object, &tf.field.text).cloned()
                    {
                        if prop.setter.is_some() {
                            self.emit_property_setter_call(&tf.object, &tf.field.text, &a.value);
                            return;
                        }
                        // Read-only property write outside the ctor —
                        // tycheck already fired the diagnostic; fall
                        // through to a backing-field write so the
                        // emitted Rust still type-checks (best-effort).
                        if prop.has_backing_field {
                            self.emit_property_backing_write(&tf.object, &tf.field.text, a);
                            return;
                        }
                    }
                }
            }
            // **Bare-name property write** (`Loading = true;` inside a
            // method — implicit `this`, §M.7/§P.9). Routes to the
            // synthesized setter exactly like `this.Loading = …`.
            // Guarded against shadowing: a parameter or local with the
            // same name wins (it's an ordinary variable assignment).
            if let Expr::Path(qn) = &a.target {
                if qn.segments.len() == 1 {
                    let name = qn.segments[0].text.clone();
                    let shadowed = self.current_fn_params.contains(&name)
                        || self.local_types.iter().any(|s| s.contains_key(&name))
                        || self.nullable_locals.contains(&name);
                    if !shadowed {
                        let prop = self
                            .enclosing_class
                            .clone()
                            .and_then(|c| self.lookup_class_ast_by_bare_or_fqn(&c))
                            .and_then(|cd| {
                                cd.properties.iter().find(|p| p.name.text == name).cloned()
                            });
                        if let Some(prop) = prop {
                            if prop.setter.is_some() {
                                if prop.is_static {
                                    let setter =
                                        juxc_ast::desugar_static_setter_name(&name);
                                    self.w.push_str("Self::");
                                    self.w.push_str(&setter);
                                    self.w.push('(');
                                    self.emit_property_setter_arg(&a.value);
                                    self.w.push_str(");\n");
                                } else {
                                    let this_expr =
                                        Expr::This(juxc_source::Span::DUMMY);
                                    self.emit_property_setter_call(
                                        &this_expr, &name, &a.value,
                                    );
                                }
                                return;
                            }
                        }
                    }
                }
            }
        }
        // `ref` FIELD store-through (§M.13): `obj.x = v` where `x` is
        // a `ref` field writes INTO the shared cell. The owner is read
        // with a SHARED borrow (the handle itself never changes); the
        // RHS evaluates into a temp first, like every store-through.
        if let Expr::Field(tf) = &a.target {
            if self.field_decl_is_ref(&tf.object, &tf.field.text) {
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                self.w.push_str("{ let __jux_v = ");
                self.emit_assign_rhs(&a.value);
                self.w.push_str("; *");
                let depth = if self.receiver_is_wrapper_class(&tf.object) {
                    self.wrapper_field_parent_depth(&tf.object, &tf.field.text)
                } else {
                    None
                };
                self.emitting_method_receiver = true;
                self.emit_expr(&tf.object);
                self.emitting_method_receiver = false;
                if let Some(d) = depth {
                    self.w.push_str(".0.borrow()");
                    for _ in 0..d {
                        self.w.push_str(".__parent");
                    }
                }
                self.w.push('.');
                self.w.push_str(&tf.field.text);
                self.w.push_str(".borrow_mut() ");
                if let Some(op) = a.op {
                    self.w.push_str(op.as_rust_str());
                }
                self.w.push_str("= __jux_v; }\n");
                self.emitting_format_arg = prev;
                return;
            }
        }
        // `ref` binding store-through (§M.13): `x = v` on a `ref`
        // local/param writes INTO the shared cell, so every alias
        // observes it. The RHS evaluates into a temp first — it may
        // read the same cell (`x = x + 1`), and an inline RHS read's
        // borrow guard would overlap the store's `borrow_mut`.
        if let Expr::Path(qn) = &a.target {
            if qn.segments.len() == 1 && self.ref_locals.contains(&qn.segments[0].text) {
                let name = qn.segments[0].text.clone();
                let prev = self.emitting_format_arg;
                self.emitting_format_arg = false;
                self.w.push_str("{ let __jux_v = ");
                self.emit_assign_rhs(&a.value);
                self.w.push_str("; *");
                self.w.push_str(&name);
                self.w.push_str(".borrow_mut() ");
                if let Some(op) = a.op {
                    self.w.push_str(op.as_rust_str());
                }
                self.w.push_str("= __jux_v; }\n");
                self.emitting_format_arg = prev;
                return;
            }
        }
        // String `+=` special-case: Rust's `String + String` and
        // `String += String` aren't implemented (only the
        // `&str`-RHS variants exist), and emitting the regular
        // `s += rhs` path would force the literal-coerce on the
        // RHS to produce a `String` that Rust then rejects. The
        // idiomatic form is `s.push_str(&rhs)` — works for both
        // `String` and `&str` RHS via `AsRef<str>` semantics.
        if matches!(a.op, Some(juxc_ast::BinaryOp::Add))
            && matches!(
                self.expr_types.get(&expr_span_of(&a.target)),
                Some(Ty::String),
            )
        {
            self.emitting_lvalue = true;
            self.emit_expr(&a.target);
            self.emitting_lvalue = false;
            self.w.push_str(".push_str(&");
            // Borrow context so a literal RHS stays `&str` (no
            // wasted `.to_string()`).
            let prev = self.emitting_format_arg;
            self.emitting_format_arg = true;
            self.emit_assign_rhs(&a.value);
            self.emitting_format_arg = prev;
            self.w.push_str(");\n");
            return;
        }
        // Mutable-static target: evaluate the RHS first into a local
        // (releasing any locks the RHS itself takes), then acquire
        // the LHS lock once for the write. Without this scoping the
        // statement `Class.x = …` deadlocks whenever the RHS reads
        // the same mutable static (`x = x + 1`, `Class.x = Class.x`,
        // …) because the LHS's `MutexGuard` is a statement-scoped
        // temporary that's still live while the RHS runs. Compound
        // forms (`x += rhs`) get the same wrap so `x += x` doesn't
        // hit the same trap. See §CR.5.7 in
        // `JUX-CLASS-REPRESENTATION-ADDENDUM.md` for the lowering
        // contract.
        let target_is_mutable_static = self.target_is_mutable_static(&a.target);
        if target_is_mutable_static {
            self.w.push_str("{ let __jux_v = ");
            self.emit_assign_rhs(&a.value);
            self.w.push_str("; ");
            self.emitting_lvalue = true;
            self.emit_expr(&a.target);
            self.emitting_lvalue = false;
            if let Some(op) = a.op {
                self.w.push(' ');
                self.w.push_str(op.as_rust_str());
                self.w.push_str("= __jux_v");
            } else {
                self.w.push_str(" = __jux_v");
            }
            self.w.push_str("; }\n");
            return;
        }
        // Wrapper-class field write (§CR.4.1): `obj.f = v` where `obj`
        // is a wrapper-shape class. Field state lives inside
        // `Rc<RefCell<C_Inner>>`, so we evaluate the RHS into a
        // statement-scoped temp first (releasing any `borrow()` the
        // RHS itself takes — `obj.f = obj.g` must not deadlock the
        // RefCell), then take a one-statement `borrow_mut()` for the
        // write:
        //
        //   { let __jux_v = <rhs>; obj.0.borrow_mut().f = __jux_v; }
        //
        // Compound forms (`obj.f += rhs`) use the same wrap so
        // `obj.f += obj.f` releases the read borrow before the
        // write borrow is taken. This mirrors the mutable-static
        // scoped-temp shape above (§CR.5.7).
        // **Field WRITE through a polymorphic-base reference** → `__set_f(v)`.
        // A base-typed value is a `Rc<dyn …Kind>` that can't expose struct
        // fields; the generated setter writes through interior mutability. A
        // compound op (`+=`) becomes a read-modify-write through the accessors:
        // `r.__set_f(r.__get_f() OP v)`.
        if let Expr::Field(tf) = &a.target {
            if !tf.safe && !matches!(&*tf.object, Expr::This(_)) {
                if let Some(bare) = self.receiver_class_bare(&tf.object) {
                    if self.poly_base_classes.contains(&bare) {
                        let field_info = self
                            .symbols
                            .lookup_field(&bare, &tf.field.text)
                            .map(|(fsig, _)| {
                                (
                                    matches!(
                                        fsig.visibility,
                                        juxc_ast::Visibility::Public
                                            | juxc_ast::Visibility::Protected
                                    ),
                                    fsig.ty.clone(),
                                )
                            });
                        if let Some((true, fty)) = field_info {
                            let field = tf.field.text.clone();
                            self.emit_expr(&tf.object);
                            self.w.push_str(".__set_");
                            self.w.push_str(&field);
                            self.w.push('(');
                            if let Some(op) = a.op {
                                // Read-modify-write: `r.__get_f() OP value`.
                                self.emit_expr(&tf.object);
                                self.w.push_str(".__get_");
                                self.w.push_str(&field);
                                self.w.push_str("() ");
                                self.w.push_str(op.as_rust_str());
                                self.w.push(' ');
                                self.emit_assign_rhs(&a.value);
                            } else if !matches!(
                                self.iface_coercion_to(&fty, &a.value),
                                crate::analysis::IfaceCoercion::None,
                            ) {
                                self.emit_expr_coerced_to_iface(&fty, &a.value);
                            } else {
                                self.emit_assign_rhs(&a.value);
                                if self.wrapper_value_needs_clone(&a.value) {
                                    self.w.push_str(".clone()");
                                }
                            }
                            self.w.push_str(");\n");
                            return;
                        }
                    }
                }
            }
        }
        if let Expr::Field(tf) = &a.target {
            if !tf.safe && self.receiver_is_wrapper_class(&tf.object) {
                // `weak` field store (§6.5): the slot holds a non-owning
                // `Weak`, so a class RHS is downgraded and `null` clears it to
                // an empty `Weak`. Only plain `=` is meaningful (a compound op
                // on a weak ref has no meaning — tycheck never produces one).
                // The RHS is computed into a temp first so any borrow it takes
                // is released before the `borrow_mut()` for the write.
                if a.op.is_none() {
                    if self
                        .wrapper_weak_field_target(&tf.object, &tf.field.text)
                        .is_some()
                    {
                        let depth = self
                            .wrapper_field_parent_depth(&tf.object, &tf.field.text)
                            .unwrap_or(0);
                        self.w.push_str("{ let __jux_v = ");
                        if matches!(&a.value, Expr::Literal(juxc_ast::Literal::Null)) {
                            self.w.push_str("std::rc::Weak::new()");
                        } else {
                            self.w.push_str("std::rc::Rc::downgrade(&(");
                            self.emit_expr(&a.value);
                            self.w.push_str(").0)");
                        }
                        self.w.push_str("; ");
                        self.emit_expr(&tf.object);
                        self.w.push_str(".0.borrow_mut()");
                        for _ in 0..depth {
                            self.w.push_str(".__parent");
                        }
                        self.w.push('.');
                        self.w.push_str(&tf.field.text);
                        self.w.push_str(" = __jux_v; }\n");
                        return;
                    }
                }
                // Walk the `__parent` chain to the slot that actually
                // declares the field — inherited-field writes
                // (`child.parentField = v`) land deeper in the inner
                // (`child.0.borrow_mut().__parent.field = v`). A `None`
                // depth (no such instance field) shouldn't reach here
                // for a wrapper receiver, but fall back to depth 0 so
                // the emitted Rust still type-checks against the
                // receiver's own class.
                let depth = self
                    .wrapper_field_parent_depth(&tf.object, &tf.field.text)
                    .unwrap_or(0);
                self.w.push_str("{ let __jux_v = ");
                let assign_nullable =
                    a.op.is_none() && self.assign_target_is_nullable(&a.target);
                self.emit_arg_with_nullable_wrap(&a.value, assign_nullable);
                // Wrapper-class share-on-store: a wrapped place stored
                // into a field hands the field a SHARED handle (§CR.4.1).
                if !assign_nullable && self.wrapper_value_needs_clone(&a.value) {
                    self.w.push_str(".clone()");
                }
                self.w.push_str("; ");
                // LHS place expression with a MUTABLE borrow.
                self.emit_expr(&tf.object);
                self.w.push_str(".0.borrow_mut()");
                for _ in 0..depth {
                    self.w.push_str(".__parent");
                }
                self.w.push('.');
                self.w.push_str(&tf.field.text);
                if let Some(op) = a.op {
                    self.w.push(' ');
                    self.w.push_str(op.as_rust_str());
                    self.w.push_str("= __jux_v");
                } else {
                    self.w.push_str(" = __jux_v");
                }
                self.w.push_str("; }\n");
                return;
            }
        }
        // Wrapper-field INDEXED write: `this.slots[i] = v` where the
        // indexed array/collection is a FIELD of a wrapper-shape class.
        // The store needs `borrow_mut()` on the owner, and the value
        // and index are hoisted into statement temps first — either may
        // read the same object, and an inline read's borrow guard would
        // overlap the store's `borrow_mut()` (RefCell panic).
        if let Expr::Index(ix) = &a.target {
            if let Expr::Field(af) = &*ix.array {
                let depth = if self.receiver_is_wrapper_class(&af.object) {
                    self.wrapper_field_parent_depth(&af.object, &af.field.text)
                } else {
                    None
                };
                if let Some(depth) = depth {
                    let prev = self.emitting_format_arg;
                    self.emitting_format_arg = false;
                    self.w.push_str("{ let __jux_v = ");
                    self.emit_assign_rhs(&a.value);
                    self.w.push_str("; let __jux_i = (");
                    self.emit_expr(&ix.index);
                    self.w.push_str(") as usize; ");
                    self.emitting_method_receiver = true;
                    self.emit_expr(&af.object);
                    self.emitting_method_receiver = false;
                    self.w.push_str(".0.borrow_mut()");
                    for _ in 0..depth {
                        self.w.push_str(".__parent");
                    }
                    self.w.push('.');
                    self.w.push_str(&af.field.text);
                    self.w.push_str("[__jux_i]");
                    if let Some(op) = a.op {
                        self.w.push(' ');
                        self.w.push_str(op.as_rust_str());
                        self.w.push_str("= __jux_v");
                    } else {
                        self.w.push_str(" = __jux_v");
                    }
                    self.w.push_str("; }\n");
                    self.emitting_format_arg = prev;
                    return;
                }
            }
        }
        // LHS: emit with the lvalue flag set so `emit_field` skips its
        // String-read `.clone()` insertion.
        self.emitting_lvalue = true;
        self.emit_expr(&a.target);
        self.emitting_lvalue = false;
        // Compound assignment lowers to Rust's matching `op=`:
        // `x += y`, `arr[i] *= n`, etc. Rust evaluates the place
        // expression exactly once even for side-effecting shapes
        // like `arr[next()] += 1`, so we don't have to introduce
        // any temp. The op spelling is the regular Rust binary
        // operator with `=` appended.
        let is_compound = a.op.is_some();
        if let Some(op) = a.op {
            self.w.push(' ');
            self.w.push_str(op.as_rust_str());
            self.w.push_str("= ");
        } else {
            self.w.push_str(" = ");
        }
        // Nullable-field assign coercion: when the LHS is a field
        // whose declared type is `T?` and the RHS isn't already
        // nullable-shaped, wrap RHS in `Some(...)`. Skipped for
        // compound forms (`obj.x += y`) because `Option<T> +=` has
        // no sensible meaning and rustc will surface the misuse.
        let assign_nullable = !is_compound && self.assign_target_is_nullable(&a.target);
        // Interface-typed LHS (`s = new Square(...)` where `s: Shape`):
        // coerce the RHS into the `Rc<dyn Trait>` representation instead of
        // the plain value + wrapper-clone path. The target's interface name
        // comes from its inferred type; we synthesize a bare `TypeRef` to
        // feed the shared coercion helper.
        let mut iface_tref: Option<juxc_ast::TypeRef> = None;
        if !is_compound {
            if let Some(ty) = self
                .expr_types
                .get(&crate::exprs::expr_span_of(&a.target))
                .cloned()
            {
                // Peel a `T?` wrapper — the LHS may be a nullable dyn slot
                // (`shape = new Square()` where `shape: Shape?`). The synthesized
                // `tref` carries the slot's nullability so the coercion helper
                // wraps `Some(...)` exactly when the slot is `Option`-shaped.
                let (inner, slot_nullable) = match ty {
                    juxc_tycheck::Ty::Nullable(inner) => (*inner, true),
                    other => (other, false),
                };
                if let juxc_tycheck::Ty::User { name, .. } = inner {
                    let bare = name.rsplit('.').next().unwrap_or(&name).to_string();
                    // Any user-typed LHS goes through the shared coercion
                    // predicate: an interface / polymorphic-base slot
                    // takes the `Rc<dyn …>` wrap, and a CONCRETE base-
                    // class slot takes the `From<Sub> for Base` `.into()`
                    // upcast (S8 — e.g. a `RuntimeException` catch binder
                    // stored into an `Exception?` field). The helper
                    // returns `None` for plain same-type assigns, which
                    // fall through to the normal path below.
                    let mut tref = crate::analysis::synth_iface_type_ref(&bare, a.span);
                    tref.nullable = slot_nullable;
                    if !matches!(
                        self.iface_coercion_to(&tref, &a.value),
                        crate::analysis::IfaceCoercion::None,
                    ) {
                        iface_tref = Some(tref);
                    }
                }
            }
        }
        if let Some(tref) = iface_tref {
            self.emit_expr_coerced_to_iface(&tref, &a.value);
        } else {
            self.emit_arg_with_nullable_wrap(&a.value, assign_nullable);
            // Wrapper-class share-on-assign (§CR.4.1): when the RHS is a
            // wrapped place (`Path`/`this` local or `xs[i]` index read), the
            // assignment must SHARE the same instance — append the cheap `Rc`
            // refcount-bump clone instead of moving out of the place. Skipped
            // for compound forms (`x += y` has no wrapped-place meaning) and
            // when the value was lifted into `Some(...)` (a nullable field
            // never takes a bare wrapped place; the helper returns false too).
            if !is_compound && !assign_nullable && self.wrapper_value_needs_clone(&a.value) {
                self.w.push_str(".clone()");
            } else if !is_compound && !assign_nullable {
                // Owned ctor param still read by a later statement —
                // clone instead of moving (see `emit_assign_rhs`).
                if let Expr::Path(qn) = &a.value {
                    if qn.segments.len() == 1
                        && self.ctor_live_after.contains(&qn.segments[0].text)
                    {
                        self.w.push_str(".clone()");
                    }
                }
            }
        }
        self.w.push_str(";\n");
    }

    /// True when an assignment target resolves to a PROPERTY with a
    /// settable accessor — either `obj.Prop` (any receiver) or a
    /// bare `Prop` inside the declaring class (implicit `this`,
    /// unshadowed). Drives the S4 compound-assign desugar: such
    /// targets must read through the getter and write through the
    /// setter rather than touching a backing field directly.
    fn assign_target_is_property_with_setter(&self, target: &Expr) -> bool {
        match target {
            Expr::Field(tf) if !tf.safe => self
                .property_on_receiver(&tf.object, &tf.field.text)
                .map(|p| p.setter.is_some())
                .unwrap_or(false),
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let name = &qn.segments[0].text;
                let shadowed = self.current_fn_params.contains(name)
                    || self.local_types.iter().any(|s| s.contains_key(name))
                    || self.nullable_locals.contains(name);
                if shadowed {
                    return false;
                }
                self.enclosing_class
                    .clone()
                    .and_then(|c| self.lookup_class_ast_by_bare_or_fqn(&c))
                    .and_then(|cd| {
                        cd.properties
                            .iter()
                            .find(|p| p.name.text == *name)
                            .map(|p| p.setter.is_some())
                    })
                    .unwrap_or(false)
            }
            _ => false,
        }
    }

    /// Emit a property write as a call to the synthesized setter
    /// (`obj.__set_Prop(value)` for an instance property, or
    /// `Class::__set_Prop(value)` for a static one). The value is
    /// emitted as a regular call argument so String literals coerce
    /// and wrapper-class places share correctly.
    pub(crate) fn emit_property_setter_call(
        &mut self,
        receiver: &Expr,
        prop_name: &str,
        value: &Expr,
    ) {
        let setter = juxc_ast::desugar_static_setter_name(prop_name);
        // Static property: `Class.Prop = v` where `Class` is a path
        // resolving to a class → `Class::__set_Prop(v)`.
        if let Expr::Path(qn) = receiver {
            if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                self.emit_fqn_path_in_rust(&class_fqn, qn.segments.len() > 1);
                self.w.push_str("::");
                self.w.push_str(&setter);
                self.w.push('(');
                self.emit_property_setter_arg(value);
                self.w.push_str(");\n");
                return;
            }
        }
        // Instance property: `obj.__set_Prop(v)`. The setter is an
        // inherent `&self` method on the (possibly wrapper) newtype;
        // its body takes the statement-scoped `borrow_mut()`.
        self.emit_expr(receiver);
        // Wrapper-class share-on-receiver isn't needed — a method call
        // borrows the receiver, it doesn't move it.
        self.w.push('.');
        self.w.push_str(&setter);
        self.w.push('(');
        self.emit_property_setter_arg(value);
        self.w.push_str(");\n");
    }

    /// Emit a single setter argument, applying the same value-position
    /// coercions a normal call argument gets (wrapper-class share).
    fn emit_property_setter_arg(&mut self, value: &Expr) {
        self.emit_expr(value);
        if self.wrapper_value_needs_clone(value) {
            self.w.push_str(".clone()");
        }
    }

    /// Best-effort fallback write of a read-only auto-property's
    /// backing field, used only when tycheck has *already* fired the
    /// access-control diagnostic (so the program won't actually build,
    /// but the emitted Rust stays well-formed). Mirrors the regular
    /// field-write path against the `__prop_<Name>` backing slot.
    pub(crate) fn emit_property_backing_write(
        &mut self,
        receiver: &Expr,
        prop_name: &str,
        a: &AssignStmt,
    ) {
        let backing = juxc_ast::desugar_backing_field_name(prop_name);
        let synthetic_target = Expr::Field(juxc_ast::FieldExpr {
            object: Box::new(receiver.clone()),
            field: juxc_ast::Ident { text: backing, span: juxc_source::Span::DUMMY },
            safe: false,
            span: juxc_source::Span::DUMMY,
        });
        let rewritten = AssignStmt {
            target: synthetic_target,
            op: a.op,
            value: a.value.clone(),
            span: a.span,
        };
        // Re-enter the regular assign path; the synthetic target names
        // a real backing field, so no property routing re-fires.
        self.emit_assign(&rewritten);
    }

    /// True iff the assignment target resolves to a non-`final`
    /// `static` class field — i.e. one of the
    /// `LazyLock<Mutex<T>>`-lowered slots. Recognized in two shapes:
    ///
    /// - **Qualified** `Class.field` — `Expr::Field` whose object is
    ///   a `Path` resolving to a class FQN, and the named field is
    ///   `is_static && !is_final`.
    /// - **Bare-name** `field` inside `class Class { … }` — single-
    ///   segment `Expr::Path` that matches a static field on
    ///   `self.enclosing_class`.
    ///
    /// Both shapes share the same Mutex-deadlock concern, so they
    /// share the temp-binding wrap in [`Self::emit_assign`].
    pub(crate) fn target_is_mutable_static(&self, target: &Expr) -> bool {
        match target {
            Expr::Field(f) => {
                let Expr::Path(qn) = f.object.as_ref() else { return false };
                let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) else { return false };
                let Some(class) = self.symbols.classes.get(&class_fqn) else { return false };
                let Some(field) = class.fields.get(f.field.text.as_str()) else { return false };
                field.is_static && !field.is_final
            }
            Expr::Path(qn) if qn.segments.len() == 1 => {
                let Some(class_name) = &self.enclosing_class else { return false };
                let Some(class) = self.lookup_class_by_bare_or_fqn(class_name) else {
                    return false;
                };
                let Some(field) = class.fields.get(qn.segments[0].text.as_str()) else {
                    return false;
                };
                field.is_static && !field.is_final
            }
            _ => false,
        }
    }

    /// True iff the assignment target is a class/record field whose
    /// declared type carries the `nullable` flag. Walks the field
    /// expression to find the receiver's class via `expr_types`,
    /// then looks up the field on that class. Conservative — a
    /// miss (no class info, no such field) returns false so the
    /// caller won't add a wrap and rustc will surface any real
    /// mismatch.
    pub(crate) fn assign_target_is_nullable(&self, target: &Expr) -> bool {
        // Bare-Path target — a LOCAL with a `T?` declared type
        // (`maybe = a;` where `C? maybe`). The nullable-locals set is
        // the live source of truth (smart-cast narrowing removes a
        // name for the narrowed region, where a raw assign would be
        // assigning into the unwrapped binding).
        if let Expr::Path(qn) = target {
            return qn.segments.len() == 1
                && self.nullable_locals.contains(&qn.segments[0].text);
        }
        let Expr::Field(f) = target else { return false };
        let Some(juxc_tycheck::Ty::User { name, .. }) =
            self.expr_types.get(&expr_span_of(&f.object))
        else {
            return false;
        };
        // Walk the class's own fields; ancestor fields would need
        // an inheritance walk like `lookup_field_type` does. For
        // Phase 1 the assign-coercion fires only on direct fields
        // — Java/Kotlin user code that assigns to an inherited
        // nullable field is rare enough that we'll wait for an
        // example to motivate the deeper walk.
        if let Some(class) = self.symbols.classes.get(name) {
            if let Some(field) = class.fields.get(&f.field.text) {
                return field.ty.nullable;
            }
        }
        if let Some(record) = self.symbols.records.get(name) {
            if let Some(c) = record.components.iter().find(|c| c.name == f.field.text) {
                return c.ty.nullable;
            }
        }
        false
    }

    /// Lower `if (cond) { … } else if (…) { … } else { … }` to its
    /// directly-corresponding Rust form. Rust uses no parentheses around
    /// `if` conditions, so we drop them.
    ///
    /// **Null smart-cast** (Kotlin-style): when the condition is
    /// `name != null` for a bare identifier `name`, lower the head
    /// to `if let Some(name) = name` so the binding inside the
    /// `then` block sees the unwrapped inner type. Pairs with the
    /// `is_some()`/`is_none()` peephole in `emit_binary` — the
    /// peephole stays the right shape for boolean-context uses
    /// (`var ok = x != null;`), while this branch handles the
    /// narrower `if` form.
    /// Emit the `let Some(<binder>) = …` head of a type-test smart-cast (the
    /// caller prepends `if ` / ` else if `). For a `dyn` source it's the
    /// runtime hook `<value>.__jux_as_T()`; for a concrete source the test is a
    /// statically-true upcast, so the binder gets `Some(<value coerced to T>)`.
    fn emit_typetest_binder_head(&mut self, t: &juxc_ast::TypeTestExpr) {
        let binder = t.binder.as_ref().expect("binder present");
        let target = t
            .ty
            .name
            .segments
            .last()
            .map(|s| s.text.clone())
            .unwrap_or_default();
        self.w.push_str("let Some(");
        self.w.push_str(&binder.text);
        self.w.push_str(") = ");
        let src = self.cast_source_bare(&t.value);
        // A dynamic-dispatch source narrows via its runtime hook — UNLESS the
        // target is the value's own static type (`if (z => Animal za)` where `z`
        // is already `Animal`). That's an identity test: always matches, and no
        // `__jux_as_<Self>` hook exists (hooks are generated only for subtypes).
        // Bind the value itself; the iface coercion clones the dyn handle.
        let dyn_downcast = src.as_deref().is_some_and(|s| self.source_is_dyn(s))
            && src.as_deref() != Some(target.as_str());
        if dyn_downcast {
            self.emit_expr(&t.value);
            self.w.push_str(".__jux_as_");
            self.w.push_str(&target);
            self.w.push_str("()");
        } else {
            self.w.push_str("Some(");
            self.emit_expr_coerced_to_iface(&t.ty, &t.value);
            self.w.push(')');
        }
    }

    pub(crate) fn emit_if(&mut self, if_stmt: &IfStmt) {
        // Smart-cast bookkeeping: when the condition is `name !=
        // null`, `name` inside the `then` block is the unwrapped
        // inner `T` (no longer `Option<T>`). Remove it from
        // `nullable_locals` for the duration of the body so
        // format-arg JuxOpt wrapping and elvis null-checks treat
        // it correctly. Restore on the way out so the rest of the
        // function still sees the original nullable shape.
        let cast_name: Option<String> =
            match_simple_not_null_check(&if_stmt.condition).map(|s| s.to_string());
        let was_nullable = cast_name
            .as_ref()
            .map_or(false, |n| self.nullable_locals.contains(n));

        // Type-test smart-cast: `if (x => Dog d) { … }` lowers to
        // `if let Some(d) = x.__jux_as_Dog() { … }` — `d` is a fresh `Dog`
        // sharing `x`'s inner cell (mutations reflect). The bare (no-binder)
        // `x => Dog` condition falls through to the plain path below.
        let typetest_binder = match &if_stmt.condition {
            Expr::TypeTest(t) if t.binder.is_some() => Some(t),
            _ => None,
        };
        if let Some(t) = typetest_binder {
            self.w.push_str("if ");
            self.emit_typetest_binder_head(t);
            self.w.push_str(" {\n");
        } else if let Some(name) = &cast_name {
            self.w.push_str("if let Some(");
            self.w.push_str(name);
            self.w.push_str(") = ");
            self.w.push_str(name);
            self.w.push_str(" {\n");
        } else {
            self.w.push_str("if ");
            self.emit_expr(&if_stmt.condition);
            self.w.push_str(" {\n");
        }
        // Apply the smart-cast: inside the `then` block the
        // binding is no longer nullable.
        if let Some(name) = &cast_name {
            if was_nullable {
                self.nullable_locals.remove(name);
            }
        }
        self.w.indent_inc();
        self.emit_block_contents(&if_stmt.then_block);
        self.w.indent_dec();
        // Restore: outside the block, the binding regains its
        // declared nullable type for subsequent uses (the next
        // else-arm, code after the if).
        if let Some(name) = &cast_name {
            if was_nullable {
                self.nullable_locals.insert(name.clone());
            }
        }
        self.w.emit_indent();
        self.w.push('}');

        // Walk an arbitrarily-long else-if chain without recursing into
        // `emit_stmt`: each nested IfStmt becomes another `} else if …`
        // segment on the same source line.
        let mut else_branch = if_stmt.else_branch.as_deref();
        while let Some(branch) = else_branch {
            match branch {
                ElseBranch::If(inner) => {
                    // `else if (x => Dog d)` → `else if let Some(d) =
                    // x.__jux_as_Dog()`; the bare/non-typetest case stays a
                    // plain `else if <cond>`.
                    let binder_test = match &inner.condition {
                        Expr::TypeTest(t) if t.binder.is_some() => Some(t),
                        _ => None,
                    };
                    if let Some(t) = binder_test {
                        self.w.push_str(" else if ");
                        self.emit_typetest_binder_head(t);
                        self.w.push_str(" {\n");
                    } else {
                        self.w.push_str(" else if ");
                        self.emit_expr(&inner.condition);
                        self.w.push_str(" {\n");
                    }
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
        Stmt::DoWhile(d) => d.span,
        Stmt::Labeled { label, .. } => label.span,
        Stmt::ForEach(f) => f.span,
        Stmt::ForC(f) => f.span,
        Stmt::Assign(a) => a.span,
        Stmt::Break(_, s) => *s,
        Stmt::Continue(_, s) => *s,
        Stmt::SuperCall(_, s) => *s,
        Stmt::Throw(_, s) => *s,
        Stmt::Try(t) => t.span,
        Stmt::Unsafe(b) => b.span,
    }
}

/// True when `block` contains a function-level `return` — the signal
/// that a `try` body needs the `Option<RetT>` return-threading shape
/// (see `RustEmitter::emit_try`). Walks every statement form
/// recursively, INCLUDING nested `try` blocks (their post-`finally`
/// re-return lands inside the outer closure too) and switch-statement
/// arm blocks. Lambda bodies are SKIPPED — a `return` there belongs to
/// the lambda, not the enclosing function.
pub(crate) fn block_contains_fn_return(block: &juxc_ast::Block) -> bool {
    block_contains_return_where(block, &|_| true)
}

/// True when `block` contains a function-level `return <expr>` (a return
/// CARRYING a value, not a bare `return;`). Used by the lambda emitter's
/// tail-try patch-up (O1): only a value-returning lambda needs the
/// `unreachable!()` type-coercion footer — a void lambda may legally
/// fall through past its try.
pub(crate) fn block_contains_valued_return(block: &juxc_ast::Block) -> bool {
    block_contains_return_where(block, &|opt| opt.is_some())
}

/// Shared walker behind [`block_contains_fn_return`] /
/// [`block_contains_valued_return`] — `pred` inspects each `return`'s
/// optional value expression and decides whether it counts.
fn block_contains_return_where(
    block: &juxc_ast::Block,
    pred: &dyn Fn(&Option<juxc_ast::Expr>) -> bool,
) -> bool {
    block
        .statements
        .iter()
        .any(|s| stmt_contains_return_where(s, pred))
}

fn stmt_contains_return_where(
    s: &Stmt,
    pred: &dyn Fn(&Option<juxc_ast::Expr>) -> bool,
) -> bool {
    match s {
        Stmt::Return(opt) => pred(opt),
        Stmt::If(i) => {
            if block_contains_return_where(&i.then_block, pred) {
                return true;
            }
            let mut cursor = i.else_branch.as_deref();
            while let Some(branch) = cursor {
                match branch {
                    juxc_ast::ElseBranch::If(inner) => {
                        if block_contains_return_where(&inner.then_block, pred) {
                            return true;
                        }
                        cursor = inner.else_branch.as_deref();
                    }
                    juxc_ast::ElseBranch::Block(b) => {
                        return block_contains_return_where(b, pred);
                    }
                }
            }
            false
        }
        Stmt::While(w) => block_contains_return_where(&w.body, pred),
        Stmt::DoWhile(d) => block_contains_return_where(&d.body, pred),
        Stmt::Labeled { stmt, .. } => stmt_contains_return_where(stmt, pred),
        Stmt::ForEach(f) => block_contains_return_where(&f.body, pred),
        Stmt::ForC(f) => block_contains_return_where(&f.body, pred),
        Stmt::Try(t) => {
            block_contains_return_where(&t.body, pred)
                || t.catches
                    .iter()
                    .any(|c| block_contains_return_where(&c.body, pred))
                || t.finally
                    .as_ref()
                    .map(|f| block_contains_return_where(f, pred))
                    .unwrap_or(false)
        }
        Stmt::Unsafe(b) => block_contains_return_where(b, pred),
        Stmt::Expr(juxc_ast::Expr::Switch(sw)) => sw.arms.iter().any(|arm| match &arm.body {
            juxc_ast::SwitchBody::Block(b) => block_contains_return_where(b, pred),
            juxc_ast::SwitchBody::Expr(_) => false,
        }),
        _ => false,
    }
}
