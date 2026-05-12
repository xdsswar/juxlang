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
        // (Migrated to Writer indent-aware API)
        // Caller is at level 0 — top-level functions sit at depth 0,
        // body at depth 1.
        // `fn name<T, U>(params) -> return {`
        // Wildcard-lift pre-pass: any `? extends T` / `? super T` /
        // `?` in a param position becomes a fresh `__Wn` generic on
        // this function with the matching bound. Phase-1 PECS
        // lowering — mirrors Java's compile-time wildcard erasure.
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
        self.w.push_str("fn ");
        self.w.push_str(&fn_decl.name.text);
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
            ReturnType::AsyncType(_) => {
                // TODO: async lowering — needs a real runtime story per §15.
                // Placeholder: emit `()` so the resulting Rust at least
                // parses. (No Jux program in flight actually uses this.)
                self.w.push_str(" -> ()");
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
                // `return expr;` → bare `expr` on its own line. Fix 1
                // unified every string source (literals, parameters,
                // fields, returns) to owned `String`, so the old
                // tail-return `.to_string()` coercion is no longer
                // needed: a bare literal here already self-coerces.
                //
                // Nullable-return wrap: same rule as the mid-body
                // return path in `emit_stmt` — a `T?`-returning fn
                // lifts a `T` value into `Some(T)`.
                let wrap_some = self.return_wants_some_wrap(expr);
                self.w.emit_indent();
                if wrap_some {
                    self.w.push_str("Some(");
                }
                self.emit_expr(expr);
                if wrap_some {
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
