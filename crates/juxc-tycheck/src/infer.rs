//! Phase C of the type checker — **expression inference**.
//!
//! [`infer_expr`] walks one [`Expr`] bottom-up and returns its inferred
//! [`Ty`]. [`infer_block`] walks a [`Block`] for its **side effects on
//! the env** — declaring locals as it descends through statements so
//! that subsequent expressions inside the block can be inferred against
//! the correct local-binding types.
//!
//! ## Silent failure
//!
//! Phase C is the inference phase, not the diagnostic phase. **No
//! diagnostics are emitted from this module.** When inference can't
//! determine a type — unknown name, unsupported expression shape,
//! field lookup on a non-class receiver — we return [`Ty::Unknown`]
//! and let Phase D produce the user-facing error at the point where
//! the type is actually needed.
//!
//! ## What the walker doesn't cover (yet)
//!
//! - **Arithmetic coercion** between operands of different numeric
//!   widths. We currently return the left operand's type for any
//!   arithmetic op; Phase D will introduce a real common-type rule.
//! - **Multi-segment paths**. `foo.bar.baz` as an expression returns
//!   `Unknown`. Once imports/qualified-name resolution lands this can
//!   resolve module-level constants or static members.
//! - **`Range` and `null` literals** — neither has a first-class type
//!   in the v1 spec; they stay `Unknown`.
//! - **Cross-extends generic substitution**. When `Dog extends
//!   Animal<int>` and `Animal<T>` exposes `get() -> T`, calling
//!   `d.get()` on a `Dog` still returns `Ty::Param("T")` rather than
//!   `int`. Substitution only fires when the member is declared on the
//!   receiver's own class — threading the extends-clause args needs a
//!   distinct pass that builds the full inheritance substitution chain.

use juxc_ast::{
    BinaryExpr, BinaryOp, Block, CallExpr, CastExpr, ElseBranch, Expr, FieldExpr,
    FloatKind, FloatLit, IndexExpr, IntKind, IntLit, Literal, NewArrayExpr, NewArrayLitExpr,
    NewObjectExpr, OperatorKind, ReturnType, Stmt, SwitchBody, TypeRef, UnaryExpr, UnaryOp,
};

use crate::env::TypeEnv;
use crate::symbol_table::{MethodSig, SymbolTable};
use crate::ty::{
    compose_extends_substitution, infer_generic_args, lower_member_type, substitute,
    substitute_via_inference, ty_from_ref, ArrayKind, Primitive, Ty,
};

// ============================================================================
// Expression inference
// ============================================================================

/// Infer the type of `expr` against `env` and `symbols`.
///
/// Returns [`Ty::Unknown`] for any expression the walker can't yet
/// figure out — never panics, never emits diagnostics. See the module
/// doc for the full coverage table.
pub fn infer_expr(expr: &Expr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    match expr {
        Expr::Literal(lit) => infer_literal(lit),
        Expr::Path(qn) => {
            // Single-segment path → look up as a local. Multi-segment
            // paths could resolve to enum-variants or imported names,
            // but neither is wired up yet — both yield Unknown.
            if qn.segments.len() == 1 {
                let name = &qn.segments[0].text;
                if let Some(ty) = env.lookup(name) {
                    return ty.clone();
                }
            }
            Ty::Unknown
        }
        Expr::This(_) => infer_this(env),
        Expr::Field(f) => infer_field(f, env, symbols),
        Expr::Index(i) => infer_index(i, env, symbols),
        Expr::Call(c) => infer_call(c, env, symbols),
        Expr::NewObject(n) => infer_new_object(n, env, symbols),
        Expr::NewArray(n) => infer_new_array(n, env, symbols),
        Expr::NewArrayLit(n) => infer_new_array_lit(n, env, symbols),
        Expr::Cast(c) => infer_cast(c, env, symbols),
        Expr::Range(_) => Ty::Unknown,
        Expr::Unary(u) => infer_unary(u, env, symbols),
        Expr::Binary(b) => infer_binary(b, env, symbols),
        Expr::SizeOf(_) => Ty::Primitive(Primitive::Int),
        Expr::InterpString(_) => Ty::String,
        Expr::Switch(s) => {
            // The arm-unification work is Phase D's job. For now we
            // pick the first arm's body type as a representative —
            // good enough for downstream code that just wants *some*
            // type to forward.
            if let Some(first) = s.arms.first() {
                match &first.body {
                    SwitchBody::Expr(e) => infer_expr(e, env, symbols),
                    SwitchBody::Block(_) => Ty::Void,
                }
            } else {
                Ty::Unknown
            }
        }
        // Lambda — Phase-1 returns `Ty::Unknown`. A proper
        // `Ty::Fn { params, return }` lands when call-site type
        // checking actually consumes the result (e.g. when
        // passing a lambda to a `Fn`-typed param). Today the
        // emitted Rust closure infers its own type at compile
        // time, so the lack of a precise Jux-side type is
        // observationally a no-op.
        Expr::Lambda(_) => Ty::Unknown,
    }
}

/// Map a literal onto its Ty.
///
/// - **Int**: the suffix decides — `42L` → `long`, `42u` → `uint`, etc.
///   Unsuffixed → `int`.
/// - **Float**: `1.5f` → `float`, otherwise `double`.
/// - **String**: always `Ty::String`.
/// - **Bool**: `Ty::Primitive(Bool)`.
/// - **Null**: `Unknown` — Jux doesn't have a first-class null type.
fn infer_literal(lit: &Literal) -> Ty {
    match lit {
        Literal::Int(IntLit { kind, .. }) => Ty::Primitive(primitive_from_int_kind(*kind)),
        Literal::Float(FloatLit { kind, .. }) => Ty::Primitive(primitive_from_float_kind(*kind)),
        Literal::String(_) => Ty::String,
        Literal::Bool(_) => Ty::Primitive(Primitive::Bool),
        Literal::Null => Ty::Unknown,
    }
}

/// Translate the lexer-supplied int-suffix into a [`Primitive`]. Used
/// only by [`infer_literal`].
fn primitive_from_int_kind(kind: Option<IntKind>) -> Primitive {
    match kind {
        None => Primitive::Int,
        Some(IntKind::Byte) => Primitive::Byte,
        Some(IntKind::UByte) => Primitive::Ubyte,
        Some(IntKind::Short) => Primitive::Short,
        Some(IntKind::UShort) => Primitive::Ushort,
        Some(IntKind::UInt) => Primitive::Uint,
        Some(IntKind::Long) => Primitive::Long,
        Some(IntKind::ULong) => Primitive::Ulong,
    }
}

/// Translate the lexer-supplied float-suffix into a [`Primitive`].
fn primitive_from_float_kind(kind: Option<FloatKind>) -> Primitive {
    match kind {
        None => Primitive::Double,
        Some(FloatKind::Float) => Primitive::Float,
    }
}

