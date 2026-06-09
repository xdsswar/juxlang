//! Constructor lowering for classes — the explicit-ctor walker (with
//! the simple-ctor fast path) and the synthetic zero-arg default for
//! classes that declare no constructor.

use std::collections::HashSet;

use juxc_ast::Expr;

use crate::analysis::{
    collect_mutated_names, extract_simple_ctor_inits, SimpleCtorInits,
};
use crate::stmts::stmt_span;
use crate::RustEmitter;

/// True when `init_expr` is a single-segment path expression whose
/// name equals `field_name`. Used by `Self { … }` emission to pick
/// Rust's struct field shorthand: `Self { x, y }` vs.
/// `Self { x: x, y: y }`. Anything more complex (a method call, a
/// `this.foo`, a literal) doesn't qualify.
fn init_is_same_named_ident(init_expr: &Expr, field_name: &str) -> bool {
    if let Expr::Path(qn) = init_expr {
        if qn.segments.len() == 1 {
            return qn.segments[0].text == field_name;
        }
    }
    false
}

/// Return the argument list of a `super(...)` call appearing anywhere
/// in `ctor`'s body, if present. Used by the wrapper-class
/// `new_inner` fallback path to forward super args into the parent's
/// `new_inner(...)`. The simple-ctor fast path reads `super_args` off
/// [`SimpleCtorInits`] instead; this helper covers the non-simple
/// (mixed-statement) body. A constructor may legally hold at most one
/// `super(...)` (tycheck enforces this), so the first match wins.
fn extract_super_args(ctor: &juxc_ast::ConstructorDecl) -> Option<Vec<Expr>> {
    for stmt in &ctor.body.statements {
        if let juxc_ast::Stmt::SuperCall(args, _) = stmt {
            return Some(args.clone());
        }
    }
    None
}

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
                self.emit_field_default_value_for(&juxc_tycheck::resolved_field_type(field));
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
        // Seed nullable-locals from this constructor's params so
        // a body that passes a `T?` parameter into a `T?` slot
        // doesn't double-wrap.
        self.nullable_locals.clear();
        for p in &ctor.params {
            if p.ty.nullable {
                self.nullable_locals.insert(p.name.text.clone());
            }
        }
        // Constructor params shadow same-named fields (the canonical
        // `Other(String test){ this.test = test; }` shape), so they must NOT be
        // rewritten by the implicit-`this` pass.
        self.current_fn_params = ctor.params.iter().map(|p| p.name.text.clone()).collect();
        for stmt in &ctor.body.statements {
            self.emit_source_marker(stmt_span(stmt));
            self.w.emit_indent();
            self.emit_stmt(stmt);
        }
        self.current_fn_params.clear();
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

        // Emit any side-effect statements first (e.g. static-field
        // counter bumps). They run at construction time, before the
        // struct literal is produced, which matches the original
        // source order for a `MyClass.counter = counter + 1;`
        // statement sitting alongside `this.field = expr;` lines.
        if !simple.side_effects.is_empty() {
            let side_effects = simple.side_effects.clone();
            for stmt in &side_effects {
                self.w.emit_indent();
                self.emit_stmt(stmt);
            }
        }

        self.w.line("Self {");
        self.w.indent_inc();
        // Sealed-parent skip: subclasses-of-sealed lower without a
        // `__parent` field (they ARE the parent enum's variant);
        // suppress the `__parent: Parent::new(...)` init line for
        // those.
        let parent_is_sealed = class_decl
            .extends
            .as_ref()
            .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
            .and_then(|bare| self.lookup_class_by_bare_or_fqn(bare).map(|c| c.is_sealed))
            .unwrap_or(false);
        // Inherited parent — emit the `__parent` slot first, before
        // the class's own fields, matching the struct declaration's
        // field order.
        if let Some(parent_ty) = &class_decl.extends {
            if !parent_is_sealed {
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
                        // Wrapper-class share-on-pass (§CR.4.1): a wrapped
                        // place forwarded into `super(...)` shares the
                        // instance with the parent slot.
                        if self.wrapper_value_needs_clone(arg) {
                            self.w.push_str(".clone()");
                        }
                    }
                }
                self.w.push_str("),\n");
            }
        }
        for field in &class_decl.fields {
            // Static fields aren't instance state — skip them
            // here. They live as `pub const` / `pub static` items
            // inside the impl block.
            if field.is_static {
                continue;
            }
            self.w.emit_indent();
            // Rust struct field shorthand: when the init is just an
            // identifier with the same name as the field
            // (`Self { x: x, … }`), emit `Self { x, … }` instead.
            // Idiomatic Rust; identical semantics.
            if let Some(init_expr) = chosen.get(field.name.text.as_str()) {
                if init_is_same_named_ident(init_expr, &field.name.text) {
                    self.w.push_str(&field.name.text);
                    self.w.push_str(",\n");
                    continue;
                }
                self.w.push_str(&field.name.text);
                self.w.push_str(": ");
                self.emit_expr(init_expr);
            } else if let Some(default) = &field.default {
                self.w.push_str(&field.name.text);
                self.w.push_str(": ");
                self.emit_expr(default);
            } else {
                self.w.push_str(&field.name.text);
                self.w.push_str(": ");
                // No assignment and no source default — fall back to
                // the type's natural default. Generic-typed fields
                // will surface a Rust compile error here, signaling
                // the user has to assign them in the constructor body.
                self.emit_field_default_value_for(&juxc_tycheck::resolved_field_type(field));
            }
            self.w.push_str(",\n");
        }
        self.w.indent_dec();
        self.w.line("}");
    }

    /// Emit a user-declared constructor for a **wrapper-shape** class
    /// (§CR.4.1 / §CR.6.4). Same signature as the legacy ctor
    /// (`pub fn new(args) -> Self`), but the body builds the inner
    /// struct and wraps it:
    ///
    /// ```text
    /// pub fn new(v: isize) -> Self {
    ///     Self(std::rc::Rc::new(std::cell::RefCell::new(C_Inner { v })))
    /// }
    /// ```
    ///
    /// The `C_Inner { … }` literal is produced by the same
    /// simple-ctor / `__self`-builder machinery the legacy path uses;
    /// we just emit a different struct name (`C_Inner`, not `Self`)
    /// and wrap the result. Constructor bodies operate on a plain
    /// `C_Inner` (`__self`), so the interior-mutability `borrow`
    /// rewrite is suppressed for the duration of the body — the
    /// field writes target `__self.field` directly.
    pub(crate) fn emit_wrapper_constructor(
        &mut self,
        class_decl: &juxc_ast::ClassDecl,
        ctor: &juxc_ast::ConstructorDecl,
    ) {
        // Emit two functions for each wrapper constructor:
        //
        //   - `new_inner(args) -> C_Inner` — builds the flattened inner
        //     struct. For a child class the `__parent` slot is built by
        //     recursively calling `Parent::new_inner(super_args)`, so a
        //     whole `extends` chain materializes one nested inner value
        //     in a single allocation (§CR.3.5).
        //   - `new(args) -> Self` — the public ctor; wraps the inner in
        //     `Rc::new(RefCell::new(...))`.
        //
        // Splitting them lets a subclass build its parent slice WITHOUT
        // double-wrapping (the parent's own `Rc<RefCell>` would split
        // identity). For a leaf simple class with no `extends`, the two
        // collapse to the obvious shape.
        self.emit_wrapper_inner_constructor(class_decl, ctor);

        // Thin public `new` delegating to `new_inner`.
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
        self.w.emit_indent();
        self.w.push_str("Self(std::rc::Rc::new(std::cell::RefCell::new(Self::new_inner(");
        for (i, param) in ctor.params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&param.name.text);
        }
        self.w.push_str("))))\n");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }

    /// Emit `fn new_inner(args) -> C_Inner` for a wrapper class — the
    /// function that builds the flattened inner struct (parent slice +
    /// own fields). See [`Self::emit_wrapper_constructor`] for why this
    /// is split out from the public `new`.
    fn emit_wrapper_inner_constructor(
        &mut self,
        class_decl: &juxc_ast::ClassDecl,
        ctor: &juxc_ast::ConstructorDecl,
    ) {
        let inner = format!("{}_Inner", class_decl.name.text);
        self.w.indent_inc();
        self.w.emit_indent();
        // `pub` so a subclass in another package can call
        // `Parent::new_inner(...)` to build its `__parent` slot.
        self.w.push_str("pub fn new_inner(");
        for (i, param) in ctor.params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_type_as_rust(&param.ty);
        }
        self.w.push_str(") -> ");
        self.w.push_str(&inner);
        // Thread the class's generic params onto the inner return type:
        // `pub fn new_inner(value: T) -> Box_Inner<T>`. `T` is in scope
        // because the enclosing `impl<T: Clone> Box<T>` declares it. The
        // `C_Inner { … }` literal in the body needs no turbofish — Rust
        // infers the args from the field initializers.
        self.emit_generic_params_as_args(&class_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();

        // Inside the ctor body the receiver is a plain `C_Inner`
        // (`__self`) — direct field access, NOT through `.0.borrow()`.
        // Suppress the wrapper rewrite so `this.f = v` lowers to
        // `__self.f = v`.
        let prev_wrapper = self.emitting_wrapper_class;
        self.emitting_wrapper_class = false;

        if let Some(simple) = extract_simple_ctor_inits(ctor) {
            // `C_Inner { __parent: Parent::new_inner(super_args), … }`.
            self.w.emit_indent();
            self.emit_wrapper_simple_ctor_inner(class_decl, &inner, &simple);
            self.w.push('\n');
        } else {
            // Fallback `__self`-builder. The `__parent` slot (when this
            // class extends another wrapper) is seeded with the parent's
            // `new_inner` so the parent slice is fully built before the
            // body's `this.field = …` writes run. Without an explicit
            // `super(...)` in the body, the parent ctor is called with
            // no args (works for parameterless parents; a clear Rust
            // error otherwise).
            self.w.emit_indent();
            self.w.push_str("let mut __self = ");
            self.w.push_str(&inner);
            self.w.push_str(" {\n");
            self.w.indent_inc();
            if let Some(parent_ty) = &class_decl.extends {
                if let Some(seg) = parent_ty.name.segments.first() {
                    self.w.emit_indent();
                    self.w.push_str("__parent: ");
                    self.w.push_str(&seg.text);
                    self.w.push_str("::new_inner(");
                    // Lift `super(args)` if present in the body.
                    if let Some(super_args) = extract_super_args(ctor) {
                        for (i, arg) in super_args.iter().enumerate() {
                            if i > 0 {
                                self.w.push_str(", ");
                            }
                            self.emit_expr(&arg);
                            // Wrapper-class share-on-pass (§CR.4.1): a
                            // wrapped place forwarded into the parent's
                            // `new_inner(...)` shares the instance.
                            if self.wrapper_value_needs_clone(arg) {
                                self.w.push_str(".clone()");
                            }
                        }
                    }
                    self.w.push_str("),\n");
                }
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
                    self.emit_field_default_value_for(&juxc_tycheck::resolved_field_type(field));
                }
                self.w.push_str(",\n");
            }
            self.w.indent_dec();
            self.w.line("};");

            self.this_alias = Some("__self".to_string());
            let mut muts = HashSet::new();
            collect_mutated_names(&ctor.body, &mut muts, &self.user_mut_methods);
            self.mutated_in_fn = muts;
            self.nullable_locals.clear();
            for p in &ctor.params {
                if p.ty.nullable {
                    self.nullable_locals.insert(p.name.text.clone());
                }
            }
            self.current_fn_params = ctor.params.iter().map(|p| p.name.text.clone()).collect();
            for stmt in &ctor.body.statements {
                self.emit_source_marker(stmt_span(stmt));
                self.w.emit_indent();
                self.emit_stmt(stmt);
            }
            self.current_fn_params.clear();
            self.this_alias = None;
            self.w.line("__self");
        }

        self.emitting_wrapper_class = prev_wrapper;
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }

    /// Emit the `C_Inner { field: expr, … }` literal for a simple
    /// wrapper-class constructor. Mirrors [`Self::emit_simple_ctor_body`]
    /// but writes the inner struct name and never has a `__parent`
    /// slot (wrapper classes are simple — no inheritance). Side-effect
    /// statements in the ctor body aren't supported on the simple
    /// path (the simple-ctor extractor only matches pure
    /// `this.field = expr;` sequences), so this is purely the literal.
    fn emit_wrapper_simple_ctor_inner(
        &mut self,
        class_decl: &juxc_ast::ClassDecl,
        inner: &str,
        simple: &SimpleCtorInits,
    ) {
        let mut chosen: std::collections::HashMap<&str, &juxc_ast::Expr> =
            std::collections::HashMap::new();
        for (name, expr) in &simple.inits {
            chosen.insert(name.as_str(), expr);
        }
        // Side-effect statements (e.g. a static-counter bump) run
        // before the literal — same ordering as the legacy
        // `emit_simple_ctor_body`. They're wrapped in a block that
        // yields the inner literal so the whole thing stays an
        // expression inside `RefCell::new(...)`.
        let has_side_effects = !simple.side_effects.is_empty();
        if has_side_effects {
            self.w.push_str("{ ");
            let side_effects = simple.side_effects.clone();
            for stmt in &side_effects {
                self.emit_stmt(stmt);
            }
        }
        self.w.push_str(inner);
        self.w.push_str(" {");
        let mut first = true;
        // `__parent: Parent::new_inner(super_args)` first when this
        // wrapper class extends another wrapper class (§CR.3.5). The
        // parent slice is built recursively so the whole chain lands in
        // one flattened inner. `super_args` come from the simple-ctor
        // extractor's lifted `super(...)` call; absent → no-arg parent
        // ctor (valid for parameterless parents).
        if let Some(parent_ty) = &class_decl.extends {
            if let Some(seg) = parent_ty.name.segments.first() {
                self.w.push_str(" __parent: ");
                self.w.push_str(&seg.text);
                self.w.push_str("::new_inner(");
                if let Some(super_args) = &simple.super_args {
                    let super_args = super_args.clone();
                    for (i, arg) in super_args.iter().enumerate() {
                        if i > 0 {
                            self.w.push_str(", ");
                        }
                        self.emit_expr(&arg);
                    }
                }
                self.w.push_str(")");
                first = false;
            }
        }
        for field in &class_decl.fields {
            if field.is_static {
                continue;
            }
            if first {
                self.w.push(' ');
            } else {
                self.w.push_str(", ");
            }
            first = false;
            if let Some(init_expr) = chosen.get(field.name.text.as_str()) {
                if init_is_same_named_ident(init_expr, &field.name.text) {
                    self.w.push_str(&field.name.text);
                    continue;
                }
                self.w.push_str(&field.name.text);
                self.w.push_str(": ");
                self.emit_expr(init_expr);
            } else if let Some(default) = &field.default {
                self.w.push_str(&field.name.text);
                self.w.push_str(": ");
                self.emit_expr(default);
            } else {
                self.w.push_str(&field.name.text);
                self.w.push_str(": ");
                self.emit_field_default_value_for(&juxc_tycheck::resolved_field_type(field));
            }
        }
        if first {
            // No instance fields — emit `C_Inner {}`.
            self.w.push_str("}");
        } else {
            self.w.push_str(" }");
        }
        if has_side_effects {
            self.w.push_str(" }");
        }
    }

    /// Synthesize a zero-arg default constructor for a wrapper-shape
    /// class that declared none. Builds an empty/`default`-filled
    /// `C_Inner` and wraps it.
    pub(crate) fn emit_wrapper_synthetic_default_constructor(
        &mut self,
        class_decl: &juxc_ast::ClassDecl,
    ) {
        let inner = format!("{}_Inner", class_decl.name.text);
        // `new_inner() -> C_Inner` — builds the empty/`default`-filled
        // inner. A `__parent` slot (when the class extends another
        // wrapper) is seeded with the parent's no-arg `new_inner()`.
        self.w.indent_inc();
        self.w.emit_indent();
        self.w.push_str("pub fn new_inner() -> ");
        self.w.push_str(&inner);
        // Thread generic params onto the inner return type, same as the
        // explicit-ctor path (`pub fn new_inner() -> Box_Inner<T>`).
        self.emit_generic_params_as_args(&class_decl.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        self.w.emit_indent();
        self.w.push_str(&inner);
        self.w.push_str(" {");
        let mut first = true;
        if let Some(parent_ty) = &class_decl.extends {
            if let Some(seg) = parent_ty.name.segments.first() {
                self.w.push_str(" __parent: ");
                self.w.push_str(&seg.text);
                self.w.push_str("::new_inner()");
                first = false;
            }
        }
        for field in &class_decl.fields {
            if field.is_static {
                continue;
            }
            if first {
                self.w.push(' ');
            } else {
                self.w.push_str(", ");
            }
            first = false;
            self.w.push_str(&field.name.text);
            self.w.push_str(": ");
            if let Some(default) = &field.default {
                self.emit_expr(default);
            } else {
                self.emit_field_default_value_for(&juxc_tycheck::resolved_field_type(field));
            }
        }
        if first {
            self.w.push_str("}");
        } else {
            self.w.push_str(" }");
        }
        self.w.push('\n');
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();

        // Thin public `new()` → wrap `new_inner()`.
        self.w.indent_inc();
        self.w.line("pub fn new() -> Self {");
        self.w.indent_inc();
        self.w.line("Self(std::rc::Rc::new(std::cell::RefCell::new(Self::new_inner())))");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
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
        // Sealed-parent skip: subclasses of sealed have no
        // `__parent` slot to initialize.
        let parent_is_sealed = class_decl
            .extends
            .as_ref()
            .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
            .and_then(|bare| self.lookup_class_by_bare_or_fqn(bare).map(|c| c.is_sealed))
            .unwrap_or(false);
        // Inherited parent — invoke the parent's zero-arg constructor.
        // For parents whose ctor takes arguments, the user MUST declare
        // an explicit constructor with `super(args);`; the synthetic
        // path is only valid for trivially-defaulted hierarchies.
        if let Some(parent_ty) = &class_decl.extends {
            if !parent_is_sealed {
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
                self.emit_field_default_value_for(&juxc_tycheck::resolved_field_type(field));
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
