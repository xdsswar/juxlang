//! Constructor lowering for classes — the explicit-ctor walker (with
//! the simple-ctor fast path) and the synthetic zero-arg default for
//! classes that declare no constructor.

use std::collections::HashSet;

use crate::analysis::{
    collect_mutated_names, extract_simple_ctor_inits, SimpleCtorInits,
};
use crate::stmts::stmt_span;
use crate::RustEmitter;

impl RustEmitter {
    /// Emit a user-declared constructor as `pub fn new(...) -> Self`.
    /// Caller (`emit_class_decl`) has the writer at level 0; the ctor
    /// signature lives at depth 1 (inside the class's `impl` block),
    /// and the body at depth 2.
    pub(crate) fn emit_constructor(
        &mut self,
        class_decl: &juxc_ast::ClassDecl,
        ctor: &juxc_ast::ConstructorDecl,
    ) {
        // (Migrated to Writer indent-aware API)
        // Caller (`emit_class_decl`) is at level 0; the ctor signature
        // sits at depth 1 (inside the `impl` block), and the body at
        // depth 2.
        self.w.indent_inc();
        self.w.emit_indent();
        self.emit_visibility(ctor.visibility);
        self.w.push_str("fn new(");
        for (i, param) in ctor.params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_type_as_rust(&param.ty);
        }
        self.w.push_str(") -> Self {\n");
        self.w.indent_inc();

        // Try the **simple-ctor fast path** first: when every statement
        // in the body is `this.field = expr;` (with an optional leading
        // `super(args);`), collapse to a direct `Self { field: expr, … }`
        // literal. Idiomatic Rust, AND it sidesteps the "need `Default`
        // for generic-typed fields" problem inherent to the fallback
        // `__self`-builder pattern.
        if let Some(simple) = extract_simple_ctor_inits(ctor) {
            self.emit_simple_ctor_body(class_decl, &simple);
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
            self.w.indent_dec();
            return;
        }

        // Fallback: the body has stmts other than this.field-init (e.g.,
        // a `print(…)` mixed in). Use the `__self` builder pattern,
        // which requires fields without explicit init to be
        // `Default`-initialized — fine for primitives, breaks for
        // unconstrained generic types. The user has to keep the ctor
        // body simple in that case.
        self.w.line("let mut __self = Self {");
        self.w.indent_inc();
        for field in &class_decl.fields {
            if field.is_static {
                continue;
            }
            self.w.emit_indent();
            self.w.push_str(&field.name.text);
            self.w.push_str(": ");
            if let Some(default) = &field.default {
                self.emit_expr(default);
            } else {
                self.emit_field_default_value_for(&field.ty);
            }
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("};");

        // Body — `this` rewrites to `__self`.
        self.this_alias = Some("__self".to_string());
        let mut muts = HashSet::new();
        collect_mutated_names(&ctor.body, &mut muts, &self.user_mut_methods);
        self.mutated_in_fn = muts;
        for stmt in &ctor.body.statements {
            self.emit_source_marker(stmt_span(stmt));
            self.w.emit_indent();
            self.emit_stmt(stmt);
        }
        self.this_alias = None;

        // Return the constructed value.
        self.w.line("__self");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }

    /// Emit the direct `Self { field: expr, … }` body for a simple
    /// constructor — one whose body is purely `this.field = expr;`
    /// lines. `inits` carries one `(field-name, init-expr)` entry per
    /// statement in source order; if the same field is assigned more
    /// than once, the **last** assignment wins (matching Java semantics
    /// for a sequence of plain assignments).
    pub(crate) fn emit_simple_ctor_body(
        &mut self,
        class_decl: &juxc_ast::ClassDecl,
        simple: &SimpleCtorInits,
    ) {
        // (Migrated to Writer indent-aware API)
        // Caller (`emit_constructor`) has the writer at level 2 — the
        // depth of statements inside `pub fn new(...) -> Self { … }`.
        // The `Self { … }` literal body sits one deeper at level 3.
        // Resolve field-name → init-expr, last assignment wins.
        let mut chosen: std::collections::HashMap<&str, &juxc_ast::Expr> =
            std::collections::HashMap::new();
        for (name, expr) in &simple.inits {
            chosen.insert(name.as_str(), expr);
        }

        self.w.line("Self {");
        self.w.indent_inc();
        // Inherited parent — emit the `__parent` slot first, before
        // the class's own fields, matching the struct declaration's
        // field order.
        if let Some(parent_ty) = &class_decl.extends {
            self.w.emit_indent();
            self.w.push_str("__parent: ");
            // Emit only the parent's bare identifier here, not the
            // full `<...>` instantiation. The `__parent` field
            // declaration already pins the parent's generic args, so
            // Rust infers them at the call site — and
            // `Parent<int>::new(...)` is invalid Rust syntax anyway
            // (would need the turbofish form `Parent::<int>::new`).
            if let Some(seg) = parent_ty.name.segments.first() {
                self.w.push_str(&seg.text);
            }
            self.w.push_str("::new(");
            // If the constructor wrote `super(args);`, lift those args
            // here. If it didn't, Phase 1 calls `Parent::new()` with
            // no arguments — fine for parameterless parents, breaks
            // (with a clear Rust error) when the parent's ctor needs
            // arguments and the user forgot to write `super(...)`.
            if let Some(args) = &simple.super_args {
                // Post Fix 1, every Jux `String` value (literal,
                // parameter, field, or call result) is already an
                // owned Rust `String`, so the per-arg `.to_string()`
                // coercion the parent's String-typed slot used to
                // need is now a no-op double-wrap. The args go in
                // verbatim; rustc verifies the types match.
                let _ = parent_ty;
                // Clone to release the borrow on `simple` before the
                // `emit_expr` calls (which need `&mut self`).
                let args = args.clone();
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.w.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
            }
            self.w.push_str("),\n");
        }
        for field in &class_decl.fields {
            // Static fields aren't instance state — skip them
            // here. They live as `pub const` / `pub static` items
            // inside the impl block.
            if field.is_static {
                continue;
            }
            self.w.emit_indent();
            self.w.push_str(&field.name.text);
            self.w.push_str(": ");
            if let Some(init_expr) = chosen.get(field.name.text.as_str()) {
                // Field assigned in body — emit its init expression
                // verbatim. Post Fix 1 a String-typed init expression
                // is always already an owned Rust `String`, so the
                // previous `.to_string()` injection is gone.
                self.emit_expr(init_expr);
            } else if let Some(default) = &field.default {
                // Field carries a Jux-source default initializer.
                self.emit_expr(default);
            } else {
                // No assignment and no source default — fall back to
                // the type's natural default. Generic-typed fields
                // will surface a Rust compile error here, signaling
                // the user has to assign them in the constructor body.
                self.emit_field_default_value_for(&field.ty);
            }
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
    }

    /// Synthesize a zero-argument default constructor when the class
    /// declared none — per §7.3.1's "implicit zero-arg constructor".
    pub(crate) fn emit_synthetic_default_constructor(&mut self, class_decl: &juxc_ast::ClassDecl) {
        // (Migrated to Writer indent-aware API)
        // Caller (`emit_class_decl`) is at level 0; synth ctor sits at
        // depth 1 inside the `impl` block.
        self.w.indent_inc();
        self.w.line("pub fn new() -> Self {");
        self.w.indent_inc();
        self.w.line("Self {");
        self.w.indent_inc();
        // Inherited parent — invoke the parent's zero-arg constructor.
        // For parents whose ctor takes arguments, the user MUST declare
        // an explicit constructor with `super(args);`; the synthetic
        // path is only valid for trivially-defaulted hierarchies.
        if let Some(parent_ty) = &class_decl.extends {
            self.w.emit_indent();
            self.w.push_str("__parent: ");
            // Same rule as the explicit-ctor path: emit the parent's
            // bare identifier and let Rust infer the generic args
            // from the `__parent` field's declared type.
            if let Some(seg) = parent_ty.name.segments.first() {
                self.w.push_str(&seg.text);
            }
            self.w.push_str("::new(),\n");
        }
        for field in &class_decl.fields {
            if field.is_static {
                continue;
            }
            self.w.emit_indent();
            self.w.push_str(&field.name.text);
            self.w.push_str(": ");
            if let Some(default) = &field.default {
                self.emit_expr(default);
            } else {
                self.emit_field_default_value_for(&field.ty);
            }
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }
}