/// `this` inside a class context lowers to `Ty::User { name: <class>, … }`
/// with each in-scope generic parameter materialized as a
/// [`Ty::Param`]. Outside a class context we return `Unknown` —
/// the parser already rejects `this` outside a class, but we stay
/// silent here per Phase C's no-diagnostics rule.
///
/// Generic-arg ordering note: the env stores generic params in a
/// `HashSet`, which has no defined iteration order, so the args list
/// we produce here is **unordered**. That's acceptable for Phase C —
/// downstream code that cares about ordering (e.g. Phase D's
/// signature unification) will need to read the params off the
/// symbol table directly.
fn infer_this(env: &TypeEnv) -> Ty {
    match &env.current_class {
        Some(name) => {
            let generic_args = env
                .generic_params
                .iter()
                .map(|p| Ty::Param(p.clone()))
                .collect();
            Ty::User {
                name: name.clone(),
                generic_args,
            }
        }
        None => Ty::Unknown,
    }
}

/// `object.field`. Three shapes are recognized:
///
/// 1. **`.length` on an array** — every array carries a `length` of
///    type `int`. Special-cased before consulting the symbol table.
/// 2. **Field on a user class** — walks the `extends` chain
///    ([`SymbolTable::lookup_field`]) so a `Dog extends Animal` can read
///    Animal's fields. When the field is declared on the receiver's own
///    class AND the receiver carries concrete generic arguments, the
///    field's type is **substituted** through the receiver's generic
///    args before being returned: a `Box<int>` with `T value` reads as
///    `int`, not `Ty::Param("T")`.
/// 3. **Component on a record** — same idea, but records have no
///    inheritance, so no chain walk. Substitution still applies when
///    the record is generic and the receiver carries arguments.
///
/// Everything else (field on a primitive, field on an enum, etc.)
/// returns `Unknown`.
fn infer_field(f: &FieldExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    // `ClassName.STATIC_FIELD` — when the receiver is a bare or
    // multi-segment path that resolves to a class FQN, look the
    // field up as a static member rather than as an instance field.
    // Instance access `obj.f` still flows through the regular
    // `infer_expr` path below; the two are distinguished by
    // whether the receiver expression names a type or a value.
    if let Expr::Path(qn) = f.object.as_ref() {
        if let Some(class_fqn) = path_resolves_to_class(qn, env, symbols) {
            if let Some(class) = symbols.classes.get(&class_fqn) {
                if let Some(field) = class.fields.get(f.field.text.as_str()) {
                    if field.is_static {
                        return lower_member_type(&field.ty, &class_fqn, symbols);
                    }
                }
            }
        }
    }
    let object_ty = infer_expr(&f.object, env, symbols);
    let field_name = f.field.text.as_str();

    // `.length` on any array → int.
    if let Ty::Array { .. } = &object_ty {
        if field_name == "length" {
            return Ty::Primitive(Primitive::Int);
        }
    }

    // Field on a user type.
    if let Ty::User { name, generic_args } = &object_ty {
        if let Some((field, declaring_class)) = symbols.lookup_field(name, field_name) {
            // Lower in the declaring class's generic-param scope so a
            // `T value;` field reads as `Ty::Param("T")` rather than
            // `Unknown` when we're outside Box's body.
            let raw = lower_member_type(&field.ty, declaring_class, symbols);
            // Compose the substitution through the extends-chain so a
            // child's `extends Parent<int>` propagates `T → int` onto
            // an inherited field. When declaring_class == name the
            // composition reduces to one hop (the receiver's own
            // scope) and behaves like the previous direct path.
            if let Some((params, args)) = compose_extends_substitution(
                name,
                generic_args,
                declaring_class,
                symbols,
            ) {
                return substitute(&raw, &params, &args);
            }
            return raw;
        }
        if let Some(record) = symbols.records.get(name) {
            if let Some(component) = record.components.iter().find(|c| c.name == field_name) {
                let raw = lower_member_type(&component.ty, name, symbols);
                return substitute(&raw, &record.generic_params, generic_args);
            }
        }
    }

    Ty::Unknown
}

/// `array[index]` returns the array's element type, or Unknown when
/// the LHS doesn't infer to an array.
fn infer_index(i: &IndexExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    let array_ty = infer_expr(&i.array, env, symbols);
    match array_ty {
        Ty::Array { element, .. } => *element,
        _ => Ty::Unknown,
    }
}

/// `callee(args…)`. Two callee shapes are handled:
///
/// 1. **Bare single-segment path** — looks up a top-level function in
///    `symbols.functions` and returns its declared return type.
/// 2. **Field-on-receiver** — looks up a method via the
///    [`SymbolTable::lookup_method`] inheritance walk (for classes) or
///    by direct name (for interfaces). When the method is found on the
///    receiver's own class and the receiver carries concrete generic
///    arguments, the return type is substituted through them — a
///    `Box<int>::get()` reads as `int` rather than `Ty::Param("T")`.
///
/// Anything else (call on a `Call` result, call on an `Index`, etc.)
/// returns `Unknown`. Overload resolution (multiple methods sharing a
/// name) lands in a later phase — the symbol-table builder still
/// rejects duplicates with `E0402`, so today there's at most one
/// candidate per name.
fn infer_call(c: &CallExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    match c.callee.as_ref() {
        // Top-level function — `helper(x)`.
        Expr::Path(qn) if qn.segments.len() == 1 => {
            let name = &qn.segments[0].text;
            if let Some(fn_sig) = symbols.functions.get(name) {
                // Generic inference (spec §T.4): when the callee is
                // generic and the call site didn't write explicit
                // `<…>`, try to recover the type args from the
                // argument types. Only the bare-param-name shape is
                // handled — see `infer_generic_args` for the rules.
                if fn_sig.generic_params.is_empty() {
                    return return_type_to_ty(&fn_sig.return_type, env, symbols);
                }
                // Lower the return type in a scratch env that has the
                // function's own generic params in scope — otherwise a
                // bare `T` return type lowers to `Ty::Unknown` in the
                // caller's env and substitution has nothing to grab.
                let base = return_type_to_ty_in_fn_scope(
                    &fn_sig.return_type,
                    &fn_sig.generic_params,
                    env,
                    symbols,
                );
                let param_tys: Vec<&TypeRef> =
                    fn_sig.params.iter().map(|p| &p.ty).collect();
                let arg_tys: Vec<Ty> = c
                    .args
                    .iter()
                    .map(|a| infer_expr(a, env, symbols))
                    .collect();
                let inferred = infer_generic_args(
                    &fn_sig.generic_params,
                    &param_tys,
                    &arg_tys,
                );
                return substitute_via_inference(
                    &base,
                    &fn_sig.generic_params,
                    &inferred,
                );
            }
            Ty::Unknown
        }
        // Method call — `obj.method(args)`.
        Expr::Field(field) => {
            let method_name = field.field.text.as_str();
            // `ClassName.staticMethod(args)` — receiver is a type
            // name, not a value. Resolve the static method
            // directly off the class's signature and return its
            // declared return type (lowered in the class's scope).
            if let Expr::Path(qn) = field.object.as_ref() {
                if let Some(class_fqn) = path_resolves_to_class(qn, env, symbols) {
                    if let Some(class) = symbols.classes.get(&class_fqn) {
                        if let Some(method) = class.methods.get(method_name) {
                            if method.is_static {
                                return return_type_in_class(
                                    &method.return_type,
                                    &class_fqn,
                                    symbols,
                                );
                            }
                        }
                    }
                }
            }
            let receiver_ty = infer_expr(&field.object, env, symbols);
            if let Ty::User { name, generic_args } = &receiver_ty {
                // Walk the class extends-chain first.
                if let Some((method, declaring_class)) =
                    symbols.lookup_method(name, method_name)
                {
                    // Lower in the declaring class's generic scope so
                    // `T get()` reads as `Param("T")`, not `Unknown`.
                    let raw = return_type_in_class(
                        &method.return_type,
                        declaring_class,
                        symbols,
                    );
                    // Compose the substitution through the
                    // extends-chain (see `infer_field` for the same
                    // pattern). For a direct method on the receiver
                    // this collapses to a single hop.
                    let after_class = match compose_extends_substitution(
                        name,
                        generic_args,
                        declaring_class,
                        symbols,
                    ) {
                        Some((params, args)) => substitute(&raw, &params, &args),
                        None => raw,
                    };
                    return method_infer_return(
                        &after_class,
                        method,
                        declaring_class,
                        &c.args,
                        env,
                        symbols,
                    );
                }
                // Record methods — records can declare methods per
                // grammar §A.2.4. No inheritance chain (records don't
                // extend), but substitution applies for the record's
                // own generic params.
                if let Some(record) = symbols.records.get(name) {
                    if let Some(method) = record.methods.get(method_name) {
                        let raw = return_type_in_class(
                            &method.return_type,
                            name,
                            symbols,
                        );
                        let after_class =
                            substitute(&raw, &record.generic_params, generic_args);
                        return method_infer_return(
                            &after_class,
                            method,
                            name,
                            &c.args,
                            env,
                            symbols,
                        );
                    }
                }
                // Interface methods. No chain (interfaces don't extend
                // classes), but substitution still applies for the
                // interface's own generic params.
                if let Some(iface) = symbols.interfaces.get(name) {
                    if let Some(method) = iface.methods.get(method_name) {
                        let raw = return_type_in_class(
                            &method.return_type,
                            name,
                            symbols,
                        );
                        let after_class =
                            substitute(&raw, &iface.generic_params, generic_args);
                        return method_infer_return(
                            &after_class,
                            method,
                            name,
                            &c.args,
                            env,
                            symbols,
                        );
                    }
                }
            }
            Ty::Unknown
        }
        _ => Ty::Unknown,
    }
}

