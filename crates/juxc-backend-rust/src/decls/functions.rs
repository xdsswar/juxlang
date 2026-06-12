//! Top-level Jux function declarations → Rust `fn`. Body emission +
//! the trailing-return elision rule that produces idiomatic Rust
//! tail expressions also lives here, since methods reuse the same
//! body-emitter (`emit_fn_body_at`).

use std::collections::HashSet;

use juxc_ast::{Block, FnDecl, ReturnType, Stmt};

use crate::analysis::collect_mutated_names;
use crate::stmts::stmt_span;
use crate::RustEmitter;

impl RustEmitter {
    /// Emit a Rust `fn` for a Jux function declaration.
    ///
    /// Visibility is intentionally dropped — every emitted function is
    /// crate-private. Inheritance and trait dispatch don't exist in this
    /// milestone, so there's nothing for visibility to mediate.
    pub(crate) fn emit_fn_decl(&mut self, fn_decl: &FnDecl) {
        // **Test-mode suppression.** When `jux test` is driving the
        // build, the synthetic test runner IS `fn main()`. The
        // user's own `void main()` (e.g. the default scaffold's
        // "Hello from Jux!") gets skipped here so we don't end up
        // with two `fn main` symbols at the crate root.
        if self.test_mode && fn_decl.name.text == "main" {
            return;
        }
        // (Migrated to Writer indent-aware API)
        // Caller is at level 0 — top-level functions sit at depth 0,
        // body at depth 1.
        // `fn name<T, U>(params) -> return {`
        // Wildcard-lift pre-pass: any `? extends T` / `? super T` /
        // `?` in a param position becomes a fresh `__Wn` generic on
        // this function with the matching bound. Phase-1 PECS
        // lowering — mirrors Java's compile-time wildcard erasure.
        //
        // **Async-main shim.** Rust requires the binary entry point
        // to be a synchronous `fn main()`. When the user wrote
        // `async void main()` / `async T main()`, we (a) rename
        // their function to `__jux_async_main` so the async body
        // still emits, and (b) append a sync `fn main()` shim that
        // calls `futures::executor::block_on(__jux_async_main())`.
        // The shim is appended after the user's body, both at the
        // same scope. For multi-unit/packaged workspaces, the
        // workspace-shim path (`emit_workspace_main_shim`) routes
        // through `__jux_async_main` instead of `main` when it sees
        // an async-typed entry — but the rename happens here so the
        // emitted symbol matches in either mode.
        let is_async_main = fn_decl.name.text == "main"
            && matches!(fn_decl.return_type, ReturnType::AsyncType(_));
        // **Args-main rename.** Rust's entry `fn main()` takes no
        // parameters, so a user `main(String[] args)` /
        // `main(String... args)` (§E.1.2) can't BE the entry — we
        // rename it to `__jux_args_main` and the shim (local or
        // workspace) passes `std::env::args().skip(1)` (skip(1):
        // Jux args exclude the program name, like Java). Async mains
        // already rename to `__jux_async_main`; params just change
        // what the shim passes.
        let is_args_main = fn_decl.name.text == "main"
            && !is_async_main
            && !fn_decl.params.is_empty();
        let mut lifter = crate::analysis::WildcardLifter::new();
        let lifted_param_tys: Vec<juxc_ast::TypeRef> = fn_decl
            .params
            .iter()
            .map(|p| {
                if crate::analysis::type_ref_has_wildcard(&p.ty) {
                    lifter.rewrite_type_ref(&p.ty)
                } else {
                    p.ty.clone()
                }
            })
            .collect();
        let mut combined_generics = fn_decl.generic_params.clone();
        combined_generics.extend(lifter.new_params.iter().cloned());

        self.w.emit_indent();
        // When the compilation unit is wrapped in `pub mod a::b::…`,
        // user-declared visibility on top-level functions becomes
        // load-bearing — the crate-root `fn main()` shim needs to
        // reach `a::b::main`, so the inner `main` must be `pub`.
        // At crate root (no package) we keep the historical
        // "drop visibility, emit a private `fn`" behavior so the
        // existing test corpus stays green.
        if !self.symbols.package.is_empty() {
            self.emit_visibility(fn_decl.visibility);
        }
        // `async T` return type in Jux maps to a Rust `async fn`
        // returning `T`. The keyword sits BEFORE `fn` per Rust
        // syntax, so we emit it ahead of the function header.
        if matches!(fn_decl.return_type, ReturnType::AsyncType(_)) {
            self.w.push_str("async ");
        }
        // `unsafe T f()` → `unsafe fn f()` (§A.2.4 modifier). The keyword
        // precedes `fn` (after `async`, matching Rust's `async unsafe fn`
        // ordering — though Jux writes `unsafe` first, the emitted Rust
        // tolerates either since `async` is rare on unsafe fns).
        if fn_decl.modifiers.contains(&juxc_ast::FnModifier::Unsafe) {
            self.w.push_str("unsafe ");
        }
        self.w.push_str("fn ");
        // Async-main / args-main rename — see comments above.
        if is_async_main {
            self.w.push_str("__jux_async_main");
        } else if is_args_main {
            self.w.push_str("__jux_args_main");
        } else {
            self.w.push_str(&fn_decl.name.text);
        }
        // Use the combined generics list so synthetic params land on
        // the signature. `<__W0: AnimalKind + Clone, …>` is emitted
        // through the same bound-aware helper used for user params,
        // so class bounds get the marker-trait rewrite consistently.
        if combined_generics.is_empty() {
            self.emit_generic_params(&fn_decl.generic_params);
        } else {
            self.emit_generic_params_with_clone_bound(&combined_generics);
        }
        self.w.push('(');
        // Params the body mutates in place (`xs.push(…)` on a by-value
        // collection param, reassignment) need Rust's `mut` binding —
        // same inference the `let mut` choice uses for locals. `out`
        // params are `&mut T` already and never need it.
        let mut param_muts = HashSet::new();
        if let Some(body) = &fn_decl.body {
            collect_mutated_names(body, &mut param_muts, &self.user_mut_methods);
        }
        for (i, param) in fn_decl.params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            if !param.is_out && param_muts.contains(&param.name.text) {
                self.w.push_str("mut ");
            }
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            if param.is_out {
                self.w.push_str("&mut "); // `out T` (§M.4) lowers to `&mut T`
            }
            self.emit_value_type_as_rust(&lifted_param_tys[i]);
        }
        self.w.push(')');

