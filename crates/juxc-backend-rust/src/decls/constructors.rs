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
/// First-statement `this(args)` delegation (§7.3.1), if present.
/// Returns the delegated argument list and the remaining body
/// statements (everything after the delegation). Tycheck enforces
/// the first-statement rule, so only index 0 is consulted.
fn extract_this_delegation(
    ctor: &juxc_ast::ConstructorDecl,
) -> Option<(Vec<Expr>, Vec<juxc_ast::Stmt>)> {
    let first = ctor.body.statements.first()?;
    if let juxc_ast::Stmt::Expr(Expr::Call(call)) = first {
        if matches!(call.callee.as_ref(), Expr::This(_)) {
            return Some((call.args.clone(), ctor.body.statements[1..].to_vec()));
        }
    }
    None
}

fn extract_super_args(ctor: &juxc_ast::ConstructorDecl) -> Option<Vec<Expr>> {
    for stmt in &ctor.body.statements {
        if let juxc_ast::Stmt::SuperCall(args, _) = stmt {
            return Some(args.clone());
        }
    }
    None
}

/// Names of constructor parameters whose type is OWNED at the Rust
/// level (String, arrays) — assigning one to a field moves it, so a
/// later read in the same body needs the assignment to `.clone()`.
fn ctor_owned_param_names(params: &[juxc_ast::Param]) -> HashSet<String> {
    params
        .iter()
        .filter(|p| {
            p.ty.array_shape.is_some()
                || p.ty
                    .name
                    .segments
                    .last()
                    .is_some_and(|s| s.text == "String")
        })
        .map(|p| p.name.text.clone())
        .collect()
}

impl RustEmitter {
    /// Emit a constructor body's statements with per-statement
    /// liveness for owned parameters: before each statement, record
    /// which owned params are still read by a LATER statement, so the
    /// assignment emitter can `.clone()` instead of moving
    /// (`this.name = name; print(name);` would otherwise be a
    /// use-after-move in the emitted Rust).
    fn emit_ctor_body_stmts(
        &mut self,
        stmts: &[juxc_ast::Stmt],
        owned_params: &HashSet<String>,
    ) {
        for (i, stmt) in stmts.iter().enumerate() {
            let mut live = HashSet::new();
            if !owned_params.is_empty() && i + 1 < stmts.len() {
                let scratch = juxc_ast::Block {
                    statements: stmts[i + 1..].to_vec(),
                    span: juxc_source::Span::DUMMY,
                };
                crate::exprs::collect_bare_names_block(&scratch, &mut |name| {
                    if owned_params.contains(name) {
                        live.insert(name.to_string());
                    }
                });
            }
            self.ctor_live_after = live;
            self.emit_source_marker(stmt_span(stmt));
            self.w.emit_indent();
            self.emit_stmt(stmt);
        }
        self.ctor_live_after.clear();
    }

    /// Constructor-overload **name suffix** for a call with
    /// `arg_count` arguments against `class_name` (bare or FQN):
    /// `""` for the first (or only) constructor, `"__K"` for overload
    /// K (§7.3.1 Phase-1 count-based selection). Re-derived here
    /// rather than threaded from tycheck because every transformed
    /// call (named-arg expansion, default filling, varargs packing)
    /// lands on a count INSIDE the same constructor's accepted range
    /// — ranges are validated pairwise-disjoint at the declaration.
    /// Span-aware constructor-overload suffix: prefers tycheck's
    /// recorded TYPED pick (§T.3 applied to constructors — S19, so
    /// same-count overloads like `Point(int)` / `Point(String)`
    /// dispatch by argument type), falling back to the count rule for
    /// synthesized calls the checker didn't visit.
    pub(crate) fn ctor_overload_suffix_for_span(
        &self,
        class_name: &str,
        arg_count: usize,
        span: juxc_source::Span,
    ) -> String {
        if let Some(&k) = self.symbols.ctor_selections.get(&span) {
            return if k == 0 {
                String::new()
            } else {
                format!("__{k}")
            };
        }
        self.ctor_overload_suffix(class_name, arg_count)
    }

    pub(crate) fn ctor_overload_suffix(&self, class_name: &str, arg_count: usize) -> String {
        let class = self
            .symbols
            .classes
            .get(class_name)
            .or_else(|| {
                self.symbols
                    .find_fqn_by_bare(class_name)
                    .and_then(|fqn| self.symbols.classes.get(&fqn))
            });
        let Some(class) = class else { return String::new() };
        if class.constructors.len() <= 1 {
            return String::new();
        }
        let idx = class.constructors.iter().position(|c| {
            let required = c
                .params
                .iter()
                .filter(|p| p.default.is_none() && !p.is_varargs)
                .count();
            let max_ok = c.params.last().is_some_and(|p| p.is_varargs)
                || arg_count <= c.params.len();
            arg_count >= required && max_ok
        });
        match idx {
            Some(0) | None => String::new(),
            Some(k) => format!("__{k}"),
        }
    }