/// Apply method-level generic inference (spec §T.4) to a return type
/// that has already had the receiver's class-level generics
/// substituted. The method's own generic params come from
/// `method.generic_params`; we only fire on the bare-param-name shape
/// in [`infer_generic_args`].
fn method_infer_return(
    after_class: &Ty,
    method: &MethodSig,
    _declaring_owner: &str,
    args: &[Expr],
    env: &TypeEnv,
    symbols: &SymbolTable,
) -> Ty {
    if method.generic_params.is_empty() {
        return after_class.clone();
    }
    let param_tys: Vec<&TypeRef> = method.params.iter().map(|p| &p.ty).collect();
    let arg_tys: Vec<Ty> = args.iter().map(|a| infer_expr(a, env, symbols)).collect();
    let inferred = infer_generic_args(&method.generic_params, &param_tys, &arg_tys);
    substitute_via_inference(after_class, &method.generic_params, &inferred)
}

/// Lower a [`ReturnType`] into a [`Ty`]. `void` → [`Ty::Void`]; the
/// `async T` form unwraps to `T` for now (we don't have a `Future<T>`
/// wrapper type yet).
fn return_type_to_ty(rt: &ReturnType, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    match rt {
        ReturnType::Void => Ty::Void,
        ReturnType::Type(t) | ReturnType::AsyncType(t) => ty_from_ref(t, env, symbols),
    }
}

/// Like [`return_type_to_ty`] but extends the caller's env with the
/// callee function's generic params before lowering. Without this, a
/// bare `T` return type on `T identity<T>(T x)` lowers to
/// [`Ty::Unknown`] in the caller's env — there's no class to lower
/// against, so we have to seed the scratch env explicitly.
fn return_type_to_ty_in_fn_scope(
    rt: &ReturnType,
    fn_generics: &[juxc_ast::TypeParam],
    _caller_env: &TypeEnv,
    symbols: &SymbolTable,
) -> Ty {
    let mut scratch = TypeEnv::new();
    for tp in fn_generics {
        scratch.add_generic_param(&tp.name.text);
    }
    match rt {
        ReturnType::Void => Ty::Void,
        ReturnType::Type(t) | ReturnType::AsyncType(t) => ty_from_ref(t, &scratch, symbols),
    }
}

/// Same as [`return_type_to_ty`] but lowers in the **declaring** type's
/// generic-param scope, via [`lower_member_type`]. Used when the
/// caller's `env` doesn't carry the declaring class's params — e.g.
/// when inferring `box.get()` from outside Box's body.
fn return_type_in_class(rt: &ReturnType, declaring_class: &str, symbols: &SymbolTable) -> Ty {
    match rt {
        ReturnType::Void => Ty::Void,
        ReturnType::Type(t) | ReturnType::AsyncType(t) => {
            lower_member_type(t, declaring_class, symbols)
        }
    }
}

/// `new Foo(args)` / `new Box<int>(arg)` → [`Ty::User`] with the
/// class's name and each explicit generic arg resolved via
/// [`ty_from_ref`]. We don't infer generic args from the call's
/// argument list yet — `new Box(42)` produces `Box<>` (empty args
/// list) and Phase D / Rust will pick that up.
fn infer_new_object(n: &NewObjectExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    // Resolve the class name to its FQN. Single-segment names go
    // through `env.unqualified` (the same map `ty_from_ref` uses
    // for type-position resolution). Multi-segment names are taken
    // verbatim as a dot-joined FQN. Falls back to the bare name
    // when neither resolves.
    let name = resolve_class_name(&n.class_name, env, symbols);
    // Explicit `<...>` on the `new` site wins: `new Box<int>(42)`
    // skips inference entirely.
    if !n.generic_args.is_empty() {
        let generic_args = n
            .generic_args
            .iter()
            .map(|g| ty_from_ref(g, env, symbols))
            .collect();
        return Ty::User { name, generic_args };
    }
    // Bare-form inference (spec §T.4): when the class/record has
    // generic params but the user didn't write them, infer from the
    // constructor arg types.
    let generic_args = infer_ctor_generic_args(&name, &n.args, env, symbols);
    Ty::User { name, generic_args }
}