        match &fn_decl.return_type {
            ReturnType::Void => {} // `void` → omit return arrow
            ReturnType::Type(t) => {
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
            ReturnType::AsyncType(t) => {
                // `async fn name(...) -> T` — the `async` was
                // emitted ahead of `fn` (see the header above).
                self.w.push_str(" -> ");
                self.emit_return_type_as_rust(t);
            }
        }

        // §O.5 where-constraints → Rust trait bounds. Each Jux
        // operator capability maps to its std trait so the body's
        // use of the operator on `T` compiles; constraints without a
        // Rust counterpart (`[]`, `()`, `in`, ranges) are call-site
        // checked only (E0941) and add no bound here.
        if !fn_decl.wheres.is_empty() {
            let mut bounds: Vec<String> = Vec::new();
            for w in &fn_decl.wheres {
                let t = w.param.text.as_str();
                let b = match w.kind {
                    juxc_ast::OperatorKind::Eq => Some(format!("{t}: PartialEq")),
                    juxc_ast::OperatorKind::Cmp => Some(format!("{t}: PartialOrd")),
                    juxc_ast::OperatorKind::Hash => Some(format!("{t}: std::hash::Hash")),
                    juxc_ast::OperatorKind::ToString => {
                        Some(format!("{t}: std::fmt::Display"))
                    }
                    juxc_ast::OperatorKind::Plus => {
                        Some(format!("{t}: std::ops::Add<Output = {t}>"))
                    }
                    juxc_ast::OperatorKind::Minus => {
                        Some(format!("{t}: std::ops::Sub<Output = {t}>"))
                    }
                    juxc_ast::OperatorKind::Mul => {
                        Some(format!("{t}: std::ops::Mul<Output = {t}>"))
                    }
                    juxc_ast::OperatorKind::Div => {
                        Some(format!("{t}: std::ops::Div<Output = {t}>"))
                    }
                    juxc_ast::OperatorKind::Rem => {
                        Some(format!("{t}: std::ops::Rem<Output = {t}>"))
                    }
                    juxc_ast::OperatorKind::Neg => {
                        Some(format!("{t}: std::ops::Neg<Output = {t}>"))
                    }
                    juxc_ast::OperatorKind::BitAnd => {
                        Some(format!("{t}: std::ops::BitAnd<Output = {t}>"))
                    }
                    juxc_ast::OperatorKind::BitOr => {
                        Some(format!("{t}: std::ops::BitOr<Output = {t}>"))
                    }
                    juxc_ast::OperatorKind::BitXor => {
                        Some(format!("{t}: std::ops::BitXor<Output = {t}>"))
                    }
                    juxc_ast::OperatorKind::BitNot => {
                        Some(format!("{t}: std::ops::Not<Output = {t}>"))
                    }
                    juxc_ast::OperatorKind::Shl => {
                        Some(format!("{t}: std::ops::Shl<Output = {t}>"))
                    }
                    juxc_ast::OperatorKind::Shr => {
                        Some(format!("{t}: std::ops::Shr<Output = {t}>"))
                    }
                    _ => None,
                };
                if let Some(b) = b {
                    if !bounds.contains(&b) {
                        bounds.push(b);
                    }
                }
            }
            if !bounds.is_empty() {
                self.w.push_str(" where ");
                self.w.push_str(&bounds.join(", "));
            }
        }

        self.w.push_str(" {\n");
        // Body sits at depth 1 — push one level for `emit_fn_body`.
        self.w.indent_inc();
        if let Some(body) = &fn_decl.body {
            // Per-function mutation pass: figure out which locals get
            // reassigned anywhere in this body. The result drives the
            // `let` vs `let mut` choice in emit_var_decl.
            let mut muts = HashSet::new();
            collect_mutated_names(body, &mut muts, &self.user_mut_methods);
            self.mutated_in_fn = muts;
            // Reset and re-seed the nullable-locals set for this fn:
            // any param whose declared type is `T?` (post-spec
            // nullable-primitive check has already rejected
            // `int?` shapes) goes in so call sites passing it
            // through to other slots don't double-wrap.
            self.nullable_locals.clear();
            for p in &fn_decl.params {
                if p.ty.nullable {
                    self.nullable_locals.insert(p.name.text.clone());
                }
            }
            // Register each parameter's type in `local_types` so name-keyed
            // receiver resolution works on params too — wrapper-class field
            // access (`s.field`), stdlib-dispatch, and enum-switch scrutinee
            // qualification all consult this when `expr_types` is unreliable.
            // The function's own generic parameter names — a bare param typed by
            // one of these is a `Ty::Param` (the backend has no `TypeEnv`, so
            // `ty_from_ref_in_env` alone can't tell `T` from an unknown class).
            let generic_param_names: std::collections::HashSet<&str> = fn_decl
                .generic_params
                .iter()
                .map(|g| g.name.text.as_str())
                .collect();
            for p in &fn_decl.params {
                let bare_generic = p.ty.array_shape.is_none()
                    && !p.ty.nullable
                    && p.ty.generic_args.is_empty()
                    && p.ty.name.segments.len() == 1
                    && generic_param_names.contains(p.ty.name.segments[0].text.as_str());
                let ty = if bare_generic {
                    juxc_tycheck::Ty::Param(p.ty.name.segments[0].text.clone())
                } else {
                    juxc_tycheck::ty_from_ref_in_env(&p.ty, &self.symbols)
                };
                // Register `User` (wrapper-class resolution) and `Param`
                // (generic-value `.clone()` decisions) params; both are consulted
                // name-keyed when `expr_types` is unreliable.
                if matches!(
                    ty,
                    juxc_tycheck::Ty::User { .. } | juxc_tycheck::Ty::Param(_)
                ) {
                    if let Some(scope) = self.local_types.last_mut() {
                        scope.insert(p.name.text.clone(), ty);
                    }
                }
            }
            // Save/restore around the body so `return "lit";` inside
            // a `String`-returning fn picks up `.to_string()` while
            // tail-position emission is consulting `current_return_type`.
            let saved = self.current_return_type.take();
            self.current_return_type = Some(fn_decl.return_type.clone());
            // The function's own `int`-typed const-generic params
            // (`fn cap<int N>()`) — bare value reads of `N` in the body
            // emit `(N as isize)`. Extends (not replaces) any enclosing
            // class's set; restored after the body.
            let prev_const_ints = self.const_int_params.clone();
            self.const_int_params
                .extend(crate::collect_const_int_params(&fn_decl.generic_params));
            let prev_type_params = self.current_type_params.clone();
            self.current_type_params
                .extend(crate::collect_type_param_names(&fn_decl.generic_params));
            // `out` params (§M.4): in scope for the body so reads/writes deref.
            let prev_out = std::mem::replace(
                &mut self.out_params,
                fn_decl
                    .params
                    .iter()
                    .filter(|p| p.is_out)
                    .map(|p| p.name.text.clone())
                    .collect(),
            );
            self.emit_fn_body(body, &fn_decl.return_type);
            self.out_params = prev_out;
            self.const_int_params = prev_const_ints;
            self.current_type_params = prev_type_params;
            self.current_return_type = saved;
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();

        // Append the sync `fn main()` shim for an async main entry.
        // The user's `async void main()` was emitted under
        // `__jux_async_main` above; rustc needs a sync `fn main()`
        // at the crate root to launch the binary, so we drive the
        // user's body through `futures::executor::block_on`.
        //
        // Two cases to handle:
        //
        //   - **No package** — the user's main sits at the crate
        //     root and the shim goes right after it, same level.
        //   - **Packaged** — the user's main is inside `pub mod
        //     a::b::…`; the shim is emitted at the crate root by
        //     `emit_workspace_main_shim` instead (it knows how to
        //     prepend the module path). Skip the local shim here
        //     so we don't produce a duplicate.
        //
        // In **workspace mode** the crate-root shim is owned by
        // `emit_workspace_main_shim` (it has each unit's real package and
        // emits one shim at the crate root). `self.symbols.package` is the
        // *merged* table's package there — non-empty even for a package-less
        // unit — so this local check can't be trusted in that mode. Gate on
        // `!workspace_mode` so the single-file (non-workspace) path emits the
        // shim here and the workspace path emits it there, never both.
        if is_async_main && self.symbols.package.is_empty() && !self.workspace_mode {
            self.w.line("fn main() {");
            self.w.indent_inc();
            self.w.emit_indent();
            if fn_decl.params.is_empty() {
                self.w
                    .push_str("futures::executor::block_on(__jux_async_main());\n");
            } else {
                self.w.push_str(
                    "futures::executor::block_on(__jux_async_main(std::env::args().skip(1).collect::<Vec<String>>()));\n",
                );
            }
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
        }
        // Same-level shim for a crate-root `main(String[] args)` —
        // mirrors the async shim above (single-unit path only;
        // workspaces route through `emit_workspace_main_shim`).
        if is_args_main && self.symbols.package.is_empty() && !self.workspace_mode {
            self.w.line("fn main() {");
            self.w.indent_inc();
            self.w.emit_indent();
            self.w.push_str("__jux_args_main(std::env::args().skip(1).collect::<Vec<String>>());\n");
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
        }
    }

    /// Emit a function's body block with **trailing-return elision** —
    /// the cosmetic rule that makes our output match idiomatic Rust:
    ///
    /// - A non-void function ending in `return expr;` emits `expr` as a
    ///   bare tail expression (no `return` keyword, no `;`). This is the
    ///   form a Rust developer would write — `fn add(a: i32, b: i32) -> i32 { a + b }`.
    /// - A `void` function ending in `return;` drops the statement
    ///   entirely (Rust returns `()` implicitly from a `{}` body).
    /// - Mid-function `return` statements stay as `return expr;` — early
    ///   returns are common and explicit `return` reads better there
    ///   than a labeled break.
    ///
    /// The pre-tail statements are emitted normally through
    /// [`Self::emit_stmt`]. This keeps `if`/`while`/`loop` bodies as
    /// regular statement blocks, so any `return` inside them stays
    /// statement-form (which is correct — those returns are early
    /// exits, not the function's value).
    pub(crate) fn emit_fn_body(&mut self, body: &Block, return_type: &ReturnType) {
        self.emit_fn_body_at(body, return_type);
    }

    /// Same as [`Self::emit_fn_body`] — kept as a separate entry point
    /// for historical reasons; both names land here. Callers
    /// (`emit_fn_decl`, `emit_method`) must have called
    /// `self.w.indent_inc()` to position the writer at the body depth
    /// before invoking.
    pub(crate) fn emit_fn_body_at(&mut self, body: &Block, return_type: &ReturnType) {
        // (Migrated to Writer indent-aware API)
        // Callers have set the writer's indent level to the body depth
        // before invoking. Body content emits via `self.w.emit_indent()`
        // (statements) or via `emit_tail_stmt` (the elided trailing
        // return).
        //
        // A REAL fn body types its try-return channels from
        // `current_return_type` — clear the lambda marker so an
        // anonymous-class method nested inside a lambda doesn't
        // inherit inference-typed channels (S9).
        let prev_lam = std::mem::take(&mut self.in_lambda_body);
        let elide_tail = matches!(
            (body.statements.last(), return_type),
            // Non-void function with explicit trailing `return expr;`.
            (Some(Stmt::Return(Some(_))), _)
            // Void function ending with a bare `return;` — equivalent
            // to "fall off the end," which Rust does for free.
            | (Some(Stmt::Return(None)), ReturnType::Void)
        );

        let last_idx = body.statements.len().saturating_sub(1);
        for (i, stmt) in body.statements.iter().enumerate() {
            // Source-map marker (no-op when `source` is None). Goes
            // before the per-statement indent so rustc errors can
            // scan up to find the nearest `.jux` line.
            self.emit_source_marker(stmt_span(stmt));
            if elide_tail && i == last_idx {
                self.emit_tail_stmt(stmt);
            } else {
                self.w.emit_indent();
                self.emit_stmt(stmt);
            }
        }
        // A non-void body ending in a `try` statement: the try lowering
        // completes its returns via a post-`finally` `if let` (see
        // `emit_try`), which leaves the Rust block's tail as `()` — but
        // the fn expects a value. Java guarantees every path through
        // such a function returns ("missing return statement" is a
        // javac compile error), so the fall-through is unreachable by
        // construction; the explicit `unreachable!` both satisfies
        // rustc's type-check and traps loudly if the guarantee is ever
        // violated.
        // `async void` carries a synthesized unit TypeRef in
        // AsyncType — value-wise it IS void, so falling off the end
        // of a try is fine there too.
        let is_void = match return_type {
            ReturnType::Void => true,
            ReturnType::AsyncType(t) => t
                .name
                .segments
                .last()
                .map(|s| s.text == "void")
                .unwrap_or(false),
            _ => false,
        };
        if !is_void
            && matches!(body.statements.last(), Some(Stmt::Try(_)))
        {
            self.w
                .line("unreachable!(\"function fell off the end of a try without returning\");");
        }
        self.in_lambda_body = prev_lam;
    }

    /// Emit the *tail* statement of a function body — the one targeted
    /// by trailing-return elision. The caller guarantees this is a
    /// `Return` statement, and that elision applies (so we know what to
    /// drop). The writer's current `indent_level` is the body depth, so
    /// `emit_indent()` produces the right leading whitespace.
    pub(crate) fn emit_tail_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Return(Some(expr)) => {
                // `return expr;` → bare `expr` on its own line.
                //
                // Nullable-return wrap: a `T?`-returning fn lifts a
                // `T` value into `Some(T)`. Two shapes:
                //
                // 1. **Direct value** (`return "hi";`,
                //    `return name;`) — outer `Some(...)` wrap.
                // 2. **Switch expression** (`return switch (x) {
                //    case A -> "warm"; case B -> null; }`) — outer
                //    wrap would force every arm to produce the
                //    same non-`Option<T>` type, but `null` doesn't
                //    fit `T`. Set the
                //    `emitting_nullable_target` flag so the
                //    switch emitter wraps each arm body
                //    individually (`A => Some(...), B => None`),
                //    and skip the outer wrap.
                let wrap_some = self.return_wants_some_wrap(expr);
                let wrap_upcast = self.return_needs_sealed_upcast(expr);
                let is_switch = matches!(expr, juxc_ast::Expr::Switch(_));
                // Interface return slot — same coercion the non-tail `return`
                // arm applies: wrap a class value in `Rc<dyn Trait>` / clone a
                // dyn handle. Mirrored here so trailing-return elision doesn't
                // drop the coercion.
                let ret_iface_ty = match &self.current_return_type {
                    Some(ReturnType::Type(t)) | Some(ReturnType::AsyncType(t))
                        if !matches!(
                            self.iface_coercion_to(t, expr),
                            crate::analysis::IfaceCoercion::None,
                        ) =>
                    {
                        Some(t.clone())
                    }
                    _ => None,
                };
                self.w.emit_indent();
                if let Some(ret_ty) = ret_iface_ty {
                    self.emit_expr_coerced_to_iface(&ret_ty, expr);
                    self.w.push('\n');
                    return;
                }
                if wrap_some && !is_switch {
                    self.w.push_str("Some(");
                }
                let prev_nullable_target = self.emitting_nullable_target;
                if wrap_some && is_switch {
                    self.emitting_nullable_target = true;
                }
                self.emit_expr(expr);
                self.emitting_nullable_target = prev_nullable_target;
                // **Wrapper-class share-on-return (§CR.4.1).** Same as the
                // non-tail `return` arm in `emit_stmt`: a tail `return <wrapped
                // place>;` (a `this`/local/`xs[i]` of a wrapped class) must hand
                // the caller a SHARED handle, not a borrow — append the cheap
                // `Rc` refcount-bump clone. Without this, `return this;` in a
                // builder method emits `self` (a `&C`) where owned `C` is
                // expected (rustc E0308). Skipped under Some/upcast wraps.
                if !wrap_some && !wrap_upcast && self.wrapper_value_needs_clone(expr) {
                    self.w.push_str(".clone()");
                }
                if wrap_upcast {
                    self.w.push_str(".into()");
                }
                if wrap_some && !is_switch {
                    self.w.push(')');
                }
                self.w.push('\n');
            }
            Stmt::Return(None) => {
                // Void tail `return;` — drop entirely. Nothing to emit.
            }
            _ => unreachable!("emit_tail_stmt called on non-Return stmt"),
        }
    }
}
