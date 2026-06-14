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

/// Convert an inferred [`juxc_tycheck::Ty`] back into a syntactic
/// [`TypeRef`], for the common cases that can appear as a class's concrete
/// generic argument: user types (with nested args), `String`, primitives,
/// in-scope params, and nullables. Returns `None` for shapes that have no
/// faithful single-`TypeRef` form (arrays, fn types, wildcards, unknown) —
/// callers fall back to leaving the original type-param in place.
///
/// Used by [`crate::RustEmitter::callee_param_type`] to substitute a
/// receiver's concrete type args (`Registry<User, …>` → `K = User`) into a
/// method's declared param type so wildcards over class params lower with
/// the real element type instead of a dangling `dyn K` (gap 5).
fn ty_to_type_ref(ty: &juxc_tycheck::Ty) -> Option<TypeRef> {
    use juxc_tycheck::ty::Primitive;
    use juxc_tycheck::Ty;
    // Build a single-segment `TypeRef` from a bare name + optional args.
    fn make(name: &str, args: Vec<GenericArg>) -> TypeRef {
        TypeRef {
            name: QualifiedName {
                segments: vec![Ident {
                    text: name.to_string(),
                    span: Span::DUMMY,
                }],
                span: Span::DUMMY,
            },
            generic_args: args,
            nullable: false,
            array_shape: None,
            fn_shape: None,
            ptr_depth: 0,
            span: Span::DUMMY,
        }
    }
    match ty {
        Ty::String => Some(make("String", Vec::new())),
        Ty::Primitive(p) => {
            // Jux source spelling of each primitive (matches the parser).
            let name = match p {
                Primitive::Int => "int",
                Primitive::Uint => "uint",
                Primitive::Byte => "byte",
                Primitive::Ubyte => "ubyte",
                Primitive::Short => "short",
                Primitive::Ushort => "ushort",
                Primitive::Long => "long",
                Primitive::Ulong => "ulong",
                Primitive::Float => "float",
                Primitive::Double => "double",
                Primitive::Bool => "bool",
                Primitive::Char => "char",
                Primitive::I8 => "i8",
                Primitive::U8 => "u8",
                Primitive::I16 => "i16",
                Primitive::U16 => "u16",
                Primitive::I32 => "i32",
                Primitive::U32 => "u32",
                Primitive::I64 => "i64",
                Primitive::U64 => "u64",
                Primitive::F32 => "f32",
                Primitive::F64 => "f64",
            };
            Some(make(name, Vec::new()))
        }
        Ty::Param(name) => Some(make(name, Vec::new())),
        Ty::User { name, generic_args } => {
            // Use the bare (last) segment — emission re-roots to `crate::…`.
            let bare = name.rsplit('.').next().unwrap_or(name.as_str());
            let mut args: Vec<GenericArg> = Vec::with_capacity(generic_args.len());
            for a in generic_args {
                args.push(GenericArg::Type(ty_to_type_ref(a)?));
            }
            Some(make(bare, args))
        }
        Ty::Nullable(inner) => {
            let mut t = ty_to_type_ref(inner)?;
            t.nullable = true;
            Some(t)
        }
        _ => None,
    }
}

/// Collapse every **bounded wildcard** generic arg in `ty` (recursively) to
/// its bound type — `Sink<? super User>` → `Sink<User>`,
/// `List<? extends Animal>` → `List<Animal>`. Unbounded `?` is left as-is
/// (it has no element to collapse to).
///
/// This mirrors the method body's WildcardLifter, which reads/writes a
/// `? extends K` / `? super K` slot as the bare element `K`. A call-site
/// argument coercion must target that same concrete element shape, so after
/// the receiver's type args are substituted in we collapse the wildcards so
/// the cast type matches the callee's lowered parameter type exactly.
fn collapse_concrete_wildcards(ty: &TypeRef) -> TypeRef {
    let generic_args: Vec<GenericArg> = ty
        .generic_args
        .iter()
        .map(|a| match a {
            GenericArg::Type(t) => GenericArg::Type(collapse_concrete_wildcards(t)),
            GenericArg::Wildcard(w) => match &w.bound {
                Some(WildcardBound::Extends(t)) | Some(WildcardBound::Super(t)) => {
                    GenericArg::Type(collapse_concrete_wildcards(t))
                }
                // Bare `?` — no element to substitute; keep the wildcard.
                None => a.clone(),
            },
        })
        .collect();
    TypeRef {
        name: ty.name.clone(),
        generic_args,
        nullable: ty.nullable,
        array_shape: ty.array_shape.clone(),
        fn_shape: ty.fn_shape.clone(),
        ptr_depth: ty.ptr_depth,
        span: ty.span,
    }
}

/// How a value must be adapted to fit an interface-typed (`Rc<dyn Trait>`)
/// value slot in stage-1 interface dispatch. Produced by
/// [`crate::RustEmitter::iface_coercion_to`].
pub(crate) enum IfaceCoercion {
    /// No interface coercion — the target isn't an interface slot, or the
    /// source needs no adaptation.
    None,
    /// Source is a concrete class implementing the target interface — wrap it
    /// in `Rc<dyn Trait>`. `clone_first` clones a reused wrapper place (cheap
    /// `Rc` bump, preserves shared identity) before wrapping.
    WrapClass { clone_first: bool },
    /// Source is already an interface value (`Rc<dyn Trait>`) flowing into
    /// another interface slot — clone the `Rc` handle (or move when fresh).
    CloneDyn { clone_first: bool },
    /// Source is a concrete subclass value flowing into a slot typed as its
    /// **direct** base class under the non-sealed, non-polymorphic open
    /// hierarchy (e.g. a `BaseErr` into an `Exception` cause slot) — slice it up
    /// to the parent via the generated `From<Sub> for Parent` with `.into()`.
    IntoBase,
}

/// A conservative "is this expression a place (lvalue) that may be used
/// again?" test — a bare variable / `this` / field / index read. Such a
/// source must be `.clone()`d (not moved) when its value flows into an
/// interface slot, so a later use doesn't hit a Rust move error. Calls and
/// `new` produce fresh temporaries and can be moved.
fn expr_is_place(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Path(_) | Expr::This(_) | Expr::Field(_) | Expr::Index(_)
    )
}

/// Synthesize a bare, non-generic [`TypeRef`] naming the interface `bare`.
/// Used where only the interface's *inferred* `Ty` is on hand (e.g. an
/// assignment LHS) but the coercion helpers want a `TypeRef` target. The
/// `span` is cosmetic — it never reaches a diagnostic from these paths.
pub(crate) fn synth_iface_type_ref(bare: &str, span: Span) -> TypeRef {
    TypeRef {
        name: QualifiedName {
            segments: vec![Ident {
                text: bare.to_string(),
                span,
            }],
            span,
        },
        generic_args: vec![],
        nullable: false,
        array_shape: None,
        fn_shape: None,
        ptr_depth: 0,
        span,
    }
}

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
    // Pre-scan for locals bound to an anonymous-class instance
    // (`var w = new Widget() { … };`). Anonymous-class methods always
    // lower with a `&mut self` receiver (matching the interface
    // trait-dispatch convention), so calling ANY method on such a local
    // takes `&mut`, which requires the binding to be `let mut`. We
    // collect these names so `collect_mutating_calls` can promote a
    // plain `obj.method()` call on them even when `method` isn't in the
    // mutating-method set.
    let mut anon_locals: HashSet<String> = HashSet::new();
    collect_anon_bound_locals(block, &mut anon_locals);
    // Promote any anon-bound local on which a method is invoked. We pass
    // the anon set in as extra `user_mut`-equivalent context: a method
    // call on an anon local mutates it regardless of the method name.
    collect_anon_method_calls(block, out, &anon_locals);
    collect_mutated_names_real(block, out, user_mut);
}

/// Walk `block` (recursively) and add to `out` any anon-bound local in
/// `anon_locals` on which a method is called (`w.render()`), since
/// anonymous-class methods lower with `&mut self`.
fn collect_anon_method_calls(
    block: &Block,
    out: &mut HashSet<String>,
    anon_locals: &HashSet<String>,
) {
    fn walk_expr(e: &Expr, out: &mut HashSet<String>, anon_locals: &HashSet<String>) {
        if let Expr::Call(c) = e {
            if let Expr::Field(f) = &*c.callee {
                if let Expr::Path(qn) = &*f.object {
                    if qn.segments.len() == 1
                        && anon_locals.contains(&qn.segments[0].text)
                    {
                        out.insert(qn.segments[0].text.clone());
                    }
                }
            }
        }
        // Recurse into the immediate sub-expressions that can carry a
        // call. Reuse the existing structural walk by re-running on
        // children via a small manual descent for the common shapes.
        match e {
            Expr::Call(c) => {
                walk_expr(&c.callee, out, anon_locals);
                for a in &c.args {
                    walk_expr(a, out, anon_locals);
                }
            }
            Expr::Field(f) => walk_expr(&f.object, out, anon_locals),
            Expr::Binary(b) => {
                walk_expr(&b.left, out, anon_locals);
                walk_expr(&b.right, out, anon_locals);
            }
            Expr::Unary(u) => walk_expr(&u.operand, out, anon_locals),
            Expr::InterpString(s) => {
                for seg in &s.segments {
                    if let juxc_ast::InterpSegment::Expr(inner) = seg {
                        walk_expr(inner, out, anon_locals);
                    }
                }
            }
            _ => {}
        }
    }
    fn walk_block(block: &Block, out: &mut HashSet<String>, anon_locals: &HashSet<String>) {
        for stmt in &block.statements {
            match stmt {
                Stmt::Expr(e) => walk_expr(e, out, anon_locals),
                Stmt::Assign(a) => walk_expr(&a.value, out, anon_locals),
                Stmt::VarDecl(v) => {
                    if let Some(init) = &v.init {
                        walk_expr(init, out, anon_locals);
                    }
                }
                Stmt::Return(Some(e)) => walk_expr(e, out, anon_locals),
                Stmt::If(if_stmt) => {
                    walk_expr(&if_stmt.condition, out, anon_locals);
                    walk_block(&if_stmt.then_block, out, anon_locals);
                    if let Some(eb) = if_stmt.else_branch.as_deref() {
                        match eb {
                            ElseBranch::Block(b) => walk_block(b, out, anon_locals),
                            ElseBranch::If(inner) => {
                                let synth = Block {
                                    statements: vec![Stmt::If(inner.clone())],
                                    span: Span::DUMMY,
                                };
                                walk_block(&synth, out, anon_locals);
                            }
                        }
                    }
                }
                Stmt::While(w) => {
                    walk_expr(&w.condition, out, anon_locals);
                    walk_block(&w.body, out, anon_locals);
                }
                Stmt::DoWhile(d) => {
                    walk_block(&d.body, out, anon_locals);
                    walk_expr(&d.condition, out, anon_locals);
                }
                Stmt::ForEach(f) => {
                    walk_expr(&f.iter, out, anon_locals);
                    walk_block(&f.body, out, anon_locals);
                }
                _ => {}
            }
        }
    }
    walk_block(block, out, anon_locals);
}

/// Collect every local-variable name in `block` (recursively) whose
/// initializer is an anonymous-class instantiation
/// (`new T() { … }`). Used by [`collect_mutated_names`] to promote
/// those bindings to `let mut` when a method is called on them, since
/// anonymous-class methods lower with `&mut self`.
fn collect_anon_bound_locals(block: &Block, out: &mut HashSet<String>) {
    for stmt in &block.statements {
        match stmt {
            Stmt::VarDecl(v) => {
                if let Some(Expr::NewObject(n)) = v.init.as_ref() {
                    if n.anonymous_body.is_some() {
                        out.insert(v.name.text.clone());
                    }
                }
            }
            Stmt::If(if_stmt) => {
                collect_anon_bound_locals(&if_stmt.then_block, out);
                if let Some(eb) = if_stmt.else_branch.as_deref() {
                    match eb {
                        ElseBranch::Block(b) => collect_anon_bound_locals(b, out),
                        ElseBranch::If(inner) => {
                            let synth = Block {
                                statements: vec![Stmt::If(inner.clone())],
                                span: Span::DUMMY,
                            };
                            collect_anon_bound_locals(&synth, out);
                        }
                    }
                }
            }
            Stmt::While(w) => collect_anon_bound_locals(&w.body, out),
            Stmt::DoWhile(d) => collect_anon_bound_locals(&d.body, out),
            Stmt::ForEach(f) => collect_anon_bound_locals(&f.body, out),
            _ => {}
        }
    }
}