/// Resolve a `new X(...)` or similar class-name reference to an FQN.
/// Single-segment names consult `env.unqualified` (same-package
/// siblings + imports). Multi-segment names are joined with `.` and
/// taken as already-qualified. Falls back to the literal name when
/// no entry resolves, matching the pre-FQN behavior for top-level
/// no-package builds.
/// True if `qn` names a known class (returning its FQN). Used by
/// `infer_field` / `infer_call` to recognize `ClassName.member` as
/// a static-member access rather than an instance field/method on
/// some value of type `ClassName`. Single-segment names route
/// through the unit's bare→FQN map; multi-segment names are
/// joined verbatim.
pub(crate) fn path_resolves_to_class(
    qn: &juxc_ast::QualifiedName,
    env: &TypeEnv,
    symbols: &SymbolTable,
) -> Option<String> {
    if qn.segments.is_empty() {
        return None;
    }
    if qn.segments.len() == 1 {
        let bare = &qn.segments[0].text;
        // Locals shadow type names (e.g. a local `Foo` would beat
        // the class `Foo` in expression scope) — Java rules don't
        // really cover this since type names start with uppercase
        // by convention, but be safe.
        if env.lookup(bare).is_some() {
            return None;
        }
        if let Some(fqn) = env.unqualified.get(bare) {
            if symbols.classes.contains_key(fqn) {
                return Some(fqn.clone());
            }
        }
        if symbols.classes.contains_key(bare) {
            return Some(bare.clone());
        }
        return None;
    }
    let joined: String = qn
        .segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".");
    if symbols.classes.contains_key(&joined) {
        return Some(joined);
    }
    None
}

fn resolve_class_name(
    qn: &juxc_ast::QualifiedName,
    env: &TypeEnv,
    symbols: &SymbolTable,
) -> String {
    if qn.segments.is_empty() {
        return String::new();
    }
    let fqn = if qn.segments.len() == 1 {
        let bare = &qn.segments[0].text;
        if let Some(fqn) = env.unqualified.get(bare) {
            if symbols.is_type_name(fqn) {
                fqn.clone()
            } else {
                bare.clone()
            }
        } else {
            bare.clone()
        }
    } else {
        qn.segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(".")
    };
    // Follow type aliases — `new Alias(args)` should land on the
    // underlying class. Walks at most a small chain (capped at 16)
    // to avoid runaway expansion on malformed aliases. Bare-name
    // resolution inside the alias's target uses the **declaring
    // unit's** context — important when the alias lives in a
    // different package than the call site (the call site never
    // imported the target name, only the alias).
    let mut cursor = fqn;
    for _ in 0..16 {
        let Some(alias) = symbols.aliases.get(&cursor) else {
            return cursor;
        };
        let alias_ctx = alias
            .unit_index
            .and_then(|idx| symbols.units.get(idx));
        let target_fqn = if alias.target.name.segments.len() == 1 {
            let bare = &alias.target.name.segments[0].text;
            // Prefer the alias's declaring unit's resolver, fall
            // back to the caller's (legacy single-file path).
            alias_ctx
                .and_then(|ctx| ctx.unqualified.get(bare))
                .or_else(|| env.unqualified.get(bare))
                .cloned()
                .unwrap_or_else(|| bare.clone())
        } else {
            alias
                .target
                .name
                .segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(".")
        };
        if !symbols.is_type_name(&target_fqn) {
            return cursor;
        }
        cursor = target_fqn;
    }
    cursor
}

/// Infer a class or record's generic-arg list from the constructor
/// call's actual arg types. Returns an empty vec when the named type
/// isn't generic (or isn't known), so callers can still build a
/// `Ty::User` with the raw shape Phase 1 already accepts.
fn infer_ctor_generic_args(
    name: &str,
    args: &[Expr],
    env: &TypeEnv,
    symbols: &SymbolTable,
) -> Vec<Ty> {
    // Classes: use the first constructor's params as the "shape" to
    // infer against. Phase 1 doesn't support constructor overloads,
    // so this is unambiguous when it resolves.
    if let Some(class) = symbols.classes.get(name) {
        if class.generic_params.is_empty() {
            return Vec::new();
        }
        let Some(ctor) = class.constructors.first() else {
            // No constructor at all — synthesized default takes zero
            // args, so there's nothing to unify on. Leave generics
            // unbound; downstream the wildcard rule in `compatible`
            // keeps things quiet.
            return Vec::new();
        };
        let param_tys: Vec<&TypeRef> = ctor.params.iter().map(|p| &p.ty).collect();
        let arg_tys: Vec<Ty> = args.iter().map(|a| infer_expr(a, env, symbols)).collect();
        let inferred = infer_generic_args(&class.generic_params, &param_tys, &arg_tys);
        return class
            .generic_params
            .iter()
            .map(|p| inferred.get(&p.name.text).cloned().unwrap_or(Ty::Unknown))
            .collect();
    }
    // Records: header components ARE the canonical constructor.
    if let Some(record) = symbols.records.get(name) {
        if record.generic_params.is_empty() {
            return Vec::new();
        }
        let param_tys: Vec<&TypeRef> = record.components.iter().map(|c| &c.ty).collect();
        let arg_tys: Vec<Ty> = args.iter().map(|a| infer_expr(a, env, symbols)).collect();
        let inferred = infer_generic_args(&record.generic_params, &param_tys, &arg_tys);
        return record
            .generic_params
            .iter()
            .map(|p| inferred.get(&p.name.text).cloned().unwrap_or(Ty::Unknown))
            .collect();
    }
    Vec::new()
}

/// `new T[size]` → fixed-size array of `T`.
fn infer_new_array(n: &NewArrayExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    Ty::Array {
        element: Box::new(ty_from_ref(&n.element_type, env, symbols)),
        kind: ArrayKind::Fixed,
    }
}

/// `new T[]{…}` or `T[]{…}`. Picks Fixed vs Dynamic per the AST node's
/// `fixed` flag — the parser stamps that based on the LHS context.
fn infer_new_array_lit(n: &NewArrayLitExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    let kind = if n.fixed {
        ArrayKind::Fixed
    } else {
        ArrayKind::Dynamic
    };
    Ty::Array {
        element: Box::new(ty_from_ref(&n.element_type, env, symbols)),
        kind,
    }
}

/// `value as T` always evaluates to `T` regardless of `value`'s type
/// — cast-validity is Phase D's job.
fn infer_cast(c: &CastExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    ty_from_ref(&c.ty, env, symbols)
}

/// Unary operators:
/// - `!x` → bool (Jux's `!` is logical-NOT only on booleans).
/// - `-x`, `~x` → same type as operand.
fn infer_unary(u: &UnaryExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    // Operator-dispatch first — if the operand is a user type whose
    // matching operator is defined (and not deleted), the operator's
    // declared return type wins over the built-in bucket rule.
    let operand_ty = infer_expr(&u.operand, env, symbols);
    if let Some(kind) = unary_op_to_kind(u.op) {
        if let Some(ret) = lookup_user_operator_return_type(&operand_ty, kind, env, symbols) {
            return ret;
        }
    }
    match u.op {
        UnaryOp::Not => Ty::Primitive(Primitive::Bool),
        UnaryOp::Neg | UnaryOp::BitNot => operand_ty,
    }
}

/// Map a [`UnaryOp`] to its overloadable [`OperatorKind`], if any.
/// `!x` isn't overridable per spec §O.2.5.
fn unary_op_to_kind(op: UnaryOp) -> Option<OperatorKind> {
    Some(match op {
        UnaryOp::Neg => OperatorKind::Minus,
        UnaryOp::BitNot => OperatorKind::BitNot,
        UnaryOp::Not => return None,
    })
}