    /// Emit the parent's CONSTRUCTOR base path for a `__parent:` init —
    /// the path only, no generic args (`Parent::new(...)` infers them
    /// from the field's declared type; `Parent<int>::new` would be
    /// invalid Rust anyway). A cross-package parent gets the
    /// `crate::a::b::Name` rooting (`extends Exception` →
    /// `crate::jux::std::exceptions::Exception`), mirroring the bare-
    /// name resolution `emit_type_as_rust` does for type positions.
    pub(crate) fn emit_parent_ctor_base_path(&mut self, parent_ty: &juxc_ast::TypeRef) {
        if parent_ty.name.segments.len() > 1 {
            let joined = parent_ty
                .name
                .segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join("::");
            self.w.push_str("crate::");
            self.w.push_str(&joined);
            return;
        }
        let Some(seg) = parent_ty.name.segments.first() else { return };
        let bare = seg.text.as_str();
        if let Some(fqn) = self.symbols.find_fqn_by_bare(bare) {
            if fqn.contains('.') {
                let cur_pkg = self.symbols.package.join(".");
                let fqn_pkg = fqn
                    .rsplit_once('.')
                    .map(|(p, _)| p.to_string())
                    .unwrap_or_default();
                if fqn_pkg != cur_pkg {
                    self.w.push_str("crate::");
                    self.w
                        .push_str(&fqn.split('.').collect::<Vec<_>>().join("::"));
                    return;
                }
            }
        }
        self.w.push_str(bare);
    }