/// Original reassignment/mutating-call walker, split out from
/// [`collect_mutated_names`] so the public entry point can run the
/// anonymous-class pre-passes first.
fn collect_mutated_names_real(
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
            Stmt::DoWhile(d) => {
                collect_mutated_names(&d.body, out, user_mut);
                collect_mutating_calls(&d.condition, out, user_mut);
            }
            Stmt::ForEach(f) => {
                collect_mutating_calls(&f.iter, out, user_mut);
                collect_mutated_names(&f.body, out, user_mut);
            }
            Stmt::Unsafe(b) => collect_mutated_names(b, out, user_mut),
            Stmt::Try(t) => {
                // Assignments and mutating calls inside a try/catch/finally
                // promote their locals to `let mut` just like any other block.
                collect_mutated_names(&t.body, out, user_mut);
                for c in &t.catches {
                    collect_mutated_names(&c.body, out, user_mut);
                }
                if let Some(fin) = &t.finally {
                    collect_mutated_names(fin, out, user_mut);
                }
            }
            Stmt::Throw(e, _) => collect_mutating_calls(e, out, user_mut),
            Stmt::ForC(f) => {
                // The init's loop variable is reassigned by the update clause
                // (`i = i + 1`), so the update's lvalue base must be collected
                // to promote the binding to `let mut`. Descend into all clauses.
                if let Some(init) = f.init.as_deref() {
                    collect_mutated_names_in_stmt(init, out, user_mut);
                }
                if let Some(upd) = f.update.as_deref() {
                    collect_mutated_names_in_stmt(upd, out, user_mut);
                }
                collect_mutated_names(&f.body, out, user_mut);
            }
            // A labeled loop wraps a real loop — its body's assignments
            // still promote locals to `let mut`. Round-trip the inner
            // statement through a one-element scratch block so the full
            // loop walkers above handle it.
            Stmt::Labeled { stmt, .. } => {
                let scratch = Block {
                    statements: vec![(**stmt).clone()],
                    span: juxc_source::Span::DUMMY,
                };
                collect_mutated_names(&scratch, out, user_mut);
            }
            // Other statement kinds — Return(None), Break, Continue,
            // SuperCall — don't carry assignments that promote a local.
            _ => {}
        }
    }
}