/// Binary operators bucket into three result-type groups:
/// - Comparison (`<`, `<=`, `>`, `>=`, `==`, `!=`) → `bool`.
/// - Logical (`&&`, `||`) → `bool`.
/// - Arithmetic / bitwise / shift → the **left** operand's type.
///
/// The arithmetic rule is intentionally simple; a proper common-type
/// rule (promoting `int + long` to `long`, etc.) lands in Phase D.
fn infer_binary(b: &BinaryExpr, env: &TypeEnv, symbols: &SymbolTable) -> Ty {
    // Operator-dispatch first — if the LHS is a user type whose
    // matching operator is defined, the operator's declared return
    // type takes precedence over the built-in bucket rule. Comparison
    // ops (`==`, `<`, etc.) almost always return bool either way, so
    // this mostly matters for arithmetic/bitwise/shift where a class
    // could declare a return type different from its LHS type.
    let left_ty = infer_expr(&b.left, env, symbols);
    if let Some(kind) = binary_op_to_kind(b.op) {
        if let Some(ret) = lookup_user_operator_return_type(&left_ty, kind, env, symbols) {
            return ret;
        }
    }
    match b.op {
        BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::Lt
        | BinaryOp::Le
        | BinaryOp::Gt
        | BinaryOp::Ge
        | BinaryOp::And
        | BinaryOp::Or => Ty::Primitive(Primitive::Bool),
        // Arithmetic, bitwise, shift — take the left operand's type.
        // Phase D will promote when operands differ in width.
        BinaryOp::Add
        | BinaryOp::Sub
        | BinaryOp::Mul
        | BinaryOp::Div
        | BinaryOp::Rem
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::BitAnd
        | BinaryOp::Shl
        | BinaryOp::Shr => left_ty,
    }
}

/// Map a [`BinaryOp`] to the [`OperatorKind`] that would override it,
/// if any. Returns `None` for ops that aren't user-overridable
/// (`&&`/`||`) or that auto-derive from another op (`!=` from `==`,
/// the four orderings from `<=>`). Phase-1 simplification: only the
/// primary form is dispatched — a user with only `<=>` declared
/// won't see operator-dispatch on `<`/`<=`/`>`/`>=` at the tycheck
/// level (the Rust trait layer covers that via PartialOrd's default
/// methods).
fn binary_op_to_kind(op: BinaryOp) -> Option<OperatorKind> {
    Some(match op {
        BinaryOp::Eq => OperatorKind::Eq,
        BinaryOp::Add => OperatorKind::Plus,
        BinaryOp::Sub => OperatorKind::Minus,
        BinaryOp::Mul => OperatorKind::Mul,
        BinaryOp::Div => OperatorKind::Div,
        BinaryOp::Rem => OperatorKind::Rem,
        BinaryOp::BitAnd => OperatorKind::BitAnd,
        BinaryOp::BitOr => OperatorKind::BitOr,
        BinaryOp::BitXor => OperatorKind::BitXor,
        BinaryOp::Shl => OperatorKind::Shl,
        BinaryOp::Shr => OperatorKind::Shr,
        _ => return None,
    })
}

/// If `receiver_ty` is a user class/record/enum AND has a non-deleted
/// declaration of `kind`, return its lowered declared return type.
/// Consults all three host kinds.
///
/// Substitution-aware: a class's `operator+(...) -> T` lowered against
/// `receiver_ty = Ty::User { generic_args: [Int] }` returns `Int`
/// rather than `Ty::Param("T")`. Mirrors the substitution Phase E
/// already does for method calls.
fn lookup_user_operator_return_type(
    receiver_ty: &Ty,
    kind: OperatorKind,
    _env: &TypeEnv,
    symbols: &SymbolTable,
) -> Option<Ty> {
    let Ty::User { name, generic_args } = receiver_ty else { return None };

    // Class lookup first; then records; then enums.
    if let Some(class) = symbols.classes.get(name) {
        if let Some(op) = class.operators.get(&kind) {
            if !op.is_deleted {
                let raw = return_type_in_class(&op.return_type, name, symbols);
                return Some(substitute(&raw, &class.generic_params, generic_args));
            }
        }
    }
    if let Some(record) = symbols.records.get(name) {
        if let Some(op) = record.operators.get(&kind) {
            if !op.is_deleted {
                let raw = return_type_in_class(&op.return_type, name, symbols);
                return Some(substitute(&raw, &record.generic_params, generic_args));
            }
        }
    }
    if let Some(enum_sig) = symbols.enums.get(name) {
        if let Some(op) = enum_sig.operators.get(&kind) {
            if !op.is_deleted {
                // Enums don't carry generic params in the AST yet, so
                // no substitution applies.
                return Some(return_type_in_class(&op.return_type, name, symbols));
            }
        }
    }
    None
}

// ============================================================================
// Block / statement walker
// ============================================================================

/// Walk `block`'s statements and **declare any locals they introduce
/// into `env`**.
///
/// This is the side-effecting pass that lets Phase C infer expressions
/// containing variable references — without it, every name lookup
/// would miss.
///
/// The walker doesn't return a type. It calls [`infer_expr`] on
/// embedded expressions purely for the **walk** (so future side-
/// effecting variants of inference can hook in here without changing
/// the public API). The returned `Ty` is discarded.
pub fn infer_block(block: &Block, env: &mut TypeEnv, symbols: &SymbolTable) {
    for stmt in &block.statements {
        infer_stmt(stmt, env, symbols);
    }
}

/// Per-statement walker. Pushes/pops scopes around nested blocks so
/// loop-vars and pattern-bindings don't leak.
fn infer_stmt(stmt: &Stmt, env: &mut TypeEnv, symbols: &SymbolTable) {
    match stmt {
        Stmt::VarDecl(v) => {
            // Prefer the declared type when present; otherwise infer
            // from the initializer.
            let ty = if let Some(declared) = &v.ty {
                ty_from_ref(declared, env, symbols)
            } else if let Some(init) = &v.init {
                infer_expr(init, env, symbols)
            } else {
                Ty::Unknown
            };
            env.declare(&v.name.text, ty);
        }
        Stmt::ForEach(f) => {
            // The loop variable's type comes from the explicit
            // annotation if present, else from the iter's element
            // type. Then push a new scope (so the binding is
            // loop-local) and walk the body.
            let iter_ty = infer_expr(&f.iter, env, symbols);
            let var_ty = if let Some(declared) = &f.var_type {
                ty_from_ref(declared, env, symbols)
            } else {
                match iter_ty {
                    Ty::Array { element, .. } => *element,
                    // Range / non-array iter: stay Unknown until we
                    // have a real iterator protocol.
                    _ => Ty::Unknown,
                }
            };
            env.push_scope();
            env.declare(&f.var_name.text, var_ty);
            infer_block(&f.body, env, symbols);
            env.pop_scope();
        }
        Stmt::If(if_stmt) => {
            // Evaluate the condition for its side effects (declares
            // nothing today, but keeps the walker total).
            let _ = infer_expr(&if_stmt.condition, env, symbols);
            env.push_scope();
            infer_block(&if_stmt.then_block, env, symbols);
            env.pop_scope();
            if let Some(else_branch) = &if_stmt.else_branch {
                infer_else_branch(else_branch, env, symbols);
            }
        }
        Stmt::While(w) => {
            let _ = infer_expr(&w.condition, env, symbols);
            env.push_scope();
            infer_block(&w.body, env, symbols);
            env.pop_scope();
        }
        Stmt::Expr(e) => {
            let _ = infer_expr(e, env, symbols);
        }
        Stmt::Assign(a) => {
            let _ = infer_expr(&a.target, env, symbols);
            let _ = infer_expr(&a.value, env, symbols);
        }
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                let _ = infer_expr(e, env, symbols);
            }
        }
        Stmt::SuperCall(args, _) => {
            for arg in args {
                let _ = infer_expr(arg, env, symbols);
            }
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
    }
}

