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
        self.w.push_str("fn ");
        // Async-main rename — see `is_async_main` comment above.
        if is_async_main {
            self.w.push_str("__jux_async_main");
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
        for (i, param) in fn_decl.params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_type_as_rust(&lifted_param_tys[i]);
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
            // Save/restore around the body so `return "lit";` inside
            // a `String`-returning fn picks up `.to_string()` while
            // tail-position emission is consulting `current_return_type`.
            let saved = self.current_return_type.take();
            self.current_return_type = Some(fn_decl.return_type.clone());
            self.emit_fn_body(body, &fn_decl.return_type);
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
            self.w
                .push_str("futures::executor::block_on(__jux_async_main());\n");
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
                self.w.emit_indent();
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
