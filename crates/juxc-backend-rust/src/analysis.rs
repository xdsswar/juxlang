//! Free-function pre-passes and lvalue/expression helpers used by the
//! action-focused emit modules.
//!
//! Split out from `lib.rs` during the action-focused module
//! reorganization. Behavior is identical to the original free functions.

use std::collections::HashSet;

use juxc_ast::{
    Block, CompilationUnit, ElseBranch, Expr, GenericArg, Ident, Literal, QualifiedName, Stmt,
    TopLevelDecl, TypeParam, TypeRef, WildcardBound,
};
use juxc_source::Span;

/// Walk a function body and collect every name that appears as the
/// target of an [`AssignStmt`].
///
/// This is a one-shot mutation analysis used by the emitter to decide
/// whether a `var` should lower to `let mut` (target of an assignment
/// somewhere in the body) or plain `let` (never reassigned). It walks
/// nested blocks — `if`/`else`/`while` bodies — but stops at function
/// boundaries (a nested function decl would have its own pass, and we
/// don't have nested functions yet anyway).
pub(crate) fn collect_mutated_names(
    block: &Block,
    out: &mut HashSet<String>,
    user_mut: &HashSet<String>,
) {
    for stmt in &block.statements {
        match stmt {
            Stmt::Assign(a) => {
                // Walk down the lvalue to find the underlying name. For
                // `arr[i] = v` the base is `arr`; for `x = v` it's `x`.
                if let Some(name) = lvalue_base_name(&a.target) {
                    out.insert(name);
                }
                collect_mutating_calls(&a.value, out, user_mut);
            }
            Stmt::Expr(e) => collect_mutating_calls(e, out, user_mut),
            Stmt::VarDecl(v) => {
                if let Some(init) = &v.init {
                    collect_mutating_calls(init, out, user_mut);
                }
            }
            Stmt::Return(Some(e)) => collect_mutating_calls(e, out, user_mut),
            Stmt::If(if_stmt) => {
                collect_mutating_calls(&if_stmt.condition, out, user_mut);
                collect_mutated_names(&if_stmt.then_block, out, user_mut);
                collect_mutated_names_in_else(if_stmt.else_branch.as_deref(), out, user_mut);
            }
            Stmt::While(w) => {
                collect_mutating_calls(&w.condition, out, user_mut);
                collect_mutated_names(&w.body, out, user_mut);
            }
            Stmt::ForEach(f) => {
                collect_mutating_calls(&f.iter, out, user_mut);
                collect_mutated_names(&f.body, out, user_mut);
            }
            // Other statement kinds — Return(None), Break, Continue —
            // don't carry expressions that could mutate.
            _ => {}
        }
    }
}