/// Recursive helper for `else if` chains. The terminal `else { … }`
/// pushes its own scope; an `else if` recurses through [`infer_stmt`]
/// machinery by handling the inline `IfStmt` directly.
fn infer_else_branch(branch: &ElseBranch, env: &mut TypeEnv, symbols: &SymbolTable) {
    match branch {
        ElseBranch::If(if_stmt) => {
            let _ = infer_expr(&if_stmt.condition, env, symbols);
            env.push_scope();
            infer_block(&if_stmt.then_block, env, symbols);
            env.pop_scope();
            if let Some(nested) = &if_stmt.else_branch {
                infer_else_branch(nested, env, symbols);
            }
        }
        ElseBranch::Block(block) => {
            env.push_scope();
            infer_block(block, env, symbols);
            env.pop_scope();
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol_table::build;
    use juxc_ast::{CompilationUnit, FnDecl, TopLevelDecl};
    use juxc_lex::lex;
    use juxc_parse::parse;
    use juxc_source::SourceFile;

    /// Drive lex → parse → symbol-table build for the given source.
    /// Returns the symbol table plus the parsed unit so tests can
    /// reach into the AST for expressions.
    fn build_table(src: &str) -> (SymbolTable, CompilationUnit) {
        let sf = SourceFile::new("test.jux", src);
        let lex_result = lex(&sf);
        assert!(lex_result.diagnostics.is_empty(), "lex: {:?}", lex_result.diagnostics);
        let parse_result = parse(&lex_result.tokens);
        assert!(
            parse_result.diagnostics.is_empty(),
            "parse: {:?}",
            parse_result.diagnostics,
        );
        let mut diags = Vec::new();
        let table = build(&parse_result.ast, &mut diags);
        assert!(diags.is_empty(), "symtab: {:?}", diags);
        (table, parse_result.ast)
    }

    /// Helper: pull the first top-level function's body out of a unit.
    fn first_fn_body(unit: &CompilationUnit) -> &Block {
        for item in &unit.items {
            if let TopLevelDecl::Function(FnDecl { body: Some(b), .. }) = item {
                return b;
            }
        }
        panic!("no top-level function with body");
    }

    /// Helper: pull a named top-level function's body out of a unit.
    fn fn_body_by_name<'a>(unit: &'a CompilationUnit, name: &str) -> &'a Block {
        for item in &unit.items {
            if let TopLevelDecl::Function(fn_decl) = item {
                if fn_decl.name.text == name {
                    return fn_decl.body.as_ref().expect("fn has body");
                }
            }
        }
        panic!("no top-level function named `{name}`");
    }

    /// Helper: return the initializer expression of the first VarDecl
    /// found in `block`.
    fn first_var_init<'a>(block: &'a Block) -> &'a Expr {
        for stmt in &block.statements {
            if let Stmt::VarDecl(v) = stmt {
                if let Some(init) = &v.init {
                    return init;
                }
            }
        }
        panic!("no var-decl with initializer in block");
    }

    /// `42` → `Primitive::Int`.
    #[test]
    fn int_literal_is_int() {
        let (table, unit) = build_table("public void main() { var x = 42; }");
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(infer_expr(init, &env, &table), Ty::Primitive(Primitive::Int));
    }

    /// `1.5` → `Primitive::Double`.
    #[test]
    fn float_literal_is_double() {
        let (table, unit) = build_table("public void main() { var x = 1.5; }");
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(
            infer_expr(init, &env, &table),
            Ty::Primitive(Primitive::Double),
        );
    }

    /// `"hi"` → `Ty::String`.
    #[test]
    fn string_literal_is_string() {
        let (table, unit) = build_table(r#"public void main() { var x = "hi"; }"#);
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(infer_expr(init, &env, &table), Ty::String);
    }

    /// `true` → `Primitive::Bool`.
    #[test]
    fn bool_literal_is_bool() {
        let (table, unit) = build_table("public void main() { var x = true; }");
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(
            infer_expr(init, &env, &table),
            Ty::Primitive(Primitive::Bool),
        );
    }

    /// A var declared via the walker is resolvable by name.
    #[test]
    fn var_lookup_after_declaration() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var x = 42;
                var y = x;
            }
            "#,
        );
        let block = first_fn_body(&unit);
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("x"), Some(&Ty::Primitive(Primitive::Int)));
        assert_eq!(env.lookup("y"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// A name that was never declared → Unknown.
    #[test]
    fn unknown_var_is_unknown() {
        use juxc_ast::QualifiedName;
        let table = SymbolTable::default();
        let env = TypeEnv::new();
        // Build a synthetic Path expression by hand.
        let qn = QualifiedName {
            segments: vec![juxc_ast::Ident {
                text: "nope".to_string(),
                span: juxc_source::Span::DUMMY,
            }],
            span: juxc_source::Span::DUMMY,
        };
        assert!(infer_expr(&Expr::Path(qn), &env, &table).is_unknown());
    }

    /// `new MyClass()` → `Ty::User { name: "MyClass" }`.
    #[test]
    fn new_object_is_user_type() {
        let (table, unit) = build_table(
            r#"
            public class MyClass {}
            public void main() {
                var x = new MyClass();
            }
            "#,
        );
        let block = first_fn_body(&unit);
        let init = first_var_init(block);
        let env = TypeEnv::new();
        let ty = infer_expr(init, &env, &table);
        match ty {
            Ty::User { name, generic_args } => {
                assert_eq!(name, "MyClass");
                assert!(generic_args.is_empty());
            }
            other => panic!("expected User type, got {other:?}"),
        }
    }

    /// `obj.x` where `x` is an `int` field → `Primitive::Int`.
    #[test]
    fn field_access_returns_field_type() {
        let (table, unit) = build_table(
            r#"
            public class Point {
                public int x;
                public int y;
            }
            public void main() {
                var p = new Point();
                var v = p.x;
            }
            "#,
        );
        let block = first_fn_body(&unit);
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("v"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// `new int[]{1, 2, 3}` → `Array { element: Int, kind: Dynamic }`.
    #[test]
    fn array_literal_is_dynamic_array() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var xs = new int[]{1, 2, 3};
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        match infer_expr(init, &env, &table) {
            Ty::Array { element, kind } => {
                assert_eq!(*element, Ty::Primitive(Primitive::Int));
                assert_eq!(kind, ArrayKind::Dynamic);
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    /// `arr[0]` → element type.
    #[test]
    fn index_into_array_returns_element() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var xs = new int[]{1, 2, 3};
                var e = xs[0];
            }
            "#,
        );
        let block = first_fn_body(&unit);
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("e"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// `sizeof(int)` → `Primitive::Int`.
    #[test]
    fn sizeof_is_int() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var s = sizeof(int);
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(infer_expr(init, &env, &table), Ty::Primitive(Primitive::Int));
    }

    /// `$"hi"` → `Ty::String`.
    #[test]
    fn interp_string_is_string() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var s = $"hi";
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(infer_expr(init, &env, &table), Ty::String);
    }

    /// `this` inside a class context → `Ty::User { name: "Foo" }`.
    #[test]
    fn this_in_class_context() {
        let table = SymbolTable::default();
        let mut env = TypeEnv::new();
        env.set_class("Foo");
        let ty = infer_expr(&Expr::This(juxc_source::Span::DUMMY), &env, &table);
        match ty {
            Ty::User { name, .. } => assert_eq!(name, "Foo"),
            other => panic!("expected User, got {other:?}"),
        }
    }

    /// `1 < 2` → `Primitive::Bool`.
    #[test]
    fn binary_lt_is_bool() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var b = 1 < 2;
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(
            infer_expr(init, &env, &table),
            Ty::Primitive(Primitive::Bool),
        );
    }

    /// `1 + 2` → `Primitive::Int` (left's type).
    #[test]
    fn binary_add_returns_left_type() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var n = 1 + 2;
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(
            infer_expr(init, &env, &table),
            Ty::Primitive(Primitive::Int),
        );
    }

    /// `!true` → `Primitive::Bool`.
    #[test]
    fn unary_not_is_bool() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var b = !true;
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(
            infer_expr(init, &env, &table),
            Ty::Primitive(Primitive::Bool),
        );
    }

    /// A top-level function call returns the declared return type.
    #[test]
    fn call_returns_function_return_type() {
        let (table, unit) = build_table(
            r#"
            public int helper() { return 1; }
            public void main() {
                var x = helper();
            }
            "#,
        );
        // Two top-level fns: pick `main`'s body directly so the var
        // we want to test lives inside it.
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("x"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// Cast `value as long` → `Primitive::Long`.
    #[test]
    fn cast_returns_target_type() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var n = 1 as long;
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(
            infer_expr(init, &env, &table),
            Ty::Primitive(Primitive::Long),
        );
    }

    /// Phase E.1 — a method declared on a superclass is reachable from
    /// the child receiver. `(new Dog()).getAge()` returns Animal's
    /// declared return type, not Unknown.
    #[test]
    fn inherited_method_resolves_in_infer() {
        let (table, unit) = build_table(
            r#"
            public class Animal {
                public int getAge() { return 0; }
            }
            public class Dog extends Animal {}
            public void main() {
                var d = new Dog();
                var n = d.getAge();
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("n"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// Phase E.1 — a field declared on a superclass is reachable from
    /// the child receiver.
    #[test]
    fn inherited_field_resolves_in_infer() {
        let (table, unit) = build_table(
            r#"
            public class Animal { public int age; }
            public class Dog extends Animal {}
            public void main() {
                var d = new Dog();
                var n = d.age;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("n"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// Phase E.2 — field access on an instantiated generic class
    /// substitutes the type parameter for the receiver's argument.
    #[test]
    fn generic_field_substitutes_through_receiver() {
        let (table, unit) = build_table(
            r#"
            public class Box<T> { public T value; }
            public void main() {
                var b = new Box<int>();
                var v = b.value;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("v"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// Phase E.2 — method-return substitution through the receiver's
    /// generic args. `Box<int>::get() -> T` returns int.
    #[test]
    fn generic_method_return_substitutes_through_receiver() {
        let (table, unit) = build_table(
            r#"
            public class Box<T> {
                public T value;
                public T get() { return this.value; }
            }
            public void main() {
                var b = new Box<int>();
                var v = b.get();
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("v"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// Operator-dispatch: `a + b` on a class whose `operator+` returns
    /// the class type produces that user type, not a primitive
    /// fallback. Substitution-aware: the user-declared `T` return
    /// gets substituted through the receiver's generic args.
    #[test]
    fn binary_plus_uses_operator_return_type() {
        let (table, unit) = build_table(
            r#"
            public class Money {
                public int cents;
                public Money(int cents) { this.cents = cents; }
                public Money operator+(Money other) { return new Money(0); }
            }
            public void main() {
                var a = new Money(10);
                var b = new Money(20);
                var c = a + b;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("c") {
            Some(Ty::User { name, .. }) => assert_eq!(name, "Money"),
            other => panic!("expected User(\"Money\"), got {other:?}"),
        }
    }

    /// Operator-dispatch with a custom return type: `operator+(Foo)
    /// -> Bar` returns `Bar`, not `Foo` (the LHS type the built-in
    /// fallback would pick).
    #[test]
    fn binary_plus_returns_user_declared_type() {
        let (table, unit) = build_table(
            r#"
            public class Bar {
                public Bar() {}
            }
            public class Foo {
                public Foo() {}
                public Bar operator+(Foo other) { return new Bar(); }
            }
            public void main() {
                var a = new Foo();
                var b = new Foo();
                var c = a + b;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("c") {
            Some(Ty::User { name, .. }) => assert_eq!(name, "Bar"),
            other => panic!("expected User(\"Bar\"), got {other:?}"),
        }
    }

    /// Deleted operator falls through to the built-in path — for a
    /// deleted `operator+`, `infer_binary` returns the LHS type the
    /// built-in rule would give. (At the use-site this gets flagged
    /// by E0935 in check_expr — see `check::tests`.)
    #[test]
    fn deleted_operator_falls_through_to_built_in() {
        let (table, unit) = build_table(
            r#"
            public class M {
                public int x;
                public M(int x) { this.x = x; }
                public M operator+(M other) = delete;
            }
            public void main() {
                var a = new M(1);
                var b = new M(2);
                var c = a + b;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        // Built-in fallback for `+` is the LHS type.
        match env.lookup("c") {
            Some(Ty::User { name, .. }) => assert_eq!(name, "M"),
            other => panic!("expected User(\"M\"), got {other:?}"),
        }
    }

    /// Primitive `+` still goes through the built-in path — no
    /// operator dispatch when the LHS isn't a user type.
    #[test]
    fn primitive_plus_unchanged() {
        let (table, unit) = build_table(
            r#"
            public void main() {
                var x = 1 + 2;
            }
            "#,
        );
        let init = first_var_init(first_fn_body(&unit));
        let env = TypeEnv::new();
        assert_eq!(infer_expr(init, &env, &table), Ty::Primitive(Primitive::Int));
    }

    /// Phase E.2 — a raw-type receiver (`new Box(...)` with no
    /// turbofish) leaves substitution off, so the field type stays as
    /// `Ty::Param("T")`. The wildcard rule in `compatible` then keeps
    /// downstream checks quiet.
    #[test]
    fn raw_generic_receiver_leaves_param_in_place() {
        let (table, unit) = build_table(
            r#"
            public class Box<T> { public T value; }
            public void main() {
                var b = new Box();
                var v = b.value;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("v") {
            Some(Ty::Param(name)) => assert_eq!(name, "T"),
            other => panic!("expected Ty::Param(\"T\"), got {other:?}"),
        }
    }

    // ============================================================================
    // Generic inference at call sites — spec §T.4
    // ============================================================================

    /// `identity(42)` should infer `T = int` and report the call's
    /// type as `int` rather than `Ty::Param("T")`.
    #[test]
    fn generic_fn_inferred_from_single_arg() {
        let (table, unit) = build_table(
            r#"
            public T identity<T>(T x) { return x; }
            public void main() { var v = identity(42); }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("v"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// `pair_eq(1, "x")` — int vs String conflict on the same T.
    /// Phase 1 inference gives up cleanly: the return type stays
    /// `Ty::Param("T")`, which the `compatible` wildcard rule then
    /// keeps quiet at downstream use-sites. Phase D will refine
    /// this into an E04xx diagnostic at the call site once we have
    /// a join-lattice over Ty.
    #[test]
    fn generic_fn_conflicting_args_falls_back() {
        let (table, unit) = build_table(
            r#"
            public T pair_eq<T>(T a, T b) { return a; }
            public void main() { var v = pair_eq(1, "x"); }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("v") {
            Some(Ty::Param(name)) => assert_eq!(name, "T"),
            other => panic!("expected Ty::Param(\"T\") on conflict, got {other:?}"),
        }
    }

    /// Non-generic functions are unaffected: the inference fast-path
    /// returns the raw lowered type.
    #[test]
    fn non_generic_fn_unaffected_by_inference() {
        let (table, unit) = build_table(
            r#"
            public int square(int x) { return x * x; }
            public void main() { var v = square(7); }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("v"), Some(&Ty::Primitive(Primitive::Int)));
    }

    // Method-level generic inference (e.g. `class P { U pick<U>(U u) }`)
    // is wired in `append_method_generic_inference` in check.rs and in
    // `method_infer_return` here, but the class-member parser lookahead
    // doesn't yet recognize `name<T>(` as a method shape, so we can't
    // construct a syntactic test for it without first extending the
    // parser. That extension is queued behind the `class A<T>` work.

    /// `new Box(42)` should infer `Box<int>` from the constructor arg.
    #[test]
    fn generic_class_inferred_from_ctor_arg() {
        let (table, unit) = build_table(
            r#"
            public class Box<T> {
                public T value;
                public Box(T v) { this.value = v; }
            }
            public void main() { var b = new Box(42); }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("b") {
            Some(Ty::User { name, generic_args }) => {
                assert_eq!(name, "Box");
                assert_eq!(generic_args, &vec![Ty::Primitive(Primitive::Int)]);
            }
            other => panic!("expected Ty::User Box<int>, got {other:?}"),
        }
    }

    /// Explicit turbofish overrides inference: `new Box<String>(42)`
    /// keeps the `String` arg (any rustc mismatch is caught downstream).
    #[test]
    fn explicit_turbofish_overrides_inference() {
        let (table, unit) = build_table(
            r#"
            public class Box<T> {
                public T value;
                public Box(T v) { this.value = v; }
            }
            public void main() { var b = new Box<String>(42); }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("b") {
            Some(Ty::User { name, generic_args }) => {
                assert_eq!(name, "Box");
                assert_eq!(generic_args, &vec![Ty::String]);
            }
            other => panic!("expected Ty::User Box<String>, got {other:?}"),
        }
    }

    /// Records: `new Pair(1, "a")` against `record Pair<A, B>(A a, B b)`
    /// should infer `Pair<int, String>`.
    #[test]
    fn generic_record_inferred_from_components() {
        let (table, unit) = build_table(
            r#"
            public record Pair<A, B>(A a, B b) {}
            public void main() { var p = new Pair(1, "a"); }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("p") {
            Some(Ty::User { name, generic_args }) => {
                assert_eq!(name, "Pair");
                assert_eq!(
                    generic_args,
                    &vec![Ty::Primitive(Primitive::Int), Ty::String],
                );
            }
            other => panic!("expected Ty::User Pair<int, String>, got {other:?}"),
        }
    }

    // ============================================================================
    // Cross-extends generic substitution
    // ============================================================================

    /// `class Dog extends Animal<int>` — a field `T value;` declared
    /// on `Animal<T>` should read as `int` when accessed through a
    /// `Dog` receiver.
    #[test]
    fn cross_extends_concrete_field_substitutes() {
        let (table, unit) = build_table(
            r#"
            public class Animal<T> {
                public T tag;
            }
            public class Dog extends Animal<int> {}
            public void main() {
                var d = new Dog();
                var t = d.tag;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("t"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// `class Dog extends Animal<int>` — calling an inherited
    /// `T speak()` should return `int` from the `Dog` receiver.
    #[test]
    fn cross_extends_concrete_method_return_substitutes() {
        let (table, unit) = build_table(
            r#"
            public class Animal<T> {
                public T tag;
                public T speak() { return this.tag; }
            }
            public class Dog extends Animal<int> {}
            public void main() {
                var d = new Dog();
                var t = d.speak();
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("t"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// `class Sub<U> extends Sup<U>` — the chain forwards the
    /// receiver's `U` binding into the parent's `T` slot.
    #[test]
    fn cross_extends_forwarded_param_substitutes() {
        let (table, unit) = build_table(
            r#"
            public class Sup<T> {
                public T value;
            }
            public class Sub<U> extends Sup<U> {}
            public void main() {
                var s = new Sub<String>();
                var v = s.value;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("v"), Some(&Ty::String));
    }

    /// Two-hop chain: `C extends B<int>`, `B<T> extends A<T>`,
    /// `A<U> { U tag; }`. A `C` receiver's `tag` should be `int`.
    #[test]
    fn cross_extends_two_hop_substitutes() {
        let (table, unit) = build_table(
            r#"
            public class A<U> { public U tag; }
            public class B<T> extends A<T> {}
            public class C extends B<int> {}
            public void main() {
                var c = new C();
                var t = c.tag;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        assert_eq!(env.lookup("t"), Some(&Ty::Primitive(Primitive::Int)));
    }

    /// Raw extends (`extends Animal`, no `<...>`) leaves the
    /// inherited field as `Ty::Param` — there's nothing to bind T to.
    #[test]
    fn cross_extends_raw_parent_leaves_param() {
        let (table, unit) = build_table(
            r#"
            public class Animal<T> { public T tag; }
            public class Dog extends Animal {}
            public void main() {
                var d = new Dog();
                var t = d.tag;
            }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("t") {
            Some(Ty::Param(name)) => assert_eq!(name, "T"),
            other => panic!("expected Ty::Param(\"T\"), got {other:?}"),
        }
    }

    /// Non-generic classes are unaffected — `new Plain()` still yields
    /// a zero-arg `Ty::User`.
    #[test]
    fn non_generic_class_ctor_unaffected() {
        let (table, unit) = build_table(
            r#"
            public class Plain { public int x = 0; }
            public void main() { var p = new Plain(); }
            "#,
        );
        let block = fn_body_by_name(&unit, "main");
        let mut env = TypeEnv::new();
        infer_block(block, &mut env, &table);
        match env.lookup("p") {
            Some(Ty::User { name, generic_args }) => {
                assert_eq!(name, "Plain");
                assert!(generic_args.is_empty());
            }
            other => panic!("expected Ty::User Plain<>, got {other:?}"),
        }
    }
}