/// Collect mutated-local names from a single statement — the init/update
/// clause of a C-style `for`. An `Assign` promotes its lvalue base (`i = i+1`
/// → `i`); a `VarDecl`/`Expr` contributes any mutating calls in its expressions.
pub(crate) fn collect_mutated_names_in_stmt(
    stmt: &Stmt,
    out: &mut HashSet<String>,
    user_mut: &HashSet<String>,
) {
    match stmt {
        Stmt::Assign(a) => {
            if let Some(name) = lvalue_base_name(&a.target) {
                out.insert(name);
            }
            collect_mutating_calls(&a.value, out, user_mut);
        }
        Stmt::VarDecl(v) => {
            if let Some(init) = &v.init {
                collect_mutating_calls(init, out, user_mut);
            }
        }
        Stmt::Expr(e) => collect_mutating_calls(e, out, user_mut),
        _ => {}
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
        // `typeof(expr)` (§5.9.10) never evaluates its operand.
        Expr::TypeOf(..) => {}
        // `out <place>` (§M.4) passes the place by `&mut`, so its base binding
        // is mutated → it must be `let mut`.
        Expr::Out(inner, _) => {
            if let Some(name) = lvalue_base_name(inner) {
                out.insert(name);
            }
            collect_mutating_calls(inner, out, user_mut);
        }
        Expr::TupleLit(elems, _) => {
            for el in elems {
                collect_mutating_calls(el, out, user_mut);
            }
        }
        Expr::ErrorProp(inner, _) => collect_mutating_calls(inner, out, user_mut),
        Expr::TryExpr(t) => {
            collect_mutated_names(&t.body, out, user_mut);
            for c in &t.catches {
                collect_mutated_names(&c.body, out, user_mut);
            }
        }
        Expr::Call(c) => {
            if let Expr::Field(f) = &*c.callee {
                // A method call counts as mutating the receiver when
                // either the hardcoded set covers it (`push`, `pop`,
                // …) or the per-program pre-pass flagged it as a user
                // method that takes `&mut self`.
                let mutates =
                    is_mutating_method(&f.field.text) || user_mut.contains(&f.field.text);
                if mutates {
                    // The receiver may be a bare local (`a.bump()`) or a
                    // deeper place path (`h.item.bump()`, `grid[i].bump()`)
                    // — in every case the BASE binding must become
                    // `let mut` for the in-place `&mut` borrow to
                    // compile (S7: field-path receivers were skipped,
                    // leaving the root binding immutable).
                    if let Some(name) = lvalue_base_name(&f.object) {
                        out.insert(name);
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
        Expr::Unary(u) => {
            // `&x` (address-of) lowers to `core::ptr::addr_of_mut!(x)`, which
            // requires `x` to be a mutable place — so taking the address of a
            // local promotes it to `let mut`.
            if u.op == juxc_ast::UnaryOp::AddrOf {
                if let Expr::Path(qn) = &*u.operand {
                    if qn.segments.len() == 1 {
                        out.insert(qn.segments[0].text.clone());
                    }
                }
            }
            collect_mutating_calls(&u.operand, out, user_mut);
        }
        Expr::Range(r) => {
            collect_mutating_calls(&r.start, out, user_mut);
            collect_mutating_calls(&r.end, out, user_mut);
        }
        Expr::Cast(c) => collect_mutating_calls(&c.value, out, user_mut),
        Expr::NotNullAssert(inner, _) => collect_mutating_calls(inner, out, user_mut),
        Expr::TypeTest(t) => collect_mutating_calls(&t.value, out, user_mut),
        Expr::Index(idx) => {
            collect_mutating_calls(&idx.array, out, user_mut);
            collect_mutating_calls(&idx.index, out, user_mut);
        }
        Expr::Field(f) => collect_mutating_calls(&f.object, out, user_mut),
        Expr::NewArray(n) => {
            collect_mutating_calls(&n.size, out, user_mut);
            for inner in &n.inner_sizes {
                collect_mutating_calls(inner, out, user_mut);
            }
        }
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
                if let Some(g) = &arm.guard {
                    collect_mutating_calls(g, out, user_mut);
                }
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
        Expr::Literal(_) | Expr::Path(_) | Expr::This(_) | Expr::Super(_) => {}
        // Walk the lambda body so a mutating call inside (e.g.
        // `xs -> xs.push(1)`) still marks the captured receiver.
        Expr::Lambda(l) => match &l.body {
            juxc_ast::LambdaBody::Expr(e) => collect_mutating_calls(e, out, user_mut),
            juxc_ast::LambdaBody::Block(b) => collect_mutated_names(b, out, user_mut),
        },
        // Elvis evaluates both sides — both can contain a mutating
        // call (e.g. `xs.pop() ?: empty()`); walk recursively.
        Expr::Elvis(e) => {
            collect_mutating_calls(&e.value, out, user_mut);
            collect_mutating_calls(&e.fallback, out, user_mut);
        }
        // Method-ref is a closure construction; no calls happen
        // until the user invokes the returned value.
        Expr::MethodRef(_) => {}
        // Ternary evaluates condition + one branch; walk all
        // three for nested mutating calls.
        Expr::Ternary(t) => {
            collect_mutating_calls(&t.condition, out, user_mut);
            collect_mutating_calls(&t.then_branch, out, user_mut);
            collect_mutating_calls(&t.else_branch, out, user_mut);
        }
        // `await expr` — the operand drives evaluation, so any
        // mutating calls inside it count just like in any other
        // value position.
        Expr::Await(inner, _) => {
            collect_mutating_calls(inner, out, user_mut);
        }
        // `++place` / `place++` (§A `incdec`, value form) STORES into the
        // place, so its base binding must be `let mut` — mark it, then
        // walk the target for any nested mutating calls (e.g. the index
        // in `arr[push(v)]++`, contrived but possible).
        Expr::IncDec(i) => {
            if let Some(name) = lvalue_base_name(&i.target) {
                out.insert(name);
            }
            collect_mutating_calls(&i.target, out, user_mut);
        }
    }
}

/// Recursively scan a Block for any direct `await` expression. Used
/// by `emit_try` to pick the sync vs async catch_unwind lowering.
///
/// Walks into nested statements (if/while/for/switch/try arms) but
/// does NOT descend into closure / lambda bodies — those introduce
/// a new function boundary and their `.await` belongs to whatever
/// async context wraps them, not the surrounding block.
pub(crate) fn block_contains_await(block: &Block) -> bool {
    block.statements.iter().any(stmt_contains_await)
}

fn stmt_contains_await(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Expr(e) | Stmt::Throw(e, _) => expr_contains_await(e),
        Stmt::Return(Some(e)) => expr_contains_await(e),
        Stmt::Return(None) => false,
        Stmt::VarDecl(v) => v.init.as_ref().is_some_and(expr_contains_await),
        Stmt::Assign(a) => expr_contains_await(&a.value) || expr_contains_await(&a.target),
        Stmt::If(i) => if_contains_await(i),
        Stmt::While(w) => expr_contains_await(&w.condition) || block_contains_await(&w.body),
        Stmt::DoWhile(d) => block_contains_await(&d.body) || expr_contains_await(&d.condition),
        // `for await` (§18.6.3) awaits per element even when neither
        // the iter expression nor the body contains a textual `await`
        // — missing it here would classify an enclosing `try` as SYNC
        // and emit `.await` inside a non-async `catch_unwind` closure.
        Stmt::ForEach(f) => {
            f.is_await || expr_contains_await(&f.iter) || block_contains_await(&f.body)
        }
        Stmt::ForC(f) => {
            f.cond.as_ref().is_some_and(expr_contains_await) || block_contains_await(&f.body)
        }
        Stmt::Try(t) => {
            block_contains_await(&t.body)
                || t.catches.iter().any(|c| block_contains_await(&c.body))
                || t.finally.as_ref().is_some_and(block_contains_await)
        }
        Stmt::SuperCall(args, _) => args.iter().any(expr_contains_await),
        Stmt::Unsafe(b) => block_contains_await(b),
        Stmt::Break(..) | Stmt::Continue(..) => false,
        Stmt::Labeled { stmt, .. } => stmt_contains_await(stmt),
    }
}

fn if_contains_await(i: &juxc_ast::IfStmt) -> bool {
    expr_contains_await(&i.condition)
        || block_contains_await(&i.then_block)
        || match i.else_branch.as_deref() {
            Some(ElseBranch::Block(b)) => block_contains_await(b),
            Some(ElseBranch::If(inner)) => if_contains_await(inner),
            None => false,
        }
}

fn expr_contains_await(e: &Expr) -> bool {
    match e {
        Expr::Await(_, _) => true,
        // `typeof` never evaluates its operand — no await runs.
        Expr::TypeOf(..) => false,
        Expr::Out(inner, _) => expr_contains_await(inner),
        Expr::TupleLit(elems, _) => elems.iter().any(expr_contains_await),
        Expr::ErrorProp(inner, _) => expr_contains_await(inner),
        Expr::TryExpr(t) => {
            block_contains_await(&t.body)
                || t.catches.iter().any(|c| block_contains_await(&c.body))
        }
        Expr::NotNullAssert(inner, _) => expr_contains_await(inner),
        Expr::Call(c) => {
            expr_contains_await(&c.callee) || c.args.iter().any(expr_contains_await)
        }
        Expr::Binary(b) => expr_contains_await(&b.left) || expr_contains_await(&b.right),
        Expr::Unary(u) => expr_contains_await(&u.operand),
        Expr::Cast(c) => expr_contains_await(&c.value),
        Expr::TypeTest(t) => expr_contains_await(&t.value),
        Expr::Range(r) => expr_contains_await(&r.start) || expr_contains_await(&r.end),
        Expr::Field(f) => expr_contains_await(&f.object),
        Expr::IncDec(i) => expr_contains_await(&i.target),
        Expr::Index(i) => expr_contains_await(&i.array) || expr_contains_await(&i.index),
        Expr::NewArray(n) => {
            expr_contains_await(&n.size) || n.inner_sizes.iter().any(|s| expr_contains_await(s))
        }
        Expr::NewArrayLit(n) => n.elements.iter().any(expr_contains_await),
        Expr::NewObject(n) => n.args.iter().any(expr_contains_await),
        Expr::Elvis(e) => expr_contains_await(&e.value) || expr_contains_await(&e.fallback),
        Expr::Ternary(t) => {
            expr_contains_await(&t.condition)
                || expr_contains_await(&t.then_branch)
                || expr_contains_await(&t.else_branch)
        }
        Expr::InterpString(s) => s.segments.iter().any(|seg| match seg {
            juxc_ast::InterpSegment::Expr(e) => expr_contains_await(e),
            _ => false,
        }),
        Expr::SizeOf(s) => expr_contains_await(&s.operand),
        Expr::Switch(s) => {
            expr_contains_await(&s.scrutinee)
                || s.arms.iter().any(|a| {
                    a.guard.as_ref().is_some_and(expr_contains_await)
                        || match &a.body {
                            juxc_ast::SwitchBody::Expr(e) => expr_contains_await(e),
                            juxc_ast::SwitchBody::Block(b) => block_contains_await(b),
                        }
                })
        }
        // Closures / lambdas open a new fn boundary — the `.await`
        // inside belongs to the closure's own async context, not the
        // surrounding block.
        Expr::Lambda(_) | Expr::MethodRef(_) => false,
        Expr::Literal(_) | Expr::Path(_) | Expr::This(_) | Expr::Super(_) => false,
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
        // Jux-spec method names: List<T> and Map<K, V> both
        // mutate through `add`/`put` and various others. Listing
        // them here lets `let` → `let mut` promotion kick in for
        // `var xs = new List<int>(); xs.add(1);` without forcing
        // the user to write a manual `mut` annotation.
        | "add" | "put" | "set"
        // Deque<T> mutators (VecDeque-backed).
        | "addFirst" | "addLast" | "removeFirst" | "removeLast"
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
        // Field chains walk through to find the lvalue root. For
        // `u.nickname = ...` the root is `u`, which must be
        // `let mut` so Rust can take `&mut self` on the field
        // assignment. For `this.field = ...` the root is `This`,
        // which isn't a local binding — `body_writes_to_this`
        // tracks that case for receiver `&mut self` promotion.
        Expr::Field(f) => lvalue_base_name(&f.object),
        Expr::This(_) => None,
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
            Stmt::DoWhile(d) => {
                if body_writes_to_this(&d.body) {
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
    /// `(field-name, init-expr)` pairs in source order — the
    /// `this.field = expr;` assignments. Same semantics as before.
    pub(crate) inits: Vec<(String, Expr)>,
    /// Side-effect statements that don't touch `this.field` —
    /// most commonly static-field compound assignments like
    /// `MyStatic = MyStatic + 1`. Emitted before the struct
    /// literal in their original source order so a counter bump
    /// still happens at construction time even when the simple
    /// path is taken. Order is preserved relative to each other
    /// but field inits all happen "logically together" in the
    /// final `Self { ... }` literal.
    pub(crate) side_effects: Vec<juxc_ast::Stmt>,
}

pub(crate) fn extract_simple_ctor_inits(ctor: &juxc_ast::ConstructorDecl) -> Option<SimpleCtorInits> {
    let mut super_args: Option<Vec<Expr>> = None;
    let mut inits = Vec::with_capacity(ctor.body.statements.len());
    let mut side_effects: Vec<juxc_ast::Stmt> = Vec::new();
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
                // `this.field = expr;` → field init.
                if let Expr::Field(f) = &a.target {
                    if matches!(&*f.object, Expr::This(_)) {
                        inits.push((f.field.text.clone(), a.value.clone()));
                        continue;
                    }
                    // A `Field` target whose object is a bare single-segment
                    // path (NOT `this`) is a **static field** write —
                    // `Registry.instances = Registry.instances + 1;` parses
                    // as a field access on the class-name path `Registry`. It
                    // touches no instance slot, so it's a pure side effect:
                    // run it before the struct literal, exactly like a
                    // bare-name static write. Keeping it on the simple path
                    // matters for generic-typed fields — the fallback
                    // `__self`-builder would `Default::default()`-init the
                    // generic field and demand `V: Default` (E0277). Side
                    // effects emit via the normal statement path, which
                    // already lowers a static-field write to its
                    // `LazyLock<Mutex<…>>` form. Anything more complex
                    // (`obj.a.b = …`, an indexed object) stays on the slow
                    // path — we can't prove it's instance-state-independent.
                    if matches!(&*f.object, Expr::Path(p) if p.segments.len() == 1) {
                        side_effects.push(stmt.clone());
                        continue;
                    }
                    return None;
                }
                // `staticField = expr;` (bare-name Path target) →
                // side effect. The simple path can run these
                // before the struct literal without needing
                // Default initialization for any generic-typed
                // field. Anything more complex (array indexing,
                // arbitrary lvalues) still falls through to the
                // slow path.
                if matches!(&a.target, Expr::Path(_)) {
                    side_effects.push(stmt.clone());
                    continue;
                }
                return None;
            }
            // Any other statement disqualifies the fast path.
            _ => return None,
        }
    }
    Some(SimpleCtorInits { super_args, inits, side_effects })
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
    collect_user_mut_methods_seeded(unit, &HashSet::new())
}

/// Like [`collect_user_mut_methods`] but with an additional `extra_seed` of
/// method names already known to need `&mut self` (e.g. external `@MutSelf`
/// stub methods). Seeding them BEFORE the fixed-point matters: a user method
/// that calls such a method on a `this`-rooted receiver (`self.backing.peek()`
/// where `peek` is a discovered `&mut self`) must itself become `&mut self`,
/// AND its callers must promote their receiver to `let mut`. Folding the extern
/// set in only AFTER the closure (the old behavior) left those transitive
/// callers out, so the method emitted `&mut self` while the call site failed to
/// promote the local (rustc E0596).
pub(crate) fn collect_user_mut_methods_seeded(
    unit: &CompilationUnit,
    extra_seed: &HashSet<String>,
) -> HashSet<String> {
    // Seed: methods that directly write to `this.field`, plus every
    // method declared on an interface (interface trait methods all
    // emit as `&mut self`, so any call to one on a `this.field`
    // receiver propagates the `&mut self` requirement up the call
    // chain), plus the caller-supplied `extra_seed`.
    let mut out: HashSet<String> = extra_seed.clone();
    for item in &unit.items {
        match item {
            TopLevelDecl::Class(class) => {
                for method in &class.methods {
                    if let Some(body) = &method.body {
                        if body_writes_to_this(body) {
                            out.insert(method.name.text.clone());
                        }
                    }
                }
            }
            TopLevelDecl::Interface(iface) => {
                for method in &iface.methods {
                    out.insert(method.name.text.clone());
                }
            }
            _ => {}
        }
    }
    // Fixed-point closure: a method that calls a `&mut self`-style
    // method (one already in `out`) on a `this`-rooted receiver
    // must itself be `&mut self`. Iterate until no new additions.
    loop {
        let mut added = false;
        for item in &unit.items {
            if let TopLevelDecl::Class(class) = item {
                for method in &class.methods {
                    if out.contains(method.name.text.as_str()) {
                        continue;
                    }
                    let Some(body) = &method.body else { continue };
                    if body_calls_mut_method_on_this(body, &out) {
                        out.insert(method.name.text.clone());
                        added = true;
                    }
                }
                for ctor in &class.constructors {
                    // Constructors don't have a "name in user_mut_methods"
                    // (they're keyed by class), but their bodies are
                    // emitted via the same `&mut self` path —
                    // already handled by ctor emission.
                    let _ = ctor;
                }
            }
        }
        if !added {
            break;
        }
    }
    out
}

/// True iff `block` contains a method call whose receiver expression
/// is rooted at `this` (e.g. `this.method(...)`, `this.field.method(...)`)
/// AND the called method's name is in `mut_methods`. Used by the
/// cascade-aware mutation pass — if such a call exists in a method
/// body, that method also needs `&mut self`.
pub(crate) fn body_calls_mut_method_on_this(
    block: &Block,
    mut_methods: &HashSet<String>,
) -> bool {
    for stmt in &block.statements {
        if stmt_calls_mut_method_on_this(stmt, mut_methods) {
            return true;
        }
    }
    false
}

fn stmt_calls_mut_method_on_this(stmt: &Stmt, mut_methods: &HashSet<String>) -> bool {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) => expr_calls_mut_method_on_this(e, mut_methods),
        Stmt::Return(None) => false,
        Stmt::VarDecl(v) => v
            .init
            .as_ref()
            .map(|e| expr_calls_mut_method_on_this(e, mut_methods))
            .unwrap_or(false),
        Stmt::Assign(a) => {
            expr_calls_mut_method_on_this(&a.target, mut_methods)
                || expr_calls_mut_method_on_this(&a.value, mut_methods)
        }
        Stmt::If(if_stmt) => {
            if let Expr::Binary(_) | Expr::Unary(_) | Expr::Path(_) = &if_stmt.condition {
                // condition rarely matters; fall through
            }
            if expr_calls_mut_method_on_this(&if_stmt.condition, mut_methods) {
                return true;
            }
            if body_calls_mut_method_on_this(&if_stmt.then_block, mut_methods) {
                return true;
            }
            // Walk else chain.
            let mut cursor = if_stmt.else_branch.as_deref();
            while let Some(b) = cursor {
                match b {
                    ElseBranch::If(inner) => {
                        if body_calls_mut_method_on_this(&inner.then_block, mut_methods) {
                            return true;
                        }
                        cursor = inner.else_branch.as_deref();
                    }
                    ElseBranch::Block(block) => {
                        return body_calls_mut_method_on_this(block, mut_methods);
                    }
                }
            }
            false
        }
        Stmt::While(w) => {
            expr_calls_mut_method_on_this(&w.condition, mut_methods)
                || body_calls_mut_method_on_this(&w.body, mut_methods)
        }
        Stmt::DoWhile(d) => {
            body_calls_mut_method_on_this(&d.body, mut_methods)
                || expr_calls_mut_method_on_this(&d.condition, mut_methods)
        }
        Stmt::ForEach(f) => {
            expr_calls_mut_method_on_this(&f.iter, mut_methods)
                || body_calls_mut_method_on_this(&f.body, mut_methods)
        }
        Stmt::ForC(f) => body_calls_mut_method_on_this(&f.body, mut_methods),
        Stmt::SuperCall(args, _) => args
            .iter()
            .any(|a| expr_calls_mut_method_on_this(a, mut_methods)),
        Stmt::Throw(e, _) => expr_calls_mut_method_on_this(e, mut_methods),
        Stmt::Try(t) => {
            if body_calls_mut_method_on_this(&t.body, mut_methods) {
                return true;
            }
            for c in &t.catches {
                if body_calls_mut_method_on_this(&c.body, mut_methods) {
                    return true;
                }
            }
            if let Some(fin) = &t.finally {
                if body_calls_mut_method_on_this(fin, mut_methods) {
                    return true;
                }
            }
            false
        }
        Stmt::Unsafe(b) => body_calls_mut_method_on_this(b, mut_methods),
        Stmt::Break(..) | Stmt::Continue(..) => false,
        Stmt::Labeled { stmt, .. } => stmt_calls_mut_method_on_this(stmt, mut_methods),
    }
}

fn expr_calls_mut_method_on_this(expr: &Expr, mut_methods: &HashSet<String>) -> bool {
    match expr {
        Expr::Call(c) => {
            if let Expr::Field(f) = &*c.callee {
                // A method call rooted at `this` is mutating when the method is
                // a known collection mutator (`this.items.push(...)`) OR a user
                // `&mut self` method. The first arm is what makes a method that
                // mutates a collection FIELD require `&mut self` (and mark its
                // callers' receivers `mut`).
                if receiver_root_is_this(&f.object)
                    && (is_mutating_method(f.field.text.as_str())
                        || mut_methods.contains(f.field.text.as_str()))
                {
                    return true;
                }
            }
            // Recurse into args / callee for chained / nested calls.
            if expr_calls_mut_method_on_this(&c.callee, mut_methods) {
                return true;
            }
            c.args
                .iter()
                .any(|a| expr_calls_mut_method_on_this(a, mut_methods))
        }
        Expr::Field(f) => expr_calls_mut_method_on_this(&f.object, mut_methods),
        Expr::Binary(b) => {
            expr_calls_mut_method_on_this(&b.left, mut_methods)
                || expr_calls_mut_method_on_this(&b.right, mut_methods)
        }
        Expr::Unary(u) => expr_calls_mut_method_on_this(&u.operand, mut_methods),
        Expr::Cast(c) => expr_calls_mut_method_on_this(&c.value, mut_methods),
        Expr::Index(i) => {
            expr_calls_mut_method_on_this(&i.array, mut_methods)
                || expr_calls_mut_method_on_this(&i.index, mut_methods)
        }
        Expr::Elvis(e) => {
            expr_calls_mut_method_on_this(&e.value, mut_methods)
                || expr_calls_mut_method_on_this(&e.fallback, mut_methods)
        }
        Expr::Ternary(t) => {
            expr_calls_mut_method_on_this(&t.condition, mut_methods)
                || expr_calls_mut_method_on_this(&t.then_branch, mut_methods)
                || expr_calls_mut_method_on_this(&t.else_branch, mut_methods)
        }
        Expr::InterpString(s) => s.segments.iter().any(|seg| match seg {
            juxc_ast::InterpSegment::Expr(e) => expr_calls_mut_method_on_this(e, mut_methods),
            _ => false,
        }),
        Expr::NewObject(n) => n
            .args
            .iter()
            .any(|a| expr_calls_mut_method_on_this(a, mut_methods)),
        // `await expr` — walk into the operand.
        Expr::Await(inner, _) => expr_calls_mut_method_on_this(inner, mut_methods),
        _ => false,
    }
}

fn receiver_root_is_this(expr: &Expr) -> bool {
    match expr {
        Expr::This(_) => true,
        Expr::Field(f) => receiver_root_is_this(&f.object),
        Expr::Call(c) => receiver_root_is_this(&c.callee),
        Expr::Index(i) => receiver_root_is_this(&i.array),
        _ => false,
    }
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
            ptr_depth: 0,
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
            ptr_depth: 0,
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

/// True if a field of type `ty` is `Default`-compatible (every Jux
/// primitive and `String` implements `Default`, arrays of those do
/// too via `Vec::default()` / array-of-Default initialization, and
/// nullable wraps default to `None`). Records derive `Default` when
/// every component qualifies so a class that stores a record-typed
/// field without an explicit initializer can still flow through
/// the struct-init shim's `Default::default()` fallback.
pub(crate) fn field_supports_default(ty: &juxc_ast::TypeRef) -> bool {
    if ty.nullable {
        // `Option<T>::default()` is `None` regardless of T.
        return true;
    }
    if ty.array_shape.is_some() {
        // Dynamic arrays lower to `Vec<T>` which is always `Default`;
        // fixed-size `[T; N]` needs T: Default + Copy — the existing
        // const-context paths take care of constant N initializers,
        // so we conservatively require the element to qualify.
        let element = juxc_ast::TypeRef {
            name: ty.name.clone(),
            generic_args: ty.generic_args.clone(),
            nullable: false,
            array_shape: None,
            fn_shape: ty.fn_shape.clone(),
            ptr_depth: 0,
            span: ty.span,
        };
        return field_supports_default(&element);
    }
    if ty.name.segments.len() != 1 || !ty.generic_args.is_empty() {
        return false;
    }
    let name = ty.name.segments[0].text.as_str();
    if name == "String" {
        return true;
    }
    is_copy_eq_primitive(name) || is_float_primitive(name)
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
/// Phase-H call sites prefer for readability. Kept around as a
/// stable handle even though the post-Fix-1 backend no longer
/// performs the `&str → String` coercion this was originally
/// introduced for.
#[allow(dead_code)]
pub(crate) fn is_jux_string_type_ref(ty: &juxc_ast::TypeRef) -> bool {
    is_jux_string_type(ty)
}

impl crate::RustEmitter {
    /// True iff `expr`'s emitted Rust value is already
    /// `Option<T>`-shaped — meaning no additional `Some(...)` wrap
    /// is needed when feeding it into a `T?` slot. Recognized
    /// shapes:
    ///
    /// - `null` literal → already `None`.
    /// - A `Path` (single-segment) to a binding we've tagged
    ///   nullable via [`Self::nullable_locals`].
    /// - A `Call` to a known function / method whose declared
    ///   return type is `T?`.
    /// - A `Field` access on a known class/record whose field's
    ///   declared type is `T?`.
    /// - An `Elvis` expression — `?:` / `??` produces a non-null
    ///   `T`, never an `Option`, so it must STILL be wrapped. So
    ///   Elvis returns `false` here.
    /// - A safe-call `obj?.field` / `obj?.method()` produces an
    ///   `Option<T>` by construction; recognized as nullable.
    ///
    /// Conservative on the no-info side: anything we can't classify
    /// returns `false`, meaning the caller wraps. Over-wrapping a
    /// value that's actually nullable would produce
    /// `Some(Some(...))` — wrong type. Under-wrapping (the case
    /// here) produces `Some(plain_value)`, which is correct as
    /// long as the value really IS plain. The trade-off favors
    /// safety: returning `false` defaults to wrap, which is the
    /// safer direction.
    pub(crate) fn expression_is_already_nullable(&self, expr: &juxc_ast::Expr) -> bool {
        // **Path queries** (single-segment ident) consult
        // `nullable_locals` as the source of truth so that
        // smart-cast removal (`emit_if`'s `if (x != null) { … }`
        // pops the binding from the set for the body) takes
        // effect. The tycheck-recorded `expr_types[span]` would
        // still say the binding is nullable at the AST level —
        // we override that for the inside of the smart-cast
        // block. Other expression shapes fall through to the
        // `expr_types` consult below.
        if let juxc_ast::Expr::Path(qn) = expr {
            if qn.segments.len() == 1 {
                return self.nullable_locals.contains(&qn.segments[0].text);
            }
        }
        // **Arithmetic/comparison results are never `Option` values.**
        // Inside a null smart-cast (`if (b != null) { print(b + 1) }`)
        // tycheck's recorded type for the BINARY may still carry the
        // operand's declared nullable wrap, but the emitted operand is
        // narrowed (`if let Some(b) = b`) — so consulting `expr_types`
        // here would wrap a plain `isize` in `JuxOpt` (rustc E0308).
        // An actual Option can't appear as a binary operand in valid
        // Rust anyway, so the answer is unconditionally "not nullable".
        if matches!(expr, juxc_ast::Expr::Binary(_)) {
            return false;
        }
        // For non-Path shapes, ask tycheck directly. With the
        // `Ty::Nullable` refactor, every expression visited by
        // `check::Checker` carries its full type — including the
        // nullable wrap — in `expr_types[span]`. Reading this
        // answer is more precise than the syntactic fallback
        // paths below.
        if let Some(juxc_tycheck::Ty::Nullable(_)) =
            self.expr_types.get(&crate::exprs::expr_span_of(expr))
        {
            return true;
        }
        match expr {
            juxc_ast::Expr::Literal(juxc_ast::Literal::Null) => true,
            juxc_ast::Expr::Path(qn) => {
                qn.segments.len() == 1
                    && self.nullable_locals.contains(&qn.segments[0].text)
            }
            juxc_ast::Expr::Call(c) => {
                // Single-segment callee: top-level fn whose return
                // type tells us the result's shape.
                if let juxc_ast::Expr::Path(qn) = &*c.callee {
                    if qn.segments.len() == 1 {
                        if let Some((_, f)) = self.symbols.lookup_function(&qn.segments[0].text) {
                            return matches!(
                                &f.return_type,
                                juxc_ast::ReturnType::Type(t) if t.nullable
                            );
                        }
                    }
                }
                // Method call: receiver-typed lookup is a tycheck
                // job. Without the dynamic dispatch we treat the
                // result as non-nullable; conservative.
                false
            }
            juxc_ast::Expr::Field(f) => {
                // `?.` produces `Option<T>` regardless of what
                // `field`'s declared type is — safe-nav semantics.
                if f.safe {
                    return true;
                }
                // Plain `obj.field`: look up the receiver's class
                // via tycheck's `expr_types`, then ask the class
                // signature whether the field is `T?`. The lookup
                // mirrors `assign_target_is_nullable` but as a
                // read-side query — Phase 1 only checks the
                // immediate class, not the inheritance chain.
                //
                // A `this.field` receiver isn't always keyed in
                // `expr_types`, so resolve it through the
                // `enclosing_class` context (same fallback
                // `assign_target_is_nullable` uses for writes).
                let recv_name = match &*f.object {
                    juxc_ast::Expr::This(_) => self.enclosing_class.clone(),
                    other => match self.expr_types.get(&crate::exprs::expr_span_of(other)) {
                        Some(juxc_tycheck::Ty::User { name, .. }) => Some(name.clone()),
                        _ => None,
                    },
                };
                let Some(name) = recv_name else {
                    return false;
                };
                let name = &name;
                if let Some(class) = self.symbols.classes.get(name) {
                    if let Some(field) = class.fields.get(&f.field.text) {
                        return field.ty.nullable;
                    }
                }
                if let Some(record) = self.symbols.records.get(name) {
                    if let Some(c) =
                        record.components.iter().find(|c| c.name == f.field.text)
                    {
                        return c.ty.nullable;
                    }
                }
                false
            }
            // Elvis returns the non-null inner — wrap when feeding
            // into a `T?` slot.
            juxc_ast::Expr::Elvis(_) => false,
            _ => false,
        }
    }

    /// Return the declared `nullable` flag of the *positional*
    /// parameter at `arg_idx` of `callee`, when we can figure out
    /// which function/method the call resolves to. `None` means
    /// "unknown" — caller should NOT wrap, since we can't tell
    /// whether the slot is `T?` or `T`.
    ///
    /// Recognized callees:
    /// - Single-segment `Path` → top-level function in
    ///   `symbols.functions`.
    /// - `Field` over a `Path` that resolves to a class, and the
    ///   field name matches a known method → static or instance
    ///   method. The class lookup uses
    ///   [`Self::path_resolves_to_class_in_emit`]; same code path
    ///   the static-method call routing uses.
    ///
    /// Instance methods on non-Path receivers (e.g. `foo().bar(x)`)
    /// would need tycheck's per-expression type to identify the
    /// receiver's class; left as a future refinement.
    pub(crate) fn callee_param_is_nullable(
        &self,
        callee: &juxc_ast::Expr,
        arg_idx: usize,
    ) -> bool {
        // Top-level fn: `f(...)`.
        if let juxc_ast::Expr::Path(qn) = callee {
            if qn.segments.len() == 1 {
                if let Some((_, f)) = self.symbols.lookup_function(&qn.segments[0].text) {
                    return f
                        .params
                        .get(arg_idx)
                        .map(|p| p.ty.nullable)
                        .unwrap_or(false);
                }
            }
        }
        // Static or instance method: `Receiver.method(...)`.
        if let juxc_ast::Expr::Field(f) = callee {
            if let juxc_ast::Expr::Path(qn) = &*f.object {
                if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                    if let Some(class) = self.symbols.classes.get(&class_fqn) {
                        if let Some(m) = class.methods.get(f.field.text.as_str()) {
                            return m
                                .params
                                .get(arg_idx)
                                .map(|p| p.ty.nullable)
                                .unwrap_or(false);
                        }
                    }
                }
            }
        }
        false
    }

    /// True when arg `arg_idx` of the call maps to a `ref` (§M.13)
    /// SHARED-reference parameter — so the call site passes an
    /// aliasing handle (`x.clone()` for `ref` args) or wraps a plain
    /// value (`Rc::new(RefCell::new(v))`). Covers free functions and
    /// static/instance methods through the symbol table.
    pub(crate) fn callee_param_is_shared_ref(
        &self,
        callee: &juxc_ast::Expr,
        arg_idx: usize,
    ) -> bool {
        self.callee_param_flag(callee, arg_idx, /*weak=*/ false)
    }

    /// True when arg `arg_idx` maps to a **`weak`** parameter (§M.14.3) of the
    /// callee — so a class argument is downgraded to a `Weak` handle at the call
    /// site. Shares its callee resolution with [`Self::callee_param_is_shared_ref`].
    pub(crate) fn callee_param_is_weak(
        &self,
        callee: &juxc_ast::Expr,
        arg_idx: usize,
    ) -> bool {
        self.callee_param_flag(callee, arg_idx, /*weak=*/ true)
    }

    /// Resolve arg `arg_idx`'s declared parameter across the bare-free-fn /
    /// static-method / instance-method callee shapes and report the requested
    /// binding-mode flag — `is_weak` when `weak`, else `is_shared_ref`. One
    /// resolution path drives both [`Self::callee_param_is_shared_ref`] and
    /// [`Self::callee_param_is_weak`].
    fn callee_param_flag(
        &self,
        callee: &juxc_ast::Expr,
        arg_idx: usize,
        weak: bool,
    ) -> bool {
        let pick = |p: &juxc_tycheck::symbol_table::ParamSig| {
            if weak {
                p.is_weak
            } else {
                p.is_shared_ref
            }
        };
        if let juxc_ast::Expr::Path(qn) = callee {
            if qn.segments.len() == 1 {
                let name = qn.segments[0].text.as_str();
                let f_sig = self
                    .symbols
                    .lookup_function(name)
                    .map(|(_, f)| f)
                    .or_else(|| {
                        // A bare name that's AMBIGUOUS only because a
                        // bindgen stub exports the same name (a user
                        // `rename` vs `rust.std.rename`): user code
                        // shadows the foreign surface, mirroring
                        // tycheck's resolution. Foreign-ness is the
                        // stub's own `@rust` path — discovered, not a
                        // name list.
                        let suffix = format!(".{name}");
                        let mut hits = self
                            .symbols
                            .functions
                            .iter()
                            .filter(|(k, f)| k.ends_with(&suffix) && f.rust_path.is_none());
                        match (hits.next(), hits.next()) {
                            (Some((_, f)), None) => Some(f),
                            _ => None,
                        }
                    });
                if let Some(f) = f_sig {
                    return f.params.get(arg_idx).map(pick).unwrap_or(false);
                }
            }
        }
        if let juxc_ast::Expr::Field(f) = callee {
            // Static `ClassName.method(...)`.
            if let juxc_ast::Expr::Path(qn) = &*f.object {
                if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                    if let Some(c) = self.symbols.classes.get(&class_fqn) {
                        if let Some(m) = c.methods.get(f.field.text.as_str()) {
                            return m.params.get(arg_idx).map(pick).unwrap_or(false);
                        }
                    }
                }
            }
            // Instance method: resolve the receiver's class.
            if let Some(bare) = self.receiver_class_bare(&f.object) {
                let sig = self.symbols.classes.get(&bare).or_else(|| {
                    self.symbols
                        .find_fqn_by_bare(&bare)
                        .and_then(|fqn| self.symbols.classes.get(&fqn))
                });
                if let Some(c) = sig {
                    if let Some(m) = c.methods.get(f.field.text.as_str()) {
                        return m.params.get(arg_idx).map(pick).unwrap_or(false);
                    }
                }
            }
        }
        false
    }

    /// True when arg `arg_idx` of a method call `recv.method(...)` maps to a
    /// **borrowed** parameter (`&T`) of an **external** (`rust.std` / crate)
    /// method — so codegen must re-add the call-site `&` (§G.9.2): a Rust
    /// `contains_key(&Q)` needs `&arg`, not `arg`. The receiver's type is read
    /// from `expr_types`; the method is looked up by its Jux (camelCase) name and
    /// the parameter's `is_ref` flag (set from the stub's `&` marker) consulted.
    pub(crate) fn callee_param_is_ref(&self, callee: &juxc_ast::Expr, arg_idx: usize) -> bool {
        let juxc_ast::Expr::Field(f) = callee else { return false };
        // Static call `ClassName.method(...)`: the receiver is a class NAME, not
        // a value, so it never appears in `expr_types`. Resolve the class
        // directly and read the static method's param. Only foreign (external)
        // static methods carry meaningful by-ref params (§G.9.2).
        if let juxc_ast::Expr::Path(qn) = &*f.object {
            if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                if let Some(c) = self.symbols.classes.get(&class_fqn) {
                    if c.is_external {
                        if let Some(m) = c.methods.get(f.field.text.as_str()) {
                            if m.is_static {
                                return m.params.get(arg_idx).map(|p| p.is_ref).unwrap_or(false);
                            }
                        }
                    }
                }
            }
        }
        // Receiver type: from `expr_types` (the normal route), falling back to
        // the name-keyed `local_types` when the receiver is a bare variable —
        // the latter is reliable inside string interpolation, where synthetic
        // spans make the `expr_types` lookup miss.
        let recv_ty_opt = self
            .expr_types
            .get(&crate::exprs::expr_span_of(&f.object))
            .cloned()
            .or_else(|| {
                if let juxc_ast::Expr::Path(qn) = &*f.object {
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
        let recv_ty = match recv_ty_opt {
            Some(juxc_tycheck::Ty::User { name, .. }) => name,
            Some(juxc_tycheck::Ty::Nullable(inner)) => match *inner {
                juxc_tycheck::Ty::User { name, .. } => name,
                _ => return false,
            },
            _ => return false,
        };
        let sig = if let Some(c) = self.symbols.classes.get(&recv_ty) {
            c
        } else {
            match self.lookup_class_by_bare_or_fqn(recv_ty.rsplit('.').next().unwrap_or(&recv_ty)) {
                Some(c) => c,
                None => return false,
            }
        };
        if !sig.is_external {
            return false;
        }
        sig.methods
            .get(f.field.text.as_str())
            .and_then(|m| m.params.get(arg_idx))
            .map(|p| p.is_ref)
            .unwrap_or(false)
    }

    // ============================================================
    // C6: foreign collection pass-by-`&mut` (Java container semantics)
    // ============================================================

    /// DISCOVERY predicate: does the parameter's declared type name a
    /// **non-`Copy`, EXTERNAL/foreign** type — a `.jux.d` stub class
    /// whose `ClassSig::is_external` is set (covers `rust.std` `Vec` /
    /// `HashMap` today and C/C++ FFI stubs later, through the SAME
    /// mechanism)? This is the "foreign type" half of the C6 rule.
    ///
    /// No hardcoded type-name list: the only signal consulted is the
    /// resolved class's `is_external` flag. Primitives, `String`,
    /// records, user classes, and the function's own generic params
    /// all resolve to NON-external (or to no class at all) and are
    /// rejected here, so they keep their current passing behavior.
    ///
    /// `generic_names` is the set of type-parameter names in scope for
    /// the declaring function/method/class — a bare param typed by one
    /// of them (`T`) is a generic, never a foreign concrete type.
    pub(crate) fn param_type_is_external(
        &self,
        ty: &juxc_ast::TypeRef,
        generic_names: &HashSet<String>,
    ) -> bool {
        // Pointer / array / function-typed params are never the
        // collection-handle shape; nullable foreign params keep their
        // (Option-wrapped) value path. Stay narrow — minimize blast
        // radius (the user was emphatic).
        if ty.ptr_depth > 0
            || ty.array_shape.is_some()
            || ty.fn_shape.is_some()
            || ty.nullable
        {
            return false;
        }
        let Some(last) = ty.name.segments.last().map(|s| s.text.as_str()) else {
            return false;
        };
        // A bare generic-parameter name (`T`) is not a concrete type.
        if ty.name.segments.len() == 1 && generic_names.contains(last) {
            return false;
        }
        // Resolve to a class signature and read the foreign flag. The
        // FQN spelling (`rust.std.Vec`) and the bare spelling (`Vec`)
        // both route through the package-aware lookup.
        self.lookup_class_by_bare_or_fqn(last)
            .map(|c| c.is_external)
            .unwrap_or(false)
    }

    /// DISCOVERY predicate: the full C6 test for ONE parameter of a
    /// function/method/constructor whose `body` is available. True iff
    ///
    ///   1. the parameter is an ordinary by-value binding (not
    ///      `final` / `out` / `ref` / `weak` / varargs — those carry
    ///      their own passing convention), AND
    ///   2. its declared type is external/foreign non-`Copy`
    ///      ([`Self::param_type_is_external`]), AND
    ///   3. the body MUTATES it — its name lands in
    ///      [`collect_mutated_names`], which already folds in
    ///      reassignment, index-assign, and any `@MutSelf` /
    ///      known-mutating method call on the binding (the latter via
    ///      `self.user_mut_methods`, seeded from the bindgen
    ///      `@MutSelf` markers — no hardcoded list).
    ///
    /// Read-only foreign params fail step 3 and keep their CURRENT
    /// by-value behavior, exactly as required.
    fn param_is_byref(
        &self,
        param: &juxc_ast::Param,
        mutated: &HashSet<String>,
        generic_names: &HashSet<String>,
    ) -> bool {
        if param.is_final
            || param.is_out
            || param.is_shared_ref
            || param.is_weak
            || param.is_varargs
        {
            return false;
        }
        if !mutated.contains(&param.name.text) {
            return false;
        }
        self.param_type_is_external(&param.ty, generic_names)
    }

    /// Compute, for one parameter list + body, the set of parameter
    /// indices that lower to `&mut T` under the C6 rule. Shared by the
    /// map pre-pass and the decl emitters so the same indices drive the
    /// signature and the call site.
    pub(crate) fn byref_param_indices(
        &self,
        params: &[juxc_ast::Param],
        body: &Block,
        generic_names: &HashSet<String>,
    ) -> HashSet<usize> {
        let mut mutated = HashSet::new();
        collect_mutated_names(body, &mut mutated, &self.user_mut_methods);
        let mut out = HashSet::new();
        for (i, p) in params.iter().enumerate() {
            if self.param_is_byref(p, &mutated, generic_names) {
                out.insert(i);
            }
        }
        out
    }

    /// Pre-pass: walk every compilation unit and record each
    /// function/method/constructor's by-`&mut` parameter indices into
    /// [`Self::byref_params`] under the shared key scheme. Idempotent —
    /// safe to call per-unit and over the whole workspace.
    pub(crate) fn populate_byref_params(&mut self, units: &[juxc_ast::CompilationUnit]) {
        for unit in units {
            // Stub units have no bodies to analyze.
            if unit.is_external {
                continue;
            }
            for item in &unit.items {
                match item {
                    juxc_ast::TopLevelDecl::Function(f) => {
                        let Some(body) = &f.body else { continue };
                        let generics: HashSet<String> = f
                            .generic_params
                            .iter()
                            .map(|g| g.name.text.clone())
                            .collect();
                        let idxs = self.byref_param_indices(&f.params, body, &generics);
                        if !idxs.is_empty() {
                            self.byref_params
                                .entry(format!("fn::{}", f.name.text))
                                .or_default()
                                .extend(idxs);
                        }
                    }
                    juxc_ast::TopLevelDecl::Class(c) => {
                        let class_generics: HashSet<String> = c
                            .generic_params
                            .iter()
                            .map(|g| g.name.text.clone())
                            .collect();
                        for m in &c.methods {
                            let Some(body) = &m.body else { continue };
                            let mut generics = class_generics.clone();
                            generics.extend(
                                m.generic_params.iter().map(|g| g.name.text.clone()),
                            );
                            let idxs = self.byref_param_indices(&m.params, body, &generics);
                            if !idxs.is_empty() {
                                self.byref_params
                                    .entry(format!("m::{}::{}", c.name.text, m.name.text))
                                    .or_default()
                                    .extend(idxs);
                            }
                        }
                        // NOTE: constructors are intentionally EXCLUDED from
                        // C6 by-`&mut` lowering. A constructor parameter is
                        // almost always forwarded INTO an owned field (the
                        // opposite of pass-by-ref intent), and the several
                        // ctor-emitter variants (plain / wrapper / delegating)
                        // each store params differently — supporting `&mut T`
                        // there would widen the blast radius for a vanishingly
                        // rare shape. Ctor params keep their current by-value
                        // behavior; decl and call site stay consistent because
                        // neither side ever finds a `ctor::` key here.
                        let _ = &c.constructors;
                    }
                    _ => {}
                }
            }
        }
    }

    /// C6 follow-up: after [`Self::populate_byref_params`] has built the
    /// full map, mark every CLASS METHOD whose body contains a
    /// self-aliasing by-`&mut` foreign-collection call
    /// (`this.m(this.field)`) as a receiver-mutating method (insert its
    /// name into `user_mut_methods`). The `std::mem::take` write-back
    /// form that lowers such a call assigns `self.field` back, so the
    /// enclosing method genuinely needs `&mut self` AND its callers must
    /// promote the receiver to `let mut` — both follow from
    /// `user_mut_methods` membership. Cheap: only fires when the map is
    /// non-empty (i.e. C6 is in play at all).
    pub(crate) fn mark_self_aliasing_mut_methods(
        &mut self,
        units: &[juxc_ast::CompilationUnit],
    ) {
        if self.byref_params.is_empty() {
            return;
        }
        let mut newly: HashSet<String> = HashSet::new();
        for unit in units {
            if unit.is_external {
                continue;
            }
            for item in &unit.items {
                if let juxc_ast::TopLevelDecl::Class(c) = item {
                    for m in &c.methods {
                        let Some(body) = &m.body else { continue };
                        let mut found = false;
                        self.scan_block_for_self_aliasing_byref(body, &mut found);
                        if found {
                            newly.insert(m.name.text.clone());
                        }
                    }
                }
            }
        }
        self.user_mut_methods.extend(newly);
    }

    /// Recursively scan `block`'s statements/expressions for a call
    /// `recv.M(recv.field, …)` whose argument is a C6 by-`&mut`
    /// foreign-collection slot AND a field rooted at the same receiver
    /// (the write-back shape). Sets `*found` on the first hit.
    pub(crate) fn scan_block_for_self_aliasing_byref(&self, block: &Block, found: &mut bool) {
        for stmt in &block.statements {
            if *found {
                return;
            }
            match stmt {
                Stmt::Expr(e) | Stmt::Throw(e, _) | Stmt::Return(Some(e)) => {
                    self.scan_expr_for_self_aliasing_byref(e, found)
                }
                Stmt::Assign(a) => {
                    self.scan_expr_for_self_aliasing_byref(&a.value, found);
                    self.scan_expr_for_self_aliasing_byref(&a.target, found);
                }
                Stmt::VarDecl(v) => {
                    if let Some(init) = &v.init {
                        self.scan_expr_for_self_aliasing_byref(init, found);
                    }
                }
                Stmt::If(i) => {
                    self.scan_expr_for_self_aliasing_byref(&i.condition, found);
                    self.scan_block_for_self_aliasing_byref(&i.then_block, found);
                    if let Some(eb) = &i.else_branch {
                        match eb.as_ref() {
                            ElseBranch::Block(b) => {
                                self.scan_block_for_self_aliasing_byref(b, found)
                            }
                            ElseBranch::If(nested) => {
                                let scratch = Block {
                                    statements: vec![Stmt::If(nested.clone())],
                                    span: juxc_source::Span::DUMMY,
                                };
                                self.scan_block_for_self_aliasing_byref(&scratch, found);
                            }
                        }
                    }
                }
                Stmt::While(w) => {
                    self.scan_expr_for_self_aliasing_byref(&w.condition, found);
                    self.scan_block_for_self_aliasing_byref(&w.body, found);
                }
                Stmt::DoWhile(d) => self.scan_block_for_self_aliasing_byref(&d.body, found),
                Stmt::ForEach(fe) => {
                    self.scan_expr_for_self_aliasing_byref(&fe.iter, found);
                    self.scan_block_for_self_aliasing_byref(&fe.body, found);
                }
                Stmt::ForC(fc) => self.scan_block_for_self_aliasing_byref(&fc.body, found),
                Stmt::Unsafe(b) => self.scan_block_for_self_aliasing_byref(b, found),
                Stmt::Try(t) => {
                    self.scan_block_for_self_aliasing_byref(&t.body, found);
                    for c in &t.catches {
                        self.scan_block_for_self_aliasing_byref(&c.body, found);
                    }
                    if let Some(fin) = &t.finally {
                        self.scan_block_for_self_aliasing_byref(fin, found);
                    }
                }
                Stmt::Labeled { stmt, .. } => {
                    let scratch = Block {
                        statements: vec![(**stmt).clone()],
                        span: juxc_source::Span::DUMMY,
                    };
                    self.scan_block_for_self_aliasing_byref(&scratch, found);
                }
                _ => {}
            }
        }
    }

    /// Expression half of [`Self::scan_block_for_self_aliasing_byref`].
    /// Only needs to reach `Call` nodes; recurses through the common
    /// value-carrying shapes (the conservative miss on an exotic nesting
    /// merely leaves a method off `user_mut_methods`, which a later
    /// rustc error would surface — but the common shapes are covered).
    fn scan_expr_for_self_aliasing_byref(&self, e: &Expr, found: &mut bool) {
        if *found {
            return;
        }
        fn root_first(p: &str) -> String {
            p.split('.').next().unwrap_or(p).to_string()
        }
        fn place_path(e: &Expr) -> Option<String> {
            match e {
                Expr::This(_) => Some("this".to_string()),
                Expr::Path(qn) if qn.segments.len() == 1 => Some(qn.segments[0].text.clone()),
                Expr::Field(f) => Some(format!("{}.{}", place_path(&f.object)?, f.field.text)),
                _ => None,
            }
        }
        if let Expr::Call(c) = e {
            if let Expr::Field(f) = &*c.callee {
                if let Some(recv) = place_path(&f.object) {
                    let recv_first = root_first(&recv);
                    for (i, arg) in c.args.iter().enumerate() {
                        if self.callee_byref_param(&c.callee, i)
                            && matches!(arg, Expr::Field(_))
                            && place_path(arg)
                                .map(|p| root_first(&p) == recv_first)
                                .unwrap_or(false)
                        {
                            *found = true;
                            return;
                        }
                    }
                }
            }
            self.scan_expr_for_self_aliasing_byref(&c.callee, found);
            for arg in &c.args {
                self.scan_expr_for_self_aliasing_byref(arg, found);
            }
            return;
        }
        match e {
            Expr::Binary(b) => {
                self.scan_expr_for_self_aliasing_byref(&b.left, found);
                self.scan_expr_for_self_aliasing_byref(&b.right, found);
            }
            Expr::Unary(u) => self.scan_expr_for_self_aliasing_byref(&u.operand, found),
            Expr::Field(f) => self.scan_expr_for_self_aliasing_byref(&f.object, found),
            Expr::Index(idx) => {
                self.scan_expr_for_self_aliasing_byref(&idx.array, found);
                self.scan_expr_for_self_aliasing_byref(&idx.index, found);
            }
            Expr::Cast(c) => self.scan_expr_for_self_aliasing_byref(&c.value, found),
            Expr::NotNullAssert(inner, _) | Expr::Await(inner, _) => {
                self.scan_expr_for_self_aliasing_byref(inner, found)
            }
            Expr::Ternary(t) => {
                self.scan_expr_for_self_aliasing_byref(&t.condition, found);
                self.scan_expr_for_self_aliasing_byref(&t.then_branch, found);
                self.scan_expr_for_self_aliasing_byref(&t.else_branch, found);
            }
            Expr::Elvis(el) => {
                self.scan_expr_for_self_aliasing_byref(&el.value, found);
                self.scan_expr_for_self_aliasing_byref(&el.fallback, found);
            }
            Expr::InterpString(s) => {
                for seg in &s.segments {
                    if let juxc_ast::InterpSegment::Expr(inner) = seg {
                        self.scan_expr_for_self_aliasing_byref(inner, found);
                    }
                }
            }
            Expr::NewObject(n) => {
                for a in &n.args {
                    self.scan_expr_for_self_aliasing_byref(a, found);
                }
            }
            _ => {}
        }
    }

    /// CALL-SITE mirror of the decl decision: does argument `arg_idx`
    /// of `callee` map to a parameter that was lowered to `&mut T`
    /// under the C6 rule? Reads the SAME [`Self::byref_params`] map the
    /// declaration consulted, so the `&mut <arg>` at the call exactly
    /// matches the `&mut T` in the signature. Handles free-function
    /// calls (`fill(v)`) and instance/static method calls
    /// (`x.fill(v)` / `Cls.fill(v)`).
    pub(crate) fn callee_byref_param(
        &self,
        callee: &juxc_ast::Expr,
        arg_idx: usize,
    ) -> bool {
        if self.byref_params.is_empty() {
            return false;
        }
        match callee {
            // Free function `name(args)`.
            juxc_ast::Expr::Path(qn) if qn.segments.len() == 1 => self
                .byref_params
                .get(&format!("fn::{}", qn.segments[0].text))
                .map(|s| s.contains(&arg_idx))
                .unwrap_or(false),
            // Method / static call `recv.method(args)`.
            juxc_ast::Expr::Field(f) => {
                let method = f.field.text.as_str();
                // Static `ClassName.method(...)`: receiver is a class name.
                if let juxc_ast::Expr::Path(qn) = &*f.object {
                    if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                        let bare = class_fqn.rsplit('.').next().unwrap_or(&class_fqn);
                        return self
                            .byref_params
                            .get(&format!("m::{bare}::{method}"))
                            .map(|s| s.contains(&arg_idx))
                            .unwrap_or(false);
                    }
                }
                // Instance `recv.method(...)`: resolve the receiver's class.
                if let Some(bare) = self.receiver_class_bare(&f.object) {
                    let bare = bare.rsplit('.').next().unwrap_or(&bare).to_string();
                    return self
                        .byref_params
                        .get(&format!("m::{bare}::{method}"))
                        .map(|s| s.contains(&arg_idx))
                        .unwrap_or(false);
                }
                false
            }
            _ => false,
        }
    }

    /// Look up the i-th formal parameter's declared type ref for
    /// the given callee expression. Mirrors
    /// [`Self::callee_param_is_nullable`] but returns the whole
    /// `TypeRef` so the caller can compare it against the arg's
    /// actual type for upcast detection. `None` when the callee
    /// can't be resolved or doesn't have a parameter at that
    /// position.
    pub(crate) fn callee_param_type(
        &self,
        callee: &juxc_ast::Expr,
        arg_idx: usize,
    ) -> Option<juxc_ast::TypeRef> {
        if let juxc_ast::Expr::Path(qn) = callee {
            if qn.segments.len() == 1 {
                if let Some((_, f)) = self.symbols.lookup_function(&qn.segments[0].text) {
                    return f.params.get(arg_idx).map(|p| p.ty.clone());
                }
            }
        }
        if let juxc_ast::Expr::Field(f) = callee {
            // Static method call: `ClassName.method(args)` — the
            // receiver path resolves to a class.
            if let juxc_ast::Expr::Path(qn) = &*f.object {
                if let Some(class_fqn) = self.path_resolves_to_class_in_emit(qn) {
                    if let Some(class) = self.symbols.classes.get(&class_fqn) {
                        if let Some(m) = class.methods.get(f.field.text.as_str()) {
                            return m.params.get(arg_idx).map(|p| p.ty.clone());
                        }
                    }
                }
            }
            // Instance method call: `recv.method(args)` where `recv`
            // is a value of some user class (`t.step(new Red(30))`).
            // Resolve the receiver's inferred type to its class, then
            // walk the `extends` chain for the method — so the param
            // type drives a sealed-upcast `.into()` wrap on the arg.
            // This is the path that fixes passing a permitted subclass
            // value (`Red`) into a sealed-parent param (`Light`).
            if let Some(juxc_tycheck::Ty::User { name, generic_args }) =
                self.expr_types.get(&crate::exprs::expr_span_of(&f.object))
            {
                let bare = name.rsplit('.').next().unwrap_or(name.as_str());
                // Receiver's concrete type args (`reg: Registry<User,
                // Container<User>, 16>` → [User, Container<User>, 16]). Used to
                // substitute the class's type params out of the resolved param
                // type so a `Sink<? super K>` slot lowers with `K = User`, not
                // a dangling `dyn K` (gap 5). Cloned up front because the
                // `lookup_class_by_bare_or_fqn` borrow below also touches
                // `self`.
                let recv_args: Vec<juxc_tycheck::Ty> = generic_args.clone();
                let mut cursor: Option<String> = Some(bare.to_string());
                let mut depth = 0usize;
                while let Some(cname) = cursor {
                    if depth > 64 {
                        break;
                    }
                    let Some(class) = self.lookup_class_by_bare_or_fqn(&cname) else {
                        break;
                    };
                    if let Some(m) = class.methods.get(f.field.text.as_str()) {
                        let pty = m.params.get(arg_idx).map(|p| p.ty.clone());
                        // Build param-name → concrete-arg substitution from the
                        // class's declared generic params zipped with the
                        // receiver's inferred args, and apply it. Only matters
                        // on the FIRST hierarchy level (the receiver's own
                        // class — the recv_args belong to it); ancestor levels
                        // would need their own arg mapping, which Phase 1
                        // doesn't track, so we substitute only at depth 0.
                        if depth == 0 {
                            if let Some(pty) = pty {
                                let mut subst: std::collections::HashMap<
                                    String,
                                    juxc_ast::TypeRef,
                                > = std::collections::HashMap::new();
                                for (param, arg) in
                                    class.generic_params.iter().zip(recv_args.iter())
                                {
                                    if let Some(arg_ref) = ty_to_type_ref(arg) {
                                        subst.insert(param.name.text.clone(), arg_ref);
                                    }
                                }
                                if subst.is_empty() {
                                    return Some(pty);
                                }
                                let substituted =
                                    crate::decls::classes::substitute_type_ref(&pty, &subst);
                                // Collapse wildcards over the now-concrete element
                                // (`Sink<? super User>` → `Sink<User>`). The method
                                // body lifts `? super K` / `? extends K` to the bare
                                // element `K` (WildcardLifter), so its signature param
                                // lowers to `Rc<dyn Sink<K>>`; the call-site arg must
                                // coerce to the SAME shape with `K = User`. Leaving the
                                // wildcard in place would lower it to `Rc<dyn UserKind>`
                                // and mismatch the method's `Sink<User>` (gap 5).
                                return Some(collapse_concrete_wildcards(&substituted));
                            }
                            return None;
                        }
                        return pty;
                    }
                    cursor = class
                        .extends
                        .as_ref()
                        .and_then(|t| t.name.segments.last().map(|s| s.text.clone()));
                    depth += 1;
                }
            }
        }
        None
    }

    /// True when `expr` would flow into a slot declared as
    /// `target_ty` and that slot is a sealed parent whose
    /// permits list names the expression's inferred class. Used
    /// by [`Self::arg_needs_sealed_upcast`] and the return /
    /// variable-init upcast paths. Factored out so the matching
    /// logic stays in one place.
    pub(crate) fn expr_needs_sealed_upcast_to(
        &self,
        target_ty: &juxc_ast::TypeRef,
        expr: &juxc_ast::Expr,
    ) -> bool {
        let Some(target_bare) = target_ty.name.segments.last().map(|s| s.text.as_str())
        else {
            return false;
        };
        // A polymorphic base never takes the slicing `.into()` upcast — its
        // value slots are `Rc<dyn …Kind>` and the wrap coercion
        // (`iface_coercion_to`) handles the upcast, pre-empting this path.
        if self.poly_base_classes.contains(target_bare) {
            return false;
        }
        let Some(parent_class) = self.lookup_class_by_bare_or_fqn(target_bare) else {
            return false;
        };
        let arg_span = crate::exprs::expr_span_of(expr);
        let Some(arg_ty) = self.expr_types.get(&arg_span) else {
            return false;
        };
        let arg_bare = match arg_ty {
            juxc_tycheck::Ty::User { name, .. } => name.split('.').next_back().unwrap_or(name),
            _ => return false,
        };
        if arg_bare == target_bare {
            return false;
        }
        // Sealed parent: arg must be in the explicit `permits`
        // list. The `From<Sub> for Sealed` impl wraps the
        // subclass into the matching enum variant.
        if parent_class.is_sealed {
            return parent_class.permits.iter().any(|p| p.as_str() == arg_bare);
        }
        // Non-sealed open parent: the auto-emitted
        // `From<Sub> for Parent` impl (see `emit_class_decl`)
        // extracts the parent slice via `.__parent`. Phase-1
        // caveat — this drops subclass identity at the boundary,
        // so an overridden method on the subclass doesn't fire
        // after the upcast. Use a sealed hierarchy for full
        // virtual dispatch.
        if let Some(arg_class) = self.lookup_class_by_bare_or_fqn(arg_bare) {
            if let Some(extends_fqn) = arg_class.extends_fqn.as_deref() {
                let parent_seg = extends_fqn
                    .rsplit('.')
                    .next()
                    .unwrap_or(extends_fqn);
                return parent_seg == target_bare;
            }
        }
        false
    }

    /// True when the enclosing function's return type is a sealed
    /// parent of the expression's inferred type — the return value
    /// needs `.into()` so the auto-`From<Sub> for Sealed` impl
    /// wraps the subclass into the matching enum variant before
    /// the value crosses the function boundary.
    pub(crate) fn return_needs_sealed_upcast(&self, expr: &juxc_ast::Expr) -> bool {
        let target_ty = match &self.current_return_type {
            Some(juxc_ast::ReturnType::Type(t)) => t.clone(),
            Some(juxc_ast::ReturnType::AsyncType(t)) => t.clone(),
            _ => return false,
        };
        self.expr_needs_sealed_upcast_to(&target_ty, expr)
    }

    /// True when the arg at `arg_idx` would be flowing into a slot
    /// of a SEALED parent class whose declared type differs from
    /// the arg's inferred type — i.e. a Java-style upcast site
    /// where we need to wrap the value via `.into()` so the emitted
    /// `From<Sub> for Sealed` impl lifts the subclass into the
    /// matching enum variant.
    ///
    /// Returns false (no wrap) when:
    ///   - The callee can't be resolved (defensive).
    ///   - The param's declared type isn't a user-defined class.
    ///   - The arg's inferred type already matches the param.
    ///   - The arg's class isn't a permitted subclass of the
    ///     param's sealed parent.
    pub(crate) fn arg_needs_sealed_upcast(
        &self,
        callee: &juxc_ast::Expr,
        arg_idx: usize,
        arg: &juxc_ast::Expr,
    ) -> bool {
        let Some(param_ty) = self.callee_param_type(callee, arg_idx) else {
            return false;
        };
        // Delegate to the general "param-typed slot wants `.into()`"
        // predicate, which now handles both sealed (variant wrap)
        // and non-sealed (`__parent` extraction) upcasts.
        self.expr_needs_sealed_upcast_to(&param_ty, arg)
    }

    /// How an expression value must be adapted to fit an **interface-typed
    /// value slot** (`Rc<dyn Trait>`). Stage-1 interface dispatch represents
    /// every interface value as a clone-able `Rc<dyn Trait>` trait object;
    /// concrete class values flowing into such a slot must be wrapped, and an
    /// interface value flowing on must be `Rc`-cloned.
    ///
    /// See [`Self::iface_coercion_to`] / [`Self::emit_expr_coerced_to_iface`].
    pub(crate) fn iface_coercion_to(
        &self,
        target_ty: &TypeRef,
        expr: &Expr,
    ) -> IfaceCoercion {
        // Array interface slots compose through their own wrappers and are
        // out of scope here. A `T?` **nullable** dyn slot, however, coerces its
        // INNER value (`Animal? a = new Dog()` → `Some(Rc<dyn AnimalKind>)`):
        // the nullable flag doesn't change the type *name*, so we leave it in
        // place and let [`Self::emit_expr_coerced_to_iface`] re-wrap the
        // coerced value in `Some(...)`.
        if target_ty.array_shape.is_some() {
            return IfaceCoercion::None;
        }
        // Concrete subclass → its direct base class under the non-sealed,
        // non-polymorphic open hierarchy (the `From<Sub> for Parent` slicing
        // model, e.g. exception causes). Detected here so every coercion call
        // site — args, returns, var-init, assignment, array elements — picks it
        // up through the same `!= None` guard the dyn cases use.
        if self.needs_base_into_upcast(target_ty, expr) {
            return IfaceCoercion::IntoBase;
        }
        // A nullable dyn slot only coerces a NON-null value being lifted into
        // the `Option`. A source that is *already* `Option`-shaped (`return
        // this.nullableField;`, `Animal? y = maybeAnimal()`) flows through
        // unchanged — `expr_types` may record a field read as non-nullable, so
        // coercing here would double-wrap (`Some(Some(...))`).
        if target_ty.nullable && self.expression_is_already_nullable(expr) {
            return IfaceCoercion::None;
        }
        let Some(target_bare) = target_ty.name.segments.last().map(|s| s.text.as_str()) else {
            return IfaceCoercion::None;
        };
        // The target slot is a dynamic-dispatch trait object — either an
        // interface (`Rc<dyn Iface>`) or a **polymorphic base class**
        // (`Rc<dyn <Name>Kind>`, Stage-2). Anything else stays concrete.
        let target_is_iface = self.lookup_interface_by_bare_or_fqn(target_bare).is_some();
        let target_is_polybase = self.poly_base_classes.contains(target_bare);
        if !target_is_iface && !target_is_polybase {
            return IfaceCoercion::None;
        }
        let Some(src_ty) = self.expr_types.get(&crate::exprs::expr_span_of(expr)) else {
            return IfaceCoercion::None;
        };
        // A **generic-param** source (`T occ` returned into an `Animal` slot,
        // where `T extends Animal`) is a concrete value at runtime that must be
        // wrapped into the trait object exactly like a concrete subtype:
        // `Rc::new(occ.clone()) as Rc<dyn Animal>`. The coercion is valid iff `T`'s
        // declared bound names the target interface / polymorphic base, which we
        // read from the in-scope `type_param_bounds`.
        if let juxc_tycheck::Ty::Param(pname) = src_ty {
            let bounded_by_target = self
                .type_param_bounds
                .get(pname)
                .map(|bounds| {
                    bounds.iter().any(|b| {
                        b.name.segments.last().map(|s| s.text.as_str()) == Some(target_bare)
                    })
                })
                .unwrap_or(false);
            if bounded_by_target {
                return IfaceCoercion::WrapClass {
                    clone_first: self.wrapper_value_needs_clone(expr),
                };
            }
            return IfaceCoercion::None;
        }
        let juxc_tycheck::Ty::User { name, .. } = src_ty else {
            return IfaceCoercion::None;
        };
        let src_bare = name.rsplit('.').next().unwrap_or(name);
        // Same-name source flowing into the trait-object slot.
        if src_bare == target_bare {
            // A *fresh construction* of the base type itself (`new Animal()`) into
            // its own polymorphic-base slot is a CONCRETE value, not an existing
            // `Rc<dyn Kind>` handle — it must be wrapped exactly like a subclass
            // instance (`Rc::new(Animal::new()) as Rc<dyn AnimalKind>`). Detect the
            // construction precisely (not just "non-place") so a call that already
            // returns the dyn base type isn't double-wrapped.
            if target_is_polybase && matches!(expr, Expr::NewObject(_)) {
                return IfaceCoercion::WrapClass {
                    clone_first: self.wrapper_value_needs_clone(expr),
                };
            }
            // Otherwise it's already a trait-object value of the same type →
            // clone the `Rc` handle.
            return IfaceCoercion::CloneDyn {
                clone_first: expr_is_place(expr),
            };
        }
        // A concrete subtype flowing into the trait-object slot → wrap it:
        //   - interface target: a class that *implements* the interface;
        //   - polymorphic-base target: a class that *extends* the base.
        // Try the inferred name as both an FQN key and a bare name (no-package).
        let relates = if target_is_iface {
            juxc_tycheck::ty::class_implements_interface(name, target_bare, &self.symbols)
                || juxc_tycheck::ty::class_implements_interface(src_bare, target_bare, &self.symbols)
        } else {
            juxc_tycheck::ty::walk_extends_reaches(src_bare, target_bare, &self.symbols)
        };
        if relates {
            return IfaceCoercion::WrapClass {
                clone_first: self.wrapper_value_needs_clone(expr),
            };
        }
        IfaceCoercion::None
    }

    /// Emit `expr` adapted into the interface value slot named by `target_ty`,
    /// or plain [`Self::emit_expr`] when no interface coercion applies.
    ///
    /// - **WrapClass** → `(std::rc::Rc::new(<expr>[.clone()]) as Rc<dyn Trait>)`.
    ///   The `as` performs the `Rc<C>` → `Rc<dyn Trait>` unsizing; a reused
    ///   wrapper place is `.clone()`d first (a cheap `Rc` bump that preserves
    ///   shared identity of the underlying object).
    /// - **CloneDyn** → `<expr>[.clone()]` — already a trait object; clone the
    ///   `Rc` handle when the source is a reused place.
    ///
    /// When `target_ty` is a **nullable** dyn slot (`T?`), the whole result is
    /// wrapped in `Some(...)` and the `as` cast uses the *peeled* (non-nullable)
    /// trait-object type — so `Animal? a = new Dog()` lowers to
    /// `Some(Rc::new(Dog) as Rc<dyn AnimalKind>)`. The call site must NOT add a
    /// second `Some(...)` (see the return / var-init suppression).
    /// True iff `expr` is a concrete subclass value flowing into a slot typed as
    /// its **direct** base class under the non-sealed, non-polymorphic open
    /// hierarchy — the exact case where the backend generated
    /// `impl From<Sub> for Parent { fn from(v) { v.__parent } }`, so a `.into()`
    /// slicing upcast is valid. Interfaces and polymorphic bases (the dyn model)
    /// are handled by [`Self::iface_coercion_to`] and excluded here. The `From`
    /// impl is one hop only, so this requires a *direct* parent match.
    pub(crate) fn needs_base_into_upcast(&self, target_ty: &TypeRef, expr: &Expr) -> bool {
        if target_ty.array_shape.is_some() || target_ty.fn_shape.is_some() {
            return false;
        }
        let Some(target_bare) = target_ty.name.segments.last().map(|s| s.text.as_str()) else {
            return false;
        };
        // Interfaces / polymorphic bases use the dyn-coercion path, not `From`.
        if self.lookup_interface_by_bare_or_fqn(target_bare).is_some()
            || self.poly_base_classes.contains(target_bare)
        {
            return false;
        }
        // A *sealed* target lowers to an enum with its own permit-based `From` +
        // coercion path — skip. A target that isn't a known user class (the
        // built-in `Exception` base, an external stub) is fine: the slicing
        // `From<Sub> for Parent` was still generated on the subclass side.
        if let Some(c) = self.lookup_class_by_bare_or_fqn(target_bare) {
            if c.is_sealed {
                return false;
            }
        }
        // The argument's static class must DIRECTLY extend the target class.
        // Resolve via `receiver_class_bare` so a local-variable argument (whose
        // type lives in `local_types`, not `expr_types`) is handled too.
        let Some(arg_bare) = self.receiver_class_bare(expr) else {
            return false;
        };
        if arg_bare == target_bare {
            return false;
        }
        let Some(sig) = self.lookup_class_by_bare_or_fqn(&arg_bare) else {
            return false;
        };
        let parent_bare = sig
            .extends_fqn
            .as_deref()
            .map(|p| p.rsplit('.').next().unwrap_or(p))
            .or_else(|| {
                sig.extends
                    .as_ref()
                    .and_then(|t| t.name.segments.last().map(|s| s.text.as_str()))
            });
        parent_bare == Some(target_bare)
    }

    pub(crate) fn emit_expr_coerced_to_iface(
        &mut self,
        target_ty: &TypeRef,
        expr: &Expr,
    ) {
        let coercion = self.iface_coercion_to(target_ty, expr);
        if matches!(coercion, IfaceCoercion::None) {
            self.emit_expr(expr);
            return;
        }
        // A nullable dyn slot owns its `Some(...)` wrap here so every call site
        // (return, var-init, args, fields, array elements) gets a complete
        // `Option<Rc<dyn …>>` value from one place.
        let nullable = target_ty.nullable;
        if nullable {
            self.w.push_str("Some(");
        }
        match coercion {
            IfaceCoercion::None => unreachable!("handled above"),
            IfaceCoercion::WrapClass { clone_first } => {
                self.w.push_str("(std::rc::Rc::new(");
                self.emit_expr(expr);
                if clone_first {
                    self.w.push_str(".clone()");
                }
                self.w.push_str(") as ");
                // Produces `std::rc::Rc<dyn Trait>` (value-position emission).
                // For a `T?` slot the `as` targets the INNER trait object, not
                // the `Option<…>` — peel the nullable for the cast type.
                if nullable {
                    let mut inner = target_ty.clone();
                    inner.nullable = false;
                    self.emit_value_type_as_rust(&inner);
                } else {
                    self.emit_value_type_as_rust(target_ty);
                }
                self.w.push(')');
            }
            IfaceCoercion::CloneDyn { clone_first } => {
                self.emit_expr(expr);
                if clone_first {
                    self.w.push_str(".clone()");
                }
            }
            IfaceCoercion::IntoBase => {
                // Slicing upcast: `(expr).into()` invokes the generated
                // `From<Sub> for Parent`. A nullable slot is wrapped in `Some(…)`
                // by the surrounding `nullable` handling.
                //
                // `.into()` CONSUMES the value. A bare place (local /
                // catch binder / field read) may still be used after
                // this statement (`k.last = e; print(e.getMessage());
                // throw e;` — S8), so clone the place into the upcast
                // instead of moving out of it. Every user class
                // derives `Clone`; fresh rvalues (ctor calls etc.)
                // skip the clone.
                self.w.push('(');
                self.emit_expr(expr);
                // Field reads already auto-clone in `emit_field`;
                // only a bare path needs the explicit copy here.
                if matches!(expr, Expr::Path(_)) {
                    self.w.push_str(".clone()");
                }
                self.w.push_str(").into()");
            }
        }
        if nullable {
            self.w.push(')');
        }
    }

    /// Wrap `arg` in `Some(arg)` if the target slot wants a
    /// nullable value and the expression isn't already
    /// `Option<T>`-shaped. Emits the wrapping parentheses; the
    /// caller calls `emit_expr` between the `Some(` and the `)`
    /// or — to keep call sites readable — relies on
    /// [`Self::emit_arg_with_nullable_wrap`] which does the whole
    /// arg emission in one call.
    pub(crate) fn emit_arg_with_nullable_wrap(
        &mut self,
        arg: &juxc_ast::Expr,
        target_is_nullable: bool,
    ) {
        let already_nullable = self.expression_is_already_nullable(arg);
        let wrap = target_is_nullable && !already_nullable;
        if wrap {
            self.w.push_str("Some(");
        }
        self.emit_expr(arg);
        if wrap {
            // **Wrapper share inside the `Some(...)` wrap (§CR.4.1).**
            // A wrapped PLACE flowing into a nullable slot must hand
            // over a shared handle, not the binding itself — `a.peer =
            // b;` followed by another use of `b` would otherwise be a
            // move-then-use (rustc E0382). The clone is the cheap `Rc`
            // refcount bump and goes INSIDE the wrap so the original
            // binding survives.
            if self.wrapper_value_needs_clone(arg) {
                self.w.push_str(".clone()");
            }
            self.w.push(')');
            return;
        }
        // Auto-clone when forwarding a *nullable local* into a
        // call's nullable arg slot. Rust's `Option<T>` isn't
        // `Copy`, so passing the bare path would move the local
        // — preventing further use by the surrounding function.
        // The clone restores Java-shape "the call borrows my
        // handle" ergonomics. Only fires for single-segment path
        // arguments referring to a binding in `nullable_locals`
        // — call-result paths (`f()`) and `?.` chains are fresh
        // values where cloning is wasteful.
        if target_is_nullable && already_nullable {
            if let juxc_ast::Expr::Path(qn) = arg {
                if qn.segments.len() == 1
                    && self.nullable_locals.contains(&qn.segments[0].text)
                {
                    self.w.push_str(".clone()");
                }
            }
        }
    }

    /// Emit `arg` as the body of a `println!` / `format!` value
    /// slot. When `arg` has nullable shape (Option<T>), wrap in
    /// the prelude's `JuxOpt(&value)` adapter so Display works —
    /// `Some(v)` prints as `v`, `None` as `"null"`. Non-nullable
    /// args pass straight through.
    pub(crate) fn emit_format_arg(&mut self, arg: &juxc_ast::Expr) {
        if self.expression_is_already_nullable(arg) {
            self.w.push_str("crate::JuxOpt(&");
            self.emit_expr(arg);
            self.w.push(')');
        } else {
            self.emit_expr(arg);
        }
    }
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
    /// Names of generic type params in scope at the rewrite site (the
    /// enclosing class's params plus the method/function's own). When a
    /// wildcard's bound NAMES one of these (`MyList<? extends E>`,
    /// `Sink<? super K>`), the param is substituted directly instead of
    /// minting a synthetic `__Wn: E` — a type param isn't a Rust trait, so
    /// the synthetic-bound form wouldn't compile.
    in_scope_params: std::collections::HashSet<String>,
}

impl WildcardLifter {
    pub(crate) fn new(in_scope_params: std::collections::HashSet<String>) -> Self {
        Self {
            new_params: Vec::new(),
            next: 0,
            in_scope_params,
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
            ptr_depth: ty.ptr_depth,
            span: ty.span,
        }
    }

    /// Mint a fresh `__Wn` TypeParam with the wildcard's bound and
    /// return a TypeRef pointing at it.
    ///
    /// - `? extends B` (covariant producer) → `<__Wn: B>`: every value
    ///   read out is a `B`, so the marker bound is exactly right.
    /// - `? super B` (contravariant consumer) → `<__Wn>` (UNBOUNDED):
    ///   Rust generics can't express "supertype of B", and reusing the
    ///   `B` bound would wrongly require `__Wn: BKind` — rejecting the
    ///   legal caller `Bag<Animal>` with `Animal: DogKind not satisfied`
    ///   (rustc E0277). An unbounded param accepts any concrete `Bag<X>`,
    ///   which is sound because Phase-1 wildcard values are pass-through
    ///   only (no member access / write-through the bound). A future
    ///   write-through phase would re-introduce a `From<B>`-style
    ///   constraint here.
    /// - bare `?` → `<__Wn>` (unbounded).
    ///
    /// Tycheck still enforces the variance distinction via PECS in
    /// `compatible`, so the relaxation here doesn't widen what type-checks.
    fn synthesize(&mut self, bound: &Option<WildcardBound>) -> TypeRef {
        // Substitute an IN-SCOPE type param named by the wildcard's bound
        // directly: `MyList<? extends E>` ⇒ `MyList<E>` (producer read as E),
        // `Sink<? super K>` ⇒ `Sink<K>` (consumer write as K). No synthetic
        // param — `__W: E`/`__W: K` is invalid Rust (params aren't traits).
        let inner: Option<&TypeRef> = match bound {
            Some(WildcardBound::Extends(b)) | Some(WildcardBound::Super(b)) => Some(b),
            None => None,
        };
        if let Some(b) = inner {
            if b.array_shape.is_none()
                && b.generic_args.is_empty()
                && b.name.segments.len() == 1
                && self.in_scope_params.contains(b.name.segments[0].text.as_str())
            {
                return b.clone();
            }
        }
        let name = format!("__W{}", self.next);
        self.next += 1;
        let bounds: Vec<TypeRef> = match bound {
            None => Vec::new(),
            Some(WildcardBound::Extends(b)) => vec![b.clone()],
            // `? super B` is contravariant — see the doc comment: an
            // unbounded param is the sound pass-through lowering.
            Some(WildcardBound::Super(_)) => Vec::new(),
        };
        let ident = Ident {
            text: name.clone(),
            span: Span::DUMMY,
        };
        self.new_params.push(TypeParam {
            name: ident.clone(),
            bounds,
            const_ty: None,
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
            ptr_depth: 0,
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