    /// Emit the argument list for a `super(args)` call, resolving the parent
    /// constructor's parameter nullable flags so non-null values passed to a
    /// `T?` parent parameter are automatically wrapped in `Some(…)`.
    ///
    /// Both the simple-ctor path and the fallback `__self`-builder path share
    /// this helper so the fix is in one place.
    fn emit_super_call_args(
        &mut self,
        parent_ty: &juxc_ast::TypeRef,
        args: &[juxc_ast::Expr],
    ) {
        let parent_bare = parent_ty
            .name
            .segments
            .last()
            .map(|s| s.text.clone())
            .unwrap_or_default();
        // Look up the overload whose arity matches; fall back to the first.
        let param_nullables: Vec<bool> = self
            .lookup_class_by_bare_or_fqn(&parent_bare)
            .and_then(|c| {
                c.constructors
                    .iter()
                    .find(|ctor| ctor.params.len() == args.len())
                    .or_else(|| c.constructors.first())
                    .map(|ctor| ctor.params.iter().map(|p| p.ty.nullable).collect())
            })
            .unwrap_or_default();
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            let nullable = param_nullables.get(i).copied().unwrap_or(false);
            self.emit_arg_with_nullable_wrap(arg, nullable);
            // Wrapper share-on-pass: a wrapped place into a non-nullable slot
            // needs the Rc clone so it becomes a shared handle, not a move.
            if !nullable && self.wrapper_value_needs_clone(arg) {
                self.w.push_str(".clone()");
            }
        }
    }

    /// Emit a user-declared constructor as `pub fn new(...) -> Self`.
    /// Caller (`emit_class_decl`) has the writer at level 0; the ctor
    /// signature lives at depth 1 (inside the class's `impl` block),
    /// and the body at depth 2.
    pub(crate) fn emit_constructor(
        &mut self,
        class_decl: &juxc_ast::ClassDecl,
        ctor: &juxc_ast::ConstructorDecl,
        ctor_idx: usize,
    ) {
        // (Migrated to Writer indent-aware API)
        // Caller (`emit_class_decl`) is at level 0; the ctor signature
        // sits at depth 1 (inside the `impl` block), and the body at
        // depth 2.
        self.w.indent_inc();
        self.w.emit_indent();
        self.emit_visibility(ctor.visibility);
        self.w.push_str("fn new");
        if ctor_idx > 0 {
            self.w.push_str(&format!("__{ctor_idx}"));
        }
        self.w.push('(');
        for (i, param) in ctor.params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_value_type_as_rust(&param.ty);
        }
        self.w.push_str(") -> Self {\n");
        self.w.indent_inc();
        // First-use trigger for `static { }` blocks (§S.4.1) — construction
        // is an observable use, so run the once-guarded static init here.
        self.emit_static_init_trigger();

        // **`this(...)` delegation** (§7.3.1): run the sibling
        // constructor first, then the rest of this body against the
        // produced value. `let mut __self = Self::new__K(args);
        // <rest>; __self` — the K pick is count-based, same rule as
        // every other constructor call site.
        if let Some((delegate_args, rest)) = extract_this_delegation(ctor) {
            self.emit_ctor_delegation(
                class_decl,
                &delegate_args,
                &rest,
                "new",
            );
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
            self.w.indent_dec();
            return;
        }

        // Try the **simple-ctor fast path** first: when every statement
        // in the body is `this.field = expr;` (with an optional leading
        // `super(args);`), collapse to a direct `Self { field: expr, … }`
        // literal. Idiomatic Rust, AND it sidesteps the "need `Default`
        // for generic-typed fields" problem inherent to the fallback
        // `__self`-builder pattern.
        //
        // A class with `init { }` blocks (§M.1) still uses the simple inits when
        // the ctor is simple — but binds them to `let mut __self` so the init
        // blocks can mutate `this` afterward. This keeps generic-typed fields
        // working (the explicit inits avoid the `__self`-builder's `Default`
        // requirement); only a NON-simple ctor body falls to the builder.
        // A class with `init { }` blocks can't use the simple fast path:
        // the fast path folds the body's `this.f = param` assignments into
        // the struct literal, but §S.4.4 / ERRATA E2 order init blocks
        // BEFORE the constructor body (Java's instance-initializer rule),
        // so an init block must observe the field-initializer values, not
        // the body's writes. The `__self`-builder below gets that order.
        let simple = if class_decl.init_blocks.is_empty() {
            extract_simple_ctor_inits(ctor)
        } else {
            None
        };
        if let Some(simple) = simple {
            // Seed nullable-locals from this constructor's `T?` params so a
            // simple `this.data = d;` of a `T?` param into a `T?` field doesn't
            // double-wrap (`Some(Some(d))`). `expression_is_already_nullable`
            // (consulted by `emit_ctor_field_init`) reads this set. The
            // `__self`-builder fallback below already seeds it; the simple-ctor
            // fast path was missing the step (gap N6).
            self.nullable_locals.clear();
            for p in &ctor.params {
                if p.ty.nullable {
                    self.nullable_locals.insert(p.name.text.clone());
                }
            }
            // Raw-pointer params (§L.6): reset + seed for the `p == null` peephole.
            self.seed_pointer_params(&ctor.params);
            self.emit_simple_ctor_body(class_decl, &simple, false);
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
            self.w.indent_dec();
            return;
        }

        // Fallback: the body has stmts other than this.field-init (e.g.,
        // a conditional mixed in). Use the `__self` builder pattern,
        // which requires fields without explicit init to be
        // `Default`-initialized — fine for primitives, breaks for
        // unconstrained generic types. The user has to keep the ctor
        // body simple in that case.
        self.w.line("let mut __self = Self {");
        self.w.indent_inc();
        // Emit the `__parent` slot first when the class has a non-sealed
        // parent, exactly like `emit_simple_ctor_body` does. Without this,
        // the `__parent` field is missing from the struct literal and rustc
        // emits E0063 (e.g. an Exception subclass with a complex ctor body).
        let parent_is_sealed = class_decl
            .extends
            .as_ref()
            .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
            .and_then(|bare| self.lookup_class_by_bare_or_fqn(bare).map(|c| c.is_sealed))
            .unwrap_or(false);
        if let Some(parent_ty) = &class_decl.extends {
            if !parent_is_sealed {
                let super_args = extract_super_args(ctor);
                self.w.emit_indent();
                self.w.push_str("__parent: ");
                self.emit_parent_ctor_base_path(parent_ty);
                let parent_bare = parent_ty
                    .name
                    .segments
                    .last()
                    .map(|s| s.text.clone())
                    .unwrap_or_default();
                let n_super = super_args.as_ref().map_or(0, |a| a.len());
                let sfx = self.ctor_overload_suffix(&parent_bare, n_super);
                self.w.push_str("::new");
                self.w.push_str(&sfx);
                self.w.push('(');
                if let Some(args) = super_args {
                    let parent_ty_clone = parent_ty.clone();
                    self.emit_super_call_args(&parent_ty_clone, &args);
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
            // `ref` field (§M.13): seed a fresh shared cell around the
            // default value — ctor-body assignments store through it.
            if field.is_ref {
                self.w.push_str("std::rc::Rc::new(std::cell::RefCell::new(");
            }
            if let Some(default) = &field.default {
                self.emit_expr(default);
            } else {
                self.emit_field_storage_default(field);
            }
            if field.is_ref {
                self.w.push_str("))");
            }
            self.w.push_str(",\n");
        }
        // PhantomData inits for type params carried only as phantom fields.
        self.emit_phantom_field_inits(class_decl);
        self.w.indent_dec();
        self.w.line("};");

        // Body — `this` rewrites to `__self`.
        self.this_alias = Some("__self".to_string());
        let mut muts = HashSet::new();
        collect_mutated_names(&ctor.body, &mut muts, &self.user_mut_methods);
        // Init blocks (§M.1) run in the same `fn new` scope, so their local
        // reassignments must be in the `let mut` set too.
        for init in &class_decl.init_blocks {
            collect_mutated_names(init, &mut muts, &self.user_mut_methods);
        }
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
        // Raw-pointer params (§L.6): reset + seed for the `p == null` peephole.
        self.seed_pointer_params(&ctor.params);
        // §S.4.4 step 4 / ERRATA E2: run every `init { }` block in source
        // order BEFORE the constructor body (Java's instance-initializer
        // order — init blocks see field-initializer values, not the body's
        // writes). Constructor params are out of scope inside init blocks
        // (a block is shared across all constructors), so the
        // `current_fn_params` shadow set stays empty for this pass.
        for init in &class_decl.init_blocks {
            for stmt in &init.statements {
                self.emit_source_marker(stmt_span(stmt));
                self.w.emit_indent();
                self.emit_stmt(stmt);
            }
        }

        // §S.4.4 step 5: the constructor body. Params shadow same-named
        // fields (the canonical `Other(String test){ this.test = test; }`
        // shape), so they must NOT be rewritten by the implicit-`this` pass.
        self.current_fn_params = ctor.params.iter().map(|p| p.name.text.clone()).collect();
        let owned = ctor_owned_param_names(&ctor.params);
        self.emit_ctor_body_stmts(&ctor.body.statements, &owned);
        self.current_fn_params.clear();
        self.this_alias = None;

        // Return the constructed value.
        self.w.line("__self");
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }

    /// Emit a single field's initializer inside a `Self { … }` /
    /// `C_Inner { … }` literal, coercing the value into the field's declared
    /// slot. Mirrors the var-init / assign coercion so a `Shape`/poly-base
    /// field gets the `Rc<dyn …>` wrap and a nullable field gets its `Some(…)`:
    ///
    /// - interface / polymorphic-base field → [`Self::emit_expr_coerced_to_iface`]
    ///   (which owns the `Some(…)` when the field is a *nullable* dyn slot);
    /// - plain nullable field (`int? x`, `Dog? d`) → `Some(<value>)` for a
    ///   non-null, not-already-`Option` value;
    /// - everything else → the value verbatim.
    pub(crate) fn emit_ctor_field_init(
        &mut self,
        field_ty: Option<&juxc_ast::TypeRef>,
        init: &juxc_ast::Expr,
    ) {
        if let Some(fty) = field_ty {
            if !matches!(
                self.iface_coercion_to(fty, init),
                crate::analysis::IfaceCoercion::None,
            ) {
                self.emit_expr_coerced_to_iface(fty, init);
                return;
            }
            // A `null` initializer for a raw-pointer field (`byte* ptr;` →
            // `this.ptr = null;`) lowers to Rust's null pointer, not `None`
            // (§L.6.1: `null` is the sole `T*` literal).
            if fty.ptr_depth > 0 && crate::stmts::is_null_literal(init) {
                self.w.push_str("std::ptr::null_mut()");
                return;
            }
            if fty.nullable
                && !crate::stmts::is_null_literal(init)
                && !self.expression_is_already_nullable(init)
            {
                self.w.push_str("Some(");
                self.emit_expr(init);
                self.w.push(')');
                return;
            }
        }
        self.emit_expr(init);
    }

    /// Emit `__phantom_<name>: std::marker::PhantomData,` init lines for
    /// every type param of `class_decl` that the struct carries only as a
    /// phantom field (see [`crate::unused_class_type_params`]). Called from
    /// every `Self { … }` / `<Name>_Inner { … }` literal path so the
    /// synthesized phantom field is always initialized. Emits nothing when
    /// the class has no phantom params. Each line is written at the current
    /// indent via `emit_indent`, matching the surrounding field inits.
    pub(crate) fn emit_phantom_field_inits(&mut self, class_decl: &juxc_ast::ClassDecl) {
        for phantom in crate::unused_class_type_params(class_decl) {
            self.w.emit_indent();
            self.w.push_str("__phantom_");
            self.w.push_str(&phantom);
            self.w.push_str(": std::marker::PhantomData,\n");
        }
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
        // When the class has `init { }` blocks, bind the literal to
        // `let mut __self = Self { … };` so the caller can run the init blocks
        // against it. Crucially this uses the constructor's EXPLICIT field
        // inits (not `Default`), so a generic-typed field (`T value`) doesn't
        // require `T: Default` the way the `__self`-builder fallback does.
        bind_to_self: bool,
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

        if bind_to_self {
            self.w.line("let mut __self = Self {");
        } else {
            self.w.line("Self {");
        }
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
                // Emit only the parent's path here, not the full `<...>`
                // instantiation. The `__parent` field declaration already
                // pins the parent's generic args, so Rust infers them at
                // the call site — and `Parent<int>::new(...)` is invalid
                // Rust syntax anyway (would need the turbofish form
                // `Parent::<int>::new`). The helper crate-roots a
                // cross-package parent (`extends Exception` →
                // `crate::jux::std::exceptions::Exception`).
                self.emit_parent_ctor_base_path(parent_ty);
                let parent_bare = parent_ty
                    .name
                    .segments
                    .last()
                    .map(|s| s.text.clone())
                    .unwrap_or_default();
                let n_super = simple.super_args.as_ref().map_or(0, |a| a.len());
                let sfx = self.ctor_overload_suffix(&parent_bare, n_super);
                self.w.push_str("::new");
                self.w.push_str(&sfx);
                self.w.push('(');
                // If the constructor wrote `super(args);`, lift those args
                // here. If it didn't, Phase 1 calls `Parent::new()` with
                // no arguments — fine for parameterless parents, breaks
                // (with a clear Rust error) when the parent's ctor needs
                // arguments and the user forgot to write `super(...)`.
                if let Some(args) = &simple.super_args {
                    // Clone to release the borrow on `simple` before the
                    // emit calls (which need `&mut self`).
                    let args = args.clone();
                    let parent_ty_clone = parent_ty.clone();
                    // Use the helper so non-null args for a nullable
                    // parent-ctor parameter are wrapped in `Some(…)`.
                    self.emit_super_call_args(&parent_ty_clone, &args);
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
            // `ref` field (§M.13): every init shape wraps into a fresh
            // shared cell — the same-name shorthand can't apply (the
            // param is the VALUE, the field is the cell).
            if field.is_ref {
                self.w.push_str(&field.name.text);
                self.w.push_str(": std::rc::Rc::new(std::cell::RefCell::new(");
                if let Some(init_expr) = chosen.get(field.name.text.as_str()) {
                    self.emit_ctor_field_init(field.ty.as_ref(), init_expr);
                    // A place init (ctor param / field) may be used by
                    // a LATER field init too — the cell takes a copy.
                    if matches!(init_expr, Expr::Path(_) | Expr::Field(_)) {
                        self.w.push_str(".clone()");
                    }
                } else if let Some(default) = &field.default {
                    self.emit_ctor_field_init(field.ty.as_ref(), default);
                } else {
                    self.emit_field_storage_default(field);
                }
                self.w.push_str(")),
");
                continue;
            }
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
                self.emit_ctor_field_init(field.ty.as_ref(), init_expr);
            } else if let Some(default) = &field.default {
                self.w.push_str(&field.name.text);
                self.w.push_str(": ");
                self.emit_ctor_field_init(field.ty.as_ref(), default);
            } else {
                self.w.push_str(&field.name.text);
                self.w.push_str(": ");
                // No assignment and no source default — fall back to
                // the type's natural default. Generic-typed fields
                // will surface a Rust compile error here, signaling
                // the user has to assign them in the constructor body.
                self.emit_field_storage_default(field);
            }
            self.w.push_str(",\n");
        }
        // PhantomData inits for type params carried only as phantom fields.
        self.emit_phantom_field_inits(class_decl);
        self.w.indent_dec();
        if bind_to_self {
            self.w.line("};");
        } else {
            self.w.line("}");
        }
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
        ctor_idx: usize,
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
        self.emit_wrapper_inner_constructor(class_decl, ctor, ctor_idx);
        let suffix = if ctor_idx > 0 { format!("__{ctor_idx}") } else { String::new() };
        // P6 (§P.9): binds on `this` recorded while the inner ctor's
        // body emitted — replayed below, after the wrapper exists.
        let ctor_binds = std::mem::take(&mut self.pending_ctor_binds);

        // Thin public `new` delegating to `new_inner`.
        self.w.indent_inc();
        self.w.emit_indent();
        self.emit_visibility(ctor.visibility);
        self.w.push_str("fn new");
        self.w.push_str(&suffix);
        self.w.push('(');
        for (i, param) in ctor.params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_value_type_as_rust(&param.ty);
        }
        self.w.push_str(") -> Self {\n");
        self.w.indent_inc();
        self.emit_static_init_trigger();
        self.w.emit_indent();
        // Wrap by rep (§CR.3.3 / §CR.4.1): `Rc::new(RefCell::new(..))` for the
        // interior-mutable rep, plain `Rc::new(..)` for read-only-shared `Rc`, or
        // `Box::new(..)` for the unique-owner `Box` rep.
        let (wrap_open, wrap_close): (&str, &str) =
            if self.box_classes.contains(&class_decl.name.text) {
                ("std::boxed::Box::new(", ")")
            } else if self.refcell_classes.contains(&class_decl.name.text) {
                ("std::rc::Rc::new(std::cell::RefCell::new(", "))")
            } else {
                ("std::rc::Rc::new(", ")")
            };
        if ctor_binds.is_empty() {
            self.w.push_str("Self(");
            self.w.push_str(wrap_open);
            self.w.push_str("Self::new_inner");
            self.w.push_str(&suffix);
            self.w.push('(');
            for (i, param) in ctor.params.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.w.push_str(&param.name.text);
            }
            self.w.push_str(")");
            self.w.push_str(wrap_close);
            self.w.push_str(")\n");
        } else {
            // Bind the wrapped instance to a local so the deferred
            // binds can hold it; params are CLONED into `new_inner`
            // (cheap `Rc` bumps / value copies) because a bind's
            // source receiver may name one of them.
            self.w.push_str("let __jux_self = Self(");
            self.w.push_str(wrap_open);
            self.w.push_str("Self::new_inner");
            self.w.push_str(&suffix);
            self.w.push('(');
            for (i, param) in ctor.params.iter().enumerate() {
                if i > 0 {
                    self.w.push_str(", ");
                }
                self.w.push_str(&param.name.text);
                self.w.push_str(".clone()");
            }
            self.w.push_str(")");
            self.w.push_str(wrap_close);
            self.w.push_str(");\n");
            // Replay each deferred bind with `this` resolved to the
            // fresh wrapper handle.
            let prev_alias = self.this_alias.replace("__jux_self".to_string());
            for b in &ctor_binds {
                self.w.emit_indent();
                self.emit_bind(
                    (b.target.0.as_ref(), &b.target.1, &b.target.2),
                    (b.source.0.as_ref(), &b.source.1, &b.source.2),
                    b.bidirectional,
                );
                self.w.push_str(";\n");
            }
            self.this_alias = prev_alias;
            self.w.line("__jux_self");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }

    /// Shared body of a delegating constructor: bind the sibling's
    /// result to `__self`, run the remaining statements with `this`
    /// aliased to it, and yield it. `base_fn` is `"new"` (plain
    /// classes) or `"new_inner"` (wrapper inner ctors).
    fn emit_ctor_delegation(
        &mut self,
        class_decl: &juxc_ast::ClassDecl,
        delegate_args: &[Expr],
        rest: &[juxc_ast::Stmt],
        base_fn: &str,
    ) {
        let sfx = self.ctor_overload_suffix(&class_decl.name.text, delegate_args.len());
        self.w.emit_indent();
        if rest.is_empty() {
            // Pure delegation — no binding needed.
            self.w.push_str("Self::");
        } else {
            self.w.push_str("let mut __self = Self::");
        }
        self.w.push_str(base_fn);
        self.w.push_str(&sfx);
        self.w.push('(');
        for (i, arg) in delegate_args.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.emit_expr(arg);
            if self.wrapper_value_needs_clone(arg) {
                self.w.push_str(".clone()");
            }
        }
        self.w.push(')');
        if rest.is_empty() {
            self.w.push('\n');
            return;
        }
        self.w.push_str(";\n");
        // Remaining statements run against the delegated value.
        let prev_alias = self.this_alias.take();
        self.this_alias = Some("__self".to_string());
        let mut muts = HashSet::new();
        let scratch = juxc_ast::Block {
            statements: rest.to_vec(),
            span: class_decl.span,
        };
        collect_mutated_names(&scratch, &mut muts, &self.user_mut_methods);
        let prev_muts = std::mem::replace(&mut self.mutated_in_fn, muts);
        for stmt in rest {
            self.emit_source_marker(stmt_span(stmt));
            self.w.emit_indent();
            self.emit_stmt(stmt);
        }
        self.mutated_in_fn = prev_muts;
        self.this_alias = prev_alias;
        self.w.line("__self");
    }

    /// Emit `fn new_inner(args) -> C_Inner` for a wrapper class — the
    /// function that builds the flattened inner struct (parent slice +
    /// own fields). See [`Self::emit_wrapper_constructor`] for why this
    /// is split out from the public `new`.
    fn emit_wrapper_inner_constructor(
        &mut self,
        class_decl: &juxc_ast::ClassDecl,
        ctor: &juxc_ast::ConstructorDecl,
        ctor_idx: usize,
    ) {
        let inner = format!("{}_Inner", class_decl.name.text);
        self.w.indent_inc();
        self.w.emit_indent();
        // `pub` so a subclass in another package can call
        // `Parent::new_inner(...)` to build its `__parent` slot.
        self.w.push_str("pub fn new_inner");
        if ctor_idx > 0 {
            self.w.push_str(&format!("__{ctor_idx}"));
        }
        self.w.push('(');
        for (i, param) in ctor.params.iter().enumerate() {
            if i > 0 {
                self.w.push_str(", ");
            }
            self.w.push_str(&param.name.text);
            self.w.push_str(": ");
            self.emit_value_type_as_rust(&param.ty);
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

        // **`this(...)` delegation** at the inner level: the sibling's
        // `new_inner__K` builds the same flattened inner struct, then
        // the rest of this body mutates it.
        if let Some((delegate_args, rest)) = extract_this_delegation(ctor) {
            self.emit_ctor_delegation(
                class_decl,
                &delegate_args,
                &rest,
                "new_inner",
            );
            self.emitting_wrapper_class = prev_wrapper;
            self.w.indent_dec();
            self.w.line("}");
            self.w.newline();
            self.w.indent_dec();
            return;
        }

        // A class with `init { }` blocks can't use the simple fast path —
        // its init blocks run after construction and mutate `this`.
        let simple = if class_decl.init_blocks.is_empty() {
            extract_simple_ctor_inits(ctor)
        } else {
            None
        };
        if let Some(simple) = simple {
            // Seed nullable-locals from this constructor's `T?` params, exactly
            // as the inline simple-ctor (~:264) and `__self`-builder (~:848)
            // paths do, so `emit_ctor_field_init` → `expression_is_already_nullable`
            // sees a `T?` param assigned to a `T?` field and does NOT re-wrap it
            // (`Some(Some(d))`). Without this the wrapped builder double-`Some`s a
            // nullable generic field initialized from a nullable ctor param.
            self.nullable_locals.clear();
            for p in &ctor.params {
                if p.ty.nullable {
                    self.nullable_locals.insert(p.name.text.clone());
                }
            }
            // Raw-pointer params (§L.6): reset + seed for the `p == null` peephole.
            self.seed_pointer_params(&ctor.params);
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
                {
                    self.w.emit_indent();
                    self.w.push_str("__parent: ");
                    self.emit_parent_ctor_base_path(parent_ty);
                    let parent_bare = parent_ty
                        .name
                        .segments
                        .last()
                        .map(|s| s.text.clone())
                        .unwrap_or_default();
                    let n_super = extract_super_args(ctor).map_or(0, |a| a.len());
                    let sfx = self.ctor_overload_suffix(&parent_bare, n_super);
                    self.w.push_str("::new_inner");
                    self.w.push_str(&sfx);
                    self.w.push('(');
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
                // `ref` field (§M.13): seed a fresh shared cell.
                if field.is_ref {
                    self.w.push_str("std::rc::Rc::new(std::cell::RefCell::new(");
                }
                if let Some(default) = &field.default {
                    self.emit_expr(default);
                } else {
                    self.emit_field_storage_default(field);
                }
                if field.is_ref {
                    self.w.push_str("))");
                }
                self.w.push_str(",\n");
            }
            // §P observer storage starts unallocated.
            self.emit_observer_field_inits_lines(class_decl);
            // PhantomData inits for phantom type params (block-literal form).
            self.emit_phantom_field_inits(class_decl);
            self.w.indent_dec();
            self.w.line("};");

            self.this_alias = Some("__self".to_string());
            let mut muts = HashSet::new();
            collect_mutated_names(&ctor.body, &mut muts, &self.user_mut_methods);
            for init in &class_decl.init_blocks {
                collect_mutated_names(init, &mut muts, &self.user_mut_methods);
            }
            self.mutated_in_fn = muts;
            self.nullable_locals.clear();
            for p in &ctor.params {
                if p.ty.nullable {
                    self.nullable_locals.insert(p.name.text.clone());
                }
            }
            // Raw-pointer params (§L.6): reset + seed for the `p == null` peephole.
            self.seed_pointer_params(&ctor.params);
            // §S.4.4 step 4 / ERRATA E2: init blocks run BEFORE the ctor
            // body (Java's instance-initializer order). Ctor params are
            // not in scope inside init blocks, so the shadow set stays
            // empty for this pass.
            for init in &class_decl.init_blocks {
                for stmt in &init.statements {
                    self.emit_source_marker(stmt_span(stmt));
                    self.w.emit_indent();
                    self.emit_stmt(stmt);
                }
            }
            // §S.4.4 step 5: the constructor body. `this` → __self.
            self.current_fn_params = ctor.params.iter().map(|p| p.name.text.clone()).collect();
            let owned = ctor_owned_param_names(&ctor.params);
            self.emit_ctor_body_stmts(&ctor.body.statements, &owned);
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
            {
                self.w.push_str(" __parent: ");
                self.emit_parent_ctor_base_path(parent_ty);
                let parent_bare = parent_ty
                    .name
                    .segments
                    .last()
                    .map(|s| s.text.clone())
                    .unwrap_or_default();
                let n_super = simple.super_args.as_ref().map_or(0, |a| a.len());
                let sfx = self.ctor_overload_suffix(&parent_bare, n_super);
                self.w.push_str("::new_inner");
                self.w.push_str(&sfx);
                self.w.push('(');
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
            // `ref` field (§M.13): wrap into a fresh shared cell.
            if field.is_ref {
                self.w.push_str(&field.name.text);
                self.w.push_str(": std::rc::Rc::new(std::cell::RefCell::new(");
                if let Some(init_expr) = chosen.get(field.name.text.as_str()) {
                    self.emit_ctor_field_init(field.ty.as_ref(), init_expr);
                    // A place init (ctor param / field) may be used by
                    // a LATER field init too — the cell takes a copy.
                    if matches!(init_expr, Expr::Path(_) | Expr::Field(_)) {
                        self.w.push_str(".clone()");
                    }
                } else if let Some(default) = &field.default {
                    self.emit_ctor_field_init(field.ty.as_ref(), default);
                } else {
                    self.emit_field_storage_default(field);
                }
                self.w.push_str("))");
                continue;
            }
            if let Some(init_expr) = chosen.get(field.name.text.as_str()) {
                if init_is_same_named_ident(init_expr, &field.name.text) {
                    self.w.push_str(&field.name.text);
                    continue;
                }
                self.w.push_str(&field.name.text);
                self.w.push_str(": ");
                self.emit_ctor_field_init(field.ty.as_ref(), init_expr);
            } else if let Some(default) = &field.default {
                self.w.push_str(&field.name.text);
                self.w.push_str(": ");
                self.emit_ctor_field_init(field.ty.as_ref(), default);
            } else {
                self.w.push_str(&field.name.text);
                self.w.push_str(": ");
                self.emit_field_storage_default(field);
            }
        }
        // §P observer storage starts unallocated.
        self.emit_observer_field_inits_inline(class_decl, &mut first);
        // PhantomData inits (inline form) for phantom type params.
        for phantom in crate::unused_class_type_params(class_decl) {
            if first {
                self.w.push(' ');
            } else {
                self.w.push_str(", ");
            }
            first = false;
            self.w.push_str("__phantom_");
            self.w.push_str(&phantom);
            self.w.push_str(": std::marker::PhantomData");
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
        // A class with `init { }` blocks binds the inner to `let mut __self`
        // so the init pass can mutate it before returning (§M.1).
        let has_init = !class_decl.init_blocks.is_empty();
        self.w.emit_indent();
        if has_init {
            self.w.push_str("let mut __self = ");
        }
        self.w.push_str(&inner);
        self.w.push_str(" {");
        let mut first = true;
        if let Some(parent_ty) = &class_decl.extends {
            if let Some(seg) = parent_ty.name.segments.first() {
                self.w.push_str(" __parent: ");
                self.w.push_str(&seg.text);
                let sfx = self.ctor_overload_suffix(&seg.text, 0);
                self.w.push_str("::new_inner");
                self.w.push_str(&sfx);
                self.w.push_str("()");
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
            // `ref` field (§M.13): seed a fresh shared cell around the
            // default value — ctor-body assignments store through it.
            if field.is_ref {
                self.w.push_str("std::rc::Rc::new(std::cell::RefCell::new(");
            }
            if let Some(default) = &field.default {
                self.emit_expr(default);
            } else {
                self.emit_field_storage_default(field);
            }
            if field.is_ref {
                self.w.push_str("))");
            }
        }
        // §P observer storage starts unallocated.
        self.emit_observer_field_inits_inline(class_decl, &mut first);
        // PhantomData inits (inline form) for phantom type params.
        for phantom in crate::unused_class_type_params(class_decl) {
            if first {
                self.w.push(' ');
            } else {
                self.w.push_str(", ");
            }
            first = false;
            self.w.push_str("__phantom_");
            self.w.push_str(&phantom);
            self.w.push_str(": std::marker::PhantomData");
        }
        if first {
            self.w.push_str("}");
        } else {
            self.w.push_str(" }");
        }
        if has_init {
            // Close the `let mut __self = … {};`, run init blocks, return __self.
            self.w.push_str(";\n");
            let prev_wrapper = self.emitting_wrapper_class;
            self.emitting_wrapper_class = false;
            self.this_alias = Some("__self".to_string());
            let mut muts = HashSet::new();
            for init in &class_decl.init_blocks {
                collect_mutated_names(init, &mut muts, &self.user_mut_methods);
            }
            self.mutated_in_fn = muts;
            for init in &class_decl.init_blocks {
                for stmt in &init.statements {
                    self.emit_source_marker(stmt_span(stmt));
                    self.w.emit_indent();
                    self.emit_stmt(stmt);
                }
            }
            self.this_alias = None;
            self.emitting_wrapper_class = prev_wrapper;
            self.w.line("__self");
        } else {
            self.w.push('\n');
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();

        // Thin public `new()` → wrap `new_inner()`. §CR.4.1: `RefCell` only for
        // the interior-mutable rep; the read-only-shared `Rc` rep drops it.
        self.w.indent_inc();
        self.w.line("pub fn new() -> Self {");
        self.w.indent_inc();
        self.emit_static_init_trigger();
        if self.box_classes.contains(&class_decl.name.text) {
            self.w.line("Self(std::boxed::Box::new(Self::new_inner()))");
        } else if self.refcell_classes.contains(&class_decl.name.text) {
            self.w.line("Self(std::rc::Rc::new(std::cell::RefCell::new(Self::new_inner())))");
        } else {
            self.w.line("Self(std::rc::Rc::new(Self::new_inner()))");
        }
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
        self.emit_static_init_trigger();
        // A class with `init { }` blocks builds into a `let mut __self`
        // binding so the init blocks can mutate `this` (= __self) before the
        // value is returned (§M.1 / §S.4.4 step 5).
        let has_init = !class_decl.init_blocks.is_empty();
        if has_init {
            self.w.line("let mut __self = Self {");
        } else {
            self.w.line("Self {");
        }
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
            // `ref` field (§M.13): seed a fresh shared cell around the
            // default value — ctor-body assignments store through it.
            if field.is_ref {
                self.w.push_str("std::rc::Rc::new(std::cell::RefCell::new(");
            }
            if let Some(default) = &field.default {
                self.emit_expr(default);
            } else {
                self.emit_field_storage_default(field);
            }
            if field.is_ref {
                self.w.push_str("))");
            }
            self.w.push_str(",\n");
        }
        // PhantomData inits for type params carried only as phantom fields.
        self.emit_phantom_field_inits(class_decl);
        self.w.indent_dec();
        if has_init {
            self.w.line("};");
            // Run the init blocks (this → __self), then return __self.
            self.this_alias = Some("__self".to_string());
            let mut muts = HashSet::new();
            for init in &class_decl.init_blocks {
                collect_mutated_names(init, &mut muts, &self.user_mut_methods);
            }
            self.mutated_in_fn = muts;
            for init in &class_decl.init_blocks {
                for stmt in &init.statements {
                    self.emit_source_marker(stmt_span(stmt));
                    self.w.emit_indent();
                    self.emit_stmt(stmt);
                }
            }
            self.this_alias = None;
            self.w.line("__self");
        } else {
            self.w.line("}");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
        self.w.indent_dec();
    }
}