/// Walk an expression looking for `obj.method(…)` calls where `method`
/// is one of the known Rust mutating methods on `Vec` and `obj` is a
/// single-segment Path. Each match adds that name to `out` so the
/// surrounding `let` binding for it gets promoted to `let mut`.
///
/// Without a real type table we can't tell whether `obj` is actually a
/// Vec — we just trust the method name. The cost is that calling a
/// same-named method on an unrelated type promotes the local to `mut`
/// unnecessarily, which only triggers Rust's `unused_mut` warning if
/// nothing else mutates the binding. Acceptable for Phase 1.
///
/// Sub-expressions are walked recursively so nested calls are caught.
pub(crate) fn collect_mutating_calls(e: &Expr, out: &mut HashSet<String>, user_mut: &HashSet<String>) {
    match e {
        Expr::Call(c) => {
            if let Expr::Field(f) = &*c.callee {
                // A method call counts as mutating the receiver when
                // either the hardcoded set covers it (`push`, `pop`,
                // …) or the per-program pre-pass flagged it as a user
                // method that takes `&mut self`.
                let mutates =
                    is_mutating_method(&f.field.text) || user_mut.contains(&f.field.text);
                if mutates {
                    if let Expr::Path(qn) = &*f.object {
                        if qn.segments.len() == 1 {
                            out.insert(qn.segments[0].text.clone());
                        }
                    }
                }
            }
            collect_mutating_calls(&c.callee, out, user_mut);
            for arg in &c.args {
                collect_mutating_calls(arg, out, user_mut);
            }
        }
        Expr::Binary(b) => {
            collect_mutating_calls(&b.left, out, user_mut);
            collect_mutating_calls(&b.right, out, user_mut);
        }
        Expr::Unary(u) => collect_mutating_calls(&u.operand, out, user_mut),
        Expr::Range(r) => {
            collect_mutating_calls(&r.start, out, user_mut);
            collect_mutating_calls(&r.end, out, user_mut);
        }
        Expr::Cast(c) => collect_mutating_calls(&c.value, out, user_mut),
        Expr::Index(idx) => {
            collect_mutating_calls(&idx.array, out, user_mut);
            collect_mutating_calls(&idx.index, out, user_mut);
        }
        Expr::Field(f) => collect_mutating_calls(&f.object, out, user_mut),
        Expr::NewArray(n) => collect_mutating_calls(&n.size, out, user_mut),
        Expr::NewArrayLit(n) => {
            for el in &n.elements {
                collect_mutating_calls(el, out, user_mut);
            }
        }
        Expr::SizeOf(s) => collect_mutating_calls(&s.operand, out, user_mut),
        Expr::InterpString(s) => {
            // Walk each `${expr}` interpolation segment; literal and
            // bare-ident segments don't contain mutating call shapes.
            for seg in &s.segments {
                if let juxc_ast::InterpSegment::Expr(inner) = seg {
                    collect_mutating_calls(inner, out, user_mut);
                }
            }
        }
        Expr::NewObject(n) => {
            for arg in &n.args {
                collect_mutating_calls(arg, out, user_mut);
            }
        }
        Expr::Switch(s) => {
            collect_mutating_calls(&s.scrutinee, out, user_mut);
            for arm in &s.arms {
                match &arm.body {
                    juxc_ast::SwitchBody::Expr(e) => {
                        collect_mutating_calls(e, out, user_mut);
                    }
                    juxc_ast::SwitchBody::Block(b) => {
                        collect_mutated_names(b, out, user_mut);
                    }
                }
            }
        }
        // Leaves with no further sub-expressions:
        // - Literal, Path: source-level atoms.
        // - This: implicit-receiver token; no body to walk.
        Expr::Literal(_) | Expr::Path(_) | Expr::This(_) => {}
        // Walk the lambda body so a mutating call inside (e.g.
        // `xs -> xs.push(1)`) still marks the captured receiver.
        Expr::Lambda(l) => match &l.body {
            juxc_ast::LambdaBody::Expr(e) => collect_mutating_calls(e, out, user_mut),
            juxc_ast::LambdaBody::Block(b) => collect_mutated_names(b, out, user_mut),
        },
    }
}

/// Known mutating methods on `Vec` (and similar Rust containers).
/// Hardcoded for Phase 1; a real type table would derive this from
/// the receiver's type signature.
pub(crate) fn is_mutating_method(name: &str) -> bool {
    matches!(
        name,
        "push" | "pop" | "clear" | "insert" | "remove" | "extend" | "truncate"
        | "swap" | "reverse" | "sort"
    )
}

/// Walk down a (possibly index-chained) lvalue expression to find its
/// underlying variable name. Returns `None` for shapes the parser
/// shouldn't produce as an lvalue.
///
/// - `x`       → `Some("x")`
/// - `arr[i]`  → `Some("arr")`
/// - `arr[i][j]` → `Some("arr")`
pub(crate) fn lvalue_base_name(e: &Expr) -> Option<String> {
    match e {
        Expr::Path(qn) if qn.segments.len() == 1 => Some(qn.segments[0].text.clone()),
        Expr::Index(idx) => lvalue_base_name(&idx.array),
        // `this.field` and `this.field[i]` count as mutations of the
        // receiver — but we don't surface a name here (the receiver
        // isn't a local binding). Callers that need to know "did this
        // body mutate self?" use `body_writes_to_this` instead.
        Expr::This(_) | Expr::Field(_) => None,
        _ => None,
    }
}

/// Returns `true` if any statement in `block` (transitively) assigns to
/// a field whose lvalue root is `Expr::This` — i.e., something like
/// `this.x = …`, `this.x[i] = …`, or compound forms. Used by
/// `emit_method` to decide between `&self` and `&mut self` receivers.
///
/// We only need a yes/no answer; the method body's per-local mutation
/// analysis (`mutated_in_fn`) is unaffected.
pub(crate) fn body_writes_to_this(block: &Block) -> bool {
    for stmt in &block.statements {
        match stmt {
            Stmt::Assign(a) => {
                if lvalue_root_is_this(&a.target) {
                    return true;
                }
            }
            Stmt::If(if_stmt) => {
                if body_writes_to_this(&if_stmt.then_block) {
                    return true;
                }
                if body_writes_to_this_in_else(if_stmt.else_branch.as_deref()) {
                    return true;
                }
            }
            Stmt::While(w) => {
                if body_writes_to_this(&w.body) {
                    return true;
                }
            }
            Stmt::ForEach(f) => {
                if body_writes_to_this(&f.body) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// True when the lvalue's deepest non-Field/Index expression is `Expr::This`.
pub(crate) fn lvalue_root_is_this(e: &Expr) -> bool {
    match e {
        Expr::This(_) => true,
        Expr::Field(f) => lvalue_root_is_this(&f.object),
        Expr::Index(i) => lvalue_root_is_this(&i.array),
        _ => false,
    }
}

/// `body_writes_to_this` helper for else-branch chains.
pub(crate) fn body_writes_to_this_in_else(branch: Option<&ElseBranch>) -> bool {
    let mut cursor = branch;
    while let Some(b) = cursor {
        match b {
            ElseBranch::If(inner) => {
                if body_writes_to_this(&inner.then_block) {
                    return true;
                }
                cursor = inner.else_branch.as_deref();
            }
            ElseBranch::Block(block) => {
                return body_writes_to_this(block);
            }
        }
    }
    false
}

/// Try to flatten a dotted-path expression (`a.b.c.d`) into a sequence
/// of identifier segments. After the parser refactor that switched
/// `parse_primary`'s identifier arm to single-segment, source paths
/// like `std.io.Stream` arrive as `Field(Field(Path("std"), "io"), "Stream")`.
/// This helper recovers the flat segment list when every node in the
/// chain is a simple ident — used by `sizeof` to apply the §5.9.3
/// rule-4 "multi-segment path → type form" branch.
///
/// Returns `None` if any node along the chain is something other than
/// a simple `Path`/`Field` (e.g. a call, index, or literal anywhere).
pub(crate) fn try_flatten_dotted_path(e: &Expr) -> Option<Vec<String>> {
    match e {
        Expr::Path(qn) => Some(qn.segments.iter().map(|s| s.text.clone()).collect()),
        Expr::Field(f) => {
            let mut base = try_flatten_dotted_path(&f.object)?;
            base.push(f.field.text.clone());
            Some(base)
        }
        _ => None,
    }
}

/// Helper for [`collect_mutated_names`]: walks an `else` chain
/// (`else if … else …`), descending into each `if`/`block` arm.
pub(crate) fn collect_mutated_names_in_else(
    branch: Option<&ElseBranch>,
    out: &mut HashSet<String>,
    user_mut: &HashSet<String>,
) {
    let mut cursor = branch;
    while let Some(b) = cursor {
        match b {
            ElseBranch::If(inner) => {
                collect_mutated_names(&inner.then_block, out, user_mut);
                cursor = inner.else_branch.as_deref();
            }
            ElseBranch::Block(block) => {
                collect_mutated_names(block, out, user_mut);
                cursor = None;
            }
        }
    }
}

/// True if `e` is a `Literal::String(_)`. Used by [`RustEmitter::emit_binary`]
/// to decide when `+` should lower as string concatenation.
pub(crate) fn is_string_literal(e: &Expr) -> bool {
    matches!(e, Expr::Literal(Literal::String(_)))
}

/// Inspect a constructor body to see whether every statement is a
/// `this.field = expr;` assignment. Returns the list of
/// `(field-name, init-expr)` pairs in source order when so, else
/// `None`. The emitter uses this for the **simple-ctor fast path** —
/// a direct `Self { field: expr, … }` literal that avoids `Default`
/// initialization (and so works for generic-typed fields).
/// Output of [`extract_simple_ctor_inits`] when the constructor body
/// matches the "simple" shape — pure `this.field = expr;` lines,
/// optionally preceded by a single `super(args);` delegation.
pub(crate) struct SimpleCtorInits {
    /// `Some(args)` when the body started with `super(args);` —
    /// extracted so the backend can lift it into the child struct's
    /// literal as `__parent: Parent::new(args)`. `None` when no super
    /// call appears (either no parent, or the user omitted the
    /// explicit call).
    pub(crate) super_args: Option<Vec<Expr>>,
    /// `(field-name, init-expr)` pairs in source order. Same semantics
    /// as before — the simple path requires every body statement that
    /// isn't the super call to be a `this.field = expr;` assignment.
    pub(crate) inits: Vec<(String, Expr)>,
}

pub(crate) fn extract_simple_ctor_inits(ctor: &juxc_ast::ConstructorDecl) -> Option<SimpleCtorInits> {
    let mut super_args: Option<Vec<Expr>> = None;
    let mut inits = Vec::with_capacity(ctor.body.statements.len());
    for (i, stmt) in ctor.body.statements.iter().enumerate() {
        match stmt {
            // `super(args);` is allowed as the first statement and
            // gets lifted into the struct literal's `__parent` slot.
            Stmt::SuperCall(args, _) => {
                if i != 0 {
                    // Java semantics: super must be first. Disqualify
                    // the simple path so the user gets the fallback
                    // (which will emit it inline at whatever position
                    // — Rust will still error, but the error will
                    // include the call site).
                    return None;
                }
                super_args = Some(args.clone());
            }
            Stmt::Assign(a) => {
                let Expr::Field(f) = &a.target else { return None };
                if !matches!(&*f.object, Expr::This(_)) {
                    return None;
                }
                inits.push((f.field.text.clone(), a.value.clone()));
            }
            // Any other statement disqualifies the fast path.
            _ => return None,
        }
    }
    Some(SimpleCtorInits { super_args, inits })
}

/// Whether a pattern's source form carried explicit parens. Lets the
/// backend distinguish unit-variant patterns (`Color.Red`) from
/// empty-paren tuple-variant patterns (`Color.Red()`) when emitting.
/// In Rust both forms compile, but only the no-paren form is canonical
/// for unit variants.
pub(crate) fn pattern_has_parens(p: &juxc_ast::Pattern) -> bool {
    matches!(p, juxc_ast::Pattern::EnumVariant { has_parens: true, .. })
}

/// Pre-pass over the compilation unit collecting the names of user-
/// defined methods whose bodies write to `this.field` — i.e. methods
/// the backend will emit with `&mut self`. The mutation analyzer in
/// every function consults this set so calling such a method on a
/// receiver promotes the receiver's binding to `let mut`.
///
/// This is a workaround for the absent type table: a same-named method
/// on a different class will also get flagged, which only costs an
/// unnecessary `mut`. Once tycheck carries receiver types we can do
/// the precise per-class lookup instead.
pub(crate) fn collect_user_mut_methods(unit: &CompilationUnit) -> HashSet<String> {
    let mut out = HashSet::new();
    for item in &unit.items {
        if let TopLevelDecl::Class(class) = item {
            for method in &class.methods {
                if let Some(body) = &method.body {
                    if body_writes_to_this(body) {
                        out.insert(method.name.text.clone());
                    }
                }
            }
        }
    }
    out
}

// ============================================================================
// Auto-derive eligibility (§O.3 — auto-derivation for value types)
// ============================================================================
//
// Records and enums get every applicable operator for free from their
// structure. The Rust mapping uses `derive(...)` for the common cases
// (`PartialEq`, `Eq`, `Hash`, `Clone`, `Copy`). Eligibility depends on
// the field/payload types — Rust won't derive `Eq` on a struct with a
// `f32` field, for instance. These helpers walk a `TypeRef` and answer:
// can this type participate in `Eq` / `Hash` / `Copy`?
//
// Conservative whenever shape is ambiguous (user-defined classes,
// unresolved generics) — being optimistic risks `derive` failures at
// rustc time, and a missing derive is recoverable while a spurious one
// breaks the build.

/// True iff `name` is a Jux primitive whose Rust mapping is `Copy`
/// **and** `Eq` (i.e., every primitive except the IEEE-754 floats and
/// — by intent — the platform-sized `int`/`uint` which are still `Eq`,
/// just listed explicitly so the set stays auditable).
fn is_copy_eq_primitive(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "byte"
            | "ubyte"
            | "short"
            | "ushort"
            | "int"
            | "uint"
            | "long"
            | "ulong"
            | "char"
            | "i8"
            | "u8"
            | "i16"
            | "u16"
            | "i32"
            | "u32"
            | "i64"
            | "u64",
    )
}

/// True iff `name` is a Jux float primitive — `Copy` but **not** `Eq`
/// or `Hash` in Rust.
fn is_float_primitive(name: &str) -> bool {
    matches!(name, "float" | "double" | "f32" | "f64")
}

/// True if a field of type `ty` is **Copy**-compatible. Records can
/// derive `Copy` iff every field qualifies.
///
/// Rules (conservative — anything we can't statically prove disqualifies):
/// - Arrays: never. Dynamic arrays lower to `Vec<T>` (not `Copy`); we
///   skip fixed arrays too since their Copy-ness depends on the element
///   AND on `T: Copy` propagation through user types.
/// - Nullable types (`T?`): never — they lower to `Option<T>` which is
///   `Copy` only if the inner is. Be conservative.
/// - Single-segment primitive name: `Copy` iff numeric/bool/char/float.
///   `String` is **not** `Copy`.
/// - Anything else (user types, multi-segment paths, generic params,
///   unresolved): not `Copy`.
pub(crate) fn field_supports_copy(ty: &juxc_ast::TypeRef) -> bool {
    if ty.array_shape.is_some() || ty.nullable {
        return false;
    }
    if !ty.generic_args.is_empty() || ty.name.segments.len() != 1 {
        return false;
    }
    let name = ty.name.segments[0].text.as_str();
    is_copy_eq_primitive(name) || is_float_primitive(name)
}

/// True if a field of type `ty` is **Eq**-compatible. Records can
/// derive `Eq` iff every field qualifies. `Hash` follows the same
/// eligibility rule — every Eq-eligible type we recognize is also
/// `Hash` in std.
///
/// Rules:
/// - Floats disqualify (`f32`/`f64` are `PartialEq` only).
/// - Other primitives qualify.
/// - `String` qualifies (Rust's `String` is `Eq` + `Hash`).
/// - Arrays: qualify iff the element does — `Vec<T>` and `[T; N]` are
///   both `Eq + Hash` when `T` is.
/// - Nullable types qualify when the inner does (`Option<T>` is `Eq`
///   when `T` is).
/// - User types, generic args, multi-segment paths: conservatively
///   **disqualify**. A future turn can grow a real symbol-table walk
///   to recognize Eq-bearing user types.
pub(crate) fn field_supports_eq(ty: &juxc_ast::TypeRef) -> bool {
    if let Some(_shape) = &ty.array_shape {
        // Construct a non-array view of the same element and recurse.
        let element = juxc_ast::TypeRef {
            name: ty.name.clone(),
            generic_args: ty.generic_args.clone(),
            nullable: ty.nullable,
            array_shape: None,
            fn_shape: ty.fn_shape.clone(),
            span: ty.span,
        };
        return field_supports_eq(&element);
    }
    if ty.nullable {
        let inner = juxc_ast::TypeRef {
            name: ty.name.clone(),
            generic_args: ty.generic_args.clone(),
            nullable: false,
            array_shape: None,
            fn_shape: ty.fn_shape.clone(),
            span: ty.span,
        };
        return field_supports_eq(&inner);
    }
    if ty.name.segments.len() != 1 || !ty.generic_args.is_empty() {
        return false;
    }
    let name = ty.name.segments[0].text.as_str();
    if name == "String" {
        return true;
    }
    is_copy_eq_primitive(name)
}

/// True if a field of type `ty` is `Hash`-compatible. Identical to the
/// `Eq` predicate for the types we recognize — std's `Hash` impls
/// cover the same set.
pub(crate) fn field_supports_hash(ty: &juxc_ast::TypeRef) -> bool {
    field_supports_eq(ty)
}

/// True if a field of type `ty` is `Display`-compatible — used to
/// decide whether to emit the auto-derived `operator string` impl
/// (§O.3.1) for the enclosing record/enum.
///
/// Rules (conservative — same shape as the other predicates):
/// - Primitives and `String`: yes (every Rust primitive plus `String`
///   implements `Display`).
/// - Arrays: no (`Vec<T>` and `[T; N]` don't implement `Display`).
/// - Nullable types: no (`Option<T>` doesn't implement `Display`).
/// - Generic params, user types, multi-segment paths: conservatively
///   **disqualify**. A future turn with a real symbol-table walk could
///   recognize Display-bearing user types and add a `T: Display` bound
///   to the generated `impl`; today we skip the impl entirely so we
///   never emit Rust that fails to compile.
pub(crate) fn field_supports_display(ty: &juxc_ast::TypeRef) -> bool {
    if ty.array_shape.is_some() || ty.nullable {
        return false;
    }
    if ty.name.segments.len() != 1 || !ty.generic_args.is_empty() {
        return false;
    }
    let name = ty.name.segments[0].text.as_str();
    name == "String" || is_copy_eq_primitive(name) || is_float_primitive(name)
}

/// True if `ty` is exactly the Jux primitive `String` — single-segment
/// path named "String", no generic args, no array shape, not nullable.
pub(crate) fn is_jux_string_type(ty: &juxc_ast::TypeRef) -> bool {
    ty.array_shape.is_none()
        && !ty.nullable
        && ty.generic_args.is_empty()
        && ty.name.segments.len() == 1
        && ty.name.segments[0].text == "String"
}

/// Alias for [`is_jux_string_type`] exported under the name some
/// Phase-H call sites prefer for readability — "this `TypeRef` is the
/// Jux `String` type" reads more naturally at the assignment-coercion
/// site than the legacy `is_jux_string_type` spelling.
pub(crate) fn is_jux_string_type_ref(ty: &juxc_ast::TypeRef) -> bool {
    is_jux_string_type(ty)
}

/// True if `name`'s first character is an uppercase letter. Used by
/// [`RustEmitter::emit_sizeof_path`] to classify a bare identifier as
/// type-form (PascalCase) vs. value-form (camelCase) per §5.9.3.
///
/// Names starting with `_` or a digit don't apply — they aren't valid
/// identifier starts per §A.1.3 anyway, so this can only see real
/// letter-led identifiers.
pub(crate) fn starts_with_uppercase(name: &str) -> bool {
    name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
}

// ============================================================================
// Wildcard generic-arg lifting — backend Phase 1 strategy for PECS
// ============================================================================

/// State threaded through [`lift_wildcards_in_type_ref`] across the
/// params of a single function/method. Each wildcard encountered is
/// replaced by a fresh synthetic type-param name (`__W0`, `__W1`, …)
/// and its bound is recorded so the caller can extend the
/// declaration-site generic-params list.
///
/// Phase-1 lowering rule: a wildcard in a parameter-position type
/// reads as `<__Wn: Bound + Clone>` on the enclosing function. Mimics
/// what Java itself does after type erasure — Java's `void
/// f(List<? extends Animal> xs)` and Rust's
/// `fn f<__W: AnimalKind + Clone>(xs: List<__W>)` are isomorphic.
///
/// Wildcards in storage positions (locals, fields, return types) are
/// not lifted here — tycheck flags those with a placeholder
/// diagnostic until a proper `Box<dyn>`-erasure strategy lands.
pub(crate) struct WildcardLifter {
    /// Synthetic TypeParams produced during the rewrite, in
    /// declaration order. Caller concatenates them after the
    /// function's own generic params.
    pub new_params: Vec<TypeParam>,
    /// Counter for the next `__Wn` name. Bumped on each fresh
    /// wildcard regardless of bound shape.
    next: usize,
}

impl WildcardLifter {
    pub(crate) fn new() -> Self {
        Self {
            new_params: Vec::new(),
            next: 0,
        }
    }

    /// Recursively walk `ty`, replacing each `GenericArg::Wildcard`
    /// with a freshly-named `GenericArg::Type` referencing a
    /// synthetic param. Returns the rewritten `TypeRef` (cloning
    /// only the spine — concrete subterms are kept by reference via
    /// `clone()`).
    pub(crate) fn rewrite_type_ref(&mut self, ty: &TypeRef) -> TypeRef {
        let generic_args = ty
            .generic_args
            .iter()
            .map(|arg| match arg {
                GenericArg::Type(inner) => GenericArg::Type(self.rewrite_type_ref(inner)),
                GenericArg::Wildcard(w) => GenericArg::Type(self.synthesize(&w.bound)),
            })
            .collect();
        TypeRef {
            name: ty.name.clone(),
            generic_args,
            nullable: ty.nullable,
            array_shape: ty.array_shape.clone(),
            fn_shape: ty.fn_shape.clone(),
            span: ty.span,
        }
    }

    /// Mint a fresh `__Wn` TypeParam with the wildcard's bound and
    /// return a TypeRef pointing at it. `? super B` collapses to the
    /// same shape as `? extends B` here — Rust generics can't
    /// express "supertype of B" directly, and Phase 1 treats the
    /// bound as a marker constraint either way. (Tycheck still
    /// enforces the variance distinction via PECS in `compatible`.)
    fn synthesize(&mut self, bound: &Option<WildcardBound>) -> TypeRef {
        let name = format!("__W{}", self.next);
        self.next += 1;
        let bounds: Vec<TypeRef> = match bound {
            None => Vec::new(),
            Some(WildcardBound::Extends(b)) => vec![b.clone()],
            Some(WildcardBound::Super(b)) => vec![b.clone()],
        };
        let ident = Ident {
            text: name.clone(),
            span: Span::DUMMY,
        };
        self.new_params.push(TypeParam {
            name: ident.clone(),
            bounds,
            span: Span::DUMMY,
        });
        TypeRef {
            name: QualifiedName {
                segments: vec![ident],
                span: Span::DUMMY,
            },
            generic_args: Vec::new(),
            nullable: false,
            array_shape: None,
            fn_shape: None,
            span: Span::DUMMY,
        }
    }
}

/// True iff `ty` contains a wildcard anywhere in its generic-arg
/// tree. Cheap guard so call sites can skip the rewrite allocation
/// for the overwhelmingly-common no-wildcards case.
pub(crate) fn type_ref_has_wildcard(ty: &TypeRef) -> bool {
    ty.generic_args.iter().any(|arg| match arg {
        GenericArg::Type(inner) => type_ref_has_wildcard(inner),
        GenericArg::Wildcard(_) => true,
    })
}
